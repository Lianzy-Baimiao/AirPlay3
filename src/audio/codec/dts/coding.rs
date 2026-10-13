// 改写自 FFmpeg 的 libavcodec/dca_core.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! 编码头（5.3.2）与比例因子

use anyhow::{Result, ensure};

use super::bits::Bits;
use super::core::*;
use super::tables::*;

impl Core {
    /// 主音频编码头；xch_base 之前的声道已在核心里解过
    pub(super) fn parse_coding_header(&mut self, b: &mut Bits, kind: Kind, xch_base: usize) -> Result<()> {
        let header_pos = b.count();
        let mut header_size = 0;
        ensure!(b.left() >= 0, "编码头越界");
        let amode_ch = CHANNELS[self.h.audio_mode] as usize;

        match kind {
            Kind::Core => {
                self.nsubframes = b.bits(4) as usize + 1;
                self.nchannels = b.bits(3) as usize + 1;
                ensure!(self.nchannels == amode_ch, "主声道数 {} 与声道排列不符", self.nchannels);
                self.ch_mask = AUDIO_MODE_CH_MASK[self.h.audio_mode];
                if self.h.lfe_present != 0 {
                    self.ch_mask |= MASK_LFE1;
                }
            }
            Kind::Xch => {
                self.nchannels = amode_ch + 1;
                self.ch_mask |= MASK_CS;
            }
            Kind::Xxch => {
                header_size = b.bits(7) as usize + 1;
                let nchannels = b.bits(3) as usize + 1;
                ensure!(nchannels <= 2, "不支持 {nchannels} 个 XXCH 声道");
                self.nchannels = amode_ch + nchannels;
                let mask = b.bits(self.xxch_mask_nbits - SPK_CS as u32);
                self.xxch_spkr_mask = mask << SPK_CS;
                ensure!(self.xxch_spkr_mask.count_ones() as usize == nchannels, "XXCH 扬声器掩码不对");
                ensure!(self.xxch_core_mask & self.xxch_spkr_mask == 0, "XXCH 扬声器与核心重叠");
                self.ch_mask = self.xxch_core_mask | self.xxch_spkr_mask;

                // 码流里带了降混系数
                if b.bit() {
                    self.xxch_dmix_embedded = b.bit();
                    // FF_DCA_DMIXTABLE_OFFSET = 242 - 201
                    let index = b.bits(6) as isize * 4 - 41 - 3;
                    ensure!((0..INV_DMIXTABLE.len() as isize).contains(&index), "XXCH 降混比例不对");
                    self.xxch_dmix_scale_inv = INV_DMIXTABLE[index as usize];
                    for ch in 0..nchannels {
                        let mask = b.bits(self.xxch_mask_nbits);
                        ensure!(mask & self.xxch_core_mask == mask, "XXCH 降混掩码不对");
                        self.xxch_dmix_mask[ch] = mask;
                    }
                    let mut k = 0;
                    for ch in 0..nchannels {
                        for n in 0..self.xxch_mask_nbits {
                            if self.xxch_dmix_mask[ch] & (1 << n) != 0 {
                                let code = b.bits(7);
                                let c = code & 63;
                                let v = if c != 0 {
                                    let index = (c * 4 - 3) as usize;
                                    ensure!(index < DMIXTABLE.len(), "XXCH 降混系数不对");
                                    if code >> 6 != 0 { DMIXTABLE[index] } else { -DMIXTABLE[index] }
                                } else {
                                    0
                                };
                                *self.xxch_dmix_coeff.get_mut(k).ok_or_else(|| anyhow::anyhow!("XXCH 降混系数太多"))? = v;
                                k += 1;
                            }
                        }
                    }
                } else {
                    self.xxch_dmix_embedded = false;
                }
            }
        }

        let chs = xch_base..self.nchannels;
        for ch in chs.clone() {
            self.nsubbands[ch] = b.bits(5) as usize + 2;
            ensure!(self.nsubbands[ch] <= SUBBANDS, "子带数不对");
        }
        for ch in chs.clone() {
            self.subband_vq_start[ch] = b.bits(5) as usize + 1;
        }
        for ch in chs.clone() {
            let mut n = b.bits(3) as usize;
            if n != 0 && kind == Kind::Xxch {
                n += xch_base - 1;
            }
            ensure!(n <= self.nchannels, "联合强度编码的声道不对");
            self.joint_intensity_index[ch] = n;
        }
        for ch in chs.clone() {
            self.transition_mode_sel[ch] = b.bits(2) as usize;
        }
        for ch in chs.clone() {
            self.scale_factor_sel[ch] = b.bits(3) as usize;
            ensure!(self.scale_factor_sel[ch] != 7, "比例因子码表不对");
        }
        for ch in chs.clone() {
            self.bit_allocation_sel[ch] = b.bits(3) as usize;
            ensure!(self.bit_allocation_sel[ch] != 7, "位分配码表不对");
        }
        for (n, &nbits) in QUANT_INDEX_SEL_NBITS.iter().enumerate() {
            for ch in chs.clone() {
                self.quant_index_sel[ch][n] = b.bits(nbits as u32) as usize;
            }
        }
        for (n, &group) in QUANT_INDEX_GROUP_SIZE.iter().enumerate() {
            for ch in chs.clone() {
                if self.quant_index_sel[ch][n] < group as usize {
                    self.scale_factor_adj[ch][n] = SCALE_FACTOR_ADJ[b.bits(2) as usize];
                }
            }
        }

        if kind == Kind::Xxch {
            // 保留位、字节对齐、CRC
            ensure!(b.seek(header_pos + header_size * 8), "XXCH 声道组头越界");
        } else if self.h.crc_present {
            b.skip(16);
        }
        Ok(())
    }

    /// 比例因子：码表 0 ~ 4 是霍夫曼编码的差值，5、6 是直接的 6 / 7 位
    pub(super) fn parse_scale(b: &mut Bits, scale_index: &mut i32, sel: usize) -> Result<i32> {
        let table: &[i32] = if sel > 5 { &SCALE_FACTOR_QUANT7 } else { &SCALE_FACTOR_QUANT6 };
        if sel < 5 {
            *scale_index += b.vlc(SCALE_FACTOR[sel]);
        } else {
            *scale_index = b.bits(sel as u32 + 1) as i32;
        }
        let i = usize::try_from(*scale_index).ok().filter(|&i| i < table.len());
        Ok(table[i.ok_or_else(|| anyhow::anyhow!("比例因子不对"))?])
    }

    /// 联合编码的比例因子（霍夫曼时也是绝对值），偏 64
    pub(super) fn parse_joint_scale(b: &mut Bits, sel: usize) -> Result<i32> {
        let v = if sel < 5 { b.vlc(SCALE_FACTOR[sel]) } else { b.bits(sel as u32 + 1) as i32 } + 64;
        let i = usize::try_from(v).ok().filter(|&i| i < JOINT_SCALE_FACTORS.len());
        Ok(JOINT_SCALE_FACTORS[i.ok_or_else(|| anyhow::anyhow!("联合比例因子不对"))?])
    }
}
