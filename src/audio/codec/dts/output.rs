// 改写自 FFmpeg 的 libavcodec/dca_core.c、dcadec.c、dcadsp.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! 合成之后：主声道对应的扬声器、撤销编码时嵌入的降混、和差解码，按 WAV 的声道顺序交错输出

use anyhow::{Result, ensure};

use super::core::*;
use super::dsp::{clip23, mul15, mul16, mul23};
use super::tables::CHANNELS;

/// DTS 扬声器 → WAV 声道位（dca2wav_norm / dca2wav_wide）
static DCA2WAV_NORM: [u8; 28] = [2, 0, 1, 9, 10, 3, 8, 4, 5, 9, 10, 6, 7, 12, 13, 14, 3, 6, 7, 11, 12, 14, 16, 15, 17, 8, 4, 5];
static DCA2WAV_WIDE: [u8; 28] = [2, 0, 1, 4, 5, 3, 8, 4, 5, 9, 10, 6, 7, 12, 13, 14, 3, 9, 10, 11, 12, 14, 16, 15, 17, 8, 4, 5];

/// 按 WAV 声道顺序排列的扬声器（ff_dca_set_channel_layout）；同一个 WAV 声道只取先出现的扬声器
pub(super) fn channel_map(mask: u32) -> (Vec<usize>, u32) {
    // 7.0 / 7.1 宽声道
    let table = if mask == 0x6001F || mask == 0x6003F { &DCA2WAV_WIDE } else { &DCA2WAV_NORM };
    let mut wav_map = [0usize; 18];
    let mut wav_mask = 0u32;
    for (spkr, &w) in table.iter().enumerate() {
        if mask & (1 << spkr) != 0 && wav_mask & (1 << w) == 0 {
            wav_map[w as usize] = spkr;
            wav_mask |= 1 << w;
        }
    }
    ((0..18).filter(|&w| wav_mask & (1 << w) != 0).map(|w| wav_map[w]).collect(), wav_mask)
}

impl Core {
    /// 第 ch 个主声道对应的扬声器
    pub(super) fn map_prm_ch_to_spkr(&self, ch: usize) -> Option<usize> {
        let mut pos = CHANNELS[self.h.audio_mode] as usize;
        let xxch = self.ext_audio_mask & (CSS_XXCH | EXSS_XXCH) != 0;
        if ch < pos {
            let spkr = PRM_CH_TO_SPKR[self.h.audio_mode][ch];
            let spkr = usize::try_from(spkr).ok()?;
            if !xxch {
                return Some(spkr);
            }
            return match spkr {
                _ if self.xxch_core_mask & (1 << spkr) != 0 => Some(spkr),
                SPK_LS if self.xxch_core_mask & (1 << SPK_LSS) != 0 => Some(SPK_LSS),
                SPK_RS if self.xxch_core_mask & (1 << SPK_RSS) != 0 => Some(SPK_RSS),
                _ => None,
            };
        }
        if self.ext_audio_mask & CSS_XCH != 0 && ch == pos {
            return Some(SPK_CS);
        }
        if xxch {
            for spkr in SPK_CS..self.xxch_mask_nbits as usize {
                if self.xxch_spkr_mask & (1 << spkr) != 0 {
                    if pos == ch {
                        return Some(spkr);
                    }
                    pos += 1;
                }
            }
        }
        None
    }

    /// filter_frame_fixed 合成之后的部分：结果按 WAV 顺序交错写进 out（24 位），返回声道数
    pub(super) fn finish_frame(&mut self, out: &mut Vec<i32>) -> Result<usize> {
        let n = self.npcmsamples;

        // 撤销嵌入的 XCH 降混
        if self.h.es_format && self.ext_audio_mask & CSS_XCH != 0 && self.h.audio_mode >= 8 {
            let cs = std::mem::take(&mut self.output[SPK_CS]);
            for spkr in [SPK_LS, SPK_RS] {
                for (d, &s) in self.output[spkr].iter_mut().zip(&cs) {
                    *d = d.wrapping_sub(mul23(s, 5931520));
                }
            }
            self.output[SPK_CS] = cs;
        }

        // 撤销嵌入的 XXCH 降混
        if self.ext_audio_mask & (CSS_XXCH | EXSS_XXCH) != 0 && self.xxch_dmix_embedded {
            let scale_inv = self.xxch_dmix_scale_inv;
            for spkr in 0..self.xxch_mask_nbits as usize {
                if self.xxch_core_mask & (1 << spkr) != 0 {
                    self.output[spkr].iter_mut().for_each(|v| *v = mul16(*v, scale_inv));
                }
            }
            let base = CHANNELS[self.h.audio_mode] as usize;
            let mut k = 0;
            for ch in base..self.nchannels {
                let src = self.map_prm_ch_to_spkr(ch).ok_or_else(|| anyhow::anyhow!("XXCH 声道没有对应的扬声器"))?;
                let s = std::mem::take(&mut self.output[src]);
                for spkr in 0..self.xxch_mask_nbits as usize {
                    if self.xxch_dmix_mask[ch - base] & (1 << spkr) != 0 {
                        let coeff = mul16(self.xxch_dmix_coeff[k], scale_inv);
                        k += 1;
                        if coeff != 0 {
                            for (d, &x) in self.output[spkr].iter_mut().zip(&s) {
                                *d = d.wrapping_sub(mul15(x, coeff));
                            }
                        }
                    }
                }
                self.output[src] = s;
            }
        }

        // 和差解码
        if self.ext_audio_mask & (CSS_XXCH | CSS_XCH | EXSS_XXCH) == 0 {
            let mode = self.h.audio_mode;
            if (self.h.sumdiff_front && mode > 0) || mode == 3 {
                self.butterflies(SPK_L, SPK_R);
            }
            if self.h.sumdiff_surround && mode >= 8 {
                self.butterflies(SPK_LS, SPK_RS);
            }
        }

        let (map, _) = channel_map(self.ch_mask);
        ensure!(map.iter().all(|&s| self.output[s].len() == n), "声道缓冲长度不对");
        out.clear();
        out.reserve(n * map.len());
        for i in 0..n {
            out.extend(map.iter().map(|&s| clip23(self.output[s][i])));
        }
        Ok(map.len())
    }

    /// (a, b) → (a + b, a - b)
    fn butterflies(&mut self, a: usize, b: usize) {
        let (lo, hi) = self.output.split_at_mut(b);
        for (x, y) in lo[a].iter_mut().zip(hi[0].iter_mut()) {
            let t = x.wrapping_sub(*y);
            *x = x.wrapping_add(*y);
            *y = t;
        }
    }
}
