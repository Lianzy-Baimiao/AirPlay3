// 改写自 FFmpeg 的 libavcodec/dca_exss.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! 扩展子流（EXSS）的头和音频资源描述：只取第一个资源里各扩展（XBR / XXCH / X96 / XLL …）的位置

use anyhow::{Result, bail, ensure};

use super::bits::Bits;
use super::core::{EXSS_LBR, EXSS_X96, EXSS_XBR, EXSS_XLL, EXSS_XXCH};

pub(super) const SYNC_SUBSTREAM: u32 = 0x6458_2025;
pub(super) const EXSS_CORE: u32 = 0x010;
const EXSS_RSV1: u32 = 0x400;
const EXSS_RSV2: u32 = 0x800;

static SAMPLING_FREQS: [u32; 16] =
    [8000, 16000, 32000, 64000, 128000, 22050, 44100, 88200, 176400, 352800, 12000, 24000, 48000, 96000, 192000, 384000];

/// 一组扬声器对（DCASpeakerPair）里的声道数
pub(super) fn count_chs_for_mask(mask: u32) -> u32 {
    ((mask & 0xFFFF) | ((mask & 0xAE66) << 16)).count_ones()
}

/// 音频资源（各字段跨帧保留，与 FFmpeg 一样）
#[derive(Default, Clone)]
pub(super) struct Asset {
    pub extension_mask: u32,
    pub nchannels_total: u32,
    pub pcm_bit_res: u32,
    pub max_sample_rate: u32,
    pub one_to_one_map_ch_to_spkr: bool,
    pub embedded_stereo: bool,
    pub embedded_6ch: bool,
    pub spkr_mask_enabled: bool,
    pub spkr_mask: u32,
    pub representation_type: u32,
    pub coding_mode: u32,
    pub asset_offset: usize,
    pub asset_size: usize,
    pub core_offset: usize,
    pub core_size: usize,
    pub xbr_offset: usize,
    pub xbr_size: usize,
    pub xxch_offset: usize,
    pub xxch_size: usize,
    pub x96_offset: usize,
    pub x96_size: usize,
    pub lbr_offset: usize,
    pub lbr_size: usize,
    pub xll_offset: usize,
    pub xll_size: usize,
    pub xll_sync_present: bool,
    pub xll_delay_nframes: u32,
    pub xll_sync_offset: usize,
    pub hd_stream_id: u32,
}

#[derive(Default)]
pub(super) struct Exss {
    exss_index: u32,
    size_nbits: u32,
    pub exss_size: usize,
    static_fields_present: bool,
    mix_metadata_enabled: bool,
    nmixoutconfigs: usize,
    nmixoutchs: [u32; 4],
    pub asset: Asset,
}

impl Exss {
    /// ff_dca_exss_parse：data 从扩展子流的同步字开始
    pub fn parse(&mut self, data: &[u8]) -> Result<()> {
        let mut b = Bits::new(data);
        b.skip(32 + 8);
        self.exss_index = b.bits(2);
        let wide = b.bit() as u32;
        let header_size = b.bits(8 + 4 * wide) as usize + 1;
        self.size_nbits = 16 + 4 * wide;
        self.exss_size = b.bits(self.size_nbits) as usize + 1;
        ensure!(self.exss_size <= data.len(), "包装不下扩展子流");

        let mut nassets = 1;
        self.static_fields_present = b.bit();
        if self.static_fields_present {
            // 参考时钟、帧长、时间码
            b.skip(5);
            if b.bit() {
                b.skip(36);
            }
            let npresents = b.bits(3) + 1;
            ensure!(npresents == 1, "不支持 {npresents} 个音频呈现");
            nassets = b.bits(3) + 1;
            ensure!(nassets == 1, "不支持 {nassets} 个音频资源");
            let active = b.bits(self.exss_index + 1);
            b.skip(active.count_ones() as usize * 8);
            self.mix_metadata_enabled = b.bit();
            if self.mix_metadata_enabled {
                b.skip(2);
                let nbits = (b.bits(2) + 1) << 2;
                self.nmixoutconfigs = b.bits(2) as usize + 1;
                for i in 0..self.nmixoutconfigs {
                    self.nmixoutchs[i] = count_chs_for_mask(b.bits(nbits));
                }
            }
        }

        let mut offset = header_size;
        for _ in 0..nassets {
            self.asset.asset_offset = offset;
            self.asset.asset_size = b.bits(self.size_nbits) as usize + 1;
            offset += self.asset.asset_size;
            ensure!(offset <= self.exss_size, "扩展子流的资源越界");
        }
        for _ in 0..nassets {
            self.parse_descriptor(&mut b)?;
            self.set_offsets()?;
        }
        ensure!(b.seek(header_size * 8), "扩展子流头越界");
        Ok(())
    }

