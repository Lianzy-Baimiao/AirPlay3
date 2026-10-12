//! 自适应 FIR 滤波器（参考库 CNNFilter，BSD 许可，见 mod.rs）：用前 order 个（饱和到 16 位的）输出做预测，
//! 系数按符号 LMS 调整，步长随信号大小变。32 位样本的系数是 32 位，其余是 16 位。
//! 溢出行为照参考库的通用实现：16 位系数时点积按 32 位回绕，32 位系数时每项按 32 位回绕再累加成 64 位。

/// 历史窗口：用到窗口末尾时把最后 order 个移回开头
const WINDOW: usize = 512;

pub(super) trait Tap: Copy + Default {
    /// 点积（16 位：32 位回绕；32 位：每项 32 位回绕，和是 64 位）
    fn dot(a: &[Self], b: &[Self]) -> i64;
    /// m += d（方向为负）或 m -= d（方向为正）
    fn adapt(m: &mut [Self], d: &[Self], negative: bool);
    fn from_i32(v: i32) -> Self;
    fn halve(self) -> Self;
    const WIDE: bool;
}

impl Tap for i16 {
    fn dot(a: &[i16], b: &[i16]) -> i64 {
        a.iter().zip(b).fold(0i32, |s, (&x, &y)| s.wrapping_add(x as i32 * y as i32)) as i64
    }
    fn adapt(m: &mut [i16], d: &[i16], negative: bool) {
        if negative {
            m.iter_mut().zip(d).for_each(|(m, &d)| *m = m.wrapping_add(d));
        } else {
            m.iter_mut().zip(d).for_each(|(m, &d)| *m = m.wrapping_sub(d));
        }
    }
    fn from_i32(v: i32) -> i16 {
        v as i16
    }
    fn halve(self) -> i16 {
        self >> 1
    }
    const WIDE: bool = false;
}

impl Tap for i32 {
    fn dot(a: &[i32], b: &[i32]) -> i64 {
        a.iter().zip(b).fold(0i64, |s, (&x, &y)| s.wrapping_add(x.wrapping_mul(y) as i64))
    }
    fn adapt(m: &mut [i32], d: &[i32], negative: bool) {
        if negative {
            m.iter_mut().zip(d).for_each(|(m, &d)| *m = m.wrapping_add(d));
        } else {
            m.iter_mut().zip(d).for_each(|(m, &d)| *m = m.wrapping_sub(d));
        }
    }
    fn from_i32(v: i32) -> i32 {
        v
    }
    fn halve(self) -> i32 {
        self >> 1
    }
    const WIDE: bool = true;
}

pub(super) struct NnFilter<T: Tap> {
    order: usize,
    shift: u32,
    round: i64,
    m: Vec<T>,
    input: Vec<T>,
    delta: Vec<T>,
    pos: usize,
    avg: i64,
    /// 3.98 起换了步长的算法
    new_delta: bool,
    /// 「过渡期」的 24 位文件：点积加舍入时按 64 位算
    pub interim: bool,
}

impl<T: Tap> NnFilter<T> {
    pub fn new(order: usize, shift: u32, version: u16) -> Self {
        NnFilter {
            order,
            shift,
            round: 1 << (shift - 1),
            m: vec![T::default(); order],
            input: vec![T::default(); order + WINDOW],
            delta: vec![T::default(); order + WINDOW],
            pos: order,
            avg: 0,
            new_delta: version >= 3980,
            interim: false,
        }
    }

    pub fn flush(&mut self) {
        self.m.fill(T::default());
        self.input[..=self.order].fill(T::default());
        self.delta[..=self.order].fill(T::default());
        self.pos = self.order;
        self.avg = 0;
    }

    /// 窄类型（非 32 位样本）的值回绕到 32 位
    fn wrap(v: i64) -> i64 {
        if T::WIDE { v } else { v as i32 as i64 }
    }

    pub fn decompress(&mut self, x: i64) -> i64 {
        let (o, p) = (self.order, self.pos);
        let dot = T::dot(&self.input[p - o..p], &self.m);
        let out = if T::WIDE || self.interim {
            Self::wrap(x.wrapping_add(dot.wrapping_add(self.round) >> self.shift))
        } else {
            Self::wrap(x.wrapping_add((dot as i32).wrapping_add(self.round as i32) as i64 >> self.shift))
        };
        if x != 0 {
            T::adapt(&mut self.m, &self.delta[p - o..p], x < 0);
        }
        let d = &mut self.delta;
        if self.new_delta || self.interim {
            let a = Self::wrap(out.wrapping_abs());
            let bits = |s: u32, mask: i64, sub: i64| T::from_i32((((out >> s) & mask) - sub) as i32);
            d[p] = if a > Self::wrap(self.avg.wrapping_mul(3)) {
                bits(25, 64, 32)
            } else if a > Self::wrap(self.avg.wrapping_mul(4)) / 3 {
                bits(26, 32, 16)
            } else if a > 0 {
                bits(27, 16, 8)
            } else {
                T::default()
            };
            self.avg = Self::wrap(self.avg.wrapping_add(Self::wrap(a.wrapping_sub(self.avg)) / 16));
            d[p - 1] = d[p - 1].halve();
            d[p - 2] = d[p - 2].halve();
            d[p - 8] = d[p - 8].halve();
        } else {
            d[p] = if out == 0 { T::default() } else { T::from_i32((((out >> 28) & 8) - 4) as i32) };
            d[p - 4] = d[p - 4].halve();
            d[p - 8] = d[p - 8].halve();
        }
        self.input[p] = T::from_i32(out.clamp(-32768, 32767) as i32);
        self.pos += 1;
        if self.pos == o + WINDOW {
            self.input.copy_within(WINDOW.., 0);
            self.delta.copy_within(WINDOW.., 0);
            self.pos = o;
        }
        out
    }
}
