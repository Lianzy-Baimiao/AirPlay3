//! 数字信号处理：10 段均衡器（RBJ peaking biquad）、音量曲线与渐变、声道适配。

use crate::config::EqSettings;

/// 均衡器频点（与原版界面一致：32 64 125 250 500 1K 2K 4K 8K 16K）
pub const EQ_FREQS: [f64; 10] = [32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
const EQ_Q: f64 = 1.414;

#[derive(Clone, Copy, Default)]
struct Coef {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

fn peaking(rate: f64, f0: f64, gain_db: f64) -> Coef {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * f0 / rate;
    let alpha = w0.sin() / (2.0 * EQ_Q);
    let cos = w0.cos();
    let a0 = 1.0 + alpha / a;
    Coef {
        b0: (1.0 + alpha * a) / a0,
        b1: -2.0 * cos / a0,
        b2: (1.0 - alpha * a) / a0,
        a1: -2.0 * cos / a0,
        a2: (1.0 - alpha / a) / a0,
    }
}

pub struct Equalizer {
    coefs: Vec<Coef>,
    /// [声道][频段] 的 (z1, z2)
    state: Vec<Vec<(f64, f64)>>,
    preamp: f32,
    pub active: bool,
}

impl Equalizer {
    pub fn new(eq: &EqSettings, rate: u32, channels: usize) -> Equalizer {
        let mut coefs = Vec::new();
        if eq.enabled {
            for (f, g) in EQ_FREQS.iter().zip(eq.bands.iter()) {
                // 增益为 0 或接近奈奎斯特频率的频段跳过
                if g.abs() > 0.05 && *f < rate as f64 * 0.45 {
                    coefs.push(peaking(rate as f64, *f, *g as f64));
                }
            }
        }
        let preamp = if eq.enabled { 10f32.powf(eq.preamp_db / 20.0) } else { 1.0 };
        let active = !coefs.is_empty() || (preamp - 1.0).abs() > 1e-4;
        Equalizer { state: vec![vec![(0.0, 0.0); coefs.len()]; channels], coefs, preamp, active }
    }

    pub fn process(&mut self, buf: &mut [f32], channels: usize) {
        if !self.active || channels == 0 || self.state.len() != channels {
            return;
        }
        for frame in buf.chunks_exact_mut(channels) {
            for (ch, s) in frame.iter_mut().enumerate() {
                let mut x = (*s * self.preamp) as f64;
                for (c, z) in self.coefs.iter().zip(self.state[ch].iter_mut()) {
                    let y = c.b0 * x + z.0;
                    z.0 = c.b1 * x - c.a1 * y + z.1;
                    z.1 = c.b2 * x - c.a2 * y;
                    x = y;
                }
                *s = x as f32;
            }
        }
    }
}

/// 音量滑块 (0..1) → 线性增益。平方曲线让小音量段更细腻。
pub fn volume_gain(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    v * v
}

/// 按缓冲区线性渐变增益，避免调音量/开始播放时的爆音。返回结束时的增益。
pub fn apply_gain_ramp(buf: &mut [f32], channels: usize, from: f32, to: f32) -> f32 {
    let frames = buf.len() / channels.max(1);
    if frames == 0 {
        return from;
    }
    if (from - to).abs() < 1e-6 {
        if (to - 1.0).abs() > 1e-6 {
            buf.iter_mut().for_each(|s| *s *= to);
        }
        return to;
    }
    for (i, frame) in buf.chunks_exact_mut(channels).enumerate() {
        let g = from + (to - from) * (i + 1) as f32 / frames as f32;
        frame.iter_mut().for_each(|s| *s *= g);
    }
    to
}

/// 声道适配（独占模式下设备不支持源声道数时）：单声道复制、多声道简单下混到立体声。
pub fn adapt_channels(input: &[f32], in_ch: usize, out_ch: usize, out: &mut Vec<f32>) {
    out.clear();
    if in_ch == out_ch {
        out.extend_from_slice(input);
        return;
    }
    for frame in input.chunks_exact(in_ch) {
        let (l, r) = match in_ch {
            1 => (frame[0], frame[0]),
            _ => {
                // L R C LFE ... ：中置以 -3dB 混入两侧，其余偶数声道归左、奇数归右
                let c = if in_ch >= 3 { frame[2] * 0.707 } else { 0.0 };
                let (mut l, mut r) = (frame[0] + c, frame[1] + c);
                for (i, s) in frame.iter().enumerate().skip(4) {
                    if i % 2 == 0 { l += s * 0.707 } else { r += s * 0.707 }
                }
                (l * 0.6, r * 0.6)
            }
        };
        for ch in 0..out_ch {
            out.push(if ch % 2 == 0 { l } else { r });
        }
    }
}
