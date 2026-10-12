//! APE 文件头（照参考库 APEHeader.cpp，BSD 许可，见 mod.rs）：3.98 起是描述符 + 文件头，以前是 32 字节的老文件头；
//! 之后是各帧在文件中的位置表（3.80 及以前还有每帧起始位的表）。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use anyhow::{Result, bail};

pub(super) const FLAG_8_BIT: u16 = 1;
pub(super) const FLAG_CRC: u16 = 2;
const FLAG_PEAK: u16 = 4;
pub(super) const FLAG_24_BIT: u16 = 8;
const FLAG_SEEK_ELEMENTS: u16 = 16;
const FLAG_CREATE_WAV: u16 = 32;
pub(super) const FLAG_FLOAT: u16 = 1 << 12;

pub(super) struct Info {
    pub version: u16,
    pub level: u16,
    pub flags: u16,
    pub channels: u16,
    pub bits: u16,
    pub rate: u32,
    pub blocks_per_frame: u32,
    pub final_blocks: u32,
    pub frames: u32,
    /// 各帧的文件位置（和 3.80 及以前的起始位）
    pub seek: Vec<u64>,
    pub seek_bits: Vec<u8>,
}

impl Info {
    pub fn total(&self) -> u64 {
        if self.frames == 0 { 0 } else { (self.frames as u64 - 1) * self.blocks_per_frame as u64 + self.final_blocks as u64 }
    }

