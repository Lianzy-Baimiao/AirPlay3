//! TAK（Tom's lossless Audio Kompressor）解码：8 / 16 / 24 位整数，最多 6 声道（官方编码器的范围）。
//!
//! 文件是 "tBaK"、元数据块（流信息、最后一帧的位置等），然后一串帧。帧头以同步字 0xA0FF 开头，
//! 带帧号和 CRC，但没有帧长：一帧在解完的地方结束（末尾是数据的 CRC）。帧与帧之间互不依赖，
//! 所以跳转只要找到所在帧的帧头（按帧号二分查找，再逐帧往后找）。
//!
//! 本目录（`tak/`）改写自 FFmpeg 的 TAK 解码器与解复用器（libavcodec/takdec.c、tak.c、takdsp.c，
//! libavformat/takdec.c；Copyright (c) 2012, 2015 Paul B Mahol），2026 年改写成 Rust 用于 AirPlay3。
//! 与项目其余部分（MIT 许可）不同，本目录以 GNU LGPL 2.1 或更新版本发布：
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

mod decode;
mod header;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Result, bail};

use self::decode::FrameDecoder;
use self::header::{FLAG_LAST, FrameHeader, MAX_HEADER, StreamInfo, parse_file, parse_frame_header};
use super::{Block, BlockDecoder};
use crate::audio::source::Spec;

/// 帧号的位数（帧号会回绕）
const NUM_BITS: u32 = 21;
/// 找第一帧时最多跳过这么多字节
const MAX_JUNK: u64 = 1 << 20;
/// 帧号比预期的大不超过这么多时，当作中间的帧坏了，补上静音
const MAX_GAP: u32 = 4096;
/// 找帧头时每次读的字节数
const CHUNK: usize = 1 << 16;

