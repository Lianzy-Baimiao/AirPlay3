// 改写自 FFmpeg 的 libavcodec/dcadec.c、dca.c（Copyright (C) 2016 foo86；dca.c 另有 Gildas Bazin 等的版权），
// LGPL 2.1+，见 mod.rs 开头的说明。
//! 解一个包（dcadec_decode_frame 的定点部分）：转成 16 位大端、核心、扩展子流、无损部分（XLL）、
//! 核心扩展、合成输出

use anyhow::{Result, bail, ensure};

use super::core::{Core, EXSS_XLL, SYNC_CORE_BE};
use super::exss::{Exss, SYNC_SUBSTREAM};
use super::output::channel_map;
use super::xll::{Xll, XllError};

const MIN_PACKET: usize = 16;
const MAX_PACKET: usize = 0x10_4000;

fn rb32(d: &[u8]) -> u32 {
    d.get(..4).map_or(0, |b| u32::from_be_bytes(b.try_into().unwrap()))
}

/// avpriv_dca_convert_bitstream：小端、14 位的码流转成 16 位大端；不是 DTS 同步字时返回 false
fn convert(src: &[u8], dst: &mut Vec<u8>) -> bool {
    dst.clear();
    let mrk = rb32(src);
    let word = |i: usize, le: bool| {
        let (a, b) = (src[2 * i], src.get(2 * i + 1).copied().unwrap_or(0));
        if le { u16::from_le_bytes([a, b]) } else { u16::from_be_bytes([a, b]) }
    };
    match mrk {
        SYNC_CORE_BE | SYNC_SUBSTREAM => dst.extend_from_slice(src),
        0xFE7F_0180 => {
            for i in 0..src.len().div_ceil(2) {
                dst.extend_from_slice(&word(i, true).to_be_bytes());
            }
            dst.truncate(src.len());
        }
        0x1FFF_E800 | 0xFF1F_00E8 => {
            let le = mrk == 0xFF1F_00E8;
            let (mut acc, mut n) = (0u32, 0);
            for i in 0..src.len().div_ceil(2) {
                acc = acc << 14 | (word(i, le) & 0x3FFF) as u32;
                n += 14;
                while n >= 8 {
                    n -= 8;
                    dst.push((acc >> n) as u8);
                }
            }
            if n > 0 {
                dst.push((acc << (8 - n)) as u8);
            }
        }
        _ => return false,
    }
    true
}

/// 一个包解出的 PCM：按 WAV 顺序交错的 24 位整数
#[derive(Default)]
pub(super) struct Pcm {
    pub samples: Vec<i32>,
    pub frames: usize,
    pub channels: usize,
    pub rate: u32,
    /// WAV 声道掩码
    pub mask: u32,
    /// 各声道对应的 DTS 扬声器（channel_map 的顺序）
    pub speakers: Vec<usize>,
}

// 包里解出了什么（DCA_PACKET_*）
const PACKET_CORE: u32 = 0x01;
const PACKET_EXSS: u32 = 0x02;
const PACKET_XLL: u32 = 0x04;
const PACKET_MASK: u32 = 0x0F;
/// XLL 同步出错后的恢复：这一帧只用有损的部分
const PACKET_RECOVERY: u32 = 0x10;
/// 核心可以给残差用（上一包的核心是好的）
const PACKET_RESIDUAL: u32 = 0x20;

pub(super) struct Decoder {
    core: Box<Core>,
    exss: Exss,
    xll: Box<Xll>,
    buf: Vec<u8>,
    packet: u32,
    /// 只解核心子流（像 ffmpeg 的 -core_only 1）
    pub core_only: bool,
    /// 上一包是按无损部分（XLL）输出的
    pub via_xll: bool,
    /// 上一包里有无损部分（不一定解得出：可能还没同步上）
    pub xll_asset: bool,
}

impl Decoder {
    pub fn new(core_only: bool) -> Self {
        Decoder {
            core: Core::new(),
            exss: Exss::default(),
            xll: Box::default(),
            buf: Vec::new(),
            packet: 0,
            core_only,
            via_xll: false,
            xll_asset: false,
        }
    }

    /// 跳转后清掉历史（dcadec_flush）
    pub fn flush(&mut self) {
        self.core.flush();
        self.xll.flush();
        self.packet &= PACKET_MASK;
    }

