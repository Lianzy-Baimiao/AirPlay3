//! 32 位浮点：主码流里是对齐到同一指数的 24 位整数尾数，这里拼回 IEEE 单精度的位（只动尾数、指数、
//! 符号，不做浮点运算）；无损时被移出的低位、0 的符号、Inf / NaN 在额外码流里。改写自参考库
//! unpack_floats.c（BSD 许可，见 mod.rs）。结果仍以 i32 存（f32 的位）。

use crate::audio::codec::bits::LsbReader;

const SHIFT_ONES: u8 = 1;
const SHIFT_SAME: u8 = 2;
const SHIFT_SENT: u8 = 4;
const ZEROS_SENT: u8 = 8;
const NEG_ZEROS: u8 = 0x10;

/// FLOAT_INFO 的四个字节：标志、移位、最大指数、归一化指数
fn info(f: [u8; 4]) -> (u8, u32, i32) {
    (f[0], (f[1] & 0x1f) as u32, f[2] as i32)
}

fn pack(sign: bool, exp: i32, mantissa: u32) -> i32 {
    ((sign as u32) << 31 | ((exp as u32) & 0xff) << 23 | (mantissa & 0x7f_ffff)) as i32
}

/// 有损（没有额外码流）
pub(super) fn plain(f: [u8; 4], buf: &mut [i32]) {
    let (flags, shift, max_exp) = info(f);
    for x in buf.iter_mut() {
        if *x == 0 {
            continue;
        }
        let mut v = ((*x as u32) << shift) as i32;
        let neg = v < 0;
        if neg {
            v = v.wrapping_neg();
        }
        let mut exp = max_exp;
        if v >= 0x100_0000 {
            while v & 0xf00_0000 != 0 {
                v >>= 1;
                exp += 1;
            }
        } else if exp > 0 {
            let mut n = 0;
            while v & 0x80_0000 == 0 {
                exp -= 1;
                if exp == 0 {
                    break;
                }
                n += 1;
                v = ((v as u32) << 1) as i32;
            }
            n &= 0x1f;
            if n > 0 && flags & SHIFT_ONES != 0 {
                v |= ((1u32 << n) - 1) as i32;
            }
        }
        *x = pack(neg, exp, v as u32);
    }
}

/// 无损（有额外码流），返回额外码流的 CRC
pub(super) fn with_wvx(f: [u8; 4], r: &mut LsbReader, buf: &mut [i32], min_zeros: u32, max_ones: u32) -> u32 {
    let (flags, shift, max_exp) = info(f);
    let mut crc = u32::MAX;
    for x in buf.iter_mut() {
        let mut exp = max_exp;
        let (mut neg, mut e, mut m) = (false, 0i32, 0u32);
        if *x == 0 {
            if flags & ZEROS_SENT != 0 {
                if r.bit() {
                    m = r.bits(23);
                    if exp >= 25 {
                        e = r.bits(8) as i32;
                    }
                    neg = r.bit();
                } else if flags & NEG_ZEROS != 0 {
                    neg = r.bit();
                }
            }
        } else {
            let mut v = ((*x as u32) << shift) as i32;
            if v < 0 {
                v = v.wrapping_neg();
                neg = true;
            }
            if v == 0x100_0000 {
                // Inf / NaN
                if r.bit() {
                    m = r.bits(23);
                }
                e = 255;
            } else {
                let mut n = 0u32;
                if exp > 0 {
                    while v & 0x80_0000 == 0 {
                        exp -= 1;
                        if exp == 0 {
                            break;
                        }
                        n += 1;
                        v = ((v as u32) << 1) as i32;
                    }
                }
                n &= 0x1f;
                if n > 0 {
                    if flags & SHIFT_ONES != 0 || (flags & SHIFT_SAME != 0 && r.bit()) {
                        v |= ((1u32 << n) - 1) as i32;
                    } else if flags & SHIFT_SENT != 0 {
                        let mask = (1u32 << n) - 1;
                        let mut zeros = 0;
                        if max_ones > 0 && n > max_ones {
                            zeros = n - max_ones;
                        }
                        if min_zeros > zeros {
                            zeros = min_zeros.min(n);
                        }
                        if n > zeros {
                            v |= ((r.bits(n - zeros) << zeros) & mask) as i32;
                        }
                    }
                }
                m = v as u32;
                e = exp;
            }
        }
        let out = pack(neg, e, m) as u32;
        crc = crc.wrapping_mul(27).wrapping_add((out & 0x7f_ffff).wrapping_mul(9)).wrapping_add(((out >> 23) & 0xff) * 3).wrapping_add(out >> 31);
        *x = out as i32;
    }
    crc
}