/// 从 `at` 读满 `buf`（文件末尾不够时读多少算多少），返回读到的字节数
fn read_at(file: &mut File, at: u64, buf: &mut [u8]) -> std::io::Result<usize> {
    file.seek(SeekFrom::Start(at))?;
    let mut got = 0;
    while got < buf.len() {
        match file.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(got)
}

/// `d[from..to]` 里第一个（CRC 正确的）帧头的位置；帧头可以伸出 `to`，但不能超出 `d`
fn find_in(d: &[u8], from: usize, to: usize) -> Option<(usize, FrameHeader)> {
    let to = to.min(d.len());
    let mut i = from;
    while i < to {
        let k = i + d[i..to].iter().position(|&b| b == 0xFF)?;
        if d.get(k + 1) == Some(&0xA0) {
            if let Some(h) = parse_frame_header(&d[k..d.len().min(k + MAX_HEADER)]) {
                return Some((k, h));
            }
        }
        i = k + 1;
    }
    None
}

/// 不改动解码位置，在文件的 `from..to` 里找第一个帧头
fn scan(file: &mut File, from: u64, to: u64) -> Result<Option<(u64, FrameHeader)>> {
    let mut buf = vec![0u8; CHUNK + MAX_HEADER];
    let mut at = from;
    while at < to {
        let got = read_at(file, at, &mut buf)?;
        if got == 0 {
            break;
        }
        // 读满时最后 MAX_HEADER 字节留给下一轮（帧头可能跨过去）
        let end = if got == buf.len() { CHUNK } else { got };
        let end = end.min((to - at) as usize);
        if let Some((k, h)) = find_in(&buf[..got], 0, end) {
            return Ok(Some((at + k as u64, h)));
        }
        if got < buf.len() {
            break;
        }
        at += end as u64;
    }
    Ok(None)
}

pub struct TakDecoder {
    file: File,
    len: u64,
    info: StreamInfo,
    spec: Spec,
    total: Option<u64>,
    /// 第一帧的位置、帧号
    start: u64,
    first_num: u32,
    /// 读入的数据（从文件的 `buf_at` 开始）
    buf: Vec<u8>,
    buf_at: u64,
    /// 下一帧的位置、序号（从第一帧数起）
    pos: u64,
    frame: u64,
    dec: FrameDecoder,
    done: bool,
}

impl TakDecoder {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let start = super::skip_id3v2(&mut file)?;
        let fh = parse_file(&mut file, start, len)?;
        // 第一帧总是从元数据之前找起（帧头有同步字和 CRC），元数据块的长度坏了也找得到
        let limit = (fh.data.unwrap_or(fh.after_magic) + MAX_JUNK).min(len);
        let Some((at, first)) = scan(&mut file, fh.after_magic, limit)? else {
            bail!("不是 TAK 文件，或者文件已损坏");
        };
        // 流信息：第一帧的帧头里没有（第一帧的帧头坏了）时用元数据里的，再不行往后找带流信息的帧（每隔约 2 秒有一个）
        let mut p = at;
        let info = loop {
            if let Some(i) = first.info.or(fh.info) {
                break i;
            }
            match scan(&mut file, p + 1, (at + 8 * MAX_JUNK).min(len))? {
                Some((q, FrameHeader { info: Some(i), .. })) if q > p => break i,
                Some((q, _)) => p = q,
                None => bail!("TAK 文件缺少流信息"),
            }
        };
        if !decode::supported(&info) {
            bail!("不支持的 TAK 编码（编码方式 {}，数据类型 {}，{} 声道，{} 位）", info.codec, info.data_type, info.channels, info.bps);
        }
        let mask = if info.mask.count_ones() as usize == info.channels { info.mask } else { 0 };
        let spec = Spec { rate: info.rate, channels: info.channels as u16, mask };
        let mut dec = TakDecoder {
            file,
            len,
            info,
            spec,
            total: None,
            start: at,
            first_num: first.num,
            buf: Vec::new(),
            buf_at: 0,
            pos: at,
            frame: 0,
            dec: FrameDecoder::new(),
            done: false,
        };
        dec.total = match [fh.info, first.info].into_iter().flatten().map(|i| i.samples).find(|&s| s > 0) {
            Some(t) => Some(t),
            None => dec.tail_total()?,
        };
        Ok(dec)
    }

    /// 流信息里没有总长度时：找文件末尾最后一帧的帧头（带“最后一帧”标志）
    fn tail_total(&mut self) -> Result<Option<u64>> {
        let from = self.len.saturating_sub(self.max_frame() as u64 * 2).max(self.start);
        let mut d = vec![0u8; (self.len - from) as usize];
        let got = read_at(&mut self.file, from, &mut d)?;
        d.truncate(got);
        let (mut i, mut last) = (0, None);
        while let Some((k, h)) = find_in(&d, i, d.len()) {
            i = k + 1;
            last = Some(h);
        }
        Ok(last.filter(|h| h.flags & FLAG_LAST != 0).map(|h| self.index(h.num, u64::MAX) * self.info.frame_samples as u64 + h.last_samples as u64))
    }

    /// 一帧最多的字节数（远大于实际可能的），读数据时至少备好这么多
    fn max_frame(&self) -> usize {
        16384 * self.info.channels * (self.info.bps as usize + 16) / 8 + 65536
    }

    /// 帧号 → 从第一帧数起的序号。帧号只有 21 位会回绕，取离 `near` 最近的那个（`near` 为 u64::MAX 时不处理回绕）
    fn index(&self, num: u32, near: u64) -> u64 {
        let m = 1u64 << NUM_BITS;
        let rel = (num.wrapping_sub(self.first_num) as u64) & (m - 1);
        if near == u64::MAX || near < m / 2 {
            return rel;
        }
        rel + (near.saturating_sub(rel) + m / 2) / m * m
    }

    /// 保证 `buf` 里有文件 `at` 起的 `need` 字节（文件末尾不够时尽量多）
    fn fill(&mut self, at: u64, need: usize) -> Result<()> {
        if at >= self.len {
            self.buf.clear();
            self.buf_at = at;
            return Ok(());
        }
        let end = at.saturating_add(need as u64).min(self.len);
        let have = self.buf_at + self.buf.len() as u64;
        if at >= self.buf_at && end <= have {
            return Ok(());
        }
        if at < self.buf_at || at > have {
            self.buf.clear();
            self.buf_at = at;
        } else {
            self.buf.drain(..(at - self.buf_at) as usize);
            self.buf_at = at;
        }
        let have = self.buf.len();
        let want = ((end - at) as usize).max(have + CHUNK * 4).min((self.len - at) as usize);
        if want > have {
            self.buf.resize(want, 0);
            let got = read_at(&mut self.file, at + have as u64, &mut self.buf[have..])?;
            self.buf.truncate(have + got);
        }
        Ok(())
    }

    /// 从 `at` 起（用读入的数据）找下一个帧头：（位置, 帧头），找不到时为 None
    fn find_header(&mut self, mut at: u64) -> Result<Option<(u64, FrameHeader)>> {
        loop {
            self.fill(at, MAX_HEADER + CHUNK)?;
            let d = &self.buf[(at - self.buf_at) as usize..];
            let eof = self.buf_at + self.buf.len() as u64 >= self.len;
            // 没读到文件末尾时，最后 MAX_HEADER 字节留到下一轮（帧头可能还没读全）
            let to = if eof { d.len() } else { d.len().saturating_sub(MAX_HEADER) };
            if let Some((k, h)) = find_in(d, 0, to) {
                return Ok(Some((at + k as u64, h)));
            }
            if eof || to == 0 {
                return Ok(None);
            }
            at += to as u64;
        }
    }

    fn silence(info: &StreamInfo, nb: usize, out: &mut Block) {
        out.int.resize(nb * info.channels, 0);
    }
}

