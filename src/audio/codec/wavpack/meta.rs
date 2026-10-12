//! WavPack 元数据的解析（照参考库 decorr_utils.c / entropy_utils.c / open_utils.c，BSD 许可，见 mod.rs）。

use anyhow::{Result, bail};

use super::block::{HYBRID, Stream};
use super::tables::{exp2s_at, restore_weight};

impl Stream {
    /// 一条元数据（`at` 是数据在块内的位置）；不认识的必需元数据、长度不对时出错
    pub(super) fn meta(&mut self, id: u8, d: &[u8], at: usize) -> Result<()> {
        let even = !d.is_empty() && d.len() % 2 == 0;
        let ok = match id {
            0x0 => true,
            0x2 => self.read_terms(d),
            0x3 => self.read_weights(d),
            0x4 => self.read_samples(d),
            0x5 => self.read_entropy(d),
            0x6 => self.read_hybrid(d),
            // 噪声整形权重：只有 .wvc 修正时用，这里只检查长度
            0x7 => d.len() == 2 || d.len() >= if self.hdr.mono_data() { 4 } else { 8 },
            0x8 | 0x9 if d.len() == 4 => {
                let dst = if id == 0x8 { &mut self.float } else { &mut self.int32 };
                dst.copy_from_slice(d);
                true
            }
            0x8 | 0x9 => false,
            0xa if even => {
                self.wv = Some((at, at + d.len()));
                true
            }
            // .wvc 修正码流（只出现在 .wvc 文件里）
            0xa | 0xb => even,
            0xc | 0x2c if even && d.len() > 4 => {
                let crc = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                self.wvx = Some((at + 4, at + d.len(), crc, id == 0x2c));
                true
            }
            0xc | 0x2c => false,
            0xd => self.read_channel_info(d),
            0xe => bail!("不支持 DSD 格式的 WavPack"),
            0x27 => {
                if d.len() == 3 || d.len() == 4 {
                    let hi = if d.len() == 4 { ((d[3] & 0x7f) as u32) << 24 } else { 0 };
                    self.rate = Some(d[0] as u32 | (d[1] as u32) << 8 | (d[2] as u32) << 16 | hi);
                }
                true
            }
            0x2b => !d.is_empty() && !d.contains(&0),
            // 其余可选的（RIFF 头尾、MD5、配置、块校验和……）跳过
            _ => id & 0x20 != 0,
        };
        if !ok {
            bail!("WavPack 元数据 0x{id:02x} 不对");
        }
        Ok(())
    }

    /// 解相关的各遍：存的顺序与解码时相反
    fn read_terms(&mut self, d: &[u8]) -> bool {
        if d.len() > self.passes.len() {
            return false;
        }
        self.nterms = d.len();
        let mono = self.hdr.mono_data();
        for (p, &b) in self.passes[..d.len()].iter_mut().rev().zip(d) {
            p.term = (b & 0x1f) as i32 - 5;
            p.delta = ((b >> 5) & 7) as i32;
            let t = p.term;
            if t == 0 || t < -3 || (9..17).contains(&t) || t > 18 || (mono && t < 0) {
                return false;
            }
        }
        true
    }

    /// 各遍的初始权重（从最后一遍往前；没给的是 0）
    fn read_weights(&mut self, d: &[u8]) -> bool {
        let stereo = !self.hdr.mono_data();
        let n = if stereo { d.len() / 2 } else { d.len() };
        if n > self.nterms {
            return false;
        }
        let mut it = d.iter().map(|&b| restore_weight(b as i8));
        for p in self.passes[..self.nterms].iter_mut().rev().take(n) {
            p.weight_a = it.next().unwrap_or(0);
            if stereo {
                p.weight_b = it.next().unwrap_or(0);
            }
        }
        true
    }

    /// 各遍的历史样本（以对数存）；个数随 term 而定
    fn read_samples(&mut self, d: &[u8]) -> bool {
        let mono = self.hdr.mono_data();
        let step = if mono { 2 } else { 4 };
        let mut pos = 0;
        // 4.02 版的混合模式在前面多存了一组（解码用不到）
        if self.hdr.version == 0x402 && self.hdr.flags & HYBRID != 0 {
            if d.len() < step {
                return false;
            }
            pos = step;
        }
        for p in self.passes[..self.nterms].iter_mut().rev() {
            if pos >= d.len() {
                break;
            }
            let (count, need) = match p.term {
                17 | 18 => (2, 2 * step),
                t if t < 0 => (1, 4),
                t => (t as usize, step),
            };
            if p.term > 8 || p.term < 0 {
                if pos + need > d.len() {
                    return false;
                }
                let s: Vec<i32> = d[pos..pos + need].chunks_exact(2).map(exp2s_at).collect();
                if p.term < 0 {
                    (p.a[0], p.b[0]) = (s[0], s[1]);
                } else {
                    (p.a[0], p.a[1]) = (s[0], s[1]);
                    if !mono {
                        (p.b[0], p.b[1]) = (s[2], s[3]);
                    }
                }
                pos += need;
                continue;
            }
            for m in 0..count {
                if pos + step > d.len() {
                    return false;
                }
                p.a[m] = exp2s_at(&d[pos..]);
                if !mono {
                    p.b[m] = exp2s_at(&d[pos + 2..]);
                }
                pos += step;
            }
        }
        pos == d.len()
    }
}
