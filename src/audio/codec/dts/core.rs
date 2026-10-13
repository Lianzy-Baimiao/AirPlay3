// 改写自 FFmpeg 的 libavcodec/dca_core.c、dca.c（Copyright (C) 2016 foo86；dca.c 另有 Gildas Bazin、
// Benjamin Zores、Benjamin Larsson、Konstantin Shishkov 的版权），LGPL 2.1+，见 mod.rs 开头的说明。
//! DTS 核心子流（Coherent Acoustics）：帧头、编码头、子帧边信息、子带样本，以及核心里的扩展 XCH / XXCH

use anyhow::{Result, ensure};

use super::bits::{Bits, crc16};
use super::tables::*;

pub(super) const MAX_CH: usize = 7;
pub(super) const SUBBANDS: usize = 32;
pub(super) const SUBBANDS_X96: usize = 64;
const SUBFRAMES: usize = 16;
/// 每个子子帧的子带样本数（DCA_SUBBAND_SAMPLES）
pub(super) const SSF_SAMPLES: usize = 8;
pub(super) const PCMBLOCK: usize = 32;
pub(super) const LFE_HISTORY: usize = 8;
pub(super) const ADPCM: usize = 4;
pub(super) const CODE_BOOKS: usize = 10;

pub(super) const SYNC_CORE_BE: u32 = 0x7FFE_8001;
pub(super) const SYNC_XCH: u32 = 0x5A5A_5A5A;
pub(super) const SYNC_XXCH: u32 = 0x4700_4A03;
pub(super) const SYNC_X96: u32 = 0x1D95_F262;
pub(super) const SYNC_XBR: u32 = 0x655E_315E;
pub(super) const SYNC_REV1AUX: u32 = 0x9A11_05A0;

// 扬声器编号（DCASpeaker）
pub(super) const SPK_L: usize = 1;
pub(super) const SPK_R: usize = 2;
pub(super) const SPK_LS: usize = 3;
pub(super) const SPK_RS: usize = 4;
pub(super) const SPK_LFE1: usize = 5;
pub(super) const SPK_CS: usize = 6;
pub(super) const SPK_LSS: usize = 9;
pub(super) const SPK_RSS: usize = 10;
pub(super) const SPEAKERS: usize = 32;

pub(super) const MASK_LFE1: u32 = 1 << SPK_LFE1;
pub(super) const MASK_CS: u32 = 1 << SPK_CS;

// 解出的扩展（DCAExtensionMask）
pub(super) const CSS_XXCH: u32 = 0x002;
pub(super) const CSS_X96: u32 = 0x004;
pub(super) const CSS_XCH: u32 = 0x008;
pub(super) const EXSS_XBR: u32 = 0x020;
pub(super) const EXSS_XXCH: u32 = 0x040;
pub(super) const EXSS_X96: u32 = 0x080;
pub(super) const EXSS_LBR: u32 = 0x100;
pub(super) const EXSS_XLL: u32 = 0x200;

/// 各声道排列（AMODE）的扬声器
pub(super) static AUDIO_MODE_CH_MASK: [u32; 10] = [0x01, 0x06, 0x06, 0x06, 0x06, 0x07, 0x46, 0x47, 0x1E, 0x1F];

/// 各声道排列里第几个主声道是哪个扬声器
pub(super) static PRM_CH_TO_SPKR: [[i8; 5]; 10] = [
    [0, -1, -1, -1, -1],
    [1, 2, -1, -1, -1],
    [1, 2, -1, -1, -1],
    [1, 2, -1, -1, -1],
    [1, 2, -1, -1, -1],
    [0, 1, 2, -1, -1],
    [1, 2, 6, -1, -1],
    [0, 1, 2, 6, -1],
    [1, 2, 3, 4, -1],
    [0, 1, 2, 3, 4],
];

static BITS_PER_SAMPLE: [u32; 8] = [16, 16, 20, 20, 0, 24, 24, 0];

