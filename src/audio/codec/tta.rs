//! TTA（True Audio）无损解码，按格式说明（tta.sourceforge.net）实现。
//!
//! 文件：可选 ID3v2 → 22 字节文件头（"TTA1"、格式、声道、位数、采样率、总样本数、CRC32）
//! → 每帧大小的表 + CRC32 → 各帧（每帧约 1.045 秒，末尾 4 字节 CRC32）。
//! 每个样本：自适应 Rice 码 → 8 阶自适应滤波 → 一阶固定预测；每个采样点的各声道再做声道间去相关。
//! 每帧开头所有状态重置，所以每帧可以单独解码（跳转直接到帧开头）。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Result, anyhow, bail};

use super::bits::LsbReader;
use super::{Block, BlockDecoder, crc32};
use crate::audio::source::Spec;

/// Rice 码参数 k 对应的 2^k（k 太大时封顶，与编码器一致）
fn pow2(k: u32) -> u32 {
    if k < 32 { 1 << k } else { 0x8000_0000 }
}

/// 2^(k+4)：Rice 参数自适应的阈值
fn thresh(k: u32) -> u32 {
    pow2(k + 4)
}

/// 8 阶自适应滤波（符号 LMS）
struct Filter {
    shift: u32,
    round: i32,
    error: i32,
    qm: [i32; 8],
    dx: [i32; 8],
    dl: [i32; 8],
}

impl Filter {
    fn new(shift: u32) -> Self {
        Filter { shift, round: 1 << (shift - 1), error: 0, qm: [0; 8], dx: [0; 8], dl: [0; 8] }
    }

    fn reset(&mut self) {
        *self = Filter::new(self.shift);
    }

    /// 输入预测残差，返回滤波后的值
    fn run(&mut self, v: i32) -> i32 {
        // 上一个样本的误差符号决定系数往哪边调
        if self.error != 0 {
            let s = self.error.signum();
            for i in 0..8 {
                self.qm[i] = self.qm[i].wrapping_add(s.wrapping_mul(self.dx[i]));
            }
        }
        let mut sum = self.round;
        for i in 0..8 {
            sum = sum.wrapping_add(self.dl[i].wrapping_mul(self.qm[i]));
        }
        let out = v.wrapping_add(sum >> self.shift);
        self.error = v;
        // 各阶的调整步长：越新的样本越大（低位置 1 保证不为 0）
        self.dx.copy_within(1.., 0);
        self.dx[4] = (self.dl[4] >> 30) | 1;
        self.dx[5] = ((self.dl[5] >> 30) | 1) << 1;
        self.dx[6] = ((self.dl[6] >> 30) | 1) << 1;
        self.dx[7] = ((self.dl[7] >> 30) | 1) << 2;
        // 延迟线：最近的输出及其 1 ~ 3 阶差分
        let d7 = out;
        let d6 = d7.wrapping_sub(self.dl[7]);
        let d5 = d6.wrapping_sub(self.dl[6]);
        let d4 = d5.wrapping_sub(self.dl[5]);
        self.dl.copy_within(1.., 0);
        (self.dl[4], self.dl[5], self.dl[6], self.dl[7]) = (d4, d5, d6, d7);
        out
    }
}

/// 一个声道的解码状态（每帧开头重置）
struct Channel {
    k: [u32; 2],
    sum: [u32; 2],
    filter: Filter,
    last: i32,
}

impl Channel {
    fn new(shift: u32) -> Self {
        let mut c = Channel { k: [0; 2], sum: [0; 2], filter: Filter::new(shift), last: 0 };
        c.reset();
        c
    }

    fn reset(&mut self) {
        self.k = [10, 10];
        self.sum = [thresh(10), thresh(10)];
        self.filter.reset();
        self.last = 0;
    }

    fn sample(&mut self, r: &mut LsbReader, bits: u32) -> i32 {
        // 损坏的数据会让 Rice 参数变得离谱：当作读到头，这一帧解到这里为止
        if self.k[0] > 32 || self.k[1] > 32 {
            r.overrun = true;
            return 0;
        }
        // 一元码前缀：0 → 用 k0 直接读；n > 0 → 先有 n-1 个 2^k1，再用 k1 读，最后加 2^k0
        let prefix = r.ones(u32::MAX);
        let mut v: u32;
        if prefix == 0 {
            v = r.bits(self.k[0]);
        } else {
            v = (prefix - 1).wrapping_mul(pow2(self.k[1]));
            v = v.wrapping_add(r.bits(self.k[1]));
            // k1 的自适应
            self.sum[1] = self.sum[1].wrapping_add(v).wrapping_sub(self.sum[1] >> 4);
            if self.k[1] > 0 && self.sum[1] < thresh(self.k[1]) {
                self.k[1] -= 1;
            } else if self.sum[1] > thresh(self.k[1] + 1) {
                self.k[1] += 1;
            }
            v = v.wrapping_add(pow2(self.k[0]));
        }
        // k0 的自适应
        self.sum[0] = self.sum[0].wrapping_add(v).wrapping_sub(self.sum[0] >> 4);
        if self.k[0] > 0 && self.sum[0] < thresh(self.k[0]) {
            self.k[0] -= 1;
        } else if self.sum[0] > thresh(self.k[0] + 1) {
            self.k[0] += 1;
        }
        // 奇数为正、偶数为负
        let d = if v & 1 != 0 { (v >> 1).wrapping_add(1) as i32 } else { ((v >> 1) as i32).wrapping_neg() };
        let f = self.filter.run(d);
        // 一阶固定预测：x += last * (2^k - 1) / 2^k
        let k = if bits == 8 { 4 } else { 5 };
        let p = if bits <= 24 { ((self.last as i64 * ((1 << k) - 1)) >> k) as i32 } else { self.last };
        let x = f.wrapping_add(p);
        self.last = x;
        x
    }
}

