// 改写自 FFmpeg 的 libavcodec/dca_parser.c（Copyright (C) 2004 Gildas Bazin、2004 Benjamin Zores、
// 2006 Benjamin Larsson、2007 Konstantin Shishkov），LGPL 2.1+，见 mod.rs 开头的说明。
//! 把码流切成包：一个包从核心（或扩展子流）的同步字开始，到下一个同类同步字为止（与 FFmpeg 的切法相同，
//! 包后面的填充算在这个包里）。包的位置按需往后找并记下来；码率固定时直接推算，跳转时不用从头找。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use anyhow::Result;

pub(super) const CORE_BE: u32 = 0x7FFE_8001;
const CORE_LE: u32 = 0xFE7F_0180;
const CORE_14B_BE: u32 = 0x1FFF_E800;
const CORE_14B_LE: u32 = 0xFF1F_00E8;
const SUBSTREAM: u32 = 0x6458_2025;

/// 同步字（及其后几位）组成的核心标记，取 state 的低 48 位
fn is_core_marker(s: u64) -> bool {
    s & 0xFFFF_FFFF_F0FF == ((CORE_14B_LE as u64) << 16 | 0xF007)
        || s & 0xFFFF_FFFF_FFF0 == ((CORE_14B_BE as u64) << 16 | 0x07F0)
        || s & 0xFFFF_FFFF_00FC == ((CORE_LE as u64) << 16 | 0x00FC)
        || s & 0xFFFF_FFFF_FC00 == ((CORE_BE as u64) << 16 | 0xFC00)
}

fn is_exss_marker(s: u64) -> bool {
    s as u32 == SUBSTREAM
}

fn core_marker(s: u64) -> u32 {
    (s >> 16) as u32
}

fn state_le(s: u64) -> u64 {
    ((s & 0xFF00_FF00) >> 8) | ((s & 0x00FF_00FF) << 8)
}

fn state_14(s: u64) -> u64 {
    ((s & 0x3FFF_0000) >> 8) | ((s & 0x0000_3FFF) >> 6)
}

fn core_framesize(s: u64) -> usize {
    ((s >> 4) & 0x3FFF) as usize + 1
}

fn exss_framesize(s: u64) -> usize {
    if s & 0x20_0000_0000 != 0 { ((s >> 5) & 0xF_FFFF) as usize + 1 } else { ((s >> 13) & 0xFFFF) as usize + 1 }
}

/// dca_find_frame_end：在 d 里找一个包。last 是上一个包的同步字（0 = 还没有）。
/// 返回 (包开头, 下一个包的开头)；找到开头但没找到结尾时第二项为 None；连开头都没有时返回 None
pub(super) fn find_frame(d: &[u8], last: &mut u32) -> Option<(usize, Option<usize>)> {
    let mut state: u64 = if *last == 0 { 0 } else { u64::MAX };
    let mut i = 0;
    let start;
    loop {
        let &b = d.get(i)?;
        state = state << 8 | b as u64;
        i += 1;
        let marker = is_core_marker(state) || is_exss_marker(state);
        if marker && (*last == 0 || *last == core_marker(state) || *last == SUBSTREAM) {
            let exss = is_exss_marker(state);
            start = i - if exss { 4 } else { 6 };
            *last = if exss { SUBSTREAM } else { core_marker(state) };
            break;
        }
    }
    let (mut phase, mut size, mut framesize) = (1, 0usize, 0usize);
    while let Some(&b) = d.get(i) {
        size += 1;
        state = state << 8 | b as u64;
        i += 1;
        if phase == 1 {
            let at = match *last {
                CORE_BE => (size == 2).then(|| (core_framesize(state), 2)),
                CORE_LE => (size == 2).then(|| (core_framesize(state_le(state)), 4)),
                CORE_14B_BE => (size == 4).then(|| (core_framesize(state_14(state)), 4)),
                CORE_14B_LE => (size == 4).then(|| (core_framesize(state_14(state_le(state))), 4)),
                _ => (size == 6).then(|| (exss_framesize(state), 4)),
            };
            if let Some((f, p)) = at {
                framesize = f;
                phase = p;
            }
            continue;
        }
        if phase == 2 && is_exss_marker(state) && framesize <= size + 2 {
            framesize = size + 2;
            phase = 3;
            continue;
        }
        if phase == 3 {
            if size == framesize + 4 {
                framesize += exss_framesize(state);
                phase = 4;
            }
            continue;
        }
        if framesize > size {
            continue;
        }
        if (is_core_marker(state) || is_exss_marker(state)) && (*last == core_marker(state) || *last == SUBSTREAM) {
            return Some((start, Some(i - if is_exss_marker(state) { 4 } else { 6 })));
        }
    }
    Some((start, None))
}

