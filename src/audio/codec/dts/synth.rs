// 改写自 FFmpeg 的 libavcodec/synth_filter.c（Copyright (c) 2008 Michael Niedermayer；Copyright (C) 2016 foo86）、
// dcadsp.c、dca_core.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! 定点的子带合成：32 / 64 子带 QMF（IMDCT + 加窗），LFE 插值，ff_dca_core_filter_fixed

use anyhow::Result;

use super::core::*;
use super::dct::{imdct_half_32, imdct_half_64};
use super::dsp::{clip23, lfe_fir_fixed, lfe_x96_fixed, norm20, norm21};
use super::tables::*;

/// 一个子带样本组 → 32 个 PCM（synth_filter_fixed）
fn synth32(h: &mut DspHist, window: &[i32], out: &mut [i32], input: &[i32]) {
    let off = h.offset;
    imdct_half_32(&mut h.hist1[off..off + 32], input);
    let buf = &h.hist1;
    let at = |k: usize| buf[(off + k) & 511] as i64;
    for i in 0..16 {
        let mut a = (h.hist2[i] as i64) << 21;
        let mut b = (h.hist2[i + 16] as i64) << 21;
        let (mut c, mut d) = (0i64, 0i64);
        for j in (0..512).step_by(64) {
            a += window[i + j] as i64 * at(i + j);
            b += window[i + j + 16] as i64 * at(15 - i + j);
            c += window[i + j + 32] as i64 * at(16 + i + j);
            d += window[i + j + 48] as i64 * at(31 - i + j);
        }
        out[i] = clip23(norm21(a));
        out[i + 16] = clip23(norm21(b));
        h.hist2[i] = norm21(c);
        h.hist2[i + 16] = norm21(d);
    }
    h.offset = off.wrapping_sub(32) & 511;
}

/// 一个子带样本组 → 64 个 PCM（synth_filter_fixed_64）
fn synth64(h: &mut DspHist, window: &[i32], out: &mut [i32], input: &[i32]) {
    let off = h.offset;
    imdct_half_64(&mut h.hist1[off..off + 64], input);
    let buf = &h.hist1;
    let at = |k: usize| buf[(off + k) & 1023] as i64;
    for i in 0..32 {
        let mut a = (h.hist2[i] as i64) << 20;
        let mut b = (h.hist2[i + 32] as i64) << 20;
        let (mut c, mut d) = (0i64, 0i64);
        for j in (0..1024).step_by(128) {
            a += window[i + j] as i64 * at(i + j);
            b += window[i + j + 32] as i64 * at(31 - i + j);
            c += window[i + j + 64] as i64 * at(32 + i + j);
            d += window[i + j + 96] as i64 * at(63 - i + j);
        }
        out[i] = clip23(norm20(a));
        out[i + 32] = clip23(norm20(b));
        h.hist2[i] = norm20(c);
        h.hist2[i + 32] = norm20(d);
    }
    h.offset = off.wrapping_sub(64) & 1023;
}

impl Core {
    /// 子带 → PCM（各扬声器一个 `output`）；x96_synth：1 = 强制 64 子带合成，-1 = 不用，0 = 看有没有 X96
    pub(super) fn filter_fixed(&mut self, mut x96_synth: i32) -> Result<()> {
        let mut x96_nchannels = 0;
        if x96_synth == 0 && self.ext_audio_mask & (CSS_X96 | EXSS_X96) != 0 {
            x96_nchannels = self.x96_nchannels;
            x96_synth = 1;
        }
        let x96 = x96_synth > 0;
        self.output_rate = self.h.sample_rate << x96 as u32;
        let nsamples = (self.h.npcmblocks * PCMBLOCK) << x96 as u32;
        self.npcmsamples = nsamples;
        for (spkr, o) in self.output.iter_mut().enumerate() {
            if self.ch_mask & (1 << spkr) != 0 {
                o.resize(nsamples, 0);
            } else {
                o.clear();
            }
        }

        // 换了滤波方式时清掉历史
        let mode = x96 as u32 | 2;
        if self.filter_mode != mode {
            self.erase_dsp_history();
            self.filter_mode = mode;
        }
        let window: &[i32] = if x96 {
            &FIR_64BANDS
        } else if self.h.filter_perfect {
            &FIR_32BANDS_PERFECT
        } else {
            &FIR_32BANDS_NONPERFECT
        };

        for ch in 0..self.nchannels {
            let spkr = self.map_prm_ch_to_spkr(ch).ok_or_else(|| anyhow::anyhow!("声道 {ch} 没有对应的扬声器"))?;
            let (sub, x96buf) = (&self.sub, &self.x96);
            let out = &mut self.output[spkr];
            let h = &mut self.dsp[ch];
            let mut input = [0i32; 64];
            for j in 0..self.h.npcmblocks {
                if !x96 {
                    for (i, v) in input[..32].iter_mut().enumerate() {
                        *v = sub.buf[sub.at(ch, i) + j];
                    }
                    synth32(h, window, &mut out[j * 32..j * 32 + 32], &input[..32]);
                    continue;
                }
                if ch < x96_nchannels {
                    // 64 个子带全有，前 32 个是残差
                    for (i, v) in input.iter_mut().enumerate() {
                        let hi = x96buf.buf[x96buf.at(ch, i) + j];
                        *v = if i < 32 { sub.buf[sub.at(ch, i) + j].wrapping_add(hi) } else { hi };
                    }
                } else {
                    for (i, v) in input[..32].iter_mut().enumerate() {
                        *v = sub.buf[sub.at(ch, i) + j];
                    }
                }
                synth64(h, window, &mut out[j * 64..j * 64 + 64], &input);
            }
        }

        if self.h.lfe_present != 0 {
            let nlfe = self.h.npcmblocks >> 1;
            let lfe = self.sub.lfe;
            let out = &mut self.output[SPK_LFE1];
            if self.h.lfe_present == 1 {
                // LFF = 1（128 倍插值）在定点方式下没有滤波器（FFmpeg 也不支持），只好输出静音
                out.fill(0);
            } else {
                let base = if x96 { nsamples / 2 } else { 0 };
                lfe_fir_fixed(&mut out[base..], &self.sub.buf, lfe + LFE_HISTORY, &LFE_FIR_64, self.h.npcmblocks);
                if x96 {
                    // 再滤掉插值镜像里 47.6 ~ 48 kHz 的成分
                    lfe_x96_fixed(out, nsamples / 2, nsamples / 2, &mut self.output_history_lfe);
                }
            }
            // LFE 历史
            for n in (0..LFE_HISTORY).rev() {
                self.sub.buf[lfe + n] = self.sub.buf[lfe + nlfe + n];
            }
        }
        Ok(())
    }
}
