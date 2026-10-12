//! 解一块：熵解码 → 各遍解相关 → 立体声还原 → CRC 校验 → 还原位深（移位、32 位整数的低位、浮点、
//! 有损时限幅）。改写自参考库 unpack.c（BSD 许可，见 mod.rs）。

use super::block::{BYTES_STORED, FALSE_STEREO, FLOAT_DATA, HYBRID, INT32_DATA, JOINT_STEREO, MAG_LSB, SHIFT_LSB, Stream};
use super::{decorr, floats};
use crate::audio::codec::bits::LsbReader;

impl Stream {
    /// 解这一块到 `out`（交错；单声道块 1 声道，其余 2 声道），返回是否正常。
    /// 码流坏了或 CRC 不对时与参考库一样整块静音（全 0）
    pub(super) fn unpack(&mut self, b: &[u8], out: &mut Vec<i32>) -> bool {
        let n = self.hdr.samples as usize;
        let mono = self.hdr.mono_data();
        out.clear();
        out.resize(n * self.hdr.channels(), 0);
        if !self.decode(b, &mut out[..if mono { n } else { 2 * n }]) {
            out.fill(0);
            return false;
        }
        if self.hdr.flags & FALSE_STEREO != 0 {
            for i in (0..n).rev() {
                out[2 * i] = out[i];
                out[2 * i + 1] = out[i];
            }
        }
        true
    }

    fn decode(&mut self, b: &[u8], buf: &mut [i32]) -> bool {
        let flags = self.hdr.flags;
        let mono = self.hdr.mono_data();
        let Some((s, e)) = self.wv else { return false };
        let mut r = LsbReader::new(&b[s..e]);
        for (i, x) in buf.iter_mut().enumerate() {
            match self.words.get(&mut r, if mono { 0 } else { i & 1 }, flags) {
                Some(v) => *x = v,
                None => return false,
            }
        }
        if r.overrun {
            return false;
        }
        for p in &mut self.passes[..self.nterms] {
            if mono { decorr::mono(p, buf) } else { decorr::stereo(p, buf) }
        }
        // 明显超出原始位数的样本说明数据坏了（混合模式放宽一些）
        let mut limit = (1i64 << ((flags >> MAG_LSB) & 0x1f)) + 2;
        if flags & HYBRID != 0 {
            limit = limit * 3 + 128;
        }
        let mut crc = u32::MAX;
        if mono {
            for &x in buf.iter() {
                if (x as i64).abs() > limit {
                    return false;
                }
                crc = crc.wrapping_mul(3).wrapping_add(x as u32);
            }
        } else {
            let joint = flags & JOINT_STEREO != 0;
            for x in buf.chunks_exact_mut(2) {
                if joint {
                    x[1] = x[1].wrapping_sub(x[0] >> 1);
                    x[0] = x[0].wrapping_add(x[1]);
                }
                let (l, r) = (x[0] as u32, x[1] as u32);
                crc = crc.wrapping_add((crc << 3).wrapping_add(l << 1).wrapping_add(l).wrapping_add(r));
            }
            // 与参考库相同：每 8 个采样点抽查一个
            if buf.chunks(16).any(|c| (c[0] as i64).abs() > limit || (c[1] as i64).abs() > limit) {
                return false;
            }
        }
        let extra = self.fixup(b, buf);
        crc == self.hdr.crc && extra.is_none_or(|(got, want)| got == want)
    }