    /// 依次排列的各扩展的位置
    fn set_offsets(&mut self) -> Result<()> {
        let a = &mut self.asset;
        let (mut offs, mut size) = (a.asset_offset, a.asset_size);
        let parts: [(u32, &mut usize, usize); 6] = [
            (EXSS_CORE, &mut a.core_offset, a.core_size),
            (EXSS_XBR, &mut a.xbr_offset, a.xbr_size),
            (EXSS_XXCH, &mut a.xxch_offset, a.xxch_size),
            (EXSS_X96, &mut a.x96_offset, a.x96_size),
            (EXSS_LBR, &mut a.lbr_offset, a.lbr_size),
            (EXSS_XLL, &mut a.xll_offset, a.xll_size),
        ];
        for (bit, off, len) in parts {
            if a.extension_mask & bit != 0 {
                *off = offs;
                if len > size {
                    bail!("扩展子流里的扩展长度不对");
                }
                offs += len;
                size -= len;
            }
        }
        Ok(())
    }
}
impl Exss {
    fn parse_descriptor(&mut self, b: &mut Bits) -> Result<()> {
        let descr_pos = b.count();
        let descr_size = b.bits(9) as usize + 1;
        let _asset_index = b.bits(3);
        let a = &mut self.asset;

        if self.static_fields_present {
            // 资源类型、语言
            if b.bit() {
                b.skip(4);
            }
            if b.bit() {
                b.skip(24);
            }
            // 附加文字
            if b.bit() {
                let text = b.bits(10) as usize + 1;
                ensure!(b.left() >= (text * 8) as isize, "扩展子流的文字越界");
                b.skip(text * 8);
            }
            a.pcm_bit_res = b.bits(5) + 1;
            a.max_sample_rate = SAMPLING_FREQS[b.bits(4) as usize];
            a.nchannels_total = b.bits(8) + 1;
            a.one_to_one_map_ch_to_spkr = b.bit();
            if a.one_to_one_map_ch_to_spkr {
                a.embedded_stereo = a.nchannels_total > 2 && b.bit();
                a.embedded_6ch = a.nchannels_total > 6 && b.bit();
                let mut nbits = 0;
                a.spkr_mask_enabled = b.bit();
                if a.spkr_mask_enabled {
                    nbits = (b.bits(2) + 1) << 2;
                    a.spkr_mask = b.bits(nbits);
                }
                let nsets = b.bits(3) as usize;
                ensure!(nsets == 0 || nbits != 0, "没有扬声器掩码却有重映射");
                let mut nspeakers = [0u32; 8];
                for n in &mut nspeakers[..nsets] {
                    *n = count_chs_for_mask(b.bits(nbits));
                }
                for &n in &nspeakers[..nsets] {
                    let nch = b.bits(5) + 1;
                    for _ in 0..n {
                        let m = b.bits(nch);
                        b.skip(m.count_ones() as usize * 5);
                    }
                }
            } else {
                a.embedded_stereo = false;
                a.embedded_6ch = false;
                a.spkr_mask_enabled = false;
                a.spkr_mask = 0;
                a.representation_type = b.bits(3);
            }
        }

        // 动态范围、对白归一化
        let drc = b.bit();
        if drc {
            b.skip(8);
        }
        if b.bit() {
            b.skip(5);
        }
        if drc && a.embedded_stereo {
            b.skip(8);
        }
        // 混音元数据
        if self.mix_metadata_enabled && b.bit() {
            b.skip(1 + 6);
            let lim = b.bits(2);
            b.skip(if lim == 3 { 8 } else { 3 });
            if b.bit() {
                for i in 0..self.nmixoutconfigs {
                    b.skip(6 * self.nmixoutchs[i] as usize);
                }
            } else {
                b.skip(6 * self.nmixoutconfigs);
            }
            let nch = a.nchannels_total + if a.embedded_6ch { 6 } else { 0 } + if a.embedded_stereo { 2 } else { 0 };
            for i in 0..self.nmixoutconfigs {
                ensure!(self.nmixoutchs[i] != 0, "混音配置的扬声器掩码不对");
                for _ in 0..nch {
                    let m = b.bits(self.nmixoutchs[i]);
                    b.skip(m.count_ones() as usize * 6);
                }
            }
        }

        // 各编码成分
        a.coding_mode = b.bits(2);
        let nbits = self.size_nbits;
        match a.coding_mode {
            0 => {
                a.extension_mask = b.bits(12);
                if a.extension_mask & EXSS_CORE != 0 {
                    a.core_size = b.bits(14) as usize + 1;
                    if b.bit() {
                        b.skip(2);
                    }
                }
                if a.extension_mask & EXSS_XBR != 0 {
                    a.xbr_size = b.bits(14) as usize + 1;
                }
                if a.extension_mask & EXSS_XXCH != 0 {
                    a.xxch_size = b.bits(14) as usize + 1;
                }
                if a.extension_mask & EXSS_X96 != 0 {
                    a.x96_size = b.bits(12) as usize + 1;
                }
                if a.extension_mask & EXSS_LBR != 0 {
                    lbr_parameters(b, a);
                }
                if a.extension_mask & EXSS_XLL != 0 {
                    xll_parameters(b, a, nbits);
                }
                if a.extension_mask & EXSS_RSV1 != 0 {
                    b.skip(16);
                }
                if a.extension_mask & EXSS_RSV2 != 0 {
                    b.skip(16);
                }
            }
            1 => {
                a.extension_mask = EXSS_XLL;
                xll_parameters(b, a, nbits);
            }
            2 => {
                a.extension_mask = EXSS_LBR;
                lbr_parameters(b, a);
            }
            _ => {
                a.extension_mask = 0;
                b.skip(14 + 8);
                if b.bit() {
                    b.skip(3);
                }
            }
        }
        if a.extension_mask & EXSS_XLL != 0 {
            a.hd_stream_id = b.bits(3);
        }
        ensure!(b.seek(descr_pos + descr_size * 8), "资源描述越界");
        Ok(())
    }
}

fn lbr_parameters(b: &mut Bits, a: &mut Asset) {
    a.lbr_size = b.bits(14) as usize + 1;
    if b.bit() {
        b.skip(2);
    }
}

fn xll_parameters(b: &mut Bits, a: &mut Asset, nbits: u32) {
    a.xll_size = b.bits(nbits) as usize + 1;
    a.xll_sync_present = b.bit();
    if a.xll_sync_present {
        b.skip(4);
        let delay_nbits = b.bits(5) + 1;
        a.xll_delay_nframes = b.bits(delay_nbits);
        a.xll_sync_offset = b.bits(nbits) as usize;
    } else {
        a.xll_delay_nframes = 0;
        a.xll_sync_offset = 0;
    }
}
