//! WavPack 块：32 字节块头 + 若干元数据（解相关参数、熵编码中值、码流……）。
//! 格式与校验规则照参考库 open_utils.c / unpack_utils.c（BSD 许可，见 mod.rs）。

use anyhow::{Result, bail};

use super::decorr::Pass;
use super::words::Words;

pub(super) const BYTES_STORED: u32 = 3;
pub(super) const MONO_FLAG: u32 = 4;
pub(super) const HYBRID: u32 = 8;
pub(super) const JOINT_STEREO: u32 = 0x10;
pub(super) const FLOAT_DATA: u32 = 0x80;
pub(super) const INT32_DATA: u32 = 0x100;
pub(super) const HYBRID_BITRATE: u32 = 0x200;
pub(super) const HYBRID_BALANCE: u32 = 0x400;
pub(super) const INITIAL: u32 = 0x800;
pub(super) const FINAL: u32 = 0x1000;
pub(super) const SHIFT_LSB: u32 = 13;
pub(super) const MAG_LSB: u32 = 18;
pub(super) const SRATE_LSB: u32 = 23;
pub(super) const HAS_CHECKSUM: u32 = 0x1000_0000;
pub(super) const FALSE_STEREO: u32 = 0x4000_0000;
pub(super) const DSD: u32 = 0x8000_0000;
/// 解出来是单声道（真单声道，或两个声道一样的「假立体声」）
pub(super) const MONO_DATA: u32 = MONO_FLAG | FALSE_STEREO;

pub(super) const RATES: [u32; 15] = [6000, 8000, 9600, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000, 64000, 88200, 96000, 192000];

pub(super) const HEADER_LEN: usize = 32;

#[derive(Clone, Copy)]
pub(super) struct Header {
    /// 整块的字节数（含块头）
    pub size: usize,
    pub version: u16,
    /// 文件总样本数（None = 未知）
    pub total: Option<u64>,
    /// 这一块第一个样本的序号
    pub index: u64,
    pub samples: u32,
    pub flags: u32,
    pub crc: u32,
}

impl Header {
    /// 校验块头（与参考库 read_next_header 的条件相同）
    pub fn parse(b: &[u8]) -> Option<Header> {
        if b.len() < HEADER_LEN || &b[..4] != b"wvpk" {
            return None;
        }
        let ok = b[4] & 1 == 0 && b[6] < 16 && b[7] == 0 && (b[6] != 0 || b[5] != 0 || b[4] >= 24)
            && b[9] == 4 && (2..=0x10).contains(&b[8]) && b[22] < 3 && b[23] == 0;
        if !ok {
            return None;
        }
        let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let total = u32_at(12);
        Some(Header {
            size: u32_at(4) as usize + 8,
            version: u16::from_le_bytes([b[8], b[9]]),
            total: (total != u32::MAX).then(|| total as u64 + ((b[11] as u64) << 32) - b[11] as u64),
            index: u32_at(16) as u64 + ((b[10] as u64) << 32),
            samples: u32_at(20),
            flags: u32_at(24),
            crc: u32_at(28),
        })
    }

    pub fn mono_data(&self) -> bool {
        self.flags & MONO_DATA != 0
    }

    /// 这一块解出的声道数（假立体声也是 2）
    pub fn channels(&self) -> usize {
        if self.flags & MONO_FLAG != 0 { 1 } else { 2 }
    }
}

/// 一块解码要用的全部状态（每块从元数据重新建立，块与块之间互不依赖）
pub(super) struct Stream {
    pub hdr: Header,
    pub passes: [Pass; 16],
    pub nterms: usize,
    pub words: Words,
    pub float: [u8; 4],
    pub int32: [u8; 4],
    /// 主码流在块内的位置
    pub wv: Option<(usize, usize)>,
    /// 额外码流（32 位整数 / 浮点的无损部分）：位置、CRC、是不是新格式
    pub wvx: Option<(usize, usize, u32, bool)>,
    /// CHANNEL_INFO：总声道数和声道掩码
    pub chan_info: Option<(u32, u32)>,
    pub rate: Option<u32>,
}

impl Stream {
    /// 解析一整块（`b` 是含块头的整块）；结构不对、校验和不对、或有不认识的必需元数据时出错
    pub fn parse(b: &[u8]) -> Result<Stream> {
        let hdr = Header::parse(b).filter(|h| h.size == b.len()).ok_or_else(|| anyhow::anyhow!("WavPack 块头不对"))?;
        if hdr.flags & MONO_DATA == MONO_DATA {
            bail!("WavPack 块的声道标志不对");
        }
        let mut s = Stream {
            hdr,
            passes: [Pass::default(); 16],
            nterms: 0,
            words: Words::default(),
            float: [0; 4],
            int32: [0; 4],
            wv: None,
            wvx: None,
            chan_info: None,
            rate: None,
        };
        let mut pos = HEADER_LEN;
        let mut checked = false;
        while b.len() - pos >= 2 {
            let start = pos;
            let mut id = b[pos];
            let mut len = (b[pos + 1] as usize) << 1;
            pos += 2;
            if id & 0x80 != 0 {
                if b.len() - pos < 2 {
                    bail!("WavPack 元数据不完整");
                }
                len += ((b[pos] as usize) << 9) + ((b[pos + 1] as usize) << 17);
                pos += 2;
            }
            if b.len() - pos < len {
                bail!("WavPack 元数据不完整");
            }
            let padded = len;
            if id & 0x40 != 0 {
                if len == 0 {
                    bail!("WavPack 元数据长度不对");
                }
                len -= 1;
            }
            if id & 0x3f == 0x2f {
                verify_checksum(b, start, id, &b[pos..pos + padded])?;
                checked = true;
            }
            id &= 0x3f;
            s.meta(id, &b[pos..pos + len], pos)?;
            pos += padded;
        }
        if pos != b.len() {
            bail!("WavPack 块的结尾不对");
        }
        if hdr.flags & HAS_CHECKSUM != 0 && !checked {
            bail!("WavPack 块缺少校验和");
        }
        if hdr.samples > 0 && hdr.flags & DSD != 0 {
            bail!("不支持 DSD 格式的 WavPack");
        }
        if hdr.samples > 0 && s.wv.is_none() {
            bail!("WavPack 块没有音频数据");
        }
        Ok(s)
    }
}

/// 块校验和：从块头开始到校验和元数据之前，每 16 位一个字（小端）
fn verify_checksum(b: &[u8], start: usize, id: u8, data: &[u8]) -> Result<()> {
    if id & 0x40 != 0 || !(2..=4).contains(&data.len()) {
        bail!("WavPack 校验和的格式不对");
    }
    let mut sum = u32::MAX;
    for w in b[..start & !1].chunks_exact(2) {
        sum = sum.wrapping_mul(3).wrapping_add(w[0] as u32 + ((w[1] as u32) << 8));
    }
    let ok = if data.len() == 4 {
        data == sum.to_le_bytes()
    } else {
        sum ^= sum >> 16;
        data == &sum.to_le_bytes()[..2]
    };
    if !ok {
        bail!("WavPack 块校验和不对");
    }
    Ok(())
}
