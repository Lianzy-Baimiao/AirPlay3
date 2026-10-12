//! 32 子带合成滤波器（改写自参考库 synth_filter.c，BSD 许可，见 mod.rs）：快速 DCT（Byeong Gi Lee）算出新的
//! 64 个 V 值，再按 ISO 11172-3 的窗加权求和。浮点运算的顺序与参考库完全一致，结果逐位相同。

use super::decoder::Decoder;
use super::tables::DI_OPT;

/// DCT 的一级蝶形：a[i] ± a[15 − i]，差乘以系数
fn stage(a: &[f32; 16], c: &[f32; 8]) -> [f32; 16] {
    let mut b = [0f32; 16];
    for i in 0..8 {
        b[i] = a[i] + a[15 - i];
        b[8 + i] = (a[i] - a[15 - i]) * c[i];
    }
    b
}

/// 第二级（每 8 个一组）
fn stage8(b: &[f32; 16], c: &[f32; 4]) -> [f32; 16] {
    let mut a = [0f32; 16];
    for g in 0..2 {
        let o = 8 * g;
        for i in 0..4 {
            a[o + i] = b[o + i] + b[o + 7 - i];
            a[o + 4 + i] = (b[o + i] - b[o + 7 - i]) * c[i];
        }
    }
    a
}

/// 第三级（每 4 个一组）
fn stage4(a: &[f32; 16]) -> [f32; 16] {
    let mut b = [0f32; 16];
    for g in 0..4 {
        let o = 4 * g;
        b[o] = a[o] + a[o + 3];
        b[o + 1] = a[o + 1] + a[o + 2];
        b[o + 2] = (a[o] - a[o + 3]) * 0.5411961079f32;
        b[o + 3] = (a[o + 1] - a[o + 2]) * 1.3065630198f32;
    }
    b
}

/// 第四级（每 2 个一组）
fn stage2(b: &[f32; 16]) -> [f32; 16] {
    let mut a = [0f32; 16];
    for g in 0..8 {
        a[2 * g] = b[2 * g] + b[2 * g + 1];
        a[2 * g + 1] = (b[2 * g] - b[2 * g + 1]) * 0.7071067691f32;
    }
    a
}

const C1: [f32; 8] = [0.5024192929, 0.5224986076, 0.5669440627, 0.6468217969, 0.7881546021, 1.0606776476, 1.7224471569, 5.1011486053];
const C2: [f32; 4] = [0.5097956061, 0.6013448834, 0.8999761939, 2.5629155636];
const C0: [f32; 16] = [
    0.5006030202, 0.5054709315, 0.5154473186, 0.5310425758, 0.5531039238, 0.5829349756, 0.6225041151, 0.6748083234, 0.7445362806,
    0.8393496275, 0.9725682139, 1.1694399118, 1.4841645956, 2.0577809811, 3.4076085091, 10.1900081635,
];

/// 32 个子带样本 → 64 个新的 V（参考库 mpc_compute_new_V，链式赋值按 C 的求值顺序展开）
fn new_v(s: &[f32; 32], v: &mut [f32]) {
    let a: [f32; 16] = std::array::from_fn(|i| s[i] + s[31 - i]);
    let a = stage2(&stage4(&stage8(&stage(&a, &C1), &C2)));
    v[48] = -a[0];
    v[0] = a[1];
    v[8] = a[3];
    v[40] = -a[2] - v[8];
    v[12] = a[7];
    v[4] = a[5] + v[12];
    v[36] = -(v[4] + a[6]);
    v[44] = -a[4] - a[6] - a[7];
    v[14] = a[15];
    v[10] = a[11] + v[14];
    v[6] = v[10] + a[13];
    v[2] = a[9] + a[13] + a[15];
    v[34] = -v[2] - a[14];
    v[38] = v[34] + a[9] - a[10] - a[11];
    let t = -(a[12] + a[14] + a[15]);
    v[46] = t - a[8];
    v[42] = t - a[10] - a[11];

    let a: [f32; 16] = std::array::from_fn(|i| (s[i] - s[31 - i]) * C0[i]);
    let a = stage2(&stage4(&stage8(&stage(&a, &C1), &C2)));
    v[15] = a[15];
    v[13] = a[7] + v[15];
    v[11] = v[13] + a[11];
    v[5] = v[11] + a[5] + a[13];
    v[9] = a[3] + a[11] + a[15];
    v[7] = v[9] + a[13];
    v[1] = a[1] + a[9] + a[13] + a[15];
    v[33] = -v[1] - a[14];
    v[3] = a[5] + a[7] + a[9] + a[13] + a[15];
    v[35] = -v[3] - a[6] - a[14];
    let mut t = -(a[10] + a[11] + a[13] + a[14] + a[15]);
    v[37] = t - a[5] - a[6] - a[7];
    v[39] = t - a[2] - a[3];
    t += a[13] - a[12];
    v[41] = t - a[2] - a[3];
    v[43] = t - a[4] - a[6] - a[7];
    let t = -(a[8] + a[12] + a[14] + a[15]);
    v[47] = t - a[0];
    v[45] = t - a[4] - a[6] - a[7];
    for i in 0..16 {
        v[32 - i] = -v[i];
    }
    for i in 0..15 {
        v[63 - i] = v[33 + i];
    }
}

/// 一个声道一帧：36 组 × 32 个样本，写到 `out[ch_off + k * stride]`
fn channel(v: &mut [f32], y: &[[f32; 32]; 36], out: &mut [f32], ch_off: usize, stride: usize) {
    v.copy_within(0..960, 2304);
    let mut p = 2304;
    for (n, ys) in y.iter().enumerate() {
        p -= 64;
        new_v(ys, &mut v[p..p + 64]);
        for k in 0..32 {
            let d = &DI_OPT[k];
            let w = |j: usize| v[p + k + j];
            let f = |x: i32| x as f32 / 65536.0;
            let s = w(0) * f(d[0]) + w(96) * f(d[1]) + w(128) * f(d[2]) + w(224) * f(d[3]) + w(256) * f(d[4]) + w(352) * f(d[5])
                + w(384) * f(d[6]) + w(480) * f(d[7]) + w(512) * f(d[8]) + w(608) * f(d[9]) + w(640) * f(d[10]) + w(736) * f(d[11])
                + w(768) * f(d[12]) + w(864) * f(d[13]) + w(896) * f(d[14]) + w(992) * f(d[15]);
            out[(n * 32 + k) * stride + ch_off] = s;
        }
    }
}

impl Decoder {
    /// 合成一帧到 `out`（交错，FRAME × 声道数）
    pub fn synthesize(&mut self, out: &mut [f32]) {
        let ch = self.channels;
        let [vl, vr] = &mut self.v;
        channel(vl, &self.y[0], out, 0, ch);
        if ch > 1 {
            channel(vr, &self.y[1], out, 1, ch);
        }
    }
}
