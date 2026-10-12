//! WavPack 元数据：熵编码初值、混合模式码率、声道信息（照参考库 entropy_utils.c / open_utils.c，BSD 许可，见 mod.rs）。

use super::block::{HYBRID_BITRATE, Stream};
use super::tables::{exp2s_at, exp2u_at};

impl Stream {
    /// 熵编码的三个中值（每声道）
    pub(super) fn read_entropy(&mut self, d: &[u8]) -> bool {
        let n = if self.hdr.mono_data() { 1 } else { 2 };
        if d.len() != 6 * n {
            return false;
        }
        for (c, v) in self.words.c[..n].iter_mut().zip(d.chunks_exact(6)) {
            c.median = [exp2u_at(&v[0..]), exp2u_at(&v[2..]), exp2u_at(&v[4..])];
        }
        true
    }

    /// 混合模式：慢速电平（按码率时）、码率累加器、可选的码率增量
    pub(super) fn read_hybrid(&mut self, d: &[u8]) -> bool {
        let n = if self.hdr.mono_data() { 1 } else { 2 };
        let mut pos = 0;
        let w = &mut self.words;
        if self.hdr.flags & HYBRID_BITRATE != 0 {
            if d.len() < 2 * n {
                return false;
            }
            for i in 0..n {
                w.c[i].slow_level = exp2u_at(&d[2 * i..]);
            }
            pos = 2 * n;
        }
        if pos + 2 * n > d.len() {
            return false;
        }
        for i in 0..n {
            w.bitrate_acc[i] = (u16::from_le_bytes([d[pos + 2 * i], d[pos + 2 * i + 1]]) as u32) << 16;
        }
        pos += 2 * n;
        w.bitrate_delta = [0; 2];
        if pos < d.len() {
            if pos + 2 * n != d.len() {
                return false;
            }
            for i in 0..n {
                w.bitrate_delta[i] = exp2s_at(&d[pos + 2 * i..]) as u32;
            }
        }
        true
    }

    /// 总声道数和声道掩码（新格式 6 / 7 字节，旧格式 1 ~ 5 字节）
    pub(super) fn read_channel_info(&mut self, d: &[u8]) -> bool {
        if d.is_empty() || d.len() > 7 {
            return false;
        }
        let (n, streams, mask) = if d.len() >= 6 {
            let n = (d[0] as u32 | ((d[2] as u32 & 0xf) << 8)) + 1;
            let streams = (d[1] as u32 | ((d[2] as u32 & 0xf0) << 4)) + 1;
            let mut mask = d[3] as u32 | (d[4] as u32) << 8 | (d[5] as u32) << 16;
            if d.len() == 7 {
                mask |= (d[6] as u32) << 24;
            }
            (n, streams, mask)
        } else {
            let n = d[0] as u32;
            let mask = d[1..].iter().enumerate().fold(0u32, |m, (i, &b)| m | (b as u32) << (8 * i));
            (n, if n > 8 { n } else { 8 }, mask)
        };
        if n < streams && d.len() >= 6 || n > streams * 2 || mask.count_ones() > n {
            return false;
        }
        self.chan_info = Some((n, mask));
        true
    }
}
