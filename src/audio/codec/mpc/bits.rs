//! 高位在前的位读取，以及 Musepack 用到的几种码（改写自参考库 mpc_bits_reader，BSD 许可，见 mod.rs）。
//! 读过数据末尾时读到的是 0，并记下 `over`。

use super::tables::{CNK, CNK_LEN, CNK_LOST, Can, Huff, LOG2, LOG2_LOST};

pub(super) struct Bits<'a> {
    d: &'a [u8],
    pub pos: usize,
    pub over: bool,
}

impl<'a> Bits<'a> {
    pub fn new(d: &'a [u8], pos: usize) -> Self {
        Bits { d, pos, over: false }
    }

    /// 从当前位置起的 32 位（末尾之后补 0）
    fn peek32(&self) -> u32 {
        let i = self.pos >> 3;
        let mut b = [0u8; 8];
        if i < self.d.len() {
            let n = (self.d.len() - i).min(8);
            b[..n].copy_from_slice(&self.d[i..i + n]);
        }
        (u64::from_be_bytes(b) << (self.pos & 7) >> 32) as u32
    }

    fn skip(&mut self, n: u32) {
        self.pos += n as usize;
        if self.pos > self.d.len() * 8 {
            self.over = true;
        }
    }

    /// 读 n 位（0 ~ 32）
    pub fn read(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let v = self.peek32() >> (32 - n);
        self.skip(n);
        v
    }

    /// 参考库 mpc_bits_huff_dec / huff_lut：找到第一个下限 ≤ 接下来 16 位的项
    pub fn huff(&mut self, t: Huff) -> i32 {
        let code = (self.peek32() >> 16) as u16;
        let e = t.iter().find(|e| e.0 <= code).unwrap_or(&t[t.len() - 1]);
        self.skip(e.1 as u32);
        e.2 as i32
    }

    /// 参考库 mpc_bits_can_dec：典范 Huffman，值 = 符号表[基数 − 码的高位]
    pub fn can(&mut self, (t, sym): Can) -> i32 {
        let code = (self.peek32() >> 16) as u16;
        let e = t.iter().find(|e| e.0 <= code).unwrap_or(&t[t.len() - 1]);
        self.skip(e.1 as u32);
        let i = ((e.2 as i32 - (code >> (16 - e.1 as u32)) as i32) & 0xFF) as usize;
        sym.get(i).copied().unwrap_or(0) as i32
    }

    /// 0 ~ max−1 里的一个值（不是 2 的整数次幂个值时，较小的那部分少用一位）
    pub fn log_dec(&mut self, max: u32) -> u32 {
        if max == 0 || max > 32 {
            return 0;
        }
        let (len, lost) = (LOG2[max as usize - 1] as u32, LOG2_LOST[max as usize - 1] as u32);
        let mut v = if len > 1 { self.read(len - 1) } else { 0 };
        if v >= lost {
            v = ((v << 1) | self.read(1)) - lost;
        }
        v
    }

    /// n 位里有 k 个 1 的组合的序号 → 位图
    pub fn enum_dec(&mut self, mut k: u32, mut n: u32) -> u32 {
        if k == 0 || k > 16 || n == 0 || n > 32 {
            return 0;
        }
        let (len, lost) = (CNK_LEN[k as usize - 1][n as usize - 1] as u32, CNK_LOST[k as usize - 1][n as usize - 1]);
        let mut code = self.read(len.saturating_sub(1));
        if code >= lost {
            code = ((code << 1) | self.read(1)).wrapping_sub(lost);
        }
        let mut bits = 0u32;
        while k > 0 && n > 0 {
            n -= 1;
            let c = CNK[k as usize - 1][n as usize];
            if code >= c {
                bits |= 1 << n;
                code -= c;
                k -= 1;
            }
        }
        bits
    }
}
