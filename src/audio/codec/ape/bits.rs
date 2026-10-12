//! 帧数据的读取（改写自参考库 UnBitArrayBase / UnBitArray / UnBitArrayOld，BSD 许可，见 mod.rs）。
//! 文件里按 32 位小端字存放、每个字从最高位读起；读入时把每 4 个字节倒序，之后就是普通的高位在前位流。
//! 3.90 以后用区间编码（range coder），3.99 起换了一套符号表和取值方法。

/// 区间编码的符号累计频率（3.90 ~ 3.98 用 1，3.99 起用 2）；第 i 个符号的宽度 = 下一项 − 这一项
pub(super) const RANGE_TOTAL_1: [u32; 65] = [
    0, 14824, 28224, 39348, 47855, 53994, 58171, 60926, 62682, 63786, 64463, 64878, 65126, 65276, 65365, 65419, 65450,
    65469, 65480, 65487, 65491, 65493, 65494, 65495, 65496, 65497, 65498, 65499, 65500, 65501, 65502, 65503, 65504,
    65505, 65506, 65507, 65508, 65509, 65510, 65511, 65512, 65513, 65514, 65515, 65516, 65517, 65518, 65519, 65520,
    65521, 65522, 65523, 65524, 65525, 65526, 65527, 65528, 65529, 65530, 65531, 65532, 65533, 65534, 65535, 65536,
];
pub(super) const RANGE_TOTAL_2: [u32; 65] = [
    0, 19578, 36160, 48417, 56323, 60899, 63265, 64435, 64971, 65232, 65351, 65416, 65447, 65466, 65476, 65482, 65485,
    65488, 65490, 65491, 65492, 65493, 65494, 65495, 65496, 65497, 65498, 65499, 65500, 65501, 65502, 65503, 65504,
    65505, 65506, 65507, 65508, 65509, 65510, 65511, 65512, 65513, 65514, 65515, 65516, 65517, 65518, 65519, 65520,
    65521, 65522, 65523, 65524, 65525, 65526, 65527, 65528, 65529, 65530, 65531, 65532, 65533, 65534, 65535, 65536,
];

const BOTTOM: u32 = 1 << 23;

pub(super) struct Bits<'a> {
    d: &'a [u8],
    pub pos: usize,
    limit: usize,
    /// 读过了数据末尾（或区间编码出错）：这一帧解到这里为止
    pub over: bool,
    low: u32,
    range: u32,
    buffer: u32,
}

impl<'a> Bits<'a> {
    /// `d` 已经按字倒序；从第 `start` 位读起，`limit` 位以后算读过头
    pub fn new(d: &'a [u8], start: usize, limit: usize) -> Self {
        Bits { d, pos: start, limit, over: false, low: 0, range: 0, buffer: 0 }
    }

    /// 从当前位置起的 64 位（至少前 57 位有效，末尾之后是 0）
    fn window(&self) -> u64 {
        let i = self.pos >> 3;
        let mut b = [0u8; 8];
        if i < self.d.len() {
            let n = (self.d.len() - i).min(8);
            b[..n].copy_from_slice(&self.d[i..i + n]);
        }
        u64::from_be_bytes(b) << (self.pos & 7)
    }

    /// 读 n 位（0 ~ 32）
    pub fn bits(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let v = (self.window() >> (64 - n)) as u32;
        self.pos += n as usize;
        if self.pos > self.limit {
            self.over = true;
        }
        v
    }

    /// 一串 0 和后面的 1：返回 0 的个数（1 也读掉）
    pub fn zeros(&mut self) -> u32 {
        let mut n = 0u32;
        loop {
            if self.pos >= self.limit {
                self.over = true;
                return n;
            }
            let z = self.window().leading_zeros();
            if z < 57 {
                self.pos += z as usize + 1;
                if self.pos > self.limit {
                    self.over = true;
                }
                return n + z;
            }
            self.pos += 57;
            n += 57;
        }
    }

    pub fn align(&mut self) {
        self.pos = (self.pos + 7) & !7;
    }

    /// 开始一段区间编码（参考库 FlushBitArray：第一个字节不用）
    pub fn range_start(&mut self) {
        self.align();
        self.bits(8);
        self.buffer = self.bits(8);
        self.low = self.buffer >> 1;
        self.range = 1 << 7;
    }

    /// 结束一段区间编码，位置移到编码器写出的末尾（3.95 以前多退回两个字节）
    pub fn range_finish(&mut self, version: u16) {
        while self.range <= BOTTOM {
            self.pos += 8;
            self.range <<= 8;
            if self.range == 0 {
                return;
            }
        }
        if version <= 3950 {
            self.pos = self.pos.saturating_sub(16);
        }
    }

    fn normalize(&mut self) -> bool {
        while self.range <= BOTTOM {
            if self.range == 0 {
                self.over = true;
                return false;
            }
            self.buffer = (self.buffer << 8) | self.bits(8);
            self.low = (self.low << 8) | ((self.buffer >> 1) & 0xff);
            self.range <<= 8;
        }
        true
    }

    /// 参考库 RangeDecodeFast：只看不取
    pub fn peek(&mut self, shift: u32) -> u32 {
        if !self.normalize() {
            return 0;
        }
        self.range >>= shift;
        if self.range == 0 {
            self.over = true;
            return 0;
        }
        self.low / self.range
    }

    /// 参考库 RangeDecodeFastWithUpdate：取 shift 位宽的均匀分布值
    pub fn take(&mut self, shift: u32) -> u32 {
        self.divide(|r| r.checked_shr(shift).unwrap_or(0))
    }

    /// 把区间按 f(range) 等分，取出落在哪一份
    pub fn divide(&mut self, f: impl Fn(u32) -> u32) -> u32 {
        if !self.normalize() {
            return 0;
        }
        self.range = f(self.range);
        if self.range == 0 {
            self.over = true;
            return 0;
        }
        let v = self.low / self.range;
        self.low %= self.range;
        v
    }

    /// 按累计频率表取一个符号（0 ~ 63）
    pub fn symbol(&mut self, total: &[u32; 65]) -> usize {
        let t = self.peek(16);
        if t >= 65536 {
            self.over = true;
            return 0;
        }
        let s = total.partition_point(|&x| x <= t) - 1;
        self.low = self.low.wrapping_sub(self.range.wrapping_mul(total[s]));
        self.range = self.range.wrapping_mul(total[s + 1] - total[s]);
        s
    }
}
