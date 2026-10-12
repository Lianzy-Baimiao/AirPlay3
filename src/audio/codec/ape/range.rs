//! 残差的取值（改写自参考库 UnBitArray::DecodeValueRange 与 UnBitArray3891To3989，BSD 许可，见 mod.rs）：
//! 先用区间编码取「溢出」部分，再取余下的低位；每个声道一个自适应状态（k、kSum），每帧开头重置。

use super::bits::{Bits, RANGE_TOTAL_1, RANGE_TOTAL_2};

/// kSum 的下限：第 k 档 = 2^(k+4)（第 0 档 0，27 档以上没有）
fn min_boundary(k: u32) -> u32 {
    match k {
        0 => 0,
        1..=27 => 1 << (k + 4),
        _ => 0,
    }
}

#[derive(Clone, Copy)]
pub(super) struct State {
    k: u32,
    ksum: u32,
}

impl State {
    pub fn new() -> Self {
        State { k: 10, ksum: (1 << 10) * 16 }
    }

    /// 更新自适应状态，把无符号的码值转成有符号的残差
    fn update(&mut self, v: i64) -> i64 {
        self.ksum = self.ksum.wrapping_add(((v + 1) / 2) as u32).wrapping_sub(self.ksum.wrapping_add(16) >> 5);
        if self.ksum < min_boundary(self.k) {
            self.k = self.k.saturating_sub(1);
        } else if min_boundary(self.k + 1) != 0 && self.ksum >= min_boundary(self.k + 1) {
            self.k += 1;
        }
        if v & 1 != 0 { (v >> 1) + 1 } else { -(v >> 1) }
    }
}

/// 3.99 起：按 kSum 定一个「枢轴」，值 = 溢出 × 枢轴 + 余数（余数在 0 ~ 枢轴 之间均匀分布）
pub(super) fn value(b: &mut Bits, s: &mut State) -> i64 {
    let mut pivot = (s.ksum / 32).max(1);
    let overflow = overflow(b, &mut pivot);
    let base = if pivot >= 1 << 16 {
        // 枢轴太大时分两段取
        let shift = 32 - pivot.leading_zeros() - 16;
        let split = 1u32 << shift;
        let hi = b.divide(|r| r / ((pivot >> shift) + 1));
        let lo = b.divide(|r| r / split);
        hi.wrapping_mul(split).wrapping_add(lo)
    } else {
        b.divide(|r| r / pivot)
    };
    s.update(base as i64 + overflow as i64 * pivot as i64)
}

/// 溢出部分：63 号符号是转义，后面是 32 位的原值；原值为 1 表示枢轴改成 32768 再取一次
fn overflow(b: &mut Bits, pivot: &mut u32) -> u32 {
    loop {
        if b.over {
            return 0;
        }
        let s = b.symbol(&RANGE_TOTAL_2);
        if s != 63 {
            return s as u32;
        }
        let hi = b.take(16);
        let lo = b.take(16);
        let v = (hi << 16) | lo;
        if v != 1 {
            return v;
        }
        *pivot = 32768;
    }
}

/// 3.90 ~ 3.98：溢出 × 2^k + k 位低位（k 跟着 kSum 变；63 号符号后面直接给 k）
pub(super) fn value_old(b: &mut Bits, s: &mut State, version: u16) -> i64 {
    let sym = b.symbol(&RANGE_TOTAL_1);
    let (k, overflow) = if sym == 63 { (b.take(5), 0) } else { (s.k.saturating_sub(1), sym as i64) };
    let mut v = if k <= 16 || version < 3910 {
        b.take(k) as i64
    } else {
        let lo = b.take(16);
        let hi = b.take(k - 16);
        lo as i64 | ((hi as i64) << 16)
    };
    v += overflow << k;
    s.update(v)
}