/// 核心帧头（5.3.1）
#[derive(Clone, Copy, Default)]
pub(super) struct Header {
    pub crc_present: bool,
    pub npcmblocks: usize,
    pub frame_size: usize,
    pub audio_mode: usize,
    pub sample_rate: u32,
    pub bit_rate: u32,
    pub drc_present: bool,
    pub ts_present: bool,
    pub aux_present: bool,
    pub ext_audio_type: u32,
    pub ext_audio_present: bool,
    pub sync_ssf: bool,
    pub lfe_present: u32,
    pub predictor_history: bool,
    pub filter_perfect: bool,
    pub source_pcm_res: u32,
    pub es_format: bool,
    pub sumdiff_front: bool,
    pub sumdiff_surround: bool,
}

/// 读核心帧头（ff_dca_parse_core_frame_header）
pub(super) fn parse_header(b: &mut Bits) -> Result<Header> {
    ensure!(b.bits(32) == SYNC_CORE_BE, "核心同步字不对");
    let mut h = Header::default();
    let _normal_frame = b.bit();
    ensure!(b.bits(5) + 1 == PCMBLOCK as u32, "不支持样本不足的帧");
    h.crc_present = b.bit();
    h.npcmblocks = b.bits(7) as usize + 1;
    ensure!(h.npcmblocks % SSF_SAMPLES == 0, "不支持的 PCM 块数 {}", h.npcmblocks);
    h.frame_size = b.bits(14) as usize + 1;
    ensure!(h.frame_size >= 96, "核心帧太短（{} 字节）", h.frame_size);
    h.audio_mode = b.bits(6) as usize;
    ensure!(h.audio_mode < 10, "不支持的声道排列 {}", h.audio_mode);
    h.sample_rate = SAMPLE_RATES[b.bits(4) as usize];
    ensure!(h.sample_rate != 0, "采样率不对");
    h.bit_rate = BIT_RATES[b.bits(5) as usize];
    ensure!(!b.bit(), "保留位不为 0");
    h.drc_present = b.bit();
    h.ts_present = b.bit();
    h.aux_present = b.bit();
    let _hdcd_master = b.bit();
    h.ext_audio_type = b.bits(3);
    h.ext_audio_present = b.bit();
    h.sync_ssf = b.bit();
    h.lfe_present = b.bits(2);
    ensure!(h.lfe_present != 3, "LFE 标志不对");
    h.predictor_history = b.bit();
    if h.crc_present {
        b.skip(16);
    }
    h.filter_perfect = b.bit();
    let _encoder_rev = b.bits(4);
    let _copy_hist = b.bits(2);
    let pcmr = b.bits(3);
    h.source_pcm_res = BITS_PER_SAMPLE[pcmr as usize];
    ensure!(h.source_pcm_res != 0, "源 PCM 位数不对");
    h.es_format = pcmr & 1 != 0;
    h.sumdiff_front = b.bit();
    h.sumdiff_surround = b.bit();
    let _dn_code = b.bits(4);
    Ok(h)
}

/// 子带样本缓冲（照 FFmpeg 的分配方式：每个声道、子带一段，前面 4 个是 ADPCM 历史；
/// 只在需要更大时重新分配并清零，各段的位置也只在那时重算）
#[derive(Default)]
pub(super) struct SubBuf {
    pub buf: Vec<i32>,
    pub stride: usize,
    bands: usize,
    /// LFE 抽样值的起点（只有核心缓冲用）
    pub lfe: usize,
}

impl SubBuf {
    /// 第 ch 声道第 band 子带的样本起点（历史在它前面 4 个）
    #[inline]
    pub fn at(&self, ch: usize, band: usize) -> usize {
        (ch * self.bands + band) * self.stride + ADPCM
    }

    /// av_fast_mallocz：要的比现有的大时才重分配（清零，多留 1/16 + 32 字节）
    pub fn alloc(&mut self, bands: usize, npcmblocks: usize, extra: usize) {
        let stride = ADPCM + npcmblocks;
        let frame = stride * MAX_CH * bands;
        let need = frame + extra;
        if need > self.buf.len() {
            let bytes = need * 4;
            let size = (bytes + bytes / 16 + 32).max(bytes) / 4;
            self.buf = vec![0; size];
            self.stride = stride;
            self.bands = bands;
            self.lfe = frame;
        }
    }

    /// 清掉各子带前面的 ADPCM 历史
    pub fn erase_history(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        for ch in 0..MAX_CH {
            for band in 0..self.bands {
                let p = self.at(ch, band);
                self.buf[p - ADPCM..p].fill(0);
            }
        }
    }
}

