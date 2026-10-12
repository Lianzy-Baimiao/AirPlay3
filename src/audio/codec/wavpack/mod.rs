//! WavPack 解码：PCM 8 ~ 32 位整数与 32 位浮点，无损与混合（有损）模式，任意声道数。
//! 混合模式的 .wvc 修正文件、DSD 不支持。
//!
//! 文件是一串块（"wvpk" 块头 + 元数据）；多声道时同一时刻的几块（各自单声道或立体声）组成一组，
//! 从带 INITIAL 标志的块到带 FINAL 标志的块。每块的解码状态都从它自己的元数据建立，块与块之间
//! 互不依赖，所以跳转只要找到所在组的开头。
//!
//! 解码算法改写自 WavPack 参考库 libwavpack 5.x（纯整数运算，结果与参考库逐位相同）。
//! 参考库以 BSD 许可发布，按其要求保留以下声明：
//!
//! Copyright (c) 1998 - 2025 David Bryant. All rights reserved.
//!
//! Redistribution and use in source and binary forms, with or without modification, are permitted
//! provided that the following conditions are met: (1) Redistributions of source code must retain
//! the above copyright notice, this list of conditions and the following disclaimer.
//! (2) Redistributions in binary form must reproduce the above copyright notice, this list of
//! conditions and the following disclaimer in the documentation and/or other materials provided
//! with the distribution. (3) Neither the name of the copyright holder nor the names of its
//! contributors may be used to endorse or promote products derived from this software without
//! specific prior written permission.
//!
//! THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
//! IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
//! FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE
//! LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
//! (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA,
//! OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
//! CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF
//! THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

mod block;
mod decorr;
mod floats;
mod info;
mod meta;
mod tables;
mod unpack;
mod words;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Result, bail};

use self::block::{BYTES_STORED, DSD, FINAL, FLOAT_DATA, HEADER_LEN, Header, INITIAL, MONO_FLAG, RATES, SRATE_LSB, Stream};
use super::{Block, BlockDecoder};
use crate::audio::source::Spec;

/// 两块之间最多跳过这么多不认识的字节（与参考库相同）
const MAX_JUNK: u64 = 1 << 20;

pub struct WvDecoder {
    file: File,
    len: u64,
    spec: Spec,
    total: Option<u64>,
    bits: u32,
    float: bool,
    /// 浮点的归一化指数（127 = 已经是 ±1）
    norm_exp: u8,
    /// 第一块的位置（跳过 ID3v2 之后）
    start: u64,
    /// 下一组的位置、第一帧的序号
    pos: u64,
    frame: u64,
    /// 已知的各组（第一帧, 位置），从文件开头连续；`known` = 已知部分的结尾（位置, 帧数）
    index: Vec<(u64, u64)>,
    known: (u64, u64),
    /// 当前组各块的（位置, 块头）
    heads: Vec<(u64, Header)>,
    buf: Vec<u8>,
    tmp: Vec<i32>,
    warned: bool,
}

