// 改写自 FFmpeg 的 libavcodec/dca_core.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! X96（96 kHz 扩展：在核心的 32 个子带之上再加 32 个）：子帧边信息与子带样本

use anyhow::{Result, bail, ensure};

use super::audio::{decode_joint, extract_audio, inverse_adpcm};
use super::bits::Bits;
use super::core::*;
use super::dsp::{clip23, dequantize, mul};
use super::tables::*;

impl Core {
    /// 线性同余随机数（-2^30 ~ 2^30 - 1），给没分配位的子带填噪声
    fn rand_x96(&mut self) -> i32 {
        self.x96_rand = self.x96_rand.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (self.x96_rand & 0x7FFF_FFFF) as i32 - 0x4000_0000
    }

    pub(super) fn parse_x96_subframe_header(&mut self, b: &mut Bits, xch_base: usize) -> Result<()> {
        ensure!(b.left() >= 0, "X96 子帧头越界");
        let chs = xch_base..self.x96_nchannels;
        let start = self.x96_subband_start;
        for ch in chs.clone() {
            for band in start..self.nsubbands[ch] {
                self.prediction_mode[ch][band] = b.bit();
            }
        }
        for ch in chs.clone() {
            for band in start..self.nsubbands[ch] {
                if self.prediction_mode[ch][band] {
                    self.prediction_vq_index[ch][band] = b.bits(12) as usize;
                }
            }
        }
        // 位分配：霍夫曼时编的是差值
        let hr = self.x96_high_res as usize;
        for ch in chs.clone() {
            let sel = self.bit_allocation_sel[ch];
            let mut abits = 0i32;
            for band in start..self.nsubbands[ch] {
                if sel < 7 {
                    abits += b.vlc(QUANT_INDEX[5 + 2 * hr][sel]);
                } else {
                    abits = b.bits(3 + hr as u32) as i32;
                }
                ensure!((0..=7 + 8 * hr as i32).contains(&abits), "X96 位分配不对");
                self.bit_allocation[ch][band] = abits as u32;
            }
        }
        // 比例因子（没分配位的子带也有）
        for ch in chs.clone() {
            let sel = self.scale_factor_sel[ch];
            let mut scale_index = 0;
            for band in start..self.nsubbands[ch] {
                self.scale_factors[ch][band >> 1][band & 1] = Self::parse_scale(b, &mut scale_index, sel)?;
            }
        }
        for ch in chs.clone() {
            if self.joint_intensity_index[ch] != 0 {
                self.joint_scale_sel[ch] = b.bits(3) as usize;
                ensure!(self.joint_scale_sel[ch] != 7, "X96 联合比例因子码表不对");
            }
        }
        for ch in chs {
            if let Some(src_ch) = self.joint_intensity_index[ch].checked_sub(1) {
                let sel = self.joint_scale_sel[ch];
                for band in self.nsubbands[ch]..self.nsubbands[src_ch] {
                    self.joint_scale_factors[ch][band] = Self::parse_joint_scale(b, sel)?;
                }
            }
        }
        if self.h.crc_present {
            b.skip(16);
        }
        Ok(())
    }

    pub(super) fn parse_x96_subframe_audio(&mut self, b: &mut Bits, sf: usize, xch_base: usize, sub_pos: &mut usize) -> Result<()> {
        let nsamples = self.nsubsubframes[sf] * SSF_SAMPLES;
        ensure!(*sub_pos + nsamples <= self.h.npcmblocks, "X96 子带样本越界");
        ensure!(b.left() >= 0, "X96 子帧越界");
        let start = self.x96_subband_start;

        // 矢量量化的子带、没分配位的子带
        for ch in xch_base..self.x96_nchannels {
            for band in start..self.nsubbands[ch] {
                let mut p = self.x96.at(ch, band) + *sub_pos;
                let scale = self.scale_factors[ch][band >> 1][band & 1];
                match self.bit_allocation[ch][band] {
                    0 if scale <= 1 => self.x96.buf[p..p + nsamples].fill(0),
                    0 => {
                        for k in 0..nsamples {
                            let r = self.rand_x96();
                            self.x96.buf[p + k] = mul(r, scale, 31);
                        }
                    }
                    1 => {
                        for ssf in 0..self.nsubsubframes[sf].div_ceil(2) {
                            let vq = &HIGH_FREQ_VQ[b.bits(10) as usize];
                            for &c in &vq[..(nsamples - ssf * 16).min(16)] {
                                self.x96.buf[p] = clip23((c as i32 * scale + (1 << 3)) >> 4);
                                p += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // 音频数据
        let mut ofs = *sub_pos;
        for ssf in 0..self.nsubsubframes[sf] {
            for ch in xch_base..self.x96_nchannels {
                ensure!(b.left() >= 0, "X96 子帧越界");
                for band in start..self.nsubbands[ch] {
                    let abits = self.bit_allocation[ch][band] as i32 - 1;
                    if abits < 1 {
                        continue;
                    }
                    let mut audio = [0i32; 8];
                    extract_audio(b, &mut audio, abits as u32, &self.quant_index_sel[ch])?;
                    let step_size = if self.h.bit_rate == 3 { LOSSLESS_QUANT } else { LOSSY_QUANT }[abits as usize];
                    let scale = self.scale_factors[ch][band >> 1][band & 1];
                    let p = self.x96.at(ch, band) + ofs;
                    dequantize(&mut self.x96.buf[p..p + SSF_SAMPLES], &audio, step_size, scale, false);
                }
            }
            if (ssf == self.nsubsubframes[sf] - 1 || self.h.sync_ssf) && b.bits(16) != 0xFFFF {
                bail!("X96 的 DSYNC 不对");
            }
            ofs += SSF_SAMPLES;
        }

        for ch in xch_base..self.x96_nchannels {
            let bands = start..self.nsubbands[ch];
            inverse_adpcm(&mut self.x96, ch, &self.prediction_vq_index[ch], &self.prediction_mode[ch], bands, *sub_pos, nsamples);
        }
        for ch in xch_base..self.x96_nchannels {
            if let Some(src) = self.joint_intensity_index[ch].checked_sub(1) {
                let bands = self.nsubbands[ch]..self.nsubbands[src];
                decode_joint(&mut self.x96, ch, src, &*self.joint_scale_factors[ch], bands, *sub_pos, nsamples);
            }
        }
        *sub_pos = ofs;
        Ok(())
    }
}
