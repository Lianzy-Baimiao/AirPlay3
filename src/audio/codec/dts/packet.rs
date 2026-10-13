// 改写自 FFmpeg 的 libavcodec/dcadec.c、dca.c（Copyright (C) 2016 foo86；dca.c 另有 Gildas Bazin 等的版权），
// LGPL 2.1+，见 mod.rs 开头的说明。
//! 解一个包（dcadec_decode_frame 的定点部分）：转成 16 位大端、核心、扩展子流、核心扩展、合成输出

use anyhow::{Result, ensure};

use super::core::{Core, EXSS_XLL, SYNC_CORE_BE};
use super::exss::{Exss, SYNC_SUBSTREAM};
use super::output::channel_map;

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

pub(super) struct Decoder {
    core: Box<Core>,
    exss: Exss,
    buf: Vec<u8>,
    /// 只解核心子流（像 ffmpeg 的 -core_only 1）
    pub core_only: bool,
    /// 码流里有 DTS-HD MA 无损部分（XLL，还不支持：只输出有损的核心）
    pub has_xll: bool,
}

impl Decoder {
    pub fn new(core_only: bool) -> Self {
        Decoder { core: Core::new(), exss: Exss::default(), buf: Vec::new(), core_only, has_xll: false }
    }

    /// 跳转后清掉历史
    pub fn flush(&mut self) {
        self.core.flush();
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
        let mut input = 0;
        let has_core = rb32(d) == SYNC_CORE_BE;
        if has_core {
            self.core.parse(d, self.core_only)?;
            // 扩展子流按 4 字节对齐
            let frame_size = self.core.h.frame_size.next_multiple_of(4);
            if d.len() - 4 > frame_size {
                input = frame_size;
            }
        }
        let mut exss_ok = false;
        if !self.core_only && rb32(&d[input..]) == SYNC_SUBSTREAM {
            exss_ok = self.exss.parse(&d[input..]).is_ok();
        }
        ensure!(has_core, "没有核心子流（只有扩展子流的 DTS-HD 还不支持）");
        if !self.core_only {
            let asset = exss_ok.then_some((&d[input..], &self.exss.asset));
            if asset.is_some_and(|(_, a)| a.extension_mask & EXSS_XLL != 0) && !self.has_xll {
                self.has_xll = true;
                log::warn!("DTS-HD MA 的无损部分（XLL）还不支持，只解有损的核心部分");
            }
            self.core.parse_exss(d, asset, false)?;
        }

        self.core.filter_fixed(0)?;
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
