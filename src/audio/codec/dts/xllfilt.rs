// 改写自 FFmpeg 的 libavcodec/dca_xll.c、dcadsp.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! XLL 的重建：反预测（自适应 / 固定）、反去相关、MSB 与 LSB 合起来、两个频带合成一个

use super::dsp::{clip23, mul16, mul22, mul23, norm16};
use super::xll::*;

/// LSB 部分的位数
pub(super) fn lsb_width(fixed: u32, c: &ChSet, band: usize, ch: usize) -> u32 {
    let adj = c.bands[band].bit_width_adjust[ch];
    let shift = c.bands[band].nscalablelsbs[ch];
    if fixed != 0 {
        fixed
    } else if shift != 0 && adj != 0 {
        shift + adj - 1
    } else {
        shift + adj
    }
}

impl Xll {
    pub(super) fn filter_band(&mut self, ci: usize, band: usize) {
        let n = self.nframesamples;
        let c = &mut self.chset[ci];
        for ch in 0..c.nchannels {
            let bd = &c.bands[band];
            let (p, order) = (bd.msb[ch], bd.adapt_pred_order[ch]);
            let Some(buf) = c.msb_buf.get_mut(p..p + n) else { continue };
            if order > 0 {
                // 反射系数 → 直接型系数
                let mut coeff = [0i32; 16];
                for j in 0..order {
                    let rc = bd.adapt_refl_coeff[ch][j];
                    for k in 0..j.div_ceil(2) {
                        let (t1, t2) = (coeff[k], coeff[j - k - 1]);
                        coeff[k] = t1.wrapping_add(mul16(rc, t2));
                        coeff[j - k - 1] = t2.wrapping_add(mul16(rc, t1));
                    }
                    coeff[j] = rc;
                }
                // 反自适应预测
                for j in 0..n.saturating_sub(order) {
                    let err: i64 = (0..order).map(|k| buf[j + k] as i64 * coeff[order - k - 1] as i64).sum();
                    buf[j + order] = buf[j + order].wrapping_sub(clip23(norm16(err)));
                }
            } else {
                // 反固定预测（积分）
                for _ in 0..bd.fixed_pred_order[ch] {
                    for k in 1..n {
                        buf[k] = buf[k].wrapping_add(buf[k - 1]);
                    }
                }
            }
        }

        // 反成对去相关，再按原来的顺序排
        if c.bands[band].decor_enabled {
            for i in 0..c.nchannels / 2 {
                let coeff = c.bands[band].decor_coeff[i];
                if coeff != 0 {
                    let (d, s) = (c.bands[band].msb[i * 2 + 1], c.bands[band].msb[i * 2]);
                    for k in 0..n {
                        let x = c.msb_buf[s + k];
                        let v = (x.wrapping_mul(coeff).wrapping_add(1 << 2)) >> 3;
                        c.msb_buf[d + k] = c.msb_buf[d + k].wrapping_add(v);
                    }
                }
            }
            let (tmp, order) = (c.bands[band].msb, c.bands[band].orig_order);
            for (&p, &o) in tmp.iter().zip(&order).take(c.nchannels) {
                c.bands[band].msb[o] = p;
            }
        }

        if c.nfreqbands == 1 {
            for i in 0..c.nchannels {
                self.output[c.ch_remap[i]] = Out::Msb(ci, c.bands[band].msb[i]);
            }
        }
    }

    pub(super) fn assemble_msbs_lsbs(&mut self, ci: usize, band: usize) {
        let (n, fixed) = (self.nframesamples, self.fixed_lsb_width);
        let c = &mut self.chset[ci];
        for ch in 0..c.nchannels {
            let shift = lsb_width(fixed, c, band, ch);
            if shift == 0 {
                continue;
            }
            let bd = &c.bands[band];
            let (m, adj) = (bd.msb[ch], bd.bit_width_adjust[ch]);
            let lsb = if bd.nscalablelsbs[ch] != 0 { bd.lsb[ch] } else { None };
            for k in 0..n {
                let mut v = c.msb_buf[m + k].wrapping_mul(1i32.wrapping_shl(shift));
                if let Some(l) = lsb {
                    v = v.wrapping_add(c.lsb_buf[l + k].wrapping_shl(adj));
                }
                c.msb_buf[m + k] = v;
            }
        }
    }

    /// 两个频带合成一个（采样率加倍）：每个声道 2 × nframesamples 个样本
    pub(super) fn assemble_freq_bands(&mut self, ci: usize) {
        let n = self.nframesamples;
        let c = &mut self.chset[ci];
        if c.asm_buf.len() < 2 * n * c.nchannels {
            c.asm_buf.resize(2 * n * c.nchannels, 0);
        }
        // dst[i] -= mul(src[i], coeff)
        let f0 = |buf: &mut [i32], d: usize, s: usize, coeff: i32| {
            for i in 0..n {
                buf[d + i] = buf[d + i].wrapping_sub(mul22(buf[s + i], coeff));
            }
        };
        let f1 = |buf: &mut [i32], d: usize, s: usize, coeff: i32| {
            for i in 0..n {
                buf[d + i] = buf[d + i].wrapping_sub(mul23(buf[s + i], coeff));
            }
        };
        for ch in 0..c.nchannels {
            let (b0, b1) = (c.bands[0].msb[ch], c.bands[1].msb[ch]);
            c.msb_buf[b0 - DECI_HISTORY..b0].copy_from_slice(&c.deci_history[ch]);
            let buf = &mut c.msb_buf;
            f0(buf, b0, b1, BAND_COEFF[0]);
            f0(buf, b1, b0, BAND_COEFF[1]);
            f0(buf, b0, b1, BAND_COEFF[2]);
            f0(buf, b1, b0, BAND_COEFF[3]);
            for i in 0..8 {
                let s0 = b0 - i;
                f1(buf, s0, b1, BAND_COEFF[i + 4]);
                f1(buf, b1, s0, BAND_COEFF[i + 12]);
                f1(buf, s0, b1, BAND_COEFF[i + 4]);
            }
            // 交错：band1[i]、band0[i - 7]
            let out = 2 * n * ch;
            for i in 0..n {
                c.asm_buf[out + 2 * i] = c.msb_buf[b1 + i];
                c.asm_buf[out + 2 * i + 1] = c.msb_buf[b0 - 7 + i];
            }
            self.output[c.ch_remap[ch]] = Out::Asm(ci, out);
        }
    }
}
