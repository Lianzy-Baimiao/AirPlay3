// 改写自 FFmpeg 的 libavcodec/dca_xll.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! XLL 的声道组头：声道、位数、采样率、降混系数、各频带的去相关与预测参数

use super::bits::Bits;
use super::dsp::{mul15, mul16};
use super::exss::{Asset, SAMPLING_FREQS};
use super::tables::{DMIX_PRIMARY_NCH, DMIXTABLE, INV_DMIXTABLE};
use super::xll::*;

/// 不是主声道组、降混已嵌入、属于层级的声道组
pub(super) fn is_hier_dmix(c: &ChSet) -> bool {
    !c.primary_chset && c.dmix_embedded && c.hier_chset
}

impl Xll {
    /// 层级里 ci 之后的下一个嵌入了降混的声道组
    pub(super) fn next_hier_dmix(&self, ci: usize) -> Option<usize> {
        if !self.chset[ci].hier_chset {
            return None;
        }
        (ci + 1..self.nchsets).find(|&i| is_hier_dmix(&self.chset[i]))
    }

    pub(super) fn parse_sub_headers(&mut self, b: &mut Bits, asset: &Asset) -> XResult<()> {
        self.nfreqbands = 0;
        self.nchannels = 0;
        self.nreschsets = 0;
        for ci in 0..self.nchsets {
            self.chset[ci].hier_ofs = self.nchannels;
            self.parse_chset_header(b, ci, asset)?;
            let c = &self.chset[ci];
            self.nfreqbands = self.nfreqbands.max(c.nfreqbands);
            if c.hier_chset {
                self.nchannels += c.nchannels;
            }
            if c.residual_encode != (1 << c.nchannels) - 1 {
                self.nreschsets += 1;
            }
        }
        // 非主声道组的降混系数预先乘上后面一层的比例
        for ci in (1..self.nchsets).rev() {
            if is_hier_dmix(&self.chset[ci]) {
                if let Some(oi) = self.next_hier_dmix(ci) {
                    let (lo, hi) = self.chset.split_at_mut(oi);
                    let (c, o) = (&mut lo[ci], &hi[0]);
                    for i in 0..c.hier_ofs.min(16) {
                        let (scale, scale_inv) = (o.dmix_scale[i], o.dmix_scale_inv[i]);
                        c.dmix_scale[i] = mul15(c.dmix_scale[i], scale);
                        c.dmix_scale_inv[i] = mul16(c.dmix_scale_inv[i], scale_inv);
                        for j in 0..c.nchannels {
                            let coeff = mul16(c.dmix_coeff[i][j], scale_inv);
                            c.dmix_coeff[i][j] = mul15(coeff, o.dmix_scale[(c.hier_ofs + j).min(15)]);
                        }
                    }
                }
            }
        }
        // 不降混：所有声道组都解
        self.nactivechsets = self.nchsets;
        Ok(())
    }

    fn parse_dmix_coeffs(&mut self, b: &mut Bits, ci: usize) -> XResult<()> {
        let c = &mut self.chset[ci];
        let m = if c.primary_chset { DMIX_PRIMARY_NCH[c.dmix_type] as usize } else { c.hier_ofs };
        let signed = |v: i32, code: u32| if code >> 8 != 0 { v } else { -v };
        for i in 0..m.min(16) {
            let mut scale_inv = 0;
            if !c.primary_chset {
                // 降混比例（只有非主声道组有）
                let code = b.bits(9);
                let index = ((code & 0xFF) as usize).wrapping_sub(41);
                if index >= INV_DMIXTABLE.len() {
                    return Err(XllError::Invalid);
                }
                scale_inv = INV_DMIXTABLE[index];
                c.dmix_scale[i] = signed(DMIXTABLE[index + 41], code);
                c.dmix_scale_inv[i] = signed(scale_inv, code);
            }
            for j in 0..c.nchannels {
                let code = b.bits(9);
                let index = (code & 0xFF) as usize;
                if index >= DMIXTABLE.len() {
                    return Err(XllError::Invalid);
                }
                let mut coeff = DMIXTABLE[index];
                if !c.primary_chset {
                    coeff = mul16(scale_inv, coeff);
                }
                c.dmix_coeff[i][j] = signed(coeff, code);
            }
        }
        Ok(())
    }