    /// 还原位深；有额外码流时返回它的（算出的 CRC，块里存的 CRC）
    fn fixup(&mut self, b: &[u8], buf: &mut [i32]) -> Option<(u32, u32)> {
        let flags = self.hdr.flags;
        let mut wvx = self.wvx.map(|(s, e, crc, new)| (LsbReader::new(&b[s..e]), crc, new));
        // 新格式的额外码流开头有一两个 5 位的参数
        let (mut min_zeros, mut max_ones, mut max_width) = (0, 0, 0);
        if let Some((r, _, true)) = wvx.as_mut() {
            if flags & FLOAT_DATA != 0 {
                min_zeros = r.bits(5);
                max_ones = r.bits(5);
            } else {
                max_width = r.bits(5);
            }
        }
        if flags & FLOAT_DATA != 0 {
            return match wvx.as_mut() {
                Some((r, want, _)) => Some((floats::with_wvx(self.float, r, buf, min_zeros, max_ones), *want)),
                None => {
                    floats::plain(self.float, buf);
                    None
                }
            };
        }
        let lossy = flags & HYBRID != 0;
        let mut shift = (flags >> SHIFT_LSB) & 0x1f;
        let mut extra = None;
        if flags & INT32_DATA != 0 {
            let [sent, zeros, ones, dups] = self.int32.map(|v| (v & 0x1f) as u32);
            if let Some((r, want, _)) = wvx.as_mut() {
                let mut crc = u32::MAX;
                for x in buf.iter_mut() {
                    if sent > 0 {
                        *x = extra_bits(r, *x, sent, max_width);
                    }
                    *x = restore_lsbs(*x, zeros, ones, dups);
                    crc = crc.wrapping_mul(9).wrapping_add((*x & 0xffff) as u32 * 3).wrapping_add(((*x >> 16) & 0xffff) as u32);
                }
                extra = Some((crc, *want));
            } else if sent == 0 && zeros + ones + dups > 0 {
                let (mut z, mut o, mut d) = (zeros, ones, dups);
                // 有损的 32 位：把去掉的低位算进移位里，这样限幅才对
                while lossy && flags & BYTES_STORED == 3 && shift < 8 {
                    if z > 0 {
                        z -= 1;
                    } else if o > 0 {
                        o -= 1;
                    } else if d > 0 {
                        d -= 1;
                    } else {
                        break;
                    }
                    shift += 1;
                }
                for x in buf.iter_mut() {
                    *x = restore_lsbs(*x, z, o, d);
                }
            } else {
                shift += zeros + sent + ones + dups;
            }
        }
        shift &= 0x1f;
        if lossy {
            let (min, max) = match flags & BYTES_STORED {
                0 => (-128i32, 127i32),
                1 => (-32768, 32767),
                2 => (-8388608, 8388607),
                _ => (i32::MIN, i32::MAX),
            };
            let (min, max) = (min >> shift, max >> shift);
            let (min_s, max_s) = (((min as u32) << shift) as i32, max << shift);
            for x in buf.iter_mut() {
                *x = if *x < min { min_s } else if *x > max { max_s } else { ((*x as u32) << shift) as i32 };
            }
        } else if shift > 0 {
            for x in buf.iter_mut() {
                *x = ((*x as u32) << shift) as i32;
            }
        }
        extra
    }
}

/// 32 位整数：从额外码流补上低位（有 max_width 时，大的样本超出部分的低位没有存，补 0）
fn extra_bits(r: &mut LsbReader, x: i32, sent: u32, max_width: u32) -> i32 {
    let v = x as u32;
    if max_width == 0 {
        return ((v << sent) | r.bits(sent)) as i32;
    }
    let magnitude = (if x < 0 { !x } else { x }) as u32;
    let width = 32 - magnitude.leading_zeros() + sent;
    let n = if width <= max_width { sent } else { sent.saturating_sub(width - max_width) };
    if n > 0 { (((v << n) | r.bits(n)) << (sent - n)) as i32 } else { (v << sent) as i32 }
}

/// 编码时去掉的低位：全 0、全 1、或都与最低位相同
fn restore_lsbs(x: i32, zeros: u32, ones: u32, dups: u32) -> i32 {
    if zeros > 0 {
        ((x as u32) << zeros) as i32
    } else if ones > 0 {
        ((x.wrapping_add(1) as u32) << ones).wrapping_sub(1) as i32
    } else if dups > 0 {
        let lsb = x & 1;
        ((x.wrapping_add(lsb) as u32) << dups).wrapping_sub(lsb as u32) as i32
    } else {
        x
    }
}
