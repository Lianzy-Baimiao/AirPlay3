// 改写自 FFmpeg 的 libavcodec/dca_xll.c、dcadata.c（Copyright (C) 2016 foo86），LGPL 2.1+，见 mod.rs 开头的说明。
//! DTS-HD Master Audio 的无损部分（XLL）：帧头、NAVI 表、PBR（峰值码率平滑）缓冲。
//! 一帧的数据可能跨包：本包用不完的部分留到下一包的前面

use super::bits::Bits;
use super::exss::Asset;

pub(super) const CHSETS_MAX: usize = 3;
pub(super) const CHANNELS_MAX: usize = 8;
pub(super) const DECI_HISTORY: usize = 8;
const PBR_BUFFER_MAX: usize = 240 << 10;
const SYNC_XLL: u32 = 0x41A2_9547;

/// 解不出的原因（FFmpeg 据此决定怎么退回）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum XllError {
    /// 开头不是同步字（跳进了 PBR 平滑期的中间，或要等解码延迟）
    Again,
    /// 数据不对（滤波时出这个错就退回只用核心）
    Invalid,
    /// 其它（不退回）
    Fatal,
}

pub(super) type XResult<T> = Result<T, XllError>;

pub(super) static REFL_COEFF: [u16; 128] = [
    0, 3070, 5110, 7140, 9156, 11154, 13132, 15085, 17010, 18904, 20764, 22588, 24373, 26117, 27818, 29474, 31085, 32648,
    34164, 35631, 37049, 38418, 39738, 41008, 42230, 43404, 44530, 45609, 46642, 47630, 48575, 49477, 50337, 51157,
    51937, 52681, 53387, 54059, 54697, 55302, 55876, 56421, 56937, 57426, 57888, 58326, 58741, 59132, 59502, 59852,
    60182, 60494, 60789, 61066, 61328, 61576, 61809, 62029, 62236, 62431, 62615, 62788, 62951, 63105, 63250, 63386,
    63514, 63635, 63749, 63855, 63956, 64051, 64140, 64224, 64302, 64376, 64446, 64512, 64573, 64631, 64686, 64737,
    64785, 64830, 64873, 64913, 64950, 64986, 65019, 65050, 65079, 65107, 65133, 65157, 65180, 65202, 65222, 65241,
    65259, 65275, 65291, 65306, 65320, 65333, 65345, 65357, 65368, 65378, 65387, 65396, 65405, 65413, 65420, 65427,
    65434, 65440, 65446, 65451, 65456, 65461, 65466, 65470, 65474, 65478, 65481, 65485, 65488, 65491,
];

pub(super) static BAND_COEFF: [i32; 20] = [
    868669, -5931642, -1228483, 4194304, -20577, 122631, -393647, 904476, -1696305, 2825313, -4430736, 6791313, 41153,
    -245210, 785564, -1788164, 3259333, -5074941, 6928550, -8204883,
];

/// n 位“折叠”的有符号数：偶数是正、奇数是负
pub(super) fn get_linear(b: &mut Bits, n: u32) -> i32 {
    let v = b.bits(n);
    ((v >> 1) ^ (v & 1).wrapping_neg()) as i32
}

/// 一元码：连续的 0 的个数，最多 len 个（遇到 1 停，这个 1 也读掉）
fn unary(b: &mut Bits, len: isize) -> u32 {
    let mut n = 0isize;
    while n < len {
        let z = b.peek32().leading_zeros() as isize;
        let room = len - n;
        if z >= room {
            b.skip(room as usize);
            return len as u32;
        }
        if z < 32 {
            b.skip(z as usize + 1);
            return (n + z) as u32;
        }
        b.skip(32);
        n += 32;
    }
    n.max(0) as u32
}

/// 莱斯码（k 位余数），折叠的有符号数
pub(super) fn get_rice(b: &mut Bits, k: u32) -> i32 {
    let left = b.left();
    let v = unary(b, left).wrapping_shl(k) | b.bits(k);
    ((v >> 1) ^ (v & 1).wrapping_neg()) as i32
}

pub(super) fn linear_array(b: &mut Bits, out: &mut [i32], n: u32) {
    if n == 0 {
        out.fill(0);
    } else {
        out.iter_mut().for_each(|v| *v = get_linear(b, n));
    }
}

pub(super) fn rice_array(b: &mut Bits, out: &mut [i32], k: u32) -> XResult<()> {
    for v in out.iter_mut() {
        if b.left() <= k as isize {
            return Err(XllError::Invalid);
        }
        *v = get_rice(b, k);
    }
    Ok(())
}

