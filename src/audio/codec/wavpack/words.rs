//! WavPack 的熵解码（改写自参考库 read_words.c / entropy_utils.c，BSD 许可，见 mod.rs）：
//! 按三个自适应中值把残差的大小分成几段，一元码选段、段内用最少的位数编码；
//! 两个声道的中值都接近 0 时，连续的 0 用游程表示；混合（有损）模式按码率限制每个样本的误差。

use super::block::{HYBRID, HYBRID_BALANCE, HYBRID_BITRATE, MONO_DATA};
use super::tables::{exp2s, log2};
use crate::audio::codec::bits::LsbReader;

#[derive(Clone, Copy, Default)]
pub(super) struct Entropy {
    pub median: [u32; 3],
    pub slow_level: u32,
    pub error_limit: u32,
}

#[derive(Default)]
pub(super) struct Words {
    pub c: [Entropy; 2],
    pub bitrate_delta: [u32; 2],
    pub bitrate_acc: [u32; 2],
    holding_one: u32,
    holding_zero: bool,
    zeros_acc: u32,
}

const LIMIT_ONES: u32 = 16;
const SLS: u32 = 8;
const SLO: u32 = 1 << (SLS - 1);
const DIV: [u32; 3] = [128, 64, 32];

fn med(c: &Entropy, i: usize) -> u32 {
    (c.median[i] >> 4) + 1
}
fn inc(c: &mut Entropy, i: usize) {
    c.median[i] = c.median[i].wrapping_add((c.median[i].wrapping_add(DIV[i]) / DIV[i]).wrapping_mul(5));
}
fn dec(c: &mut Entropy, i: usize) {
    c.median[i] = c.median[i].wrapping_sub((c.median[i].wrapping_add(DIV[i] - 2) / DIV[i]).wrapping_mul(2));
}
fn decay(c: &mut Entropy) {
    c.slow_level = c.slow_level.wrapping_sub(c.slow_level.wrapping_add(SLO) >> SLS);
}

/// 在 0..=maxcode 里读一个值（不是 2 的整数次幂个值时，较小的那部分少用一位）
fn read_code(r: &mut LsbReader, maxcode: u32) -> u32 {
    if maxcode < 2 {
        return if maxcode == 1 { r.bits(1) } else { 0 };
    }
    let n = 32 - maxcode.leading_zeros();
    let extras = ((1u64 << n) - maxcode as u64 - 1) as u32;
    let code = r.bits(n - 1);
    if code >= extras { (code << 1) - extras + r.bits(1) } else { code }
}

/// 变长的计数：k 个 1 + 0，再 k-1 位（最高位的 1 省略）；读满 33 个 1 = 数据坏了
fn read_escape(r: &mut LsbReader) -> Option<u32> {
    let k = r.ones(33);
    match k {
        33 => None,
        0 | 1 => Some(k),
        _ => Some(r.bits(k - 1) | 1 << (k - 1)),
    }
}

impl Words {
    /// 读一个残差（`chan` 是立体声的第几声道）；None = 数据坏了或读到头
    pub fn get(&mut self, r: &mut LsbReader, chan: usize, flags: u32) -> Option<i32> {
        if self.c[0].median[0] < 2 && self.c[1].median[0] < 2 && !self.holding_zero && self.holding_one == 0 {
            if self.zeros_acc > 0 {
                self.zeros_acc -= 1;
                if self.zeros_acc > 0 {
                    decay(&mut self.c[chan]);
                    return Some(0);
                }
            } else {
                self.zeros_acc = read_escape(r)?;
                if self.zeros_acc > 0 {
                    decay(&mut self.c[chan]);
                    self.c[0].median = [0; 3];
                    self.c[1].median = [0; 3];
                    return Some(0);
                }
            }
        }
        let ones = if self.holding_zero {
            self.holding_zero = false;
            0
        } else {
            let mut n = r.ones(LIMIT_ONES + 1);
            if n == LIMIT_ONES + 1 {
                return None;
            }
            if n == LIMIT_ONES {
                n = read_escape(r)?.wrapping_add(LIMIT_ONES);
            }
            let held = self.holding_one;
            self.holding_one = n & 1;
            self.holding_zero = self.holding_one == 0;
            (n >> 1) + held
        };
        let hybrid = flags & HYBRID != 0;
        if hybrid && chan == 0 {
            self.update_error_limit(flags);
        }
        let c = &mut self.c[chan];
        let (mut low, mut high);
        if ones == 0 {
            low = 0;
            high = med(c, 0) - 1;
            dec(c, 0);
        } else {
            low = med(c, 0);
            inc(c, 0);
            if ones == 1 {
                high = low.wrapping_add(med(c, 1)).wrapping_sub(1);
                dec(c, 1);
            } else {
                low = low.wrapping_add(med(c, 1));
                inc(c, 1);
                if ones == 2 {
                    high = low.wrapping_add(med(c, 2)).wrapping_sub(1);
                    dec(c, 2);
                } else {
                    low = low.wrapping_add((ones - 2).wrapping_mul(med(c, 2)));
                    high = low.wrapping_add(med(c, 2)).wrapping_sub(1);
                    inc(c, 2);
                }
            }
        }
        let mid = if !hybrid {
            low.wrapping_add(read_code(r, high.wrapping_sub(low)))
        } else {
            low &= 0x7fff_ffff;
            high = (high & 0x7fff_ffff).max(low);
            if c.error_limit == 0 {
                read_code(r, high - low) + low
            } else {
                let mut mid = (high + low + 1) >> 1;
                while high - low > c.error_limit {
                    if r.bit() {
                        low = mid;
                    } else {
                        high = mid - 1;
                    }
                    mid = (high + low + 1) >> 1;
                }
                mid
            }
        };
        let neg = r.bit();
        if flags & HYBRID_BITRATE != 0 {
            decay(c);
            c.slow_level = c.slow_level.wrapping_add(log2(mid) as u32);
        }
        Some(if neg { !mid as i32 } else { mid as i32 })
    }

