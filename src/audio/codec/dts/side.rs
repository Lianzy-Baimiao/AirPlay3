// 改写自 FFmpeg 的 libavcodec/dca_core.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! 子帧边信息（5.4.1）：预测、位分配、瞬态位置、比例因子

use anyhow::{Result, ensure};

use super::bits::Bits;
use super::core::*;
use super::tables::*;

impl Core {
    pub(super) fn parse_subframe_header(&mut self, b: &mut Bits, sf: usize, kind: Kind, xch_base: usize) -> Result<()> {
        ensure!(b.left() >= 0, "子帧头越界");
        let chs = xch_base..self.nchannels;

        if kind == Kind::Core {
            self.nsubsubframes[sf] = b.bits(2) as usize + 1;
            // 最后一个子子帧的样本数
            b.skip(3);
        }

        // 预测模式、预测系数的码本地址
        for ch in chs.clone() {
            for band in 0..self.nsubbands[ch] {
                self.prediction_mode[ch][band] = b.bit();
            }
        }
        for ch in chs.clone() {
            for band in 0..self.nsubbands[ch] {
                if self.prediction_mode[ch][band] {
                    self.prediction_vq_index[ch][band] = b.bits(12) as usize;
                }
            }
        }

        // 位分配
        for ch in chs.clone() {
            let sel = self.bit_allocation_sel[ch];
            for band in 0..self.subband_vq_start[ch] {
                let abits = if sel < 5 { b.vlc(BIT_ALLOCATION[sel]) as u32 } else { b.bits(sel as u32 - 1) };
                ensure!(abits <= 26, "位分配不对");
                self.bit_allocation[ch][band] = abits;
            }
        }

        // 瞬态位置：只有不止一个子子帧时才可能有
        for ch in chs.clone() {
            self.transition_mode[sf][ch] = [0; SUBBANDS];
            if self.nsubsubframes[sf] > 1 {
                let sel = self.transition_mode_sel[ch];
                for band in 0..self.subband_vq_start[ch] {
                    if self.bit_allocation[ch][band] != 0 {
                        self.transition_mode[sf][ch][band] = b.vlc(TRANSITION_MODE[sel]) as usize;
                    }
                }
            }
        }

        // 比例因子
        for ch in chs.clone() {
            let sel = self.scale_factor_sel[ch];
            let mut scale_index = 0;
            for band in 0..self.subband_vq_start[ch] {
                if self.bit_allocation[ch][band] != 0 {
                    self.scale_factors[ch][band][0] = Self::parse_scale(b, &mut scale_index, sel)?;
                    if self.transition_mode[sf][ch][band] != 0 {
                        self.scale_factors[ch][band][1] = Self::parse_scale(b, &mut scale_index, sel)?;
                    }
                } else {
                    self.scale_factors[ch][band][0] = 0;
                }
            }
            // 高频矢量量化的子带
            for band in self.subband_vq_start[ch]..self.nsubbands[ch] {
                self.scale_factors[ch][band][0] = Self::parse_scale(b, &mut scale_index, sel)?;
            }
        }

        // 联合编码的码表与比例因子
        for ch in chs.clone() {
            if self.joint_intensity_index[ch] != 0 {
                self.joint_scale_sel[ch] = b.bits(3) as usize;
                ensure!(self.joint_scale_sel[ch] != 7, "联合比例因子码表不对");
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

        // 动态范围系数、CRC
        if self.h.drc_present && kind == Kind::Core {
            b.skip(8);
        }
        if self.h.crc_present {
            b.skip(16);
        }
        Ok(())
    }
}
