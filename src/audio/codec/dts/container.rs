// .dtshd 的块结构照 FFmpeg 的 libavformat/dtshddec.c（Copyright (c) 2012 Paul B Mahol），LGPL 2.1+，
// 见 mod.rs 开头的说明。
//! 找文件里 DTS 码流的位置：裸 .dts（可带 ID3v2）、.dtshd（DTSHDHDR 块结构，带无缝播放信息）、
//! WAV 里的 DTS（CD 上常见的 14 位格式：data 块里不是 PCM 而是 DTS 码流）

use std::fs::File;
use std::path::Path;

use anyhow::{Result, bail};

use super::stream::{find_frame, read_at};

/// .dtshd 的 AUPR-HDR：采样率、总样本数、开头要去掉的样本数（都按 rate 计）
#[derive(Clone, Copy, Debug)]
pub(super) struct Gapless {
    pub rate: u32,
    pub samples: u64,
    pub padding: u64,
}

pub(super) struct Container {
    pub data: std::ops::Range<u64>,
    pub gapless: Option<Gapless>,
}

fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes(b[..8].try_into().unwrap())
}

/// .dtshd：一串 (8 字节类型, 8 字节长度, 内容) 的块；码流在 STRMDATA 里
fn dtshd(file: &mut File, len: u64) -> Result<Container> {
    let (mut at, mut data, mut gapless) = (0u64, None, None);
    let mut h = [0u8; 16];
    while at + 16 <= len {
        read_at(file, at, &mut h)?;
        let (ty, size) = (&h[..8], be64(&h[8..]));
        if !(4..=1 << 61).contains(&size) {
            bail!("DTS-HD 的块长度不对");
        }
        let body = at + 16;
        match ty {
            b"STRMDATA" => data = Some(body..(body + size).min(len)),
            b"AUPR-HDR" if size >= 21 => {
                let mut a = [0u8; 21];
                read_at(file, body, &mut a)?;
                let rate = u32::from_be_bytes([0, a[3], a[4], a[5]]);
                let orig = (u32::from_be_bytes(a[12..16].try_into().unwrap()) as u64) << 8 | a[16] as u64;
                let padding = u16::from_be_bytes([a[19], a[20]]) as u64;
                if rate > 0 {
                    gapless = Some(Gapless { rate, samples: orig, padding });
                }
            }
            _ => {}
        }
        at = body.saturating_add(size);
    }
    match data {
        Some(data) => Ok(Container { data, gapless }),
        None => bail!("DTS-HD 文件里没有码流"),
    }
}

/// WAV 的 data 块的位置；不是 WAV 时返回 None
fn wav_data(file: &mut File, len: u64) -> Option<std::ops::Range<u64>> {
    let mut h = [0u8; 12];
    read_at(file, 0, &mut h).ok()?;
    if &h[..4] != b"RIFF" || &h[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12u64;
    let mut c = [0u8; 8];
    while at + 8 <= len {
        read_at(file, at, &mut c).ok()?;
        let size = u32::from_le_bytes(c[4..8].try_into().unwrap()) as u64;
        if &c[..4] == b"data" {
            return Some(at + 8..(at + 8 + size).min(len));
        }
        at += 8 + size + (size & 1);
    }
    None
}

/// 数据开头（64 KB 内）有两个前后相接的 DTS 包
fn looks_like_dts(file: &mut File, data: &std::ops::Range<u64>) -> bool {
    let mut buf = vec![0u8; (data.end - data.start).min(1 << 17) as usize];
    let got = read_at(file, data.start, &mut buf).unwrap_or(0);
    let mut last = 0;
    match find_frame(&buf[..got.min(1 << 16)], &mut last) {
        Some((_, Some(end))) => matches!(find_frame(&buf[end..got], &mut last), Some((0, _))),
        _ => false,
    }
}

/// WAV 里装的是 DTS 码流
pub fn is_dts_wav(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else { return false };
    let len = file.metadata().map_or(0, |m| m.len());
    wav_data(&mut file, len).is_some_and(|d| looks_like_dts(&mut file, &d))
}

pub(super) fn open(file: &mut File) -> Result<Container> {
    let len = file.metadata()?.len();
    let mut h = [0u8; 8];
    read_at(file, 0, &mut h)?;
    if &h == b"DTSHDHDR" {
        match dtshd(file, len) {
            Ok(c) => return Ok(c),
            // 块结构坏了：当裸码流找同步字（没有无缝信息）
            Err(e) => log::warn!("DTS-HD 文件头不对（{e}），当裸码流解"),
        }
        return Ok(Container { data: 0..len, gapless: None });
    }
    if let Some(data) = wav_data(file, len) {
        return Ok(Container { data, gapless: None });
    }
    let start = super::super::skip_id3v2(file)?.min(len);
    Ok(Container { data: start..len, gapless: None })
}
