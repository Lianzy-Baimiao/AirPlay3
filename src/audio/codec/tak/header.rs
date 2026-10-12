// TAK 的文件头（元数据块）、流信息、帧头、CRC。
// 改写自 FFmpeg 的 libavcodec/tak.c 与 libavformat/takdec.c（Copyright (c) 2012 Paul B Mahol），
// 以 GNU LGPL 2.1 或更新版本发布，见 mod.rs 开头的说明。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use anyhow::Result;

use crate::audio::codec::bits::LsbReader as Bits;

pub(super) const FLAG_LAST: u32 = 1;
pub(super) const FLAG_HAS_INFO: u32 = 2;
pub(super) const FLAG_HAS_METADATA: u32 = 4;
pub(super) const CODEC_MONO_STEREO: u32 = 2;
pub(super) const CODEC_MULTICHANNEL: u32 = 4;
/// 帧头最长的字节数（含流信息与 CRC）
pub(super) const MAX_HEADER: usize = 37;
/// 帧头开头的同步字（低位在前读出的 16 位）
const SYNC: u32 = 0xA0FF;

/// TAK 的声道号 → WAVEFORMATEXTENSIBLE 声道位（与 FFmpeg 的 AV_CH_* 相同）
const LAYOUT: [u32; 19] = [0, 0x1, 0x2, 0x4, 0x8, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400, 0x800, 0x1000, 0x2000, 0x4000, 0x8000, 0x10000, 0x20000];
const DURATION: [u32; 10] = [3, 4, 6, 8, 4096, 8192, 16384, 512, 1024, 2048];

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(super) struct StreamInfo {
    pub codec: u32,
    pub data_type: u32,
    pub rate: u32,
    pub channels: usize,
    pub bps: u32,
    pub mask: u32,
    pub samples: u64,
    pub frame_samples: u32,
}

/// 帧的采样点数：前四种按时长（94 / 125 / 188 / 250 ms），其余是固定值，不能超过 250 ms
fn frame_samples(rate: u32, t: u32) -> Option<u32> {
    let (n, max) = if t <= 3 {
        ((rate * DURATION[t as usize]) >> 5, 16384)
    } else if (t as usize) < DURATION.len() {
        (DURATION[t as usize], (rate * DURATION[3]) >> 5)
    } else {
        return None;
    };
    (n > 0 && n <= max).then_some(n)
}

/// 多于 32 位：先读低 32 位
fn bits64(r: &mut Bits, n: u32) -> u64 {
    let lo = r.bits(32) as u64;
    lo | (r.bits(n - 32) as u64) << 32
}

pub(super) fn parse_streaminfo(r: &mut Bits) -> Option<StreamInfo> {
    let codec = r.bits(6);
    r.bits(4);
    let frame_type = r.bits(4);
    let samples = bits64(r, 35);
    let data_type = r.bits(3);
    let rate = r.bits(18) + 6000;
    let bps = r.bits(5) + 8;
    let channels = r.bits(4) as usize + 1;
    let mut mask = 0;
    if r.bit() {
        r.bits(5);
        if r.bit() {
            for _ in 0..channels {
                mask |= LAYOUT.get(r.bits(6) as usize).copied().unwrap_or(0);
            }
        }
    }
    let frame_samples = frame_samples(rate, frame_type)?;
    Some(StreamInfo { codec, data_type, rate, channels, bps, mask, samples, frame_samples })
}

pub(super) struct FrameHeader {
    pub flags: u32,
    pub num: u32,
    /// 最后一帧的采样点数（不是最后一帧时为 0）
    pub last_samples: u32,
    pub info: Option<StreamInfo>,
    /// 帧头（含它自己的 CRC）的字节数
    pub size: usize,
}