/// 每个声道的合成滤波历史
pub(super) struct DspHist {
    pub hist1: [i32; 1024],
    pub hist2: [i32; 64],
    pub offset: usize,
}

impl Default for DspHist {
    fn default() -> Self {
        DspHist { hist1: [0; 1024], hist2: [0; 64], offset: 0 }
    }
}

/// 64 个子带一组（标准库的数组只给 32 个以内的实现 Default）
#[derive(Clone, Copy)]
pub(super) struct B64<T>(pub [T; SUBBANDS_X96]);

impl<T: Default + Copy> Default for B64<T> {
    fn default() -> Self {
        B64([T::default(); SUBBANDS_X96])
    }
}

impl<T> std::ops::Deref for B64<T> {
    type Target = [T; SUBBANDS_X96];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::DerefMut for B64<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// 核心解码器的状态（DCACoreDecoder 的定点部分）
#[derive(Default)]
pub(super) struct Core {
    pub h: Header,

    // 编码头
    pub nsubframes: usize,
    pub nchannels: usize,
    pub ch_mask: u32,
    pub nsubbands: [usize; MAX_CH],
    pub subband_vq_start: [usize; MAX_CH],
    pub joint_intensity_index: [usize; MAX_CH],
    pub transition_mode_sel: [usize; MAX_CH],
    pub scale_factor_sel: [usize; MAX_CH],
    pub bit_allocation_sel: [usize; MAX_CH],
    pub quant_index_sel: [[usize; CODE_BOOKS]; MAX_CH],
    pub scale_factor_adj: [[i32; CODE_BOOKS]; MAX_CH],

    // 子帧边信息
    pub nsubsubframes: [usize; SUBFRAMES],
    pub prediction_mode: [B64<bool>; MAX_CH],
    pub prediction_vq_index: [B64<usize>; MAX_CH],
    pub bit_allocation: [B64<u32>; MAX_CH],
    pub transition_mode: [[[usize; SUBBANDS]; MAX_CH]; SUBFRAMES],
    pub scale_factors: [[[i32; 2]; SUBBANDS]; MAX_CH],
    pub joint_scale_sel: [usize; MAX_CH],
    pub joint_scale_factors: [B64<i32>; MAX_CH],

    // 扩展
    pub ext_audio_mask: u32,
    pub xch_pos: usize,
    pub xxch_crc_present: bool,
    pub xxch_mask_nbits: u32,
    pub xxch_core_mask: u32,
    pub xxch_spkr_mask: u32,
    pub xxch_dmix_embedded: bool,
    pub xxch_dmix_scale_inv: i32,
    pub xxch_dmix_mask: [u32; 2],
    pub xxch_dmix_coeff: [i32; 12],
    pub xxch_pos: usize,
    pub x96_rev_no: u32,
    pub x96_crc_present: bool,
    pub x96_nchannels: usize,
    pub x96_high_res: bool,
    pub x96_subband_start: usize,
    pub x96_rand: u32,
    pub x96_pos: usize,

    pub sub: SubBuf,
    pub x96: SubBuf,
    pub dsp: [DspHist; MAX_CH],
    pub output: [Vec<i32>; SPEAKERS],
    pub output_history_lfe: i32,
    pub npcmsamples: usize,
    pub output_rate: u32,
    pub filter_mode: u32,
}

/// 编码头的种类
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Core,
    Xch,
    Xxch,
}

impl Core {
    pub fn new() -> Box<Self> {
        let mut c = Box::<Core>::default();
        c.x96_rand = 1;
        c
    }

    /// 跳转后清掉各种历史（ff_dca_core_flush）
    pub fn flush(&mut self) {
        if !self.sub.buf.is_empty() {
            self.sub.erase_history();
            let l = self.sub.lfe;
            self.sub.buf[l..l + LFE_HISTORY].fill(0);
        }
        self.x96.erase_history();
        self.erase_dsp_history();
    }

    pub fn erase_dsp_history(&mut self) {
        self.dsp = Default::default();
        self.output_history_lfe = 0;
    }