    /// 混合模式：按码率（和慢速电平）算出这个样本允许的误差
    fn update_error_limit(&mut self, flags: u32) {
        let mut br = [0i32; 2];
        let stereo = flags & MONO_DATA == 0;
        let n = 1 + stereo as usize;
        for ((acc, delta), b) in self.bitrate_acc[..n].iter_mut().zip(self.bitrate_delta).zip(&mut br) {
            *acc = acc.wrapping_add(delta);
            *b = (*acc >> 16) as i32;
        }
        if flags & HYBRID_BITRATE == 0 {
            for (c, &b) in self.c[..n].iter_mut().zip(&br) {
                c.error_limit = exp2s(b) as u32;
            }
            return;
        }
        let slow = [0, 1].map(|i| (self.c[i].slow_level.wrapping_add(SLO) >> SLS) as i32);
        if stereo && flags & HYBRID_BALANCE != 0 {
            let balance = (slow[1] - slow[0] + br[1] + 1) >> 1;
            br = if balance > br[0] {
                [0, br[0] * 2]
            } else if -balance > br[0] {
                [br[0] * 2, 0]
            } else {
                [br[0] - balance, br[0] + balance]
            };
        }
        for ((c, s), b) in self.c[..n].iter_mut().zip(slow).zip(br) {
            let d = s - b;
            c.error_limit = if d > -0x100 { exp2s(d + 0x100) as u32 } else { 0 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 低位在前的写入器 + 与参考库编码器相同的 `write_code`，检验 `read_code` 能原样读回
    struct W(Vec<u8>, u32);
    impl W {
        fn put(&mut self, v: u32, n: u32) {
            for k in 0..n {
                if self.1 % 8 == 0 {
                    self.0.push(0);
                }
                *self.0.last_mut().unwrap() |= (((v >> k) & 1) as u8) << (self.1 % 8);
                self.1 += 1;
            }
        }
        fn code(&mut self, v: u32, maxcode: u32) {
            if maxcode < 2 {
                self.put(v, maxcode);
                return;
            }
            let n = 32 - maxcode.leading_zeros();
            let extras = (1u32 << n) - maxcode - 1;
            if v < extras {
                self.put(v, n - 1);
            } else {
                let t = v + extras;
                self.put(t >> 1, n - 1);
                self.put(t & 1, 1);
            }
        }
    }

    #[test]
    fn code_roundtrip() {
        for maxcode in 0..300u32 {
            let mut w = W(Vec::new(), 0);
            for v in 0..=maxcode {
                w.code(v, maxcode);
            }
            let mut r = LsbReader::new(&w.0);
            for v in 0..=maxcode {
                assert_eq!(read_code(&mut r, maxcode), v, "maxcode {maxcode}");
            }
            assert_eq!(r.position(), w.1 as usize);
        }
    }

    #[test]
    fn escape_roundtrip() {
        for v in [0u32, 1, 2, 3, 7, 8, 1000, 1 << 20, u32::MAX >> 1] {
            let mut w = W(Vec::new(), 0);
            let k = 32 - v.leading_zeros();
            w.put((1u32 << k) - 1, k);
            w.put(0, 1);
            if k >= 2 {
                w.put(v, k - 1);
            }
            assert_eq!(read_escape(&mut LsbReader::new(&w.0)), Some(v));
        }
        assert_eq!(read_escape(&mut LsbReader::new(&[0xff; 8])), None);
    }
}