/// 从 `at` 读满 `buf`（文件末尾不够时读多少算多少），返回读到的字节数
fn read_at(file: &mut File, at: u64, buf: &mut [u8]) -> std::io::Result<usize> {
    file.seek(SeekFrom::Start(at))?;
    let mut got = 0;
    while got < buf.len() {
        match file.read(&mut buf[got..])? {
            0 => break,
            n => got += n,
        }
    }
    Ok(got)
}
impl WvDecoder {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let start = super::skip_id3v2(&mut file)?;
        let mut d = WvDecoder {
            file,
            len,
            spec: Spec { rate: 44_100, channels: 2, mask: 0 },
            total: None,
            bits: 16,
            float: false,
            norm_exp: 127,
            start,
            pos: start,
            frame: 0,
            index: Vec::new(),
            known: (start, 0),
            heads: Vec::new(),
            buf: Vec::new(),
            tmp: Vec::new(),
            warned: false,
        };
        // 第一个有音频的块决定格式；前面只有元数据的块里也可能有声道信息、采样率
        let (mut at, mut chan_info, mut rate, mut seen) = (start, None, None, false);
        let (h, s) = loop {
            let Some((p, h)) = d.next_header(at) else {
                bail!(if seen { "WavPack 文件损坏，没有能解码的块" } else { "不是 WavPack 文件（或是 3.x 以前的旧格式）" })
            };
            seen = true;
            if h.flags & DSD != 0 {
                bail!("不支持 DSD 格式的 WavPack");
            }
            if p + h.size as u64 > len {
                bail!("WavPack 文件不完整");
            }
            at = p + h.size as u64;
            d.buf.resize(h.size, 0);
            read_at(&mut d.file, p, &mut d.buf)?;
            let Ok(s) = Stream::parse(&d.buf) else { continue };
            chan_info = chan_info.or(s.chan_info.filter(|c| c.0 > 0));
            rate = rate.or(s.rate);
            if h.samples > 0 {
                break (h, s);
            }
        };
        let channels = chan_info.map_or(h.channels() as u32, |c| c.0);
        let mask = chan_info.map_or(if channels == 1 { 4 } else { 3 }, |c| c.1);
        let srate = (h.flags >> SRATE_LSB) & 0xf;
        let rate = rate.unwrap_or(RATES.get(srate as usize).copied().unwrap_or(44_100));
        if channels > 64 || rate == 0 {
            bail!("WavPack 参数不对：{channels} 声道 {rate} Hz");
        }
        d.spec = Spec { rate, channels: channels as u16, mask };
        d.float = h.flags & FLOAT_DATA != 0;
        d.bits = if d.float { 32 } else { ((h.flags & BYTES_STORED) + 1) * 8 };
        d.norm_exp = s.float[3];
        // 文件头的总长只在从第 0 个样本开始时可信；否则（截取的片段、流式编码）扫一遍块头
        d.total = h.total.filter(|&t| h.index == 0 && t > 0);
        if d.total.is_none() {
            d.scan_to(u64::MAX);
            d.total = Some(d.known.1);
        }
        Ok(d)
    }

    /// 从 `at` 起找下一个认得的块头（跳过中间不认识的数据，最多 1 MB），返回位置和块头
    fn next_header(&mut self, mut at: u64) -> Option<(u64, Header)> {
        let mut h = [0u8; HEADER_LEN];
        if read_at(&mut self.file, at, &mut h).ok()? == HEADER_LEN {
            if let Some(hdr) = Header::parse(&h) {
                return Some((at, hdr));
            }
        }
        let limit = at + MAX_JUNK;
        let mut chunk = vec![0u8; 64 * 1024];
        while at < limit {
            let n = read_at(&mut self.file, at, &mut chunk).ok()?;
            if n < HEADER_LEN {
                return None;
            }
            for i in 0..=n - HEADER_LEN {
                if chunk[i] == b'w' {
                    if let Some(hdr) = Header::parse(&chunk[i..]) {
                        return Some((at + i as u64, hdr));
                    }
                }
            }
            at += (n - HEADER_LEN + 1) as u64;
        }
        None
    }

    /// 从 `at` 起找下一组完整的块（跳过只有元数据的块、不在组开头的块），记在 `heads`；
    /// 返回这一组的开头和结尾。文件在组中间截断时返回 None（与 ffmpeg 一样丢掉不完整的最后一组）
    fn group(&mut self, mut at: u64) -> Option<(u64, u64)> {
        self.heads.clear();
        let ch = self.spec.channels as usize;
        let mut filled = 0;
        loop {
            let (p, h) = self.next_header(at)?;
            at = p + h.size as u64;
            if at > self.len {
                return None;
            }
            if h.samples == 0 || (self.heads.is_empty() && h.flags & INITIAL == 0) {
                continue;
            }
            self.heads.push((p, h));
            filled += if h.flags & MONO_FLAG != 0 { 1 } else { 2 };
            if h.flags & FINAL != 0 || filled >= ch {
                return Some((self.heads[0].0, at));
            }
        }
    }

    /// 解下一组到 `out`，返回帧数；0 = 结束
    fn decode_group(&mut self, out: &mut Block) -> Result<usize> {
        out.clear();
        out.bits = self.bits;
        let Some((from, to)) = self.group(self.pos) else { return Ok(0) };
        let n = self.heads[0].1.samples as usize;
        let ch = self.spec.channels as usize;
        out.int.resize(n * ch, 0);
        let mut offset = 0;
        for k in 0..self.heads.len() {
            let (p, h) = self.heads[k];
            self.buf.resize(h.size, 0);
            if read_at(&mut self.file, p, &mut self.buf)? < h.size {
                return Ok(0);
            }
            let ok = match Stream::parse(&self.buf) {
                Ok(mut s) => s.unpack(&self.buf, &mut self.tmp),
                Err(e) => {
                    self.tmp.clear();
                    if !self.warned {
                        log::warn!("WavPack：{e}");
                    }
                    false
                }
            };
            if !ok && !self.warned {
                self.warned = true;
                log::warn!("WavPack 第 {} 帧起的数据坏了，这一段静音", self.frame);
            }
            // 各块的声道依次排开；立体声块落在最后一个声道上时只取左声道（与参考库相同）
            let bc = h.channels();
            let take = bc.min(ch.saturating_sub(offset));
            if take > 0 {
                for (dst, src) in out.int.chunks_exact_mut(ch).zip(self.tmp.chunks_exact(bc)) {
                    dst[offset..offset + take].copy_from_slice(&src[..take]);
                }
            }
            offset += bc;
        }
        if from >= self.known.0 {
            self.index.push((self.frame, from));
            self.known = (to, self.frame + n as u64);
        }
        self.pos = to;
        self.frame += n as u64;
        if self.float {
            // 解出的是 f32 的位；不是 ±1 范围的浮点按归一化指数缩放
            let e = self.norm_exp as i32;
            let scale = if e == 0 || e == 127 { 1.0 } else { 2f32.powi(127 - e) };
            out.float.extend(out.int.iter().map(|&v| f32::from_bits(v as u32) * scale));
            out.int.clear();
        }
        Ok(n)
    }

    /// 把已知的组扩展到包含第 `frame` 帧（或到文件末尾）：只读块头
    fn scan_to(&mut self, frame: u64) {
        while self.known.1 <= frame {
            let Some((from, to)) = self.group(self.known.0) else { break };
            let n = self.heads[0].1.samples as u64;
            self.index.push((self.known.1, from));
            self.known = (to, self.known.1 + n);
        }
    }

    fn seek_to(&mut self, frame: u64) -> u64 {
        self.scan_to(frame);
        let i = self.index.partition_point(|&(f, _)| f <= frame);
        let (f, p) = if i == 0 { (0, self.start) } else { self.index[i - 1] };
        self.frame = f;
        self.pos = p;
        f
    }
}

impl BlockDecoder for WvDecoder {
    fn spec(&self) -> Spec {
        self.spec
    }

    fn total_frames(&self) -> Option<u64> {
        self.total
    }

    fn decode(&mut self, out: &mut Block) -> Result<usize> {
        self.decode_group(out)
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        Ok(self.seek_to(frame))
    }
}
