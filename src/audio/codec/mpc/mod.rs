//! Musepack（MPC）解码：SV7（"MP+"）与 SV8（"MPCK"）。有损的子带编码：每帧 1152 个采样点 = 36 × 32 个子带
//! 样本，反量化后经 32 子带合成滤波器输出。合成滤波器有 481 个采样点的延迟，开头的这一段丢掉。
//!
//! 解码算法改写自 Musepack 参考库 libmpcdec r475（浮点版，结果与参考库逐位相同）。
//! 参考库以 BSD 许可发布，按其要求保留以下声明：
//!
//! Copyright (c) 2005-2009, The Musepack Development Team. All rights reserved.
//!
//! Redistribution and use in source and binary forms, with or without modification, are permitted
//! provided that the following conditions are met: (1) Redistributions of source code must retain
//! the above copyright notice, this list of conditions and the following disclaimer.
//! (2) Redistributions in binary form must reproduce the above copyright notice, this list of
//! conditions and the following disclaimer in the documentation and/or other materials provided
//! with the distribution. (3) Neither the name of the The Musepack Development Team nor the names
//! of its contributors may be used to endorse or promote products derived from this software
//! without specific prior written permission.
//!
//! THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
//! IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
//! FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR
//! CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
//! DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
//! DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
//! IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
//! OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

mod bits;
mod decoder;
mod read;
mod stream;
mod synth;
mod tables;

use std::fs::File;
use std::path::Path;

use anyhow::Result;

use self::bits::Bits;
use self::decoder::{DELAY, Decoder, FRAME};
use self::stream::{Info, Reader, block_head};
use super::{Block, BlockDecoder};
use crate::audio::source::Spec;

/// SV7 跳转时往前多解的帧数（让比例因子等状态恢复，与参考库相同）
const SV7_PREROLL: u64 = 32;

pub struct MpcDecoder {
    info: Info,
    rd: Reader,
    dec: Decoder,
    /// 已知的各帧（SV7，位）/ 各 AP 块（SV8，字节）的位置，从第一个起连续
    index: Vec<u64>,
    /// 下一帧 / 下一块的序号
    next: u64,
    /// SV8：当前 AP 块的数据（相对 base 的位），剩下的帧数
    block: Option<(u64, u64, u32)>,
    pcm: Vec<f32>,
    done: bool,
}

