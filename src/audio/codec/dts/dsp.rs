// 改写自 FFmpeg 的 libavcodec/dcamath.h、dcadsp.c、synth_filter.c、dcaadpcm.h
// （Copyright (C) 2016 foo86；synth_filter.c 另有 Copyright (c) 2008 Michael Niedermayer），LGPL 2.1+，见 mod.rs 开头的说明。
//! 定点运算：舍入移位、23 位限幅、反量化、ADPCM 预测、LFE 插值、子带合成滤波

use super::tables::ADPCM_VB;

/// 加上半个单位后右移 bits 位（bits ≤ 0 时不移）
#[inline]
pub(super) fn norm(a: i64, bits: i32) -> i32 {
    if bits > 0 { (a.wrapping_add(1i64 << (bits - 1)) >> bits) as i32 } else { a as i32 }
}

#[inline]
pub(super) fn norm13(a: i64) -> i32 {
    norm(a, 13)
}

#[inline]
pub(super) fn norm16(a: i64) -> i32 {
    norm(a, 16)
}

#[inline]
pub(super) fn norm20(a: i64) -> i32 {
    norm(a, 20)
}

#[inline]
pub(super) fn norm21(a: i64) -> i32 {
    norm(a, 21)
}

#[inline]
pub(super) fn norm23(a: i64) -> i32 {
    norm(a, 23)
}

#[inline]
pub(super) fn mul(a: i32, b: i32, bits: i32) -> i32 {
    norm(a as i64 * b as i64, bits)
}

#[inline]
pub(super) fn mul15(a: i32, b: i32) -> i32 {
    mul(a, b, 15)
}

#[inline]
pub(super) fn mul16(a: i32, b: i32) -> i32 {
    mul(a, b, 16)
}

#[inline]
pub(super) fn mul17(a: i32, b: i32) -> i32 {
    mul(a, b, 17)
}

#[inline]
pub(super) fn mul22(a: i32, b: i32) -> i32 {
    mul(a, b, 22)
}

#[inline]
pub(super) fn mul23(a: i32, b: i32) -> i32 {
    mul(a, b, 23)
}

/// 限制在 24 位有符号数的范围内
#[inline]
pub(super) fn clip23(a: i32) -> i32 {
    a.clamp(-(1 << 23), (1 << 23) - 1)
}

/// 反量化（ff_dca_core_dequantize）：乘量化步长和比例因子；residual 时加到原值上
pub(super) fn dequantize(out: &mut [i32], input: &[i32], step_size: i32, scale: i32, residual: bool) {
    let mut step_scale = step_size as i64 * scale as i64;
    let mut shift = 0;
    // 比例因子的精度限制在 22 位
    if step_scale > 1 << 23 {
        shift = 31 - ((step_scale >> 23) as u32 | 1).leading_zeros() as i32 + 1;
        step_scale >>= shift;
    }
    for (o, &x) in out.iter_mut().zip(input) {
        let v = clip23(norm(x as i64 * step_scale, 22 - shift));
        *o = if residual { o.wrapping_add(v) } else { v };
    }
}

/// ADPCM 预测：hist 是前 4 个样本（时间顺序）
#[inline]
pub(super) fn adpcm_predict(pred_id: usize, hist: &[i32]) -> i32 {
    let c = &ADPCM_VB[pred_id];
    let p: i64 = (0..4).map(|i| hist[3 - i] as i64 * c[i] as i64).sum();
    clip23(norm13(p))
}

/// LFE 插值（64 倍）：lfe[start..] 是本帧的抽样值，之前有 7 个历史样本
pub(super) fn lfe_fir_fixed(pcm: &mut [i32], lfe: &[i32], start: usize, coeff: &[i32; 256], npcmblocks: usize) {
    for i in 0..npcmblocks / 2 {
        let s = start + i;
        let o = &mut pcm[i * 64..i * 64 + 64];
        for j in 0..32 {
            let (mut a, mut b) = (0i64, 0i64);
            for k in 0..8 {
                a += coeff[j * 8 + k] as i64 * lfe[s - k] as i64;
                b += coeff[255 - j * 8 - k] as i64 * lfe[s - k] as i64;
            }
            o[j] = clip23(norm23(a));
            o[32 + j] = clip23(norm23(b));
        }
    }
}

/// X96 时把 LFE 再插值一倍：读 buf[src..src+len]，写 buf[0..2*len]（原地，先读后写）
pub(super) fn lfe_x96_fixed(buf: &mut [i32], src: usize, len: usize, hist: &mut i32) {
    let mut prev = *hist;
    for i in 0..len {
        let s = buf[src + i];
        let a = 2097471i64 * s as i64 + 6291137i64 * prev as i64;
        let b = 6291137i64 * s as i64 + 2097471i64 * prev as i64;
        prev = s;
        buf[2 * i] = clip23(norm23(a));
        buf[2 * i + 1] = clip23(norm23(b));
    }
    *hist = prev;
}
