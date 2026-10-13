// 改写自 FFmpeg 的 libavcodec/dca_core.c、dcadsp.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! 主音频数据（5.5）：取样本（块码 / 霍夫曼 / 直接）、反量化、高频矢量量化、ADPCM、联合编码

use anyhow::{Result, ensure};

use super::bits::Bits;
use super::core::*;
use super::dsp::{adpcm_predict, clip23, dequantize, mul17, mul23};
use super::tables::*;

static BLOCK_CODE_NBITS: [u32; 7] = [7, 10, 12, 13, 15, 17, 19];

/// 块码：两个码各拆出 4 个 levels 进制的样本
pub(super) fn parse_block_codes(b: &mut Bits, audio: &mut [i32; 8], abits: u32) -> Result<()> {
    let n = BLOCK_CODE_NBITS[abits as usize - 1];
    let (mut c1, mut c2) = (b.bits(n) as i32, b.bits(n) as i32);
    let levels = QUANT_LEVELS[abits as usize];
    let offset = (levels - 1) / 2;
    for (k, v) in audio.iter_mut().enumerate() {
        let c = if k < 4 { &mut c1 } else { &mut c2 };
        *v = *c % levels - offset;
        *c /= levels;
    }
    ensure!(c1 | c2 == 0, "块码不对");
    Ok(())
}

/// 读一个子带的 8 个样本；返回是否用了霍夫曼码（要调整比例因子）
pub(super) fn extract_audio(b: &mut Bits, audio: &mut [i32; 8], abits: u32, sel_of: &[usize; CODE_BOOKS]) -> Result<bool> {
    if abits == 0 {
        *audio = [0; 8];
        return Ok(false);
    }
    if abits as usize <= CODE_BOOKS {
        let sel = sel_of[abits as usize - 1];
        if sel < QUANT_INDEX_GROUP_SIZE[abits as usize - 1] as usize {
            let t = QUANT_INDEX[abits as usize - 1][sel];
            audio.iter_mut().for_each(|v| *v = b.vlc(t));
            return Ok(true);
        }
        if abits <= 7 {
            parse_block_codes(b, audio, abits)?;
            return Ok(false);
        }
    }
    // 不再编码，直接存的
    audio.iter_mut().for_each(|v| *v = b.sbits(abits - 3));
    Ok(false)
}

/// 反 ADPCM：有预测的子带加上由前 4 个样本预测的值
pub(super) fn inverse_adpcm(
    sb: &mut SubBuf,
    ch: usize,
    vq: &[usize; SUBBANDS_X96],
    mode: &[bool; SUBBANDS_X96],
    bands: std::ops::Range<usize>,
    ofs: usize,
    len: usize,
) {
    for band in bands.filter(|&band| mode[band]) {
        let p = sb.at(ch, band) + ofs;
        for j in 0..len {
            let x = adpcm_predict(vq[band], &sb.buf[p + j - 4..p + j]);
            sb.buf[p + j] = clip23(sb.buf[p + j].wrapping_add(x));
        }
    }
}

/// 联合强度编码：从源声道按比例复制高频子带
pub(super) fn decode_joint(sb: &mut SubBuf, ch: usize, src: usize, scales: &[i32], bands: std::ops::Range<usize>, ofs: usize, len: usize) {
    for band in bands {
        let (d, s) = (sb.at(ch, band) + ofs, sb.at(src, band) + ofs);
        for j in 0..len {
            sb.buf[d + j] = clip23(mul17(sb.buf[s + j], scales[band]));
        }
    }
}

impl Core {
    pub(super) fn parse_subframe_audio(
        &mut self,
        b: &mut Bits,
        sf: usize,
        kind: Kind,
        xch_base: usize,
        sub_pos: &mut usize,
        lfe_pos: &mut usize,
    ) -> Result<()> {
        let nsamples = self.nsubsubframes[sf] * SSF_SAMPLES;
        ensure!(*sub_pos + nsamples <= self.h.npcmblocks, "子带样本越界");
        ensure!(b.left() >= 0, "子帧越界");

        // 高频矢量量化的子带
        for ch in xch_base..self.nchannels {
            for band in self.subband_vq_start[ch]..self.nsubbands[ch] {
                let coeff = &HIGH_FREQ_VQ[b.bits(10) as usize];
                let scale = self.scale_factors[ch][band][0];
                let p = self.sub.at(ch, band) + *sub_pos;
                for (j, v) in self.sub.buf[p..p + nsamples].iter_mut().enumerate() {
                    *v = clip23((coeff[j] as i32 * scale + (1 << 3)) >> 4);
                }
            }
        }

        // LFE
        if self.h.lfe_present != 0 && kind == Kind::Core {
            let n = 2 * self.h.lfe_present as usize * self.nsubsubframes[sf];
            let mut audio = [0i32; 16];
            audio[..n].iter_mut().for_each(|v| *v = b.sbits(8));
            let index = b.bits(8) as usize;
            ensure!(index < SCALE_FACTOR_QUANT7.len(), "LFE 比例因子不对");
            // 量化步长 0.035
            let scale = mul23(4697620, SCALE_FACTOR_QUANT7[index]);
            let base = self.sub.lfe + *lfe_pos;
            for (k, &a) in audio[..n].iter().enumerate() {
                self.sub.buf[base + k] = clip23(a.wrapping_mul(scale) >> 4);
            }
            *lfe_pos += n;
        }

        // 音频数据
        let mut ofs = *sub_pos;
        for ssf in 0..self.nsubsubframes[sf] {
            for ch in xch_base..self.nchannels {
                ensure!(b.left() >= 0, "子帧越界");
                for band in 0..self.subband_vq_start[ch] {
                    let abits = self.bit_allocation[ch][band];
                    let mut audio = [0i32; 8];
                    let huffman = extract_audio(b, &mut audio, abits, &self.quant_index_sel[ch])?;
                    let step_size = if self.h.bit_rate == 3 { LOSSLESS_QUANT } else { LOSSY_QUANT }[abits as usize];
                    let trans = self.transition_mode[sf][ch][band];
                    let mut scale = self.scale_factors[ch][band][(trans != 0 && ssf >= trans) as usize];
                    if huffman {
                        let adj = self.scale_factor_adj[ch][abits as usize - 1] as i64;
                        scale = clip23(((adj * scale as i64) >> 22) as i32);
                    }
                    let p = self.sub.at(ch, band) + ofs;
                    dequantize(&mut self.sub.buf[p..p + SSF_SAMPLES], &audio, step_size, scale, false);
                }
            }
            // DSYNC
            if (ssf == self.nsubsubframes[sf] - 1 || self.h.sync_ssf) && b.bits(16) != 0xFFFF {
                anyhow::bail!("DSYNC 不对");
            }
            ofs += SSF_SAMPLES;
        }

        for ch in xch_base..self.nchannels {
            let bands = 0..self.nsubbands[ch];
            inverse_adpcm(&mut self.sub, ch, &self.prediction_vq_index[ch], &self.prediction_mode[ch], bands, *sub_pos, nsamples);
        }
        for ch in xch_base..self.nchannels {
            if let Some(src) = self.joint_intensity_index[ch].checked_sub(1) {
                let bands = self.nsubbands[ch]..self.nsubbands[src];
                decode_joint(&mut self.sub, ch, src, &*self.joint_scale_factors[ch], bands, *sub_pos, nsamples);
            }
        }
        *sub_pos = ofs;
        Ok(())
    }
}