/// 从 at 读满 buf（不够时读多少算多少），返回读到的字节数
pub(super) fn read_at(file: &mut File, at: u64, buf: &mut [u8]) -> std::io::Result<usize> {
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

/// 找第一个包时最多跳过这么多字节
pub(super) const MAX_JUNK: u64 = 1 << 20;
/// 包的最大长度（FFmpeg 的 MAX_PACKET_SIZE）；更长时就地切开
pub(super) const MAX_PACKET: usize = 0x10_4000;

/// 码率可变的码流不超过这么大时，打开时整个读一遍建索引
const WHOLE_SCAN: u64 = 64 << 20;

/// 文件里 DTS 码流（data 范围）的包
pub(super) struct Stream {
    file: File,
    pub data: std::ops::Range<u64>,
    buf: Vec<u8>,
    /// 找包用的状态（上一个包的同步字）
    last: u32,
    /// 已找到的包：开头、结尾
    starts: Vec<u64>,
    ends: Vec<u64>,
    /// 码率固定时的包长（前几个包一样长时认为固定，用到时逐个核对同步字）
    cbr: Option<u64>,
    eof: bool,
}

impl Stream {
    pub fn new(file: File, data: std::ops::Range<u64>) -> Result<Self> {
        let mut s = Stream { file, data, buf: Vec::new(), last: 0, starts: Vec::new(), ends: Vec::new(), cbr: None, eof: false };
        while s.starts.len() < 8 && s.scan()? {}
        let lens: Vec<u64> = s.starts.windows(2).map(|w| w[1] - w[0]).collect();
        if lens.len() >= 4 && lens.iter().all(|&l| l == lens[0]) {
            s.cbr = Some(lens[0]);
        }
        Ok(s)
    }

    /// 往后找一个包，记到 starts / ends；没有更多包时返回 false
    fn scan(&mut self) -> Result<bool> {
        if self.eof {
            return Ok(false);
        }
        let from = self.ends.last().copied().unwrap_or(self.data.start);
        let limit = if self.starts.is_empty() { MAX_JUNK as usize } else { 0 } + MAX_PACKET;
        let mut win = 1 << 16;
        loop {
            let want = win.min((self.data.end - from) as usize).min(limit);
            self.buf.resize(want, 0);
            let got = read_at(&mut self.file, from, &mut self.buf)?;
            self.buf.truncate(got);
            let mut last = self.last;
            let found = find_frame(&self.buf, &mut last);
            let all = from + got as u64 >= self.data.end;
            let (start, end) = match found {
                None if all || got >= limit => {
                    self.eof = true;
                    return Ok(false);
                }
                Some((s, Some(e))) => (s, e),
                Some((s, None)) if all || got >= limit => (s, got),
                _ => {
                    win *= 2;
                    continue;
                }
            };
            self.last = last;
            let end = end.min(start + MAX_PACKET);
            self.starts.push(from + start as u64);
            self.ends.push(from + end as u64);
            self.eof = from + end as u64 >= self.data.end;
            return Ok(true);
        }
    }

    /// 一次读进剩下的码流，找出所有的包（与逐个找的切法相同）
    fn scan_all(&mut self) {
        let from = self.ends.last().copied().unwrap_or(self.data.start);
        let mut all = vec![0u8; (self.data.end - from) as usize];
        let got = read_at(&mut self.file, from, &mut all).unwrap_or(0);
        all.truncate(got);
        let mut at = 0;
        while !self.eof {
            let mut last = self.last;
            let (start, end) = match find_frame(&all[at..], &mut last) {
                Some((s, e)) => (s, e.unwrap_or(all.len() - at)),
                None => {
                    self.eof = true;
                    break;
                }
            };
            self.last = last;
            let end = end.min(start + MAX_PACKET);
            self.starts.push(from + (at + start) as u64);
            self.ends.push(from + (at + end) as u64);
            at += end;
            self.eof = at >= all.len();
        }
    }

    /// 码率固定时推算出的第 k 个包的开头是不是同步字
    fn cbr_ok(&mut self, k: usize) -> bool {
        let Some(len) = self.cbr else { return false };
        let start = self.starts[0] + k as u64 * len;
        let mut h = [0u8; 6];
        if start + len > self.data.end || read_at(&mut self.file, start, &mut h).unwrap_or(0) < 6 {
            return false;
        }
        let s = h.iter().fold(0u64, |s, &b| s << 8 | b as u64);
        is_core_marker(s) && core_marker(s) == self.last
    }

    /// 第 k 个包的位置；没有这个包时返回 None
    pub fn range(&mut self, k: usize) -> Result<Option<(u64, u64)>> {
        if k >= self.starts.len() {
            if let Some(len) = self.cbr {
                // 不是最后一个包（后面还放得下一个包）时直接推算
                let start = self.starts[0] + k as u64 * len;
                if start + 2 * len <= self.data.end && self.cbr_ok(k) {
                    return Ok(Some((start, start + len)));
                }
                // 最后一个包（或核对不上）：前一个包对得上时，把推算出的包都记下，只从那里往后找
                if k > self.starts.len() && self.cbr_ok(k - 1) {
                    for i in self.starts.len()..k {
                        let s = self.starts[0] + i as u64 * len;
                        self.starts.push(s);
                        self.ends.push(s + len);
                    }
                }
            }
        }
        while k >= self.starts.len() {
            if !self.scan()? {
                return Ok(None);
            }
        }
        Ok(Some((self.starts[k], self.ends[k])))
    }

    /// 读第 k 个包
    pub fn packet(&mut self, k: usize, out: &mut Vec<u8>) -> Result<bool> {
        let Some((s, e)) = self.range(k)? else { return Ok(false) };
        out.resize((e - s) as usize, 0);
        let got = read_at(&mut self.file, s, out)?;
        out.truncate(got);
        Ok(true)
    }

    /// 估计总包数：码率固定时直接算；否则用开头的包和文件中间几处的包长平均
    pub fn est_packets(&mut self) -> u64 {
        let Some(&first) = self.starts.first() else { return 0 };
        let span = self.data.end - first;
        if let Some(len) = self.cbr {
            return span / len;
        }
        // 码率可变（DTS-HD）且不太大时，整个读一遍找出所有的包：总长准确，跳转也不用再找
        if span <= WHOLE_SCAN && !self.eof {
            self.scan_all();
        }
        if self.eof {
            return self.starts.len() as u64;
        }
        // 开头的包常常比平均小（片头安静），只用均匀分布在全文件的 64 处
        let mut lens = Vec::new();
        let mut buf = vec![0u8; 1 << 16];
        for i in 0..64u64 {
            let at = first + span * (2 * i + 1) / 128;
            let got = read_at(&mut self.file, at, &mut buf).unwrap_or(0);
            let mut last = self.last;
            if let Some((s, Some(e))) = find_frame(&buf[..got], &mut last) {
                lens.push((e - s) as u64);
            }
        }
        if lens.is_empty() {
            lens.extend(self.starts.iter().zip(&self.ends).map(|(s, e)| e - s));
        }
        let avg = lens.iter().sum::<u64>() as f64 / lens.len().max(1) as f64;
        (span as f64 / avg.max(1.0)).round() as u64
    }
}