pub struct TtaDecoder {
    file: File,
    spec: Spec,
    total: u64,
    bits: u32,
    /// 每帧的样本数（每声道）
    frame_len: u64,
    /// 各帧在文件中的起点（最后多一个 = 数据末尾）
    offsets: Vec<u64>,
    next: usize,
    chans: Vec<Channel>,
    buf: Vec<u8>,
}

impl TtaDecoder {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let start = super::skip_id3v2(&mut file)?;
        file.seek(SeekFrom::Start(start))?;
        let mut h = [0u8; 22];
        file.read_exact(&mut h).map_err(|_| anyhow!("不是 TTA 文件"))?;
        if &h[..4] != b"TTA1" {
            bail!("不是 TTA 文件");
        }
        let u16_at = |i: usize| u16::from_le_bytes([h[i], h[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
        let (format, channels, bits, rate, total) = (u16_at(4), u16_at(6), u16_at(8) as u32, u32_at(10), u32_at(14) as u64);
        if crc32(&h[..18]) != u32_at(18) {
            log::warn!("TTA 文件头校验不对：{}", path.display());
        }
        match format {
            1 => {}
            2 => bail!("加密的 TTA 文件（需要密码），不支持"),
            f => bail!("不支持的 TTA 格式 {f}"),
        }
        if channels == 0 || !(8..=32).contains(&bits) || !(1000..=768_000).contains(&rate) {
            bail!("TTA 参数不对：{channels} 声道 {bits} 位 {rate} Hz");
        }
        // 每帧 256/245 秒（约 1.045 秒）
        let frame_len = 256 * rate as u64 / 245;
        let frames = total.div_ceil(frame_len) as usize;
        let mut table = vec![0u8; frames * 4 + 4];
        file.read_exact(&mut table).map_err(|_| anyhow!("TTA 帧表不完整"))?;
        let mut offsets = Vec::with_capacity(frames + 1);
        let mut pos = start + 22 + table.len() as u64;
        offsets.push(pos);
        for c in table[..frames * 4].chunks_exact(4) {
            pos += u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as u64;
            offsets.push(pos);
        }
        let byte_bits = bits.div_ceil(8) * 8;
        let shift = [10, 9, 10, 12][(byte_bits / 8 - 1) as usize];
        let spec = Spec { rate, channels, mask: 0 };
        let chans = (0..channels).map(|_| Channel::new(shift)).collect();
        Ok(TtaDecoder { file, spec, total, bits: byte_bits, frame_len, offsets, next: 0, chans, buf: Vec::new() })
    }

    /// 第 i 帧有多少样本（最后一帧可能不满）
    fn frame_samples(&self, i: usize) -> u64 {
        self.frame_len.min(self.total.saturating_sub(i as u64 * self.frame_len))
    }
}

impl BlockDecoder for TtaDecoder {
    fn spec(&self) -> Spec {
        self.spec
    }

    fn total_frames(&self) -> Option<u64> {
        Some(self.total)
    }

    fn decode(&mut self, out: &mut Block) -> Result<usize> {
        out.clear();
        out.bits = self.bits;
        let i = self.next;
        if i + 1 >= self.offsets.len() {
            return Ok(0);
        }
        let (a, b) = (self.offsets[i], self.offsets[i + 1]);
        self.next += 1;
        self.buf.resize((b - a) as usize, 0);
        self.file.seek(SeekFrom::Start(a))?;
        // 截断的文件：读到多少解多少
        let mut got = 0;
        while got < self.buf.len() {
            match self.file.read(&mut self.buf[got..])? {
                0 => break,
                n => got += n,
            }
        }
        let data = &self.buf[..got];
        let full = got == self.buf.len() && got >= 4;
        if full && crc32(&data[..got - 4]) != u32::from_le_bytes(data[got - 4..].try_into().unwrap()) {
            log::warn!("TTA 第 {i} 帧校验不对");
        }
        let n = self.frame_samples(i) as usize;
        let ch = self.chans.len();
        for c in &mut self.chans {
            c.reset();
        }
        let mut r = LsbReader::new(if full { &data[..got - 4] } else { data });
        out.int.reserve(n * ch);
        let mut cur = vec![0i32; ch];
        for _ in 0..n {
            for (k, c) in self.chans.iter_mut().enumerate() {
                cur[k] = c.sample(&mut r, self.bits);
            }
            if r.overrun {
                // 数据不够：只保留解完整的采样点，后面的帧也不再解
                self.next = self.offsets.len();
                break;
            }
            // 声道间去相关：最后一声道加上前一声道的一半，再从后往前逐个还原
            if ch > 1 {
                cur[ch - 1] = cur[ch - 1].wrapping_add(cur[ch - 2] / 2);
                for k in (0..ch - 1).rev() {
                    cur[k] = cur[k + 1].wrapping_sub(cur[k]);
                }
            }
            out.int.extend_from_slice(&cur);
        }
        Ok(out.int.len() / ch)
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        let i = ((frame / self.frame_len) as usize).min(self.offsets.len().saturating_sub(2));
        self.next = i;
        Ok(i as u64 * self.frame_len)
    }
}