/// 一个频带（96 kHz 以上分两个频带）
#[derive(Default, Clone)]
pub(super) struct Band {
    pub decor_enabled: bool,
    pub orig_order: [usize; CHANNELS_MAX],
    pub decor_coeff: [i32; CHANNELS_MAX / 2],
    pub adapt_pred_order: [usize; CHANNELS_MAX],
    pub highest_pred_order: usize,
    pub fixed_pred_order: [usize; CHANNELS_MAX],
    pub adapt_refl_coeff: [[i32; 16]; CHANNELS_MAX],
    pub dmix_embedded: bool,
    pub lsb_section_size: usize,
    pub nscalablelsbs: [u32; CHANNELS_MAX],
    pub bit_width_adjust: [u32; CHANNELS_MAX],
    /// 各声道 MSB 样本在 ChSet::msb_buf 里的起点（像 FFmpeg 的指针：去相关后按原来的顺序重排）
    pub msb: [usize; CHANNELS_MAX],
    /// LSB 样本在 ChSet::lsb_buf 里的起点
    pub lsb: [Option<usize>; CHANNELS_MAX],
}

/// 声道组
#[derive(Default)]
pub(super) struct ChSet {
    pub nchannels: usize,
    /// 第 i 位为 0：这个声道是在核心上加的残差
    pub residual_encode: u32,
    pub pcm_bit_res: u32,
    pub storage_bit_res: u32,
    pub freq: u32,
    pub primary_chset: bool,
    pub dmix_coeffs_present: bool,
    pub dmix_embedded: bool,
    pub dmix_type: usize,
    pub hier_chset: bool,
    pub hier_ofs: usize,
    /// 降混系数 [下层的第几个声道][本组的声道]
    pub dmix_coeff: [[i32; CHANNELS_MAX]; 16],
    pub dmix_scale: [i32; 16],
    pub dmix_scale_inv: [i32; 16],
    pub ch_mask: u32,
    pub ch_remap: [usize; CHANNELS_MAX],
    pub nfreqbands: usize,
    pub nabits: u32,
    pub bands: [Band; 2],
    pub seg_common: bool,
    pub rice_code_flag: [bool; CHANNELS_MAX],
    pub bitalloc_hybrid_linear: [u32; CHANNELS_MAX],
    pub bitalloc_part_a: [u32; CHANNELS_MAX],
    pub bitalloc_part_b: [u32; CHANNELS_MAX],
    pub nsamples_part_a: [usize; CHANNELS_MAX],
    pub deci_history: [[i32; DECI_HISTORY]; CHANNELS_MAX],
    pub msb_buf: Vec<i32>,
    pub lsb_buf: Vec<i32>,
    pub asm_buf: Vec<i32>,
}

/// 各扬声器的输出在哪：(声道组, 起点)，MSB 缓冲或合频带后的缓冲
#[derive(Default, Clone, Copy)]
pub(super) enum Out {
    #[default]
    None,
    Msb(usize, usize),
    Asm(usize, usize),
}

#[derive(Default)]
pub(super) struct Xll {
    pub frame_size: usize,
    pub nchsets: usize,
    pub nframesegs: usize,
    pub nsegsamples_log2: u32,
    pub nsegsamples: usize,
    pub nframesamples: usize,
    pub seg_size_nbits: u32,
    pub band_crc_present: u32,
    pub scalable_lsbs: bool,
    pub ch_mask_nbits: u32,
    pub fixed_lsb_width: u32,
    pub chset: [ChSet; CHSETS_MAX],
    pub navi: Vec<usize>,
    pub nfreqbands: usize,
    pub nchannels: usize,
    pub nreschsets: usize,
    pub nactivechsets: usize,
    hd_stream_id: u32,
    pbr: Vec<u8>,
    pbr_delay: u32,
    pub output: [Out; 32],
    pub output_mask: u32,
}

impl Xll {
    fn parse_common_header(&mut self, b: &mut Bits) -> XResult<()> {
        if b.bits(32) != SYNC_XLL {
            return Err(XllError::Again);
        }
        if b.bits(4) + 1 > 1 {
            return Err(XllError::Fatal);
        }
        let header_size = b.bits(8) as usize + 1;
        let nbits = b.bits(5) + 1;
        let frame_size = b.bits(nbits) as usize;
        if frame_size >= PBR_BUFFER_MAX {
            return Err(XllError::Invalid);
        }
        self.frame_size = frame_size + 1;
        self.nchsets = b.bits(4) as usize + 1;
        if self.nchsets > CHSETS_MAX {
            return Err(XllError::Fatal);
        }
        let segs_log2 = b.bits(4);
        self.nframesegs = 1 << segs_log2;
        if self.nframesegs > 1024 {
            return Err(XllError::Invalid);
        }
        self.nsegsamples_log2 = b.bits(4);
        if self.nsegsamples_log2 == 0 {
            return Err(XllError::Invalid);
        }
        self.nsegsamples = 1 << self.nsegsamples_log2;
        if self.nsegsamples > 512 {
            return Err(XllError::Invalid);
        }
        self.nframesamples = 1 << (self.nsegsamples_log2 + segs_log2);
        if self.nframesamples > 65536 {
            return Err(XllError::Invalid);
        }
        self.seg_size_nbits = b.bits(5) + 1;
        self.band_crc_present = b.bits(2);
        self.scalable_lsbs = b.bit();
        self.ch_mask_nbits = b.bits(5) + 1;
        self.fixed_lsb_width = if self.scalable_lsbs { b.bits(4) } else { 0 };
        if !b.seek(header_size * 8) {
            return Err(XllError::Invalid);
        }
        Ok(())
    }

