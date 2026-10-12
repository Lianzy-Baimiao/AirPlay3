//! 3.80 ~ 3.89 的自适应 Rice 码（改写自参考库 UnBitArrayOld，BSD 许可，见 mod.rs）。
//! 一帧里每个声道一整串残差，按 kSum 自适应地选 k。

use super::bits::Bits;

/// 3.86 以前：k 按前 64 个值的和选，档位表（下限 / 上限）
fn min_old(k: usize) -> u32 {
    match k {
        0 => 0,
        1..=25 => 1 << (k + 6),
        _ => 0,
    }
}
fn max_old(k: usize) -> u32 {
    if k <= 24 { 1 << (k + 7) } else { 0 }
}
/// 3.86 ~ 3.89 的档位表
fn min_new(k: u32) -> u32 {
    match k {
        0 => 0,
        1..=27 => 1 << (k + 4),
        _ => 0,
    }
}
fn max_new(k: u32) -> u32 {
    if k <= 26 { 1 << (k + 5) } else { 0 }
}

/// 无符号码值 → 有符号残差（奇数为正）
fn signed_i(v: i32) -> i32 {
    if v & 1 != 0 { (v >> 1).wrapping_add(1) } else { (v >> 1).wrapping_neg() }
}
fn signed_u(v: u32) -> i32 {
    let h = (v >> 1) as i32;
    if v & 1 != 0 { h.wrapping_add(1) } else { h.wrapping_neg() }
}

/// Rice 码：一串 0 是高位，再 k 位低位
fn rice(b: &mut Bits, k: u32) -> u32 {
    let q = b.zeros();
    if k == 0 {
        return q;
    }
    let k = k.min(32);
    q.wrapping_shl(k) | b.bits(k)
}

/// 值的位数（参考库 Get_K）
fn bit_len(x: u32) -> u32 {
    32 - x.leading_zeros()
}

/// 记下第一次读过头的位置
fn mark(b: &Bits, i: usize, n: usize, valid: &mut usize) {
    if b.over && *valid == n {
        *valid = i;
    }
}

/// 3.86 以前；返回数据读完以前解出的个数
pub(super) fn array_old(b: &mut Bits, out: &mut [i32]) -> usize {
    let n = out.len();
    let mut valid = n;
    for i in 0..n.min(5) {
        out[i] = rice(b, 10) as i32;
        mark(b, i, n, &mut valid);
    }
    if n <= 5 {
        out.iter_mut().for_each(|x| *x = signed_i(*x));
        return valid;
    }
    let mut ksum = out[..5].iter().fold(0u32, |s, &x| s.wrapping_add(x as u32));
    let mut k = bit_len(ksum / 10);
    for q in 5..n.min(64) {
        out[q] = rice(b, k) as i32;
        mark(b, q, n, &mut valid);
        ksum = ksum.wrapping_add(out[q] as u32);
        k = bit_len(ksum / (q as u32 + 1) / 2);
    }
    if n <= 64 {
        out.iter_mut().for_each(|x| *x = signed_i(*x));
        return valid;
    }
    let mut k = bit_len(ksum >> 7) as usize;
    let (mut kmin, mut kmax) = (min_old(k), max_old(k));
    for i in 64..n {
        out[i] = rice(b, k as u32) as i32;
        mark(b, i, n, &mut valid);
        ksum = ksum.wrapping_add(out[i].wrapping_sub(out[i - 64]) as u32);
        out[i - 64] = signed_i(out[i - 64]);
        if ksum < kmin {
            while k > 0 {
                k -= 1;
                if ksum >= min_old(k) {
                    break;
                }
            }
        } else if ksum >= kmax {
            while max_old(k + 1) != 0 {
                k += 1;
                if ksum < max_old(k) {
                    break;
                }
            }
        }
        (kmin, kmax) = (min_old(k), max_old(k));
    }
    out[n - 64..].iter_mut().for_each(|x| *x = signed_i(*x));
    valid
}

/// 3.86 ~ 3.89：k 每个值更新一次；3.89 起一串 0 太长时 k 加大。返回数据读完以前解出的个数
pub(super) fn array_rice(b: &mut Bits, out: &mut [i32], version: u16) -> usize {
    let (mut k, mut ksum) = (10u32, 1024u32 * 16);
    let n = out.len();
    let mut valid = n;
    for (i, x) in out.iter_mut().enumerate() {
        let mut ov = b.zeros();
        if version > 3880 {
            while ov >= 16 {
                k += 4;
                ov -= 16;
            }
        }
        let v = if k != 0 {
            let kk = k.min(32);
            ov.wrapping_shl(kk) | b.bits(kk)
        } else {
            ov
        };
        ksum = ksum.wrapping_add(v).wrapping_sub(ksum.wrapping_add(8) >> 4);
        k = k.min(31);
        if ksum < min_new(k) {
            k = k.saturating_sub(1);
        } else if max_new(k) != 0 && ksum >= max_new(k) {
            k += 1;
        }
        *x = signed_u(v);
        if b.over && valid == n {
            valid = i;
        }
    }
    valid
}