    fn parse_chset_header(&mut self, b: &mut Bits, ci: usize, asset: &Asset) -> XResult<()> {
        let header_pos = b.count();
        let header_size = b.bits(10) as usize + 1;
        let (nchsets, nsegsamples, mask_nbits) = (self.nchsets, self.nsegsamples, self.ch_mask_nbits);
        let p = (self.chset[0].nfreqbands, self.chset[0].freq, self.chset[0].pcm_bit_res, self.chset[0].storage_bit_res);
        {
            let c = &mut self.chset[ci];
            c.nchannels = b.bits(4) as usize + 1;
            if c.nchannels > CHANNELS_MAX {
                return Err(XllError::Fatal);
            }
            c.residual_encode = b.bits(c.nchannels as u32);
            c.pcm_bit_res = b.bits(5) + 1;
            c.storage_bit_res = b.bits(5) + 1;
            if !matches!(c.storage_bit_res, 16 | 20 | 24) {
                return Err(XllError::Fatal);
            }
            if c.pcm_bit_res > c.storage_bit_res {
                return Err(XllError::Invalid);
            }
            c.freq = SAMPLING_FREQS[b.bits(4) as usize];
            if c.freq > 192000 {
                return Err(XllError::Fatal);
            }
            // 采样率修正、替换组：不支持
            if b.bits(2) != 0 || b.bits(2) != 0 {
                return Err(XllError::Fatal);
            }
        }
        if asset.one_to_one_map_ch_to_spkr {
            let c = &mut self.chset[ci];
            c.primary_chset = b.bit();
            if c.primary_chset != (ci == 0) {
                return Err(XllError::Invalid);
            }
            c.dmix_coeffs_present = b.bit();
            c.dmix_embedded = c.dmix_coeffs_present && b.bit();
            if c.dmix_coeffs_present && c.primary_chset {
                c.dmix_type = b.bits(3) as usize;
                if c.dmix_type >= 7 {
                    return Err(XllError::Invalid);
                }
            }
            c.hier_chset = b.bit();
            if !c.hier_chset && nchsets != 1 {
                return Err(XllError::Fatal);
            }
            if c.dmix_coeffs_present {
                self.parse_dmix_coeffs(b, ci)?;
            }
            let c = &mut self.chset[ci];
            // 声道掩码
            if !b.bit() {
                return Err(XllError::Fatal);
            }
            c.ch_mask = b.bits(mask_nbits);
            if c.ch_mask.count_ones() as usize != c.nchannels {
                return Err(XllError::Invalid);
            }
            let mut j = 0;
            for i in 0..mask_nbits as usize {
                if c.ch_mask & (1 << i) != 0 {
                    c.ch_remap[j] = i;
                    j += 1;
                }
            }
        } else {
            // 只支持 LtRt 立体声
            let c = &mut self.chset[ci];
            if c.nchannels != 2 || nchsets != 1 || b.bit() {
                return Err(XllError::Fatal);
            }
            c.primary_chset = true;
            c.dmix_coeffs_present = false;
            c.dmix_embedded = false;
            c.hier_chset = false;
            c.ch_mask = 0x6;
            c.ch_remap[0] = 1;
            c.ch_remap[1] = 2;
        }

        let c = &mut self.chset[ci];
        if c.freq > 96000 {
            // 额外的频带：不支持
            if b.bit() {
                return Err(XllError::Fatal);
            }
            c.nfreqbands = 2;
        } else {
            c.nfreqbands = 1;
        }
        // 采样率按第一个频带算，合频带后再加倍
        c.freq >>= c.nfreqbands - 1;
        if ci != 0 && (c.nfreqbands, c.freq, c.pcm_bit_res, c.storage_bit_res) != p {
            return Err(XllError::Fatal);
        }
        c.nabits = if c.storage_bit_res > 16 { 5 } else if c.storage_bit_res > 8 { 4 } else { 3 };
        // 嵌入的降混、抽取器可能溢出，多留一位
        if (nchsets > 1 || c.nfreqbands > 1) && c.nabits < 5 {
            c.nabits += 1;
        }
        self.parse_band_headers(b, ci, nsegsamples)?;
        if !b.seek(header_pos + header_size * 8) {
            return Err(XllError::Invalid);
        }
        Ok(())
    }