    /// NAVI 表：各频带、段、声道组的数据长度
    fn parse_navi(&mut self, b: &mut Bits) -> XResult<()> {
        if self.nfreqbands * self.nframesegs * self.nchsets > 1024 {
            return Err(XllError::Invalid);
        }
        self.navi.clear();
        for band in 0..self.nfreqbands {
            for _ in 0..self.nframesegs {
                for ci in 0..self.nchsets {
                    let mut size = 0;
                    if self.chset[ci].nfreqbands > band {
                        size = b.bits(self.seg_size_nbits) as usize;
                        if size >= self.frame_size {
                            return Err(XllError::Invalid);
                        }
                        size += 1;
                    }
                    self.navi.push(size);
                }
            }
        }
        // 字节对齐、CRC
        b.align();
        b.skip(16);
        Ok(())
    }

    fn parse_frame(&mut self, data: &[u8], asset: &Asset) -> XResult<()> {
        let mut b = Bits::new(data);
        self.parse_common_header(&mut b)?;
        self.parse_sub_headers(&mut b, asset)?;
        self.parse_navi(&mut b)?;
        self.parse_band_data(&mut b)?;
        // 帧末尾可能有 DTS:X 的扩展（不影响解码）
        if !b.seek(self.frame_size * 8) {
            return Err(XllError::Invalid);
        }
        Ok(())
    }

    fn clear_pbr(&mut self) {
        self.pbr.clear();
        self.pbr_delay = 0;
    }

    fn copy_to_pbr(&mut self, data: &[u8], delay: u32) -> XResult<()> {
        if data.len() > PBR_BUFFER_MAX {
            return Err(XllError::Fatal);
        }
        self.pbr.clear();
        self.pbr.extend_from_slice(data);
        self.pbr_delay = delay;
        Ok(())
    }

    fn parse_frame_no_pbr(&mut self, mut data: &[u8], asset: &Asset) -> XResult<()> {
        let mut r = self.parse_frame(data, asset);
        // 开头不是同步字：跳进了 PBR 平滑期的中间，改从本包里的同步字开始
        if r == Err(XllError::Again) && asset.xll_sync_present && asset.xll_sync_offset < data.len() {
            data = &data[asset.xll_sync_offset..];
            if asset.xll_delay_nframes > 0 {
                // 有解码延迟：先存起来，等够了帧数再解（这期间只用核心）
                self.copy_to_pbr(data, asset.xll_delay_nframes)?;
                return Err(XllError::Again);
            }
            r = self.parse_frame(data, asset);
        }
        r?;
        if self.frame_size > data.len() {
            return Err(XllError::Fatal);
        }
        // 本包没用完：开始 PBR 平滑期
        if self.frame_size < data.len() {
            self.copy_to_pbr(&data[self.frame_size..], 0)?;
        }
        Ok(())
    }

    fn parse_frame_pbr(&mut self, data: &[u8], asset: &Asset) -> XResult<()> {
        if data.len() > PBR_BUFFER_MAX - self.pbr.len() {
            self.clear_pbr();
            return Err(XllError::Fatal);
        }
        self.pbr.extend_from_slice(data);
        // 同步出错之后的解码延迟
        if self.pbr_delay > 0 {
            self.pbr_delay -= 1;
            if self.pbr_delay > 0 {
                return Err(XllError::Again);
            }
        }
        let pbr = std::mem::take(&mut self.pbr);
        let r = self.parse_frame(&pbr, asset);
        self.pbr = pbr;
        if let Err(e) = r {
            self.clear_pbr();
            return Err(e);
        }
        if self.frame_size > self.pbr.len() {
            self.clear_pbr();
            return Err(XllError::Fatal);
        }
        if self.frame_size == self.pbr.len() {
            self.clear_pbr();
        } else {
            self.pbr.drain(..self.frame_size);
        }
        Ok(())
    }

    /// ff_dca_xll_parse：exss 是扩展子流（从它的同步字起）
    pub fn parse(&mut self, exss: &[u8], asset: &Asset) -> XResult<()> {
        if self.hd_stream_id != asset.hd_stream_id {
            self.clear_pbr();
            self.hd_stream_id = asset.hd_stream_id;
        }
        let data = exss.get(asset.xll_offset..asset.xll_offset + asset.xll_size).ok_or(XllError::Fatal)?;
        if self.pbr.is_empty() { self.parse_frame_no_pbr(data, asset) } else { self.parse_frame_pbr(data, asset) }
    }

    pub fn flush(&mut self) {
        self.clear_pbr();
    }
}