    pub fn decode(&mut self, packet: &[u8], out: &mut Pcm) -> Result<()> {
        ensure!((MIN_PACKET..=MAX_PACKET).contains(&packet.len()), "包长不对（{} 字节）", packet.len());
        let mrk = rb32(packet);
        let mut buf = std::mem::take(&mut self.buf);
        let d: &[u8] = if mrk == SYNC_CORE_BE || mrk == SYNC_SUBSTREAM {
            packet
        } else {
            let ok = (0..packet.len() - MIN_PACKET + 1).any(|i| convert(&packet[i..], &mut buf));
            ensure!(ok, "不是 DTS 包");
            &buf
        };
        let r = self.decode_be(d, out);
        self.buf = buf;
        r
    }

    fn decode_be(&mut self, d: &[u8], out: &mut Pcm) -> Result<()> {
        let prev = self.packet;
        self.packet = 0;
        self.via_xll = false;
        self.xll_asset = false;
        let mut input = 0;
        if rb32(d) == SYNC_CORE_BE {
            self.core.parse(d, self.core_only)?;
            self.packet |= PACKET_CORE;
            // 扩展子流按 4 字节对齐
            let frame_size = self.core.h.frame_size.next_multiple_of(4);
            if d.len() - 4 > frame_size {
                input = frame_size;
            }
        }
        let has_core = self.packet & PACKET_CORE != 0;

        if !self.core_only {
            let exss = &d[input..];
            if rb32(exss) == SYNC_SUBSTREAM && self.exss.parse(exss).is_ok() {
                self.packet |= PACKET_EXSS;
            }
            let asset = (self.packet & PACKET_EXSS != 0).then_some((exss, &self.exss.asset));
            if let Some((data, a)) = asset.filter(|(_, a)| a.extension_mask & EXSS_XLL != 0) {
                self.xll_asset = true;
                match self.xll.parse(data, a) {
                    Ok(()) => self.packet |= PACKET_XLL,
                    // 同步出错：上一包有无损部分时用有损的部分顶上
                    Err(XllError::Again) if prev & PACKET_XLL != 0 && has_core => {
                        self.packet |= PACKET_XLL | PACKET_RECOVERY;
                    }
                    Err(_) => {}
                }
            }
            if has_core {
                self.core.parse_exss(d, asset, self.packet & PACKET_XLL != 0)?;
            }
        }

        if self.packet & PACKET_XLL != 0 {
            if has_core {
                // 无损部分是 96 kHz、核心是 48 kHz 时用 64 子带合成
                let x96 = self.xll.chset[0].freq == 96000 && self.core.h.sample_rate == 48000;
                self.core.filter_fixed(if x96 { 1 } else { -1 })?;
                // 跳转后的第一个包：有多个声道组时只用有损的部分（与参考解码器一样，免得有咔嗒声）
                if prev & PACKET_RESIDUAL == 0 && self.xll.nreschsets > 0 && self.xll.nchsets > 1 {
                    self.packet |= PACKET_RECOVERY;
                }
                self.packet |= PACKET_RESIDUAL;
            }
            let recovery = self.packet & PACKET_RECOVERY != 0;
            match self.xll.filter_frame(&self.core, has_core, recovery, out) {
                Ok(()) => {
                    self.via_xll = true;
                    return Ok(());
                }
                // 数据不对时退回只用核心（核心已经合成好了）
                Err(XllError::Invalid) if has_core => return self.core_output(out),
                Err(e) => bail!("DTS-HD MA 的无损部分解不出（{e:?}）"),
            }
        }
        ensure!(has_core, "没有能解的子流");
        self.core.filter_fixed(0)?;
        self.core_output(out)?;
        self.packet |= PACKET_RESIDUAL;
        Ok(())
    }

    /// 只用核心输出（filter_frame_fixed 合成之后的部分）
    fn core_output(&mut self, out: &mut Pcm) -> Result<()> {
        out.channels = self.core.finish_frame(&mut out.samples)?;
        ensure!(out.channels > 0, "没有声道");
        out.frames = self.core.npcmsamples;
        out.rate = self.core.output_rate;
        let (speakers, mask) = channel_map(self.core.ch_mask);
        out.speakers = speakers;
        out.mask = mask;
        Ok(())
    }
}