    /// 各频带：成对去相关、自适应 / 固定预测、LSB 分开存的位数
    fn parse_band_headers(&mut self, b: &mut Bits, ci: usize, nsegsamples: usize) -> XResult<()> {
        let (seg_size_nbits, frame_size, crc, scalable) =
            (self.seg_size_nbits, self.frame_size, self.band_crc_present, self.scalable_lsbs);
        let c = &mut self.chset[ci];
        let (nch, dmix_embedded) = (c.nchannels, c.dmix_embedded);
        for band in 0..c.nfreqbands {
            let bd = &mut c.bands[band];
            bd.decor_enabled = b.bit();
            if bd.decor_enabled && nch > 1 {
                let ch_nbits = usize::BITS - (nch - 1).leading_zeros();
                for i in 0..nch {
                    bd.orig_order[i] = b.bits(ch_nbits) as usize;
                    if bd.orig_order[i] >= nch {
                        return Err(XllError::Invalid);
                    }
                }
                for i in 0..nch / 2 {
                    bd.decor_coeff[i] = if b.bit() { get_linear(b, 7) } else { 0 };
                }
            } else {
                for i in 0..nch {
                    bd.orig_order[i] = i;
                }
                bd.decor_coeff[..nch / 2].fill(0);
            }

            bd.highest_pred_order = 0;
            for i in 0..nch {
                bd.adapt_pred_order[i] = b.bits(4) as usize;
                bd.highest_pred_order = bd.highest_pred_order.max(bd.adapt_pred_order[i]);
            }
            if bd.highest_pred_order > nsegsamples {
                return Err(XllError::Invalid);
            }
            for i in 0..nch {
                bd.fixed_pred_order[i] = if bd.adapt_pred_order[i] != 0 { 0 } else { b.bits(2) as usize };
            }
            for i in 0..nch {
                for j in 0..bd.adapt_pred_order[i] {
                    let k = get_linear(b, 8);
                    if k == -128 {
                        return Err(XllError::Invalid);
                    }
                    let r = REFL_COEFF[k.unsigned_abs() as usize] as i32;
                    bd.adapt_refl_coeff[i][j] = if k < 0 { -r } else { r };
                }
            }

            bd.dmix_embedded = dmix_embedded && (band == 0 || b.bit());

            // MSB / LSB 分开存
            if (band == 0 && scalable) || (band != 0 && b.bit()) {
                bd.lsb_section_size = b.bits(seg_size_nbits) as usize;
                if bd.lsb_section_size > frame_size {
                    return Err(XllError::Invalid);
                }
                // LSB 之后可能有 CRC
                if bd.lsb_section_size != 0 && (crc > 2 || (band == 0 && crc > 1)) {
                    bd.lsb_section_size += 2;
                }
                for i in 0..nch {
                    bd.nscalablelsbs[i] = b.bits(4);
                    if bd.nscalablelsbs[i] != 0 && bd.lsb_section_size == 0 {
                        return Err(XllError::Invalid);
                    }
                }
            } else {
                bd.lsb_section_size = 0;
                bd.nscalablelsbs[..nch].fill(0);
            }
            // 制作时丢掉的位数
            if (band == 0 && scalable) || (band != 0 && b.bit()) {
                for i in 0..nch {
                    bd.bit_width_adjust[i] = b.bits(4);
                }
            } else {
                bd.bit_width_adjust[..nch].fill(0);
            }
        }
        Ok(())
    }
}