impl MpcDecoder {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let info = stream::parse(&mut file)?;
        let samples = if info.version == 7 && info.gapless { info.samples.div_ceil(FRAME as u64) * FRAME as u64 } else { info.samples };
        let dec = Decoder::new(info.version, info.max_band, info.ms, info.channels, samples, info.beg_silence);
        let rd = Reader::new(file, info.version == 7, info.base);
        let index = vec![info.first];
        Ok(MpcDecoder { info, rd, dec, index, next: 0, block: None, pcm: vec![0.0; FRAME * 2], done: false })
    }

    /// SV7：第 i 帧的位置（顺着帧长往后找）
    fn sv7_pos(&mut self, i: u64) -> Result<u64> {
        while (self.index.len() as u64) <= i {
            let p = *self.index.last().unwrap();
            self.rd.load(p / 8, 8)?;
            let mut r = Bits::new(&self.rd.buf, (p - self.rd.at * 8) as usize);
            let len = r.read(20) as u64;
            self.index.push(p + 20 + len);
        }
        Ok(self.index[i as usize])
    }

    /// SV8：第 i 个 AP 块的位置（顺着块头往后找，跳过中间的其它块）；没有了返回 None
    fn sv8_pos(&mut self, i: u64) -> Result<Option<u64>> {
        while (self.index.len() as u64) <= i {
            let mut p = *self.index.last().unwrap();
            let mut first = true;
            loop {
                self.rd.load(p, 16)?;
                let Some((key, size, head)) = block_head(&self.rd.buf[(p - self.rd.at) as usize..]) else { return Ok(None) };
                if !first && &key == b"AP" {
                    break;
                }
                if &key == b"SE" {
                    return Ok(None);
                }
                first = false;
                p += head + size;
            }
            self.index.push(p);
        }
        Ok(Some(self.index[i as usize]))
    }

    /// 读完一帧的位流以后：反量化、合成、去掉要跳过的部分（参考库 mpc_decoder_decode_frame 的后半）；
    /// 返回输出的采样点数（在 `pcm` 开头）
    fn finish(dec: &mut Decoder, r: &mut Bits, pcm: &mut [f32], mut left: i64) -> usize {
        let (ch, frame) = (dec.channels, FRAME as u64);
        if dec.skip < frame + DELAY {
            dec.requantize();
            dec.synthesize(pcm);
        }
        dec.decoded += frame;
        // SV7 最后一帧后面有 11 位：这一帧的有效采样点数
        if dec.version == 7 && dec.decoded.wrapping_sub(dec.samples) < frame {
            let last = r.read(11) as u64;
            if dec.decoded == dec.samples {
                let last = if last == 0 { frame } else { last };
                dec.samples = dec.samples + last - frame;
                left += last as i64 - frame as i64;
            }
        }
        let mut n = left.clamp(0, frame as i64) as u64;
        if dec.skip > 0 {
            if n <= dec.skip {
                dec.skip -= n;
                n = 0;
            } else {
                n -= dec.skip;
                let s = dec.skip as usize;
                pcm.copy_within(s * ch..(s + n as usize) * ch, 0);
                dec.skip = 0;
            }
        }
        n as usize
    }

    /// 解一帧；None = 结束（或数据坏了、截断了，与参考库一样到此为止）
    fn decode_one(&mut self) -> Result<Option<usize>> {
        let (samples, decoded) = (self.dec.samples, self.dec.decoded);
        let left = samples as i64 - decoded as i64 + DELAY as i64;
        if left <= 0 {
            return Ok(None);
        }
        if self.info.version == 7 {
            let p = self.sv7_pos(self.next)?;
            let end = self.sv7_pos(self.next + 1)?;
            let last = samples.wrapping_sub(decoded).wrapping_sub(1) < FRAME as u64;
            self.rd.load(p / 8, ((end - p) / 8 + 16) as usize)?;
            let start = (p + 20 - self.rd.at * 8) as usize;
            let mut r = Bits::new(&self.rd.buf, start);
            self.dec.read_sv7(&mut r);
            let n = Self::finish(&mut self.dec, &mut r, &mut self.pcm, left);
            self.next += 1;
            let want = (end - p - 20) as usize + if last { 11 } else { 0 };
            if r.pos > self.rd.valid * 8 || r.pos - start != want {
                return Ok(None);
            }
            return Ok(Some(n));
        }
        if self.block.is_none() {
            let Some(p) = self.sv8_pos(self.next)? else { return Ok(None) };
            self.rd.load(p, 16)?;
            let Some((key, size, head)) = block_head(&self.rd.buf[(p - self.rd.at) as usize..]) else { return Ok(None) };
            if &key != b"AP" {
                return Ok(None);
            }
            self.block = Some(((p + head) * 8, size * 8, 1 << self.info.block_pwr));
            self.next += 1;
        }
        let (pos, bits, frames) = self.block.unwrap();
        let key = frames == 1 << self.info.block_pwr;
        self.rd.load(pos / 8, (bits / 8 + 16) as usize)?;
        let start = (pos - self.rd.at * 8) as usize;
        let mut r = Bits::new(&self.rd.buf, start);
        self.dec.read_sv8(&mut r, key);
        let used = (r.pos - start) as u64;
        if r.pos > self.rd.valid * 8 || used > bits {
            return Ok(None);
        }
        let n = Self::finish(&mut self.dec, &mut r, &mut self.pcm, left);
        let (bits, frames) = (bits - used, frames - 1);
        if frames == 0 && bits > 7 {
            return Ok(None);
        }
        self.block = (frames > 0).then_some((pos + used, bits, frames));
        Ok(Some(n))
    }

    /// 跳到第 `frame` 个采样点（参考库 mpc_demux_seek_sample）：SV7 往前多解 32 帧、比例因子先置成 1；
    /// SV8 从所在 AP 块的开头解。之后丢掉合成延迟和块内前面的部分，输出正好从 `frame` 开始
    fn seek_to(&mut self, frame: u64) -> Result<()> {
        let dest = (frame + self.info.beg_silence).min(self.info.samples);
        let block = (FRAME as u64) << self.info.block_pwr;
        let mut fwd = dest / block;
        let mut skip = DELAY + dest % block;
        if self.info.version == 7 {
            let back = fwd.min(SV7_PREROLL);
            fwd -= back;
            skip += FRAME as u64 * back;
            // 参考库 memset(SCF_Index, fwd != 0)：每个字节是 1
            let v = if fwd != 0 { 0x0101_0101 } else { 0 };
            self.dec.scf_i = [[[v; 3]; 32]; 2];
            self.sv7_pos(fwd)?;
        } else {
            while self.sv8_pos(fwd)?.is_none() && fwd > 0 {
                fwd -= 1;
            }
        }
        self.next = fwd;
        self.block = None;
        self.dec.decoded = fwd * block;
        self.dec.skip = skip;
        self.done = false;
        Ok(())
    }
}

impl BlockDecoder for MpcDecoder {
    fn spec(&self) -> Spec {
        Spec { rate: self.info.rate, channels: self.info.channels as u16, mask: 0 }
    }

    fn total_frames(&self) -> Option<u64> {
        Some(self.info.samples - self.info.beg_silence)
    }

    fn decode(&mut self, out: &mut Block) -> Result<usize> {
        out.clear();
        out.bits = 32;
        while !self.done {
            match self.decode_one()? {
                None => self.done = true,
                Some(0) => {}
                Some(n) => {
                    out.float.extend_from_slice(&self.pcm[..n * self.info.channels]);
                    return Ok(n);
                }
            }
        }
        Ok(0)
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        self.seek_to(frame)?;
        Ok(frame)
    }
}