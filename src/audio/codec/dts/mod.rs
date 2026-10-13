//! DTS（DTS Coherent Acoustics）解码：核心子流（最多 5.1，48 kHz），以及 XCH / XXCH（6.1、7.1）、
//! X96（96 kHz）、XBR（高码率残差）扩展。用 FFmpeg 的定点算法，与 `ffmpeg -flags bitexact` 逐样本相同。
//! DTS-HD MA 的无损部分（XLL）和 DTS Express（LBR）还不支持：DTS-HD MA 只输出它里面有损的核心部分。
//!
//! 文件可以是裸码流（.dts，16 位大端 / 小端、14 位）、.dtshd（带无缝播放信息），或 WAV 里的 DTS（CD 上
//! 常见）。码流按同步字切成包，每包 512 或 1024 个样本；跳转时先解前一个包把滤波器的历史补上。
//!
//! 本目录（`dts/`）改写自 FFmpeg 的 DTS 解码器、解析器与解复用器（libavcodec/dca_core.c、dca_exss.c、
//! dcadec.c、dcadsp.c、dcadct.c、dcadata.c、dcahuff.c、dcamath.h、dcaadpcm.h、synth_filter.c、dca.c、
//! dca_parser.c，libavformat/dtshddec.c；Copyright (C) 2016 foo86，Copyright (C) 2004 Gildas Bazin，
//! Copyright (C) 2004 Benjamin Zores，Copyright (C) 2006 Benjamin Larsson，Copyright (C) 2007 Konstantin
//! Shishkov，Copyright (c) 2008 Michael Niedermayer，Copyright (c) 2012 Paul B Mahol），2026 年改写成 Rust
//! 用于 AirPlay3。与项目其余部分（MIT 许可）不同，本目录以 GNU LGPL 2.1 或更新版本发布：
//!
//! This library is free software; you can redistribute it and/or modify it under the terms of the
//! GNU Lesser General Public License as published by the Free Software Foundation; either version
//! 2.1 of the License, or (at your option) any later version.
//!
//! This library is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without
//! even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
//! Lesser General Public License for more details.
//!
//! You should have received a copy of the GNU Lesser General Public License along with this library
//! (LICENSE-LGPL-2.1.txt in the repository root); if not, write to the Free Software Foundation,
//! Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

mod audio;
mod bits;
mod coding;
mod container;
mod core;
mod dct;
mod dsp;
mod exss;
mod frame;
mod output;
mod packet;
mod side;
mod stream;
mod synth;
mod tables;
mod x96;
mod x96f;
mod xbr;

use std::fs::File;
use std::path::Path;

use anyhow::{Result, bail};

pub use self::container::is_dts_wav;
use self::packet::{Decoder, Pcm};
use self::stream::Stream;
use super::{Block, BlockDecoder};
use crate::audio::source::Spec;

/// 打开时最多试解这么多个包找第一个好的
const MAX_BAD_START: usize = 64;
/// 跳转时先解这么多样本补上历史：没有历史时 ADPCM 预测的误差随着往后解逐渐变小（快慢看内容，
/// 测试样本里 3000 ~ 8000 个样本后与从头解的逐位相同）；解这么多不到 1 毫秒
const PREROLL: u64 = 8192;

pub struct DtsDecoder {
    stream: Stream,
    dec: Decoder,
    packet: Vec<u8>,
    pcm: Pcm,
    /// 第一个包定下的输出格式
    spec: Spec,
    speakers: Vec<usize>,
    /// 每包的样本数
    spp: u64,
    /// 下一个要解的包
    next: usize,
    /// 开头要去掉的样本数、去掉之后的总样本数（exact：来自 .dtshd 的头，可以照它截尾）
    padding: u64,
    total: u64,
    exact: bool,
    /// 已输出到的位置（去掉开头之后）
    pos: u64,
    /// 解好、还没交出去的第一个包
    pending: bool,
}

