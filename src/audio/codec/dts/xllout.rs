// 改写自 FFmpeg 的 libavcodec/dca_xll.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! XLL 的输出：残差声道加上核心的有损声道、撤销层级降混、出错恢复时只用有损的部分、按 WAV 顺序交错

use super::core::Core;
use super::dsp::{clip23, mul15, mul16};
use super::output::channel_map;
use super::packet::Pcm;
use super::xll::*;
use super::xllfilt::lsb_width;
use super::xllhdr::is_hier_dmix;

impl Xll {
    /// 各声道组的缓冲够用（出错恢复时用的是上一帧的参数）
    fn buffers_ok(&self) -> bool {
        let n = self.nframesamples;
        self.chset[..self.nactivechsets].iter().all(|c| {
            let deci = if c.nfreqbands > 1 { DECI_HISTORY } else { 0 };
            (0..c.nfreqbands).all(|b| {
                let bd = &c.bands[b];
                (0..c.nchannels).all(|ch| {
                    bd.msb[ch] >= deci
                        && bd.msb[ch] + n <= c.msb_buf.len()
                        && (bd.nscalablelsbs[ch] == 0 || bd.lsb[ch].is_some_and(|l| l + n <= c.lsb_buf.len()))
                })
            })
        })
    }

    /// 撤销层级降混：从下层的声道减去 o 组的声道乘以系数
    fn undo_down_mix(&mut self, oi: usize, band: usize) {
        let n = self.nframesamples;
        let (o_nch, o_hier_ofs) = (self.chset[oi].nchannels, self.chset[oi].hier_ofs);
        let (mut nchannels, mut row) = (0, 0);
        for ci in 0..self.nactivechsets {
            if !self.chset[ci].hier_chset {
                continue;
            }
            for j in 0..self.chset[ci].nchannels {
                for k in 0..o_nch {
                    let o = &self.chset[oi];
                    let coeff = o.dmix_coeff[row.min(15)][k];
                    if coeff == 0 {
                        continue;
                    }
                    let at = o.bands[band].msb[k];
                    let src = o.msb_buf[at..at + n].to_vec();
                    let hist = o.deci_history[k];
                    let c = &mut self.chset[ci];
                    let d = c.bands[band].msb[j];
                    for (x, &s) in c.msb_buf[d..d + n].iter_mut().zip(&src) {
                        *x = x.wrapping_sub(mul15(s, coeff));
                    }
                    if band != 0 {
                        for (x, &s) in c.deci_history[j].iter_mut().zip(&hist) {
                            *x = x.wrapping_sub(mul15(s, coeff));
                        }
                    }
                }
                row += 1;
            }
            nchannels += self.chset[ci].nchannels;
            if nchannels >= o_hier_ofs {
                break;
            }
        }
    }

    /// 出错恢复：清掉频带数据，核心里有的声道都改成“残差”（即直接用核心的有损声道）
    fn force_lossy_output(&mut self, ci: usize, core: &Core) {
        for band in 0..self.chset[ci].nfreqbands {
            self.clear_band(ci, band, None);
        }
        let c = &mut self.chset[ci];
        for ch in 0..c.nchannels {
            if c.residual_encode & (1 << ch) != 0 && core.map_spkr(c.ch_remap[ch]).is_some() {
                c.residual_encode &= !(1 << ch);
            }
        }
    }

