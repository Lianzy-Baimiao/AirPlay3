// 改写自 FFmpeg 的 libavcodec/dca_xll.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! XLL 的频带数据：每段的莱斯码 / 混合莱斯码 / 直接存的残差，第二个频带的抽取器历史，LSB 部分

use super::bits::Bits;
use super::xll::*;

impl Xll {
    /// MSB 缓冲：每个频带、声道一段（两个频带时每段前面留 8 个抽取器历史）
    fn alloc_msb(&mut self, ci: usize) {
        let c = &mut self.chset[ci];
        let ndeci = if c.nfreqbands > 1 { DECI_HISTORY } else { 0 };
        let nchsamples = self.nframesamples + ndeci;
        let need = nchsamples * c.nchannels * c.nfreqbands;
        if c.msb_buf.len() < need {
            c.msb_buf.resize(need, 0);
        }
        let mut p = ndeci;
        for band in 0..c.nfreqbands {
            for ch in 0..c.nchannels {
                c.bands[band].msb[ch] = p;
                p += nchsamples;
            }
        }
    }

    fn alloc_lsb(&mut self, ci: usize) {
        let n = self.nframesamples;
        let c = &mut self.chset[ci];
        let need: usize = (0..c.nfreqbands).filter(|&b| c.bands[b].lsb_section_size != 0).count() * n * c.nchannels;
        if need == 0 {
            return;
        }
        if c.lsb_buf.len() < need {
            c.lsb_buf.resize(need, 0);
        }
        let mut p = 0;
        for band in 0..c.nfreqbands {
            for ch in 0..c.nchannels {
                c.bands[band].lsb[ch] = if c.bands[band].lsb_section_size != 0 {
                    p += n;
                    Some(p - n)
                } else {
                    None
                };
            }
        }
    }

    pub(super) fn parse_band_data(&mut self, b: &mut Bits) -> XResult<()> {
        for ci in 0..self.nactivechsets {
            self.alloc_msb(ci);
            self.alloc_lsb(ci);
        }
        let mut navi_pos = b.count();
        let mut k = 0;
        for band in 0..self.nfreqbands {
            for seg in 0..self.nframesegs {
                for ci in 0..self.nchsets {
                    if self.chset[ci].nfreqbands > band {
                        navi_pos += self.navi[k] * 8;
                        if navi_pos > b.size() {
                            return Err(XllError::Invalid);
                        }
                        if ci < self.nactivechsets && self.parse_segment(b, ci, band, seg, navi_pos).is_err() {
                            self.clear_band(ci, band, Some(seg));
                        }
                        b.set_pos(navi_pos);
                    }
                    k += 1;
                }
            }
        }
        Ok(())
    }

