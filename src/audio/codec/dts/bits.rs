// 改写自 FFmpeg 的 get_bits.h / dca.h（LGPL 2.1+），见 mod.rs 开头的说明。
//! DTS 用的位读取（高位在前）。照 FFmpeg 带检查的读法：读过末尾得到 0，位置最多停在末尾之后 8 位
//! （FFmpeg 的 size_in_bits_plus8），所以“剩下的位数”最小是 -8，各处用它判断帧被截断。

use super::tables::Vlc;

pub(super) struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    size: usize,
}

impl<'a> Bits<'a> {
    pub fn new(d: &'a [u8]) -> Self {
        Bits { d, pos: 0, size: d.len() * 8 }
    }

    pub fn data(&self) -> &'a [u8] {
        self.d
    }

    /// 已读的位数
    pub fn count(&self) -> usize {
        self.pos
    }

    /// 总位数
    pub fn size(&self) -> usize {
        self.size
    }

    /// 剩下的位数（读过头时为负）
    pub fn left(&self) -> isize {
        self.size as isize - self.pos as isize
    }

    /// 从当前位置起的 32 位，不移动位置
    pub fn peek32(&self) -> u32 {
        let byte = self.pos >> 3;
        let v = (0..5).fold(0u64, |v, i| v << 8 | self.d.get(byte + i).copied().unwrap_or(0) as u64);
        (v >> (8 - (self.pos & 7))) as u32
    }

    pub fn skip(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.size + 8);
    }

    /// 读 n 位（0 ~ 32）
    pub fn bits(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let v = self.peek32() >> (32 - n);
        self.skip(n as usize);
        v
    }

    pub fn bit(&mut self) -> bool {
        self.bits(1) != 0
    }

    /// 读 n 位补码（1 ~ 32）
    pub fn sbits(&mut self, n: u32) -> i32 {
        if n == 0 {
            return 0;
        }
        ((self.bits(n) << (32 - n)) as i32) >> (32 - n)
    }

    /// 按码表读一个符号（表是完整的前缀码，任何位串都能解出）
    pub fn vlc(&mut self, t: Vlc) -> i32 {
        let p = self.peek32();
        let i = t.partition_point(|e| e.0 <= p) - 1;
        self.skip(t[i].1 as usize);
        t[i].2 as i32
    }

    /// 跳到第 p 位（ff_dca_seek_bits）：不能往回跳、不能跳过末尾
    pub fn seek(&mut self, p: usize) -> bool {
        if p < self.pos || p > self.size {
            return false;
        }
        self.pos = p;
        true
    }

    /// 无条件移到第 p 位（FFmpeg 的 skip_bits_long，位置限制在末尾之后 8 位内）
    pub fn set_pos(&mut self, p: usize) {
        self.pos = p.min(self.size + 8);
    }

    /// 跳到下一个字节边界
    pub fn align(&mut self) {
        let n = self.pos.wrapping_neg() & 7;
        self.skip(n);
    }
}

/// CRC-16/CCITT（多项式 0x1021，初值 0xFFFF）；数据连同末尾存的 CRC 一起算，结果为 0 表示正确
pub(super) fn crc16(data: &[u8]) -> u16 {
    let mut c = 0xFFFFu16;
    for &b in data {
        c ^= (b as u16) << 8;
        for _ in 0..8 {
            c = if c & 0x8000 != 0 { c << 1 ^ 0x1021 } else { c << 1 };
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msb_bits() {
        let d = [0b1010_1100u8, 0xFF, 0x01, 0x80, 0x12];
        let mut r = Bits::new(&d);
        assert_eq!(r.bits(3), 0b101);
        assert_eq!(r.bits(5), 0b01100);
        assert_eq!(r.bits(12), 0xFF0);
        assert_eq!(r.sbits(4), 1);
        r.align();
        assert_eq!(r.count(), 24);
        assert_eq!(r.bits(16), 0x8012);
        assert_eq!(r.left(), 0);
        // 读过末尾：得 0，位置停在末尾后 8 位
        assert_eq!(r.bits(32), 0);
        assert_eq!(r.left(), -8);
        assert!(!r.seek(48));
        let mut r = Bits::new(&d);
        r.skip(4);
        assert_eq!(r.peek32(), 0xCFF0_1801);
        assert_eq!(r.sbits(4), -4);
        assert!(r.seek(40));
        assert!(!r.seek(39));
    }

    #[test]
    fn vlc_and_crc() {
        // 转换模式码表 0：0 / 10 / 110 / 111 → 0 / 1 / 2 / 3
        let d = [0b0101_1011u8, 0b1000_0000];
        let mut r = Bits::new(&d);
        let t = super::super::tables::TRANSITION_MODE[0];
        assert_eq!([r.vlc(t), r.vlc(t), r.vlc(t), r.vlc(t)], [0, 1, 2, 3]);
        assert_eq!(r.count(), 9);
        assert_eq!(crc16(b"123456789"), 0x29B1);
        assert_eq!(crc16(&[b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', 0x29, 0xB1]), 0);
    }
}