impl BlockDecoder for TakDecoder {
    fn spec(&self) -> Spec {
        self.spec
    }

    fn total_frames(&self) -> Option<u64> {
        self.total
    }

    fn decode(&mut self, out: &mut Block) -> Result<usize> {
        out.clear();
        out.bits = self.info.bps;
        if self.done {
            return Ok(0);
        }
        let Some((at, h)) = self.find_header(self.pos)? else {
            self.done = true;
            return Ok(0);
        };
        let fs = self.info.frame_samples as usize;
        let m = 1u32 << NUM_BITS;
        let d = h.num.wrapping_sub(self.first_num).wrapping_sub(self.frame as u32) & (m - 1);
        if d != 0 && d <= MAX_GAP {
            // 中间有帧坏了（找不到它的帧头）：补一帧静音，下次再解这一帧
            self.frame += 1;
            Self::silence(&self.info, fs, out);
            return Ok(fs);
        }
        if d > MAX_GAP {
            // 帧号对不上（跳过了很多帧，或者往回）：以帧头为准
            self.frame = if d < m / 2 { self.frame + d as u64 } else { self.frame.saturating_sub((m - d) as u64) };
        }
        // 帧头里的流信息：基本参数必须不变（帧长可以变）
        let key = |i: &StreamInfo| (i.codec, i.data_type, i.rate, i.channels, i.bps);
        let same = h.info.is_none_or(|i| key(&i) == key(&self.info));
        if let Some(i) = h.info.filter(|_| same) {
            self.info = i;
        }
        let nb = if h.flags & FLAG_LAST != 0 { h.last_samples as usize } else { self.info.frame_samples as usize };
        self.fill(at, self.max_frame())?;
        let b = &self.buf[(at - self.buf_at) as usize..];
        let end = if same { self.dec.decode(b, h.size, &self.info, nb) } else { None };
        match end {
            Some(n) => {
                self.pos = at + n as u64;
                self.dec.interleave(self.info.bps, &mut out.int);
            }
            None => {
                // 这一帧坏了：输出静音，从帧头之后找下一帧
                self.pos = at + h.size as u64;
                Self::silence(&self.info, nb, out);
            }
        }
        self.frame += 1;
        self.done = h.flags & FLAG_LAST != 0;
        Ok(nb)
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        let fs = self.info.frame_samples as u64;
        let mut target = frame / fs;
        if let Some(t) = self.total {
            target = target.min(t.saturating_sub(1) / fs);
        }
        // 按帧号二分查找：lo 处的帧不晚于目标；窗口小到一两帧时停
        let (mut lo, mut lo_n, mut hi) = (self.start, 0u64, self.len);
        let span = (self.len - self.start).max(1) as u128;
        while hi - lo > self.max_frame() as u64 {
            let mid = lo + (hi - lo) / 2;
            let Some((p, h)) = scan(&mut self.file, mid, hi)? else {
                hi = mid;
                continue;
            };
            // 帧号的回绕按位置估计
            let near = self.total.map_or(u64::MAX, |t| ((p - self.start) as u128 * (t / fs) as u128 / span) as u64);
            let n = self.index(h.num, near);
            if n <= target {
                (lo, lo_n) = (p, n);
            } else {
                hi = p;
            }
        }
        // 往后逐帧找帧头，直到目标所在的帧
        while lo_n < target {
            match self.find_header(lo + 1)? {
                Some((p, h)) => {
                    let n = lo_n + ((h.num.wrapping_sub(self.first_num).wrapping_sub(lo_n as u32)) & ((1 << NUM_BITS) - 1)) as u64;
                    if n > target || n == lo_n {
                        break;
                    }
                    (lo, lo_n) = (p, n);
                }
                None => break,
            }
        }
        self.pos = lo;
        self.frame = lo_n;
        self.done = false;
        Ok(lo_n * fs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_header_in_junk() {
        // 一个只有帧头的“帧”：同步字、标志 0、帧号 5，再加 CRC
        let mut h = vec![0xFF, 0xA0, 5 << 3, 0, 0];
        let c = header::crc24(&h);
        h.extend_from_slice(&c.to_le_bytes()[..3]);
        let mut d = vec![0xFFu8, 0xA0, 0x12, 0xFF, 0x00];
        d.extend_from_slice(&h);
        let (k, fh) = find_in(&d, 0, d.len()).unwrap();
        assert_eq!((k, fh.num, fh.size), (5, 5, 8));
        assert!(find_in(&d, 6, d.len()).is_none());
    }
}
