//! 位读取器。读过数据末尾时读到的是 0，并记下 `overrun`（截断的文件：解码到那里为止）。

/// 低位在前（TTA、WavPack、TAK）
pub struct LsbReader<'a> {
    data: &'a [u8],
    pos: usize,
    cache: u64,
    have: u32,
    pub overrun: bool,
}

impl<'a> LsbReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        LsbReader { data, pos: 0, cache: 0, have: 0, overrun: false }
    }

    /// 补满缓存；末尾之后补 0
    fn refill(&mut self) {
        while self.have <= 56 {
            let b = self.data.get(self.pos).copied().unwrap_or(0);
            self.pos += 1;
            self.cache |= (b as u64) << self.have;
            self.have += 8;
        }
    }

    /// 已经读过的位数
    pub fn position(&self) -> usize {
        self.pos * 8 - self.have as usize
    }

    /// 剩下的位数（读过头时为负）
    pub fn left(&self) -> isize {
        (self.data.len() * 8) as isize - self.position() as isize
    }

    fn check(&mut self) {
        if self.position() > self.data.len() * 8 {
            self.overrun = true;
        }
    }

    /// 读 n 位（0 ~ 32）
    pub fn bits(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        if self.have < n {
            self.refill();
        }
        let v = (self.cache & ((1u64 << n) - 1)) as u32;
        self.cache >>= n;
        self.have -= n;
        self.check();
        v
    }

    pub fn bit(&mut self) -> bool {
        self.bits(1) != 0
    }

    /// 一元码：连续的 1 的个数（遇到 0 停，这个 0 也读掉）；读满 `max` 个 1 时停（不读 0）
    pub fn ones(&mut self, max: u32) -> u32 {
        let mut n = 0;
        while n < max {
            if self.have == 0 {
                self.refill();
            }
            let run = self.cache.trailing_ones().min(self.have).min(max - n);
            n += run;
            self.cache = self.cache.checked_shr(run).unwrap_or(0);
            self.have -= run;
            if n == max {
                break;
            }
            if self.have > 0 {
                // 下一位是 0
                self.cache >>= 1;
                self.have -= 1;
                break;
            }
        }
        self.check();
        n
    }

    /// 跳到下一个字节边界
    pub fn align(&mut self) {
        let r = self.have % 8;
        self.cache >>= r;
        self.have -= r;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsb_bits() {
        let d = [0b1010_1100u8, 0xFF, 0x01, 0x80];
        let mut r = LsbReader::new(&d);
        assert_eq!(r.bits(2), 0b00);
        assert_eq!(r.bits(3), 0b011);
        assert_eq!(r.bits(3), 0b101);
        assert_eq!(r.bits(9), 0x1FF);
        assert_eq!(r.position(), 17);
        r.align();
        assert_eq!(r.position(), 24);
        assert_eq!(r.bits(8), 0x80);
        assert!(!r.overrun);
        assert_eq!(r.left(), 0);
        assert_eq!(r.bits(1), 0);
        assert!(r.overrun);
    }

    #[test]
    fn lsb_unary() {
        // 0b0000_0111：三个 1 再一个 0；然后 0xFF 0xFF 0x0F 里接着 20 个 1 再一个 0
        let d = [0b0000_0111u8, 0xFF, 0xFF, 0x0F, 0x00];
        let mut r = LsbReader::new(&d);
        assert_eq!(r.ones(100), 3);
        assert_eq!(r.position(), 4);
        assert_eq!(r.bits(4), 0);
        assert_eq!(r.ones(100), 20);
        assert_eq!(r.position(), 8 + 21);
        assert!(!r.overrun);
        // 读满上限时不读后面的位
        let mut r = LsbReader::new(&[0xFFu8, 0xFF]);
        assert_eq!(r.ones(5), 5);
        assert_eq!(r.position(), 5);
        // 跨过 64 位缓存的长一元码
        let mut r = LsbReader::new(&[0xFFu8; 12]);
        assert_eq!(r.ones(90), 90);
        assert!(!r.overrun);
        // 全是 1 读到末尾
        let mut r = LsbReader::new(&[0xFFu8]);
        assert_eq!(r.ones(100), 8);
        assert!(r.overrun);
    }
}
