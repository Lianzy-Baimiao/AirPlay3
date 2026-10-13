// 改写自 FFmpeg 的 libavcodec/dca_core.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! X96 的编码头和帧（核心里的、扩展子流里的）

use anyhow::{Result, ensure};

use super::bits::Bits;
use super::core::*;
use super::tables::*;

impl Core {
    fn parse_x96_coding_header(&mut self, b: &mut Bits, exss: bool, xch_base: usize) -> Result<()> {
        let header_pos = b.count();
        let mut header_size = 0;
        ensure!(b.left() >= 0, "X96 编码头越界");
        if exss {
            header_size = b.bits(7) as usize + 1;
        }
        self.x96_high_res = b.bit();
        if self.x96_rev_no < 8 {
            self.x96_subband_start = b.bits(5) as usize;
            ensure!(self.x96_subband_start <= 27, "X96 起始子带不对");
        } else {
            self.x96_subband_start = SUBBANDS;
        }
        let chs = xch_base..self.x96_nchannels;
        for ch in chs.clone() {
            self.nsubbands[ch] = b.bits(6) as usize + 1;
            ensure!(self.nsubbands[ch] >= SUBBANDS, "X96 子带数不对");
        }
        for ch in chs.clone() {
            let mut n = b.bits(3) as usize;
            if n != 0 && xch_base != 0 {
                n += xch_base - 1;
            }
            ensure!(n <= self.x96_nchannels, "X96 联合强度编码的声道不对");
            self.joint_intensity_index[ch] = n;
        }
        for ch in chs.clone() {
            self.scale_factor_sel[ch] = b.bits(3) as usize;
            ensure!(self.scale_factor_sel[ch] < 6, "X96 比例因子码表不对");
        }
        for ch in chs.clone() {
            self.bit_allocation_sel[ch] = b.bits(3) as usize;
        }
        for (n, &nbits) in QUANT_INDEX_SEL_NBITS.iter().enumerate().take(6 + 4 * self.x96_high_res as usize) {
            for ch in chs.clone() {
                self.quant_index_sel[ch][n] = b.bits(nbits as u32) as usize;
            }
        }
        if exss {
            ensure!(b.seek(header_pos + header_size * 8), "X96 声道组头越界");
        } else if self.h.crc_present {
            b.skip(16);
        }
        Ok(())
    }

    fn parse_x96_frame_data(&mut self, b: &mut Bits, exss: bool, xch_base: usize) -> Result<()> {
        self.parse_x96_coding_header(b, exss, xch_base)?;
        let mut sub_pos = 0;
        for sf in 0..self.nsubframes {
            self.parse_x96_subframe_header(b, xch_base)?;
            self.parse_x96_subframe_audio(b, sf, xch_base, &mut sub_pos)?;
        }
        let n = self.h.npcmblocks;
        for ch in xch_base..self.x96_nchannels {
            let mut nsubbands = self.nsubbands[ch];
            if let Some(src) = self.joint_intensity_index[ch].checked_sub(1) {
                nsubbands = nsubbands.max(self.nsubbands[src]);
            }
            // 更新 ADPCM 历史，清掉没用到的子带
            for band in 0..SUBBANDS_X96 {
                let p = self.x96.at(ch, band) - ADPCM;
                if band >= self.x96_subband_start && band < nsubbands {
                    self.x96.buf.copy_within(p + n..p + n + ADPCM, p);
                } else {
                    self.x96.buf[p..p + ADPCM + n].fill(0);
                }
            }
        }
        Ok(())
    }

    fn alloc_x96(&mut self) {
        self.x96.alloc(SUBBANDS_X96, self.h.npcmblocks, 0);
        if !self.h.predictor_history {
            self.x96.erase_history();
        }
    }

    /// 核心里的 X96
    pub(super) fn parse_x96_frame(&mut self, b: &mut Bits) -> Result<()> {
        self.x96_rev_no = b.bits(4);
        ensure!((1..=8).contains(&self.x96_rev_no), "X96 版本不对");
        self.x96_crc_present = false;
        self.x96_nchannels = self.nchannels;
        self.alloc_x96();
        self.parse_x96_frame_data(b, false, 0)?;
        ensure!(b.seek(self.h.frame_size * 8), "X96 帧越界");
        Ok(())
    }

    /// 扩展子流里的 X96
    pub(super) fn parse_x96_frame_exss(&mut self, b: &mut Bits) -> Result<()> {
        let mut frame_size = [0usize; 4];
        let mut nchannels = [0usize; 4];
        let header_pos = b.count();
        ensure!(b.bits(32) == SYNC_X96, "X96 同步字不对");
        let header_size = b.bits(6) as usize + 1;
        self.x96_rev_no = b.bits(4);
        ensure!((1..=8).contains(&self.x96_rev_no), "X96 版本不对");
        self.x96_crc_present = b.bit();
        let nchsets = b.bits(2) as usize + 1;
        for f in &mut frame_size[..nchsets] {
            *f = b.bits(12) as usize + 1;
        }
        for n in &mut nchannels[..nchsets] {
            *n = b.bits(3) as usize + 1;
        }
        ensure!(b.seek(header_pos + header_size * 8), "X96 帧头越界");
        self.alloc_x96();

        self.x96_nchannels = 0;
        let mut base = 0;
        for i in 0..nchsets {
            let pos = b.count();
            if base + nchannels[i] <= self.nchannels {
                self.x96_nchannels = base + nchannels[i];
                self.parse_x96_frame_data(b, true, base)?;
            }
            base += nchannels[i];
            ensure!(b.seek(pos + frame_size[i] * 8), "X96 声道组越界");
        }
        Ok(())
    }
}
