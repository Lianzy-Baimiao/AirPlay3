// 改写自 FFmpeg 的 libavcodec/dca_core.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! 一帧（或一个扩展声道组）的全部子帧，以及核心里的 XCH / XXCH 扩展声道

use anyhow::{Result, ensure};

use super::bits::Bits;
use super::core::*;
use super::exss::Asset;
use super::tables::CHANNELS;

impl Core {
    pub(super) fn parse_frame_data(&mut self, b: &mut Bits, kind: Kind, xch_base: usize) -> Result<()> {
        self.parse_coding_header(b, kind, xch_base)?;
        let (mut sub_pos, mut lfe_pos) = (0, LFE_HISTORY);
        for sf in 0..self.nsubframes {
            self.parse_subframe_header(b, sf, kind, xch_base)?;
            self.parse_subframe_audio(b, sf, kind, xch_base, &mut sub_pos, &mut lfe_pos)?;
        }

        let n = self.h.npcmblocks;
        for ch in xch_base..self.nchannels {
            // 这个声道实际用到的子带数
            let mut nsubbands = self.nsubbands[ch];
            if let Some(src) = self.joint_intensity_index[ch].checked_sub(1) {
                nsubbands = nsubbands.max(self.nsubbands[src]);
            }
            for band in 0..SUBBANDS {
                let p = self.sub.at(ch, band) - ADPCM;
                if band < nsubbands {
                    // 本帧最后 4 个样本作下一帧 ADPCM 的历史
                    self.sub.buf.copy_within(p + n..p + n + ADPCM, p);
                } else {
                    // 清掉没用到的子带
                    self.sub.buf[p..p + ADPCM + n].fill(0);
                }
            }
        }
        Ok(())
    }

    /// 核心里的 XCH（第 6 个声道 Cs）
    pub(super) fn parse_xch_frame(&mut self, b: &mut Bits) -> Result<()> {
        ensure!(self.ch_mask & MASK_CS == 0, "已经有 Cs 了还有 XCH");
        self.parse_frame_data(b, Kind::Xch, self.nchannels)?;
        // 不信 XCH 里的帧长，直接到核心帧末尾
        ensure!(b.seek(self.h.frame_size * 8), "XCH 帧越界");
        Ok(())
    }

    /// XXCH（核心里或扩展子流里）
    pub(super) fn parse_xxch_frame(&mut self, b: &mut Bits) -> Result<()> {
        let header_pos = b.count();
        ensure!(b.bits(32) == SYNC_XXCH, "XXCH 同步字不对");
        let header_size = b.bits(6) as usize + 1;
        self.xxch_crc_present = b.bit();
        self.xxch_mask_nbits = b.bits(5) + 1;
        ensure!(self.xxch_mask_nbits > SPK_CS as u32, "XXCH 扬声器掩码位数不对");
        let nchsets = b.bits(2) + 1;
        ensure!(nchsets == 1, "不支持 {nchsets} 个 XXCH 声道组");
        let frame_size = b.bits(14) as usize + 1;
        self.xxch_core_mask = b.bits(self.xxch_mask_nbits);

        // 核心的扬声器掩码要与核心一致（Ls/Rs 可以写成 Lss/Rss）
        let mut mask = self.ch_mask;
        if mask & (1 << SPK_LS) != 0 && self.xxch_core_mask & (1 << SPK_LSS) != 0 {
            mask = (mask & !(1 << SPK_LS)) | 1 << SPK_LSS;
        }
        if mask & (1 << SPK_RS) != 0 && self.xxch_core_mask & (1 << SPK_RSS) != 0 {
            mask = (mask & !(1 << SPK_RS)) | 1 << SPK_RSS;
        }
        ensure!(mask == self.xxch_core_mask, "XXCH 的核心扬声器掩码与核心不符");

        ensure!(b.seek(header_pos + header_size * 8), "XXCH 帧头越界");
        self.parse_frame_data(b, Kind::Xxch, self.nchannels)?;
        ensure!(b.seek(header_pos + header_size * 8 + frame_size * 8), "XXCH 声道组越界");
        Ok(())
    }

    /// 核心之后的扩展（ff_dca_core_parse_exss）：packet 是整个包，exss 是扩展子流（从它的同步字起）及其资源；
    /// xll：本包的 XLL 解得出来（那时不用 X96）。扩展解不出时退回只用核心
    pub(super) fn parse_exss(&mut self, packet: &[u8], exss: Option<(&[u8], &Asset)>, xll: bool) -> Result<()> {
        let mask = exss.map_or(0, |(_, a)| a.extension_mask);
        let part = |off: usize, size: usize| -> Result<&[u8]> {
            let d = exss.map_or(&[][..], |(d, _)| d);
            d.get(off..off + size).ok_or_else(|| anyhow::anyhow!("扩展越界"))
        };
        let in_core = |pos: usize| {
            let mut b = Bits::new(packet);
            b.set_pos(pos);
            b
        };

        // (X)XCH
        let (ret, ext) = if mask & EXSS_XXCH != 0 {
            let a = exss.unwrap().1;
            (self.parse_xxch_frame(&mut Bits::new(part(a.xxch_offset, a.xxch_size)?)), EXSS_XXCH)
        } else if self.xxch_pos != 0 {
            (self.parse_xxch_frame(&mut in_core(self.xxch_pos)), CSS_XXCH)
        } else if self.xch_pos != 0 {
            (self.parse_xch_frame(&mut in_core(self.xch_pos)), CSS_XCH)
        } else {
            (Ok(()), 0)
        };
        if ret.is_err() {
            // 退回主声道组
            self.nchannels = CHANNELS[self.h.audio_mode] as usize;
            self.ch_mask = AUDIO_MODE_CH_MASK[self.h.audio_mode];
            if self.h.lfe_present != 0 {
                self.ch_mask |= MASK_LFE1;
            }
        } else {
            self.ext_audio_mask |= ext;
        }

        // XBR
        if mask & EXSS_XBR != 0 {
            let a = exss.unwrap().1;
            if self.parse_xbr_frame(&mut Bits::new(part(a.xbr_offset, a.xbr_size)?)).is_ok() {
                self.ext_audio_mask |= EXSS_XBR;
            }
        }

        // X96（解 XLL 时不用）
        if !xll {
            if mask & EXSS_X96 != 0 {
                let a = exss.unwrap().1;
                if self.parse_x96_frame_exss(&mut Bits::new(part(a.x96_offset, a.x96_size)?)).is_ok() {
                    self.ext_audio_mask |= EXSS_X96;
                }
            } else if self.x96_pos != 0 && self.parse_x96_frame(&mut in_core(self.x96_pos)).is_ok() {
                self.ext_audio_mask |= CSS_X96;
            }
        }
        Ok(())
    }
}