impl DtsDecoder {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with(path, false)
    }

    /// core_only：只解核心子流（测试时与 `ffmpeg -core_only 1` 对比）
    pub fn open_with(path: &Path, core_only: bool) -> Result<Self> {
        let mut file = File::open(path)?;
        let c = container::open(&mut file)?;
        let mut stream = Stream::new(file, c.data.clone())?;
        let mut dec = Decoder::new(core_only);
        let (mut packet, mut pcm) = (Vec::new(), Pcm::default());
        // 第一个能解的包定下输出格式
        let mut first = None;
        for k in 0..MAX_BAD_START {
            if !stream.packet(k, &mut packet)? {
                break;
            }
            if dec.decode(&packet, &mut pcm).is_ok() {
                first = Some(k);
                break;
            }
        }
        let Some(first) = first else { bail!("不是 DTS 文件，或解不出来") };
        let spec = Spec { rate: pcm.rate, channels: pcm.channels as u16, mask: pcm.mask };
        let spp = pcm.frames as u64;
        let (padding, total, exact) = match c.gapless {
            // .dtshd 的头按它自己的采样率计（解不出 X96、XLL 时与输出的不同）
            Some(g) => {
                let scale = |v: u64| v * spec.rate as u64 / g.rate as u64;
                (scale(g.padding), scale(g.samples), true)
            }
            None => (0, stream.est_packets() * spp, false),
        };
        let mut d = DtsDecoder {
            stream,
            dec,
            packet,
            pcm,
            spec,
            speakers: Vec::new(),
            spp,
            // pending：第 next 个包已经解好了
            next: first,
            padding,
            total,
            exact,
            pos: 0,
            pending: true,
        };
        d.speakers = d.pcm.speakers.clone();
        if first > 0 {
            // 前面解不出的包当静音，从头重来
            d.dec.flush();
            d.next = 0;
            d.pending = false;
        }
        Ok(d)
    }

    /// 这个包的 PCM 按锁定的声道排列写进 out；格式对不上（采样率、样本数变了）时写静音
    fn emit(&self, ok: bool, out: &mut Vec<i32>) {
        let ch = self.speakers.len();
        let n = self.spp as usize;
        out.clear();
        let p = &self.pcm;
        if !ok || p.rate != self.spec.rate || p.frames != n {
            out.resize(n * ch, 0);
            return;
        }
        if p.speakers == self.speakers {
            out.extend_from_slice(&p.samples[..n * ch]);
            return;
        }
        // 声道变了（如某包的 XXCH 坏了退回 5.1）：同一个扬声器放到原来的位置，缺的补 0
        let at: Vec<Option<usize>> = self.speakers.iter().map(|s| p.speakers.iter().position(|t| t == s)).collect();
        out.reserve(n * ch);
        for f in p.samples.chunks_exact(p.channels).take(n) {
            out.extend(at.iter().map(|i| i.map_or(0, |i| f[i])));
        }
    }
}

impl BlockDecoder for DtsDecoder {
    fn spec(&self) -> Spec {
        self.spec
    }

    fn total_frames(&self) -> Option<u64> {
        Some(self.total)
    }

    fn decode(&mut self, out: &mut Block) -> Result<usize> {
        out.clear();
        out.bits = 24;
        let ch = self.speakers.len();
        loop {
            if self.exact && self.pos >= self.total {
                return Ok(0);
            }
            let k = self.next;
            let ok = if self.pending {
                self.pending = false;
                true
            } else {
                if !self.stream.packet(k, &mut self.packet)? {
                    return Ok(0);
                }
                match self.dec.decode(&self.packet, &mut self.pcm) {
                    Ok(()) => true,
                    Err(e) => {
                        log::warn!("DTS 第 {k} 个包解不出（{e}），这一包静音");
                        false
                    }
                }
            };
            self.next = k + 1;
            let mut buf = std::mem::take(&mut out.int);
            self.emit(ok, &mut buf);
            out.int = buf;

            // 去掉开头的填充、截掉总长之后的部分
            let start = k as u64 * self.spp;
            let head = self.padding.saturating_sub(start).min(self.spp) as usize;
            let mut n = self.spp as usize - head;
            if self.exact {
                n = n.min((self.total - self.pos) as usize);
            }
            if n == 0 {
                out.int.clear();
                continue;
            }
            out.int.drain(..head * ch);
            out.int.truncate(n * ch);
            self.pos += n as u64;
            return Ok(n);
        }
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        let k = ((frame + self.padding) / self.spp) as usize;
        self.dec.flush();
        self.pending = false;
        // 先解前面的包，补上滤波器和 ADPCM 的历史
        let preroll = PREROLL.div_ceil(self.spp.max(1)) as usize;
        for j in k.saturating_sub(preroll)..k {
            if self.stream.packet(j, &mut self.packet)? {
                let _ = self.dec.decode(&self.packet, &mut self.pcm);
            }
        }
        self.next = k;
        self.pos = (k as u64 * self.spp).saturating_sub(self.padding);
        Ok(self.pos)
    }
}