    /// 残差声道加上核心的有损声道（降到同样的位数）
    fn combine_residual(&mut self, ci: usize, core: &Core, has_core: bool) -> XResult<()> {
        let n = self.nframesamples;
        if !has_core {
            return Err(XllError::Fatal);
        }
        if self.chset[ci].freq != core.output_rate || n != core.npcmsamples {
            return Err(XllError::Invalid);
        }
        // 本组的降混嵌在下一层里时，先撤销核心的预缩放（残差没有缩放）
        let o = self.next_hier_dmix(ci).map(|oi| self.chset[oi].dmix_scale_inv);
        let fixed = self.fixed_lsb_width;
        let c = &mut self.chset[ci];
        for ch in 0..c.nchannels {
            if c.residual_encode & (1 << ch) != 0 {
                continue;
            }
            let spkr = core.map_spkr(c.ch_remap[ch]).ok_or(XllError::Invalid)?;
            let shift = 24 - c.pcm_bit_res as i32 + lsb_width(fixed, c, 0, ch) as i32;
            if shift > 24 {
                return Err(XllError::Invalid);
            }
            let round = if shift > 0 { 1 << (shift - 1) } else { 0 };
            let scale_inv = o.map(|s| s[(c.hier_ofs + ch).min(15)]);
            let d = c.bands[0].msb[ch];
            for (x, &s) in c.msb_buf[d..d + n].iter_mut().zip(&core.output[spkr]) {
                let v = match scale_inv {
                    Some(si) => clip23(mul16(s, si).wrapping_add(round) >> shift),
                    None => s.wrapping_add(round) >> shift,
                };
                *x = x.wrapping_add(v);
            }
        }
        Ok(())
    }

    /// ff_dca_xll_filter_frame（不降混时）：out 是按 WAV 顺序交错的 24 位 PCM
    pub fn filter_frame(&mut self, core: &Core, has_core: bool, recovery: bool, out: &mut Pcm) -> XResult<()> {
        if recovery {
            for ci in 0..self.nchsets {
                if ci < self.nactivechsets {
                    self.force_lossy_output(ci, core);
                }
                if !self.chset[ci].primary_chset {
                    self.chset[ci].dmix_embedded = false;
                }
            }
            self.scalable_lsbs = false;
            self.fixed_lsb_width = 0;
        }
        if !self.buffers_ok() {
            return Err(XllError::Invalid);
        }

        self.output_mask = 0;
        for ci in 0..self.nactivechsets {
            self.filter_band(ci, 0);
            let c = &self.chset[ci];
            if c.residual_encode != (1 << c.nchannels) - 1 {
                self.combine_residual(ci, core, has_core)?;
            }
            if self.scalable_lsbs {
                self.assemble_msbs_lsbs(ci, 0);
            }
            if self.chset[ci].nfreqbands > 1 {
                self.filter_band(ci, 1);
                self.assemble_msbs_lsbs(ci, 1);
            }
            self.output_mask |= self.chset[ci].ch_mask;
        }
        // 撤销层级降混（所有声道组都解，用不到 FFmpeg 的 scale_down_mix）
        for ci in 1..self.nchsets {
            if is_hier_dmix(&self.chset[ci]) {
                for band in 0..self.chset[ci].nfreqbands {
                    if self.chset[ci].bands[band].dmix_embedded {
                        self.undo_down_mix(ci, band);
                    }
                }
            }
        }
        if self.nfreqbands > 1 {
            for ci in 0..self.nactivechsets {
                self.assemble_freq_bands(ci);
            }
        }

        // 按 WAV 顺序输出：16 位的存储限幅到 16 位（再放到 24 位里）
        let p = &self.chset[0];
        let s16 = p.storage_bit_res == 16;
        let shift = if s16 { 16 } else { 24 } - p.pcm_bit_res;
        let n = self.nframesamples << (self.nfreqbands - 1);
        let (speakers, mask) = channel_map(self.output_mask);
        if speakers.is_empty() {
            return Err(XllError::Fatal);
        }
        let mut planes = Vec::with_capacity(speakers.len());
        for &s in &speakers {
            let plane = match self.output[s] {
                Out::Msb(ci, at) => self.chset[ci].msb_buf.get(at..at + n),
                Out::Asm(ci, at) => self.chset[ci].asm_buf.get(at..at + n),
                Out::None => None,
            };
            planes.push(plane.ok_or(XllError::Invalid)?);
        }
        out.samples.clear();
        out.samples.reserve(n * speakers.len());
        for i in 0..n {
            out.samples.extend(planes.iter().map(|pl| {
                let v = pl[i].wrapping_mul(1 << shift);
                if s16 { v.clamp(-32768, 32767) << 8 } else { clip23(v) }
            }));
        }
        out.frames = n;
        out.channels = speakers.len();
        out.rate = p.freq << (self.nfreqbands - 1);
        out.mask = mask;
        out.speakers = speakers;
        Ok(())
    }
}