    fn parse_segment(&mut self, b: &mut Bits, ci: usize, band: usize, seg: usize, band_data_end: usize) -> XResult<()> {
        let (nseg, nseg_log2) = (self.nsegsamples, self.nsegsamples_log2);
        let c = &mut self.chset[ci];
        let nch = c.nchannels;

        // 编码参数（第 1 段之后可以沿用上一段的）
        if !(seg != 0 && b.bit()) {
            c.seg_common = b.bit();
            let k = if c.seg_common { 1 } else { nch };
            for i in 0..k {
                c.rice_code_flag[i] = b.bit();
                c.bitalloc_hybrid_linear[i] =
                    if !c.seg_common && c.rice_code_flag[i] && b.bit() { b.bits(c.nabits) + 1 } else { 0 };
            }
            for i in 0..k {
                if seg == 0 {
                    c.bitalloc_part_a[i] = b.bits(c.nabits);
                    if !c.rice_code_flag[i] && c.bitalloc_part_a[i] != 0 {
                        c.bitalloc_part_a[i] += 1;
                    }
                    let bd = &c.bands[band];
                    c.nsamples_part_a[i] = if c.seg_common { bd.highest_pred_order } else { bd.adapt_pred_order[i] };
                } else {
                    c.bitalloc_part_a[i] = 0;
                    c.nsamples_part_a[i] = 0;
                }
                c.bitalloc_part_b[i] = b.bits(c.nabits);
                if !c.rice_code_flag[i] && c.bitalloc_part_b[i] != 0 {
                    c.bitalloc_part_b[i] += 1;
                }
            }
        }

        // 残差：A 部分（第 0 段开头、预测阶数那么多个）和 B 部分
        for i in 0..nch {
            let k = if c.seg_common { 0 } else { i };
            let na = c.nsamples_part_a[k].min(nseg);
            let start = c.bands[band].msb[i] + seg * nseg;
            let (rice, hybrid) = (c.rice_code_flag[k], c.bitalloc_hybrid_linear[k]);
            let (ba, bb) = (c.bitalloc_part_a[k], c.bitalloc_part_b[k]);
            if b.left() < 0 {
                return Err(XllError::Invalid);
            }
            let seg_buf = &mut c.msb_buf[start..start + nseg];
            let (part_a, part_b) = seg_buf.split_at_mut(na);
            if !rice {
                linear_array(b, part_a, ba);
                linear_array(b, part_b, bb);
            } else {
                rice_array(b, part_a, ba)?;
                if hybrid != 0 {
                    // 混合莱斯码：几个孤立的大样本直接存
                    let niso = b.bits(nseg_log2);
                    part_b.fill(0);
                    for _ in 0..niso {
                        let loc = b.bits(nseg_log2) as usize;
                        if loc >= part_b.len() {
                            return Err(XllError::Invalid);
                        }
                        part_b[loc] = -1;
                    }
                    for v in part_b.iter_mut() {
                        *v = if *v != 0 { get_linear(b, hybrid) } else { get_rice(b, bb) };
                    }
                } else {
                    rice_array(b, part_b, bb)?;
                }
            }
        }

        // 第二个频带的抽取器历史
        if seg == 0 && band == 1 {
            let nbits = b.bits(5) + 1;
            for i in 0..nch {
                for j in 1..DECI_HISTORY {
                    c.deci_history[i][j] = b.sbits(nbits);
                }
            }
        }

        // LSB 部分在本段数据的末尾
        let lsb_size = c.bands[band].lsb_section_size;
        if lsb_size != 0 {
            if !band_data_end.checked_sub(lsb_size * 8).is_some_and(|p| b.seek(p)) {
                return Err(XllError::Invalid);
            }
            for i in 0..nch {
                let n = c.bands[band].nscalablelsbs[i];
                if n != 0 {
                    let at = c.bands[band].lsb[i].ok_or(XllError::Invalid)? + seg * nseg;
                    c.lsb_buf[at..at + nseg].iter_mut().for_each(|v| *v = b.bits(n) as i32);
                }
            }
        }
        if !b.seek(band_data_end) {
            return Err(XllError::Invalid);
        }
        Ok(())
    }

    /// 清掉一段（seg = None 时整帧）的频带数据
    pub(super) fn clear_band(&mut self, ci: usize, band: usize, seg: Option<usize>) {
        let (ofs, n) = match seg {
            Some(s) => (s * self.nsegsamples, self.nsegsamples),
            None => (0, self.nframesamples),
        };
        let c = &mut self.chset[ci];
        let bd = &c.bands[band];
        for ch in 0..c.nchannels {
            let p = bd.msb[ch] + ofs;
            if let Some(v) = c.msb_buf.get_mut(p..p + n) {
                v.fill(0);
            }
            if bd.lsb_section_size != 0 {
                if let Some(v) = bd.lsb[ch].and_then(|l| c.lsb_buf.get_mut(l + ofs..l + ofs + n)) {
                    v.fill(0);
                }
            }
        }
        if seg.is_none_or(|s| s == 0) && band != 0 {
            c.deci_history = Default::default();
        }
        if seg.is_none() {
            c.bands[band].nscalablelsbs = [0; CHANNELS_MAX];
            c.bands[band].bit_width_adjust = [0; CHANNELS_MAX];
        }
    }
}
