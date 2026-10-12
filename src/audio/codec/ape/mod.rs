//! Monkey's Audio（APE）解码：3.80 起的所有版本（含 3.99 起的新格式、24 / 32 位、浮点、多声道）。
//!
//! 文件：（可选 ID3v2）→ 文件头 → 各帧位置表 → 各帧。每帧开头所有状态重置，所以跳转直接到帧开头。
//! 帧数据按 32 位小端字存放；一帧以 CRC（3.82 起附带「特殊帧」标志：静音、假立体声）开头。
//!
//! 解码算法改写自 Monkey's Audio SDK（Matthew T. Ashland，结果与官方解码器逐位相同）。
//! SDK 以 3-clause BSD 许可发布，按其要求保留以下声明：
//!
//! Copyright 2000-2026 Matthew T. Ashland. All rights reserved.
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

mod anti;
mod bits;
mod frame;
mod header;
mod nnfilter;
mod predictor;
mod range;
mod rice;
mod unprepare;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Result, anyhow};

use self::bits::Bits;
use self::frame::Outcome;
use self::header::{FLAG_FLOAT, Info};
use self::predictor::Predictor;
use super::{Block, BlockDecoder};
use crate::audio::source::Spec;

/// 帧末尾多读的字节（区间编码会预读几个字节）
const SLACK: usize = 16;

pub struct ApeDecoder {
    file: File,
    info: Info,
    spec: Spec,
    float: bool,
    /// 帧数据的结尾（去掉末尾的标签）
    end: u64,
    next: u32,
    done: bool,
    preds: Vec<Predictor>,
    interim: bool,
    buf: Vec<u8>,
    work: [Vec<i32>; 3],
    warned: bool,
}

impl ApeDecoder {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let info = header::parse(&mut file, len)?;
        let end = header::data_end(&mut file, len);
        let preds = if info.version >= 3930 {
            (0..info.channels)
                .map(|_| Predictor::new(info.level, info.version, info.bits))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| anyhow!("不支持的 APE 压缩级别 {}", info.level))?
        } else {
            if info.level > 4000 {
                return Err(anyhow!("不支持的 APE 压缩级别 {}", info.level));
            }
            Vec::new()
        };
        let spec = Spec { rate: info.rate, channels: info.channels, mask: 0 };
        let float = info.flags & FLAG_FLOAT != 0;
        Ok(ApeDecoder { file, info, spec, float, end, next: 0, done: false, preds, interim: false, buf: Vec::new(), work: Default::default(), warned: false })
    }

    /// 读第 i 帧的数据（按字倒序），返回（起始位, 可读的位数, 数据是否完整）
    fn load(&mut self, i: u32) -> Result<Option<(usize, usize, bool)>> {
        let info = &self.info;
        let Some(&at) = info.seek.get(i as usize) else { return Ok(None) };
        let next = info.seek.get(i as usize + 1).copied().unwrap_or(self.end).max(at);
        // 字的边界按第一帧对齐；3.80 及以前一帧可以从字的中间开始
        let (base, start_bit) = if info.version <= 3800 {
            (at, *info.seek_bits.get(i as usize).unwrap_or(&0) as usize)
        } else {
            let rem = (at - info.seek[0]) % 4;
            (at - rem, rem as usize * 8)
        };
        let need = (next - base) as usize;
        self.buf.clear();
        self.buf.resize(need + SLACK, 0);
        self.file.seek(SeekFrom::Start(base))?;
        let mut got = 0;
        while got < self.buf.len() {
            match self.file.read(&mut self.buf[got..])? {
                0 => break,
                n => got += n,
            }
        }
        if got <= start_bit / 8 {
            return Ok(None);
        }
        let len = self.buf.len().div_ceil(4) * 4;
        self.buf.resize(len, 0);
        self.buf.chunks_exact_mut(4).for_each(|w| w.reverse());
        // 只有完整的字才能用（倒序后不完整的字里位置对不上）
        Ok(Some((start_bit, got / 4 * 32, got >= need)))
    }

    fn decode_frame(&mut self, out: &mut Block) -> Result<usize> {
        out.clear();
        out.bits = self.info.bits as u32;
        if self.done || self.next >= self.info.frames {
            return Ok(0);
        }
        let i = self.next;
        let n = self.info.frame_blocks(i) as usize;
        let Some((start, limit, complete)) = self.load(i)? else {
            self.done = true;
            return Ok(0);
        };
        self.next += 1;
        let mut outcome = self.run(start, limit, n, complete, &mut out.int);
        // 24 位解不对：可能是 Monkey's Audio 5.01 ~ 8.51 写的「过渡期」文件（或反过来），换另一种算法再解；
        // 解对了就以后都用它，两种都不对时不变（参考库这里只换不回，一帧坏了会让后面全静音）
        if matches!(outcome, Outcome::Bad) && self.info.bits == 24 && self.info.version >= 3950 {
            self.set_interim(!self.interim);
            outcome = self.run(start, limit, n, complete, &mut out.int);
            if matches!(outcome, Outcome::Bad) {
                self.set_interim(!self.interim);
            }
        }
        let ch = self.info.channels as usize;
        let got = match outcome {
            Outcome::Ok => n,
            Outcome::Partial(k) => {
                self.done = true;
                out.int.truncate(k * ch);
                k
            }
            Outcome::Bad => {
                if !self.warned {
                    self.warned = true;
                    log::warn!("APE 第 {i} 帧的数据坏了，这一帧静音");
                }
                out.int.clear();
                out.int.resize(n * ch, 0);
                if self.float {
                    out.float.resize(n * ch, 0.0);
                    out.int.clear();
                }
                return Ok(n);
            }
        };
        if self.float {
            out.float.extend(out.int.iter().map(|&v| unprepare::float(v)));
            out.int.clear();
        }
        Ok(got)
    }

    fn set_interim(&mut self, on: bool) {
        self.interim = on;
        self.preds.iter_mut().for_each(|p| p.set_interim(on));
    }

    fn run(&mut self, start: usize, limit: usize, n: usize, complete: bool, out: &mut Vec<i32>) -> Outcome {
        let mut b = Bits::new(&self.buf, start, limit);
        if self.info.version >= 3930 {
            frame::modern(&self.info, &mut self.preds, &mut b, n, complete, out)
        } else {
            frame::old(&self.info, &mut b, n, complete, &mut self.work, out)
        }
    }
}

impl BlockDecoder for ApeDecoder {
    fn spec(&self) -> Spec {
        self.spec
    }

    fn total_frames(&self) -> Option<u64> {
        Some(self.info.total())
    }

    fn decode(&mut self, out: &mut Block) -> Result<usize> {
        self.decode_frame(out)
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        let bpf = self.info.blocks_per_frame as u64;
        let i = (frame / bpf).min(self.info.frames.saturating_sub(1) as u64);
        self.next = i as u32;
        self.done = false;
        Ok(i * bpf)
    }
}