//! 文件头与读文件（照参考库 mpc_demux.c / streaminfo.c，BSD 许可，见 mod.rs）。
//! SV7："MP+" + 24 字节的头，之后是一串帧；数据按 32 位小端字存放，读入时每 4 字节倒序。
//! SV8："MPCK" + 若干块（2 字节名字 + 变长的块大小）：SH 是流信息，AP 是一组帧，SE 是结尾。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use anyhow::{Result, bail};

use super::bits::Bits;

const RATES: [u32; 4] = [44100, 48000, 37800, 32000];

pub(super) struct Info {
    pub version: u8,
    pub channels: usize,
    pub rate: u32,
    pub max_band: usize,
    pub ms: bool,
    pub gapless: bool,
    pub samples: u64,
    pub beg_silence: u64,
    /// SV8：每个 AP 块 2^block_pwr 帧
    pub block_pwr: u32,
    /// 数据的起点（SV7：头的位置 + 4，按字对齐；SV8：第一个块的位置）
    pub base: u64,
    /// SV7：第一帧的位置（相对 base 的位，= 168）；SV8：第一个 AP 块的位置（相对 base 的字节）
    pub first: u64,
}

/// 读文件：缓存一段（SV7 按字倒序，起点按字对齐到 base）
pub(super) struct Reader {
    file: File,
    swap: bool,
    base: u64,
    /// 缓存起点（相对 base 的字节）
    pub at: u64,
    pub buf: Vec<u8>,
    /// 缓存里真正从文件读到的字节数（之后是补的 0；SV7 只算完整的字）
    pub valid: usize,
}

impl Reader {
    pub fn new(file: File, swap: bool, base: u64) -> Self {
        Reader { file, swap, base, at: u64::MAX, buf: Vec::new(), valid: 0 }
    }

    /// 让缓存包含 [from, from + len)（相对 base 的字节）；文件末尾之后是 0
    pub fn load(&mut self, from: u64, len: usize) -> Result<()> {
        if self.at != u64::MAX && from >= self.at && from + len as u64 <= self.at + self.buf.len() as u64 {
            return Ok(());
        }
        let start = from & !3;
        let n = (len + (from - start) as usize + 3).max(64 * 1024).div_ceil(4) * 4;
        self.buf.clear();
        self.buf.resize(n, 0);
        self.file.seek(SeekFrom::Start(self.base + start))?;
        let mut got = 0;
        while got < n {
            match self.file.read(&mut self.buf[got..])? {
                0 => break,
                k => got += k,
            }
        }
        if self.swap {
            self.buf.chunks_exact_mut(4).for_each(|w| w.reverse());
        }
        self.at = start;
        self.valid = if self.swap { got / 4 * 4 } else { got };
        Ok(())
    }
}

/// 变长的块大小（每字节 7 位，最高位 = 后面还有）
pub(super) fn varint(r: &mut Bits) -> u64 {
    let mut v = 0u64;
    for _ in 0..10 {
        let b = r.read(8);
        v = (v << 7) | (b & 0x7F) as u64;
        if b & 0x80 == 0 {
            break;
        }
    }
    v
}

pub(super) fn parse(file: &mut File) -> Result<Info> {
    let head = crate::audio::codec::skip_id3v2(file)?;
    let mut h = [0u8; 4];
    file.seek(SeekFrom::Start(head))?;
    file.read_exact(&mut h).map_err(|_| anyhow::anyhow!("不是 Musepack 文件"))?;
    let info = if &h[..3] == b"MP+" {
        if h[3] & 15 != 7 {
            bail!("不支持的 Musepack 版本 SV{}", h[3] & 15);
        }
        let mut b = [0u8; 24];
        file.read_exact(&mut b).map_err(|_| anyhow::anyhow!("Musepack 文件头不完整"))?;
        b.chunks_exact_mut(4).for_each(|w| w.reverse());
        let mut r = Bits::new(&b, 0);
        let frames = r.read(32) as u64;
        r.read(1);
        let ms = r.read(1) != 0;
        let max_band = r.read(6) as usize;
        r.read(4 + 2);
        let rate = RATES[r.read(2) as usize];
        for _ in 0..5 {
            r.read(16);
        }
        let gapless = r.read(1) != 0;
        let last = r.read(11) as u64;
        if last > 1152 {
            bail!("Musepack 文件头不对");
        }
        let last = if last == 0 { 1152 } else { last };
        let samples = (frames * 1152).saturating_sub(if gapless { 1152 - last } else { super::decoder::DELAY });
        // 头是 168 位（不是整字），第一帧紧接着开始
        Info { version: 7, channels: 2, rate, max_band, ms, gapless, samples, beg_silence: 0, block_pwr: 0, base: head + 4, first: 168 }
    } else if &h == b"MPCK" {
        parse_sv8(file, head + 4)?
    } else {
        bail!("不是 Musepack 文件");
    };
    if info.max_band == 0 || info.max_band >= 32 || info.channels == 0 || info.channels > 2 || info.samples <= info.beg_silence {
        bail!("Musepack 参数不对");
    }
    Ok(info)
}

/// 块头：（名字, 数据的字节数, 块头的字节数）
pub(super) fn block_head(b: &[u8]) -> Option<([u8; 2], u64, u64)> {
    let mut r = Bits::new(b, 0);
    let key = [r.read(8) as u8, r.read(8) as u8];
    let size = varint(&mut r);
    let head = (r.pos / 8) as u64;
    if r.over || !key.iter().all(|c| c.is_ascii_uppercase()) || size < head {
        return None;
    }
    Some((key, size - head, head))
}

fn parse_sv8(file: &mut File, base: u64) -> Result<Info> {
    let mut rd = Reader::new(file.try_clone()?, false, base);
    let mut at = 0u64;
    let mut info = None;
    // 最多看 64 个块（流信息、回放增益、编码器信息、位置表……都在第一个 AP 块之前）
    for _ in 0..64 {
        rd.load(at, 16)?;
        let off = (at - rd.at) as usize;
        let Some((key, size, head)) = block_head(&rd.buf[off..]) else { bail!("Musepack 块头不对") };
        if &key == b"AP" {
            let mut i: Info = info.ok_or_else(|| anyhow::anyhow!("Musepack 文件没有流信息"))?;
            i.first = at;
            return Ok(i);
        }
        if &key == b"SH" {
            rd.load(at, (head + size) as usize)?;
            let off = (at - rd.at) as usize + head as usize;
            let d = &rd.buf[off..off + size as usize];
            if size < 5 || u32::from_be_bytes([d[0], d[1], d[2], d[3]]) != crate::audio::codec::crc32(&d[4..]) {
                bail!("Musepack 流信息校验不对");
            }
            let mut r = Bits::new(d, 32);
            if r.read(8) != 8 {
                bail!("不支持的 Musepack 版本");
            }
            let samples = varint(&mut r);
            let beg_silence = varint(&mut r);
            let rate = RATES.get(r.read(3) as usize).copied().unwrap_or(0);
            let max_band = r.read(5) as usize + 1;
            let channels = r.read(4) as usize + 1;
            let ms = r.read(1) != 0;
            let block_pwr = r.read(3) * 2;
            if rate == 0 {
                bail!("Musepack 采样率不对");
            }
            info = Some(Info { version: 8, channels, rate, max_band, ms, gapless: true, samples, beg_silence, block_pwr, base, first: 0 });
        }
        if &key == b"SE" {
            break;
        }
        at += head + size;
    }
    bail!("Musepack 文件没有音频")
}