/// 帧头：同步字、标志、帧号、（最后一帧的采样点数）、（流信息）、CRC。CRC 不对时返回 None
pub(super) fn parse_frame_header(b: &[u8]) -> Option<FrameHeader> {
    let mut r = Bits::new(b);
    if r.bits(16) != SYNC {
        return None;
    }
    let flags = r.bits(3);
    let num = r.bits(21);
    let mut last_samples = 0;
    if flags & FLAG_LAST != 0 {
        last_samples = r.bits(14) + 1;
        r.bits(2);
    }
    let mut info = None;
    if flags & FLAG_HAS_INFO != 0 {
        info = Some(parse_streaminfo(&mut r)?);
        if r.bits(6) != 0 {
            r.bits(25);
        }
        r.align();
    }
    if flags & FLAG_HAS_METADATA != 0 || r.left() < 24 {
        return None;
    }
    r.bits(24);
    let size = r.position() / 8;
    (!r.overrun && check_crc(&b[..size])).then_some(FrameHeader { flags, num, last_samples, info, size })
}

/// CRC-24/OpenPGP（多项式 0x864CFB，初值 0xB704CE），结果按小端存在数据后面 3 字节
pub(super) fn crc24(d: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let mut c = (i as u32) << 16;
            for _ in 0..8 {
                c = if c & 0x80_0000 != 0 { (c << 1) ^ 0x86_4CFB } else { c << 1 };
            }
            *v = c & 0xFF_FFFF;
        }
        t
    });
    d.iter().fold(0xB7_04CEu32, |c, &x| ((c << 8) & 0xFF_FFFF) ^ t[((c >> 16) as u8 ^ x) as usize])
}

/// 最后 3 字节是前面数据的 CRC
pub(super) fn check_crc(b: &[u8]) -> bool {
    if b.len() < 4 {
        return false;
    }
    let (d, c) = b.split_at(b.len() - 3);
    crc24(d) == u32::from_le_bytes([c[0], c[1], c[2], 0])
}

pub(super) struct FileHeader {
    /// 元数据之后的位置（"tBaK" 之后；没有 "tBaK" 时是开头）
    pub after_magic: u64,
    /// 元数据块说的第一帧的位置（元数据块损坏时为 None）
    pub data: Option<u64>,
    /// 元数据里的流信息
    pub info: Option<StreamInfo>,
}

/// "tBaK" 和元数据块（类型、24 位长度；流信息等块末尾有 CRC），直到结束块。
/// 没有 "tBaK" 时当作没有文件头的裸码流。元数据块损坏时也不算错：第一帧总是从 "tBaK" 之后找
pub(super) fn parse_file(f: &mut File, start: u64, len: u64) -> Result<FileHeader> {
    let mut magic = [0u8; 4];
    f.seek(SeekFrom::Start(start))?;
    if f.read_exact(&mut magic).is_err() || &magic != b"tBaK" {
        return Ok(FileHeader { after_magic: start, data: Some(start), info: None });
    }
    let mut hdr = FileHeader { after_magic: start + 4, data: None, info: None };
    let mut at = start + 4;
    loop {
        let mut h = [0u8; 4];
        f.seek(SeekFrom::Start(at))?;
        if f.read_exact(&mut h).is_err() {
            return Ok(hdr);
        }
        let kind = h[0] & 0x7F;
        let size = u32::from_le_bytes([h[1], h[2], h[3], 0]) as u64;
        at += 4;
        if kind == 0 {
            hdr.data = Some(at);
            return Ok(hdr);
        }
        // 流信息（CRC 不对时不用，以第一帧帧头里的为准）
        if kind == 1 && hdr.info.is_none() && (4..=4096).contains(&size) {
            let mut d = vec![0u8; size as usize];
            if f.read_exact(&mut d).is_ok() && check_crc(&d) {
                hdr.info = parse_streaminfo(&mut Bits::new(&d[..d.len() - 3]));
            }
        }
        at += size;
        if at > len {
            return Ok(hdr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc() {
        assert_eq!(crc24(b"123456789"), 0x21_CF02);
        assert!(check_crc(b"123456789\x02\xCF\x21"));
        assert!(!check_crc(b"123456789\x21\xCF\x02"));
    }

    #[test]
    fn durations() {
        // 44.1 kHz：250 ms = 11025；固定 16384 超过 250 ms，不允许
        assert_eq!(frame_samples(44_100, 3), Some(11_025));
        assert_eq!(frame_samples(44_100, 6), None);
        assert_eq!(frame_samples(96_000, 6), Some(16_384));
        assert_eq!(frame_samples(8_000, 4), None);
        assert_eq!(frame_samples(48_000, 10), None);
    }
}