    /// 解析核心帧（ff_dca_core_parse）；data 是整个包（已转成 16 位大端）；core_only 时不找核心里的扩展
    pub fn parse(&mut self, data: &[u8], core_only: bool) -> Result<()> {
        self.ext_audio_mask = 0;
        self.xch_pos = 0;
        self.xxch_pos = 0;
        self.x96_pos = 0;
        let mut b = Bits::new(data);
        self.h = parse_header(&mut b)?;
        let n = self.h.npcmblocks;
        self.sub.alloc(SUBBANDS, n, LFE_HISTORY + n / 2);
        if !self.h.predictor_history {
            self.sub.erase_history();
        }
        self.parse_frame_data(&mut b, Kind::Core, 0)?;
        self.parse_optional_info(&mut b, core_only);
        // DTS in WAV 的变通：帧长不超过包长
        self.h.frame_size = self.h.frame_size.min(data.len());
        Ok(())
    }
}
impl Core {
    /// 时间码、辅助数据，再从核心帧末尾往前找扩展的同步字（4 字节对齐，倒着找以免误认）
    fn parse_optional_info(&mut self, b: &mut Bits, core_only: bool) {
        if self.h.ts_present {
            b.skip(32);
        }
        if self.h.aux_present {
            // 辅助数据只有降混系数（不降混用不到）；出错也照样往下解
            let _ = self.parse_aux_data(b);
        }
        if core_only || !self.h.ext_audio_present || !matches!(self.h.ext_audio_type, 0 | 2 | 6) {
            return;
        }
        let d = b.data();
        let mut sync_pos = (self.h.frame_size / 4).min(b.size() / 32) as isize - 1;
        let last_pos = (b.count() / 32) as isize;
        let word = |p: isize| u32::from_be_bytes(d[p as usize * 4..p as usize * 4 + 4].try_into().unwrap());
        let mut w2 = 0u32;
        while sync_pos >= last_pos {
            let w1 = word(sync_pos);
            let p = sync_pos as usize * 4;
            match self.h.ext_audio_type {
                0 if w1 == SYNC_XCH => {
                    // XCH 同步字到核心帧末尾的距离等于 XCH 帧长（老码流可差 1）；再核对 AMODE
                    let size = (w2 >> 22) as usize + 1;
                    let dist = self.h.frame_size as isize - p as isize;
                    if size >= 96 && (size as isize == dist || size as isize - 1 == dist) && (w2 >> 15 & 0x7F) == 0x08 {
                        self.xch_pos = p * 8 + 49;
                        return;
                    }
                }
                2 if w1 == SYNC_X96 => {
                    let size = (w2 >> 20) as usize + 1;
                    if size >= 96 && size as isize == self.h.frame_size as isize - p as isize {
                        self.x96_pos = p * 8 + 44;
                        return;
                    }
                }
                6 if w1 == SYNC_XXCH => {
                    // XXCH 帧头的 CRC 必须对
                    let size = (w2 >> 26) as usize + 1;
                    let dist = d.len() - p;
                    if size >= 11 && size <= dist && crc16(&d[p + 4..p + size]) == 0 {
                        self.xxch_pos = p * 8;
                        return;
                    }
                }
                _ => {}
            }
            w2 = w1;
            sync_pos -= 1;
        }
    }

    fn parse_aux_data(&mut self, b: &mut Bits) -> Result<()> {
        ensure!(b.left() >= 0, "辅助数据越界");
        // 字节数不可信
        b.skip(6);
        let pad = b.count().wrapping_neg() & 31;
        b.skip(pad);
        ensure!(b.bits(32) == SYNC_REV1AUX, "辅助数据同步字不对");
        if b.bit() {
            b.skip(47);
        }
        if b.bit() {
            let ty = b.bits(3) as usize;
            ensure!(ty < 7, "降混类型不对");
            let m = DMIX_PRIMARY_NCH[ty] as usize;
            let n = CHANNELS[self.h.audio_mode] as usize + (self.h.lfe_present != 0) as usize;
            for _ in 0..m * n {
                ensure!((b.bits(9) & 0xFF) < DMIXTABLE.len() as u32, "降混系数不对");
            }
        }
        b.align();
        b.skip(16);
        Ok(())
    }
}
