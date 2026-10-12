//! 自带的解码器：原版支持、而 symphonia / Media Foundation 不支持的格式（TTA、WavPack、Monkey's Audio、
//! Musepack、TAK、DTS）。都是纯 Rust，按扩展名选用；每个解码器一次解出一块（一帧 / 一个 block），
//! 由 `BlockSource` 转成播放用的 f32 并做到精确到样本的跳转。

use std::path::Path;

use anyhow::Result;

use super::source::{Source, Spec};

pub mod bits;
pub mod tta;

/// 解出的一块（交错排列）。整数样本按容器位数（8 / 16 / 24 / 32）存为有符号数；
/// `float` 不为空时是浮点样本（-1 ~ 1），忽略 `int`
#[derive(Default)]
pub struct Block {
    pub int: Vec<i32>,
    pub float: Vec<f32>,
    pub bits: u32,
}

impl Block {
    pub fn clear(&mut self) {
        self.int.clear();
        self.float.clear();
    }
}

pub trait BlockDecoder: Send {
    fn spec(&self) -> Spec;
    /// 总帧数（每声道样本数）
    fn total_frames(&self) -> Option<u64>;
    /// 解下一块到 `out`（先清空），返回帧数；0 = 结束
    fn decode(&mut self, out: &mut Block) -> Result<usize>;
    /// 跳到包含第 `frame` 帧的那一块的开头，返回这一块第一帧的位置
    fn seek(&mut self, frame: u64) -> Result<u64>;
}

/// 用自带解码器打开（按扩展名）；不是这些格式时返回 None
pub fn open_decoder(path: &Path) -> Option<Result<Box<dyn BlockDecoder>>> {
    let boxed = |r: Result<_>| r.map(|d| Box::new(d) as Box<dyn BlockDecoder>);
    match crate::util::ext_lower(path).as_str() {
        "tta" => Some(boxed(tta::TtaDecoder::open(path))),
        _ => None,
    }
}

/// 同上，包装成播放用的 `Source`
pub fn open(path: &Path) -> Option<Result<Box<dyn Source>>> {
    open_decoder(path).map(|r| r.map(|dec| Box::new(BlockSource { dec, block: Block::default(), skip: 0 }) as Box<dyn Source>))
}

/// 把整块解码器包装成 `Source`：整数样本换算成 f32；跳转时先跳到所在块的开头，再丢掉前面多出的样本
pub struct BlockSource {
    dec: Box<dyn BlockDecoder>,
    block: Block,
    skip: u64,
}

impl Source for BlockSource {
    fn spec(&self) -> Spec {
        self.dec.spec()
    }

    fn total_frames(&self) -> Option<u64> {
        self.dec.total_frames()
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<usize> {
        out.clear();
        let ch = self.dec.spec().channels.max(1) as usize;
        loop {
            let n = self.dec.decode(&mut self.block)?;
            if n == 0 {
                return Ok(0);
            }
            let skip = (self.skip as usize).min(n);
            self.skip -= skip as u64;
            if skip == n {
                continue;
            }
            let b = &self.block;
            if b.float.is_empty() {
                let scale = 1.0 / (1u64 << (b.bits.clamp(1, 32) - 1)) as f32;
                out.extend(b.int[skip * ch..n * ch].iter().map(|&s| s as f32 * scale));
            } else {
                out.extend_from_slice(&b.float[skip * ch..n * ch]);
            }
            return Ok(n - skip);
        }
    }

    fn seek(&mut self, frame: u64) -> Result<()> {
        let start = self.dec.seek(frame)?;
        self.skip = frame.saturating_sub(start);
        Ok(())
    }
}

/// CRC-32（IEEE 802.3，与 zlib 相同）
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *v = c;
        }
        t
    });
    !data.iter().fold(!0u32, |c, &b| t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8))
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc() {
        assert_eq!(super::crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(super::crc32(b""), 0);
    }
}
