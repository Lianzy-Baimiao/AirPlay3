// 改写自 FFmpeg 的 libavcodec/dca_core.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! XBR（扩展子流里的高码率残差，加到核心子带样本上）

use anyhow::{Result, bail, ensure};

use super::audio::parse_block_codes;
use super::bits::Bits;
use super::core::*;
use super::dsp::dequantize;
use super::tables::*;

impl Core {
    fn parse_xbr_subframe(
        &mut self,
        b: &mut Bits,
        chs: std::ops::Range<usize>,
        nsubbands: &[usize; 32],
        transition: bool,
        sf: usize,
        sub_pos: &mut usize,
    ) -> Result<()> {
        let mut nabits = [0u32; MAX_CH];
        let mut bit_allocation = [[0u32; SUBBANDS]; MAX_CH];
        let mut scale_nbits = [0u32; MAX_CH];
        let mut scale_factors = [[[0i32; 2]; SUBBANDS]; MAX_CH];

        ensure!(*sub_pos + self.nsubsubframes[sf] * SSF_SAMPLES <= self.h.npcmblocks, "子带样本越界");
        ensure!(b.left() >= 0, "XBR 子帧越界");

        for ch in chs.clone() {
            nabits[ch] = b.bits(2) + 2;
        }
        for ch in chs.clone() {
            for ba in &mut bit_allocation[ch][..nsubbands[ch]] {
                *ba = b.bits(nabits[ch]);
                ensure!(*ba <= 26, "XBR 位分配不对");
            }
        }
        for ch in chs.clone() {
            scale_nbits[ch] = b.bits(3);
            ensure!(scale_nbits[ch] != 0, "XBR 比例因子位数不对");
        }
        for ch in chs.clone() {
            let table: &[i32] = if self.scale_factor_sel[ch] > 5 { &SCALE_FACTOR_QUANT7 } else { &SCALE_FACTOR_QUANT6 };
            for band in 0..nsubbands[ch] {
                if bit_allocation[ch][band] != 0 {
                    let mut read = || {
                        let i = b.bits(scale_nbits[ch]) as usize;
                        table.get(i).copied().ok_or_else(|| anyhow::anyhow!("XBR 比例因子不对"))
                    };
                    scale_factors[ch][band][0] = read()?;
                    if transition && self.transition_mode[sf][ch][band] != 0 {
                        scale_factors[ch][band][1] = read()?;
                    }
                }
            }
        }

        let mut ofs = *sub_pos;
        for ssf in 0..self.nsubsubframes[sf] {
            for ch in chs.clone() {
                ensure!(b.left() >= 0, "XBR 子帧越界");
                for band in 0..nsubbands[ch] {
                    let abits = bit_allocation[ch][band];
                    let mut audio = [0i32; 8];
                    if abits > 7 {
                        audio.iter_mut().for_each(|v| *v = b.sbits(abits - 3));
                    } else if abits > 0 {
                        parse_block_codes(b, &mut audio, abits)?;
                    } else {
                        continue;
                    }
                    let trans = if transition { self.transition_mode[sf][ch][band] } else { 0 };
                    let scale = scale_factors[ch][band][(trans != 0 && ssf >= trans) as usize];
                    let p = self.sub.at(ch, band) + ofs;
                    dequantize(&mut self.sub.buf[p..p + SSF_SAMPLES], &audio, LOSSLESS_QUANT[abits as usize], scale, true);
                }
            }
            if (ssf == self.nsubsubframes[sf] - 1 || self.h.sync_ssf) && b.bits(16) != 0xFFFF {
                bail!("XBR 的 DSYNC 不对");
            }
            ofs += SSF_SAMPLES;
        }
        *sub_pos = ofs;
        Ok(())
    }

    pub(super) fn parse_xbr_frame(&mut self, b: &mut Bits) -> Result<()> {
        let mut frame_size = [0usize; 4];
        let mut nchannels = [0usize; 4];
        let mut nsubbands = [0usize; 32];
        let header_pos = b.count();
        ensure!(b.bits(32) == SYNC_XBR, "XBR 同步字不对");
        let header_size = b.bits(6) as usize + 1;
        let nchsets = b.bits(2) as usize + 1;
        for f in &mut frame_size[..nchsets] {
            *f = b.bits(14) as usize + 1;
        }
        let transition = b.bit();
        let mut ch2 = 0;
        for n in &mut nchannels[..nchsets] {
            *n = b.bits(3) as usize + 1;
            let band_nbits = b.bits(2) + 5;
            for _ in 0..*n {
                nsubbands[ch2] = b.bits(band_nbits) as usize + 1;
                ensure!(nsubbands[ch2] <= SUBBANDS, "XBR 子带数不对");
                ch2 += 1;
            }
        }
        ensure!(b.seek(header_pos + header_size * 8), "XBR 帧头越界");

        let mut base = 0;
        for i in 0..nchsets {
            let pos = b.count();
            if base + nchannels[i] <= self.nchannels {
                let mut sub_pos = 0;
                for sf in 0..self.nsubframes {
                    self.parse_xbr_subframe(b, base..base + nchannels[i], &nsubbands, transition, sf, &mut sub_pos)?;
                }
            }
            base += nchannels[i];
            ensure!(b.seek(pos + frame_size[i] * 8), "XBR 声道组越界");
        }
        Ok(())
    }
}