    pub fn frame_blocks(&self, i: u32) -> u32 {
        if i + 1 == self.frames { self.final_blocks } else { self.blocks_per_frame }
    }
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn read_exact_at(f: &mut File, at: u64, n: usize) -> Result<Vec<u8>> {
    let mut v = vec![0u8; n];
    f.seek(SeekFrom::Start(at))?;
    f.read_exact(&mut v).map_err(|_| anyhow::anyhow!("APE 文件头不完整"))?;
    Ok(v)
}

/// 找到 "MAC " / "MACF"（跳过 ID3v2 和其后的填充 0，再最多找 1 MB）
fn find(f: &mut File, len: u64) -> Result<u64> {
    let mut at = crate::audio::codec::skip_id3v2(f)?;
    let mut head = vec![0u8; (len.saturating_sub(at)).min(1 << 20) as usize + 4];
    f.seek(SeekFrom::Start(at))?;
    let n = f.read(&mut head)?;
    let head = &head[..n];
    if at > 0 {
        let pad = head.iter().take_while(|&&b| b == 0).count();
        at += pad as u64;
        return find_in(&head[pad..]).map(|i| at + i as u64);
    }
    find_in(head).map(|i| at + i as u64)
}

fn find_in(b: &[u8]) -> Result<usize> {
    b.windows(4).position(|w| w == b"MAC " || w == b"MACF").ok_or_else(|| anyhow::anyhow!("不是 Monkey's Audio 文件"))
}

pub(super) fn parse(f: &mut File, len: u64) -> Result<Info> {
    let base = find(f, len)?;
    let common = read_exact_at(f, base, 6)?;
    let version = u16_at(&common, 4);
    let (mut info, seek_at, seek_n) = if version >= 3980 {
        let d = read_exact_at(f, base, 52)?;
        let (desc_bytes, head_bytes, seek_bytes) = (u32_at(&d, 8).max(52) as u64, u32_at(&d, 12).max(24) as u64, u32_at(&d, 16));
        let h = read_exact_at(f, base + desc_bytes, 24)?;
        let info = Info {
            version: u16_at(&d, 4),
            level: u16_at(&h, 0),
            flags: u16_at(&h, 2),
            blocks_per_frame: u32_at(&h, 4),
            final_blocks: u32_at(&h, 8),
            frames: u32_at(&h, 12),
            bits: u16_at(&h, 16),
            channels: u16_at(&h, 18),
            rate: u32_at(&h, 20),
            seek: Vec::new(),
            seek_bits: Vec::new(),
        };
        (info, base + desc_bytes + head_bytes, seek_bytes / 4)
    } else {
        let h = read_exact_at(f, base, 32)?;
        let (level, flags) = (u16_at(&h, 6), u16_at(&h, 8));
        let (head_bytes, frames) = (u32_at(&h, 16) as u64, u32_at(&h, 24));
        let mut at = base + 32;
        if flags & FLAG_PEAK != 0 {
            at += 4;
        }
        let mut seek_n = frames;
        if flags & FLAG_SEEK_ELEMENTS != 0 {
            seek_n = u32_at(&read_exact_at(f, at, 4)?, 0);
            at += 4;
        }
        if flags & FLAG_CREATE_WAV == 0 {
            at += head_bytes;
        }
        let bits = if flags & FLAG_8_BIT != 0 { 8 } else if flags & FLAG_24_BIT != 0 { 24 } else { 16 };
        let bpf = if version >= 3950 {
            73728 * 4
        } else if version >= 3900 || (version >= 3800 && level == 4000) {
            73728
        } else {
            9216
        };
        let info = Info {
            version,
            level,
            flags,
            channels: u16_at(&h, 10),
            bits,
            rate: u32_at(&h, 12),
            blocks_per_frame: bpf,
            final_blocks: u32_at(&h, 28),
            frames,
            seek: Vec::new(),
            seek_bits: Vec::new(),
        };
        (info, at, seek_n)
    };
    if !(3800..=3990).contains(&info.version) && info.version != 4110 {
        bail!("不支持的 Monkey's Audio 版本 {:.2}（支持 3.80 及以后）", info.version as f32 / 1000.0);
    }
    let ch_ok = if info.version < 3930 { (1..=2).contains(&info.channels) } else { (1..=32).contains(&info.channels) };
    let bits_ok = matches!(info.bits, 8 | 16 | 24) || (info.bits == 32 && info.version >= 3930);
    if !ch_ok || !bits_ok || !(1000..=5000).contains(&info.level) || info.level % 1000 != 0 || info.rate == 0 {
        bail!("APE 参数不对：{} 声道 {} 位 {} Hz，压缩级别 {}", info.channels, info.bits, info.rate, info.level);
    }
    if info.frames == 0 || info.blocks_per_frame == 0 || info.final_blocks > info.blocks_per_frame {
        bail!("APE 文件没有音频或文件头不对");
    }
    if info.blocks_per_frame as u64 * info.channels as u64 > 64 << 20 {
        bail!("APE 每帧太大");
    }
    // 位置表（32 位，超过 4 GB 时回绕）
    if seek_n as u64 > len / 4 {
        bail!("APE 位置表不对");
    }
    let t = read_exact_at(f, seek_at, seek_n as usize * 4)?;
    let (mut add, mut prev) = (0u64, 0u32);
    for c in t.chunks_exact(4) {
        let v = u32_at(c, 0);
        if v < prev {
            add += 1 << 32;
        }
        info.seek.push(add + v as u64 + base);
        prev = v;
    }
    if info.version <= 3800 {
        info.seek_bits = read_exact_at(f, seek_at + seek_n as u64 * 4, seek_n as usize).unwrap_or_default();
    }
    Ok(info)
}

/// 帧数据的结尾（去掉末尾的 APEv2 / ID3v1 标签）
pub(super) fn data_end(f: &mut File, len: u64) -> u64 {
    let mut end = len;
    let mut tail = [0u8; 128];
    if end >= 128 && f.seek(SeekFrom::Start(end - 128)).is_ok() && f.read_exact(&mut tail).is_ok() && &tail[..3] == b"TAG" {
        end -= 128;
    }
    let mut foot = [0u8; 32];
    if end >= 32 && f.seek(SeekFrom::Start(end - 32)).is_ok() && f.read_exact(&mut foot).is_ok() && &foot[..8] == b"APETAGEX" {
        let size = u32_at(&foot, 12) as u64;
        let has_header = u32_at(&foot, 20) & (1 << 31) != 0;
        end = end.saturating_sub(size + if has_header { 32 } else { 0 });
    }
    end
}