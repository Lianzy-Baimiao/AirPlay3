//! 预测器（改写自参考库 NewPredictor / NewPredictorOld / Interim，BSD 许可，见 mod.rs）：残差先倒着过
//! 1 ~ 3 级 NN 滤波器，再加上两组自适应低阶预测（本声道、立体声时另一声道），最后一阶固定滤波。
//! 32 位样本用 64 位整数运算，其余用 32 位（回绕）。

use super::nnfilter::{NnFilter, Tap};

/// 各压缩级别的 NN 滤波器（阶数, 移位），按编码顺序；解码时倒过来
fn nn_spec(level: u16) -> Option<&'static [(usize, u32)]> {
    Some(match level {
        1000 => &[],
        2000 => &[(16, 11)],
        3000 => &[(64, 11)],
        4000 => &[(256, 13), (32, 10)],
        5000 => &[(1280, 15), (256, 13), (16, 11)],
        _ => return None,
    })
}

fn filters<T: Tap>(level: u16, version: u16) -> Option<Vec<NnFilter<T>>> {
    Some(nn_spec(level)?.iter().rev().map(|&(o, s)| NnFilter::new(o, s, version)).collect())
}

const WINDOW: usize = 256;
const HIST: usize = 8;

/// 3.95 起；`T` 是 NN 滤波器的系数类型（32 位样本 i32，其余 i16）
pub(super) struct Pred3950<T: Tap> {
    nn: Vec<NnFilter<T>>,
    pa: [i64; WINDOW + HIST],
    pb: [i64; WINDOW + HIST],
    ada: [i64; WINDOW + HIST],
    adb: [i64; WINDOW + HIST],
    pos: usize,
    ma: [i64; 4],
    mb: [i64; 5],
    last_a: i64,
    s1a: i32,
    s1b: i32,
    /// 「过渡期」（Monkey's Audio 5.01 ~ 8.51 写的 24 位文件）：预测值按 64 位算完再截
    interim: bool,
}

impl<T: Tap> Pred3950<T> {
    fn new(level: u16, version: u16) -> Option<Self> {
        let z = [0i64; WINDOW + HIST];
        let mut p = Pred3950 {
            nn: filters(level, version)?,
            pa: z,
            pb: z,
            ada: z,
            adb: z,
            pos: HIST,
            ma: [0; 4],
            mb: [0; 5],
            last_a: 0,
            s1a: 0,
            s1b: 0,
            interim: false,
        };
        p.flush();
        Some(p)
    }

    fn w(v: i64) -> i64 {
        if T::WIDE { v } else { v as i32 as i64 }
    }

    fn flush(&mut self) {
        self.nn.iter_mut().for_each(NnFilter::flush);
        for b in [&mut self.pa, &mut self.pb, &mut self.ada, &mut self.adb] {
            b[..=HIST].fill(0);
        }
        self.pos = HIST;
        (self.ma, self.mb) = ([360, 317, -109, 98], [0; 5]);
        (self.last_a, self.s1a, self.s1b) = (0, 0, 0);
    }

    fn decompress(&mut self, a: i64, b: i64) -> i64 {
        let w = Self::w;
        if self.pos == WINDOW + HIST {
            for buf in [&mut self.pa, &mut self.pb, &mut self.ada, &mut self.adb] {
                buf.copy_within(WINDOW.., 0);
            }
            self.pos = HIST;
        }
        let mut x = w(a);
        for f in &mut self.nn {
            x = f.decompress(x);
        }
        let p = self.pos;
        self.pa[p] = self.last_a;
        self.pa[p - 1] = w(self.pa[p].wrapping_sub(self.pa[p - 1]));
        // 另一声道先过一阶固定滤波（参考库 Stage1FilterB.Compress）
        let bi = b as i32;
        self.pb[p] = w((bi as i64).wrapping_sub(w(self.s1b as i64 * 31) >> 5));
        self.s1b = bi;
        self.pb[p - 1] = w(self.pb[p].wrapping_sub(self.pb[p - 1]));
        let sa = (0..4).fold(0i64, |s, i| s.wrapping_add(self.pa[p - i].wrapping_mul(self.ma[i])));
        let sb = (0..5).fold(0i64, |s, i| s.wrapping_add(self.pb[p - i].wrapping_mul(self.mb[i])));
        let cur = if T::WIDE || self.interim {
            w(x.wrapping_add(w(sa.wrapping_add(sb >> 1) >> 10)))
        } else {
            let t = (sa as i32).wrapping_add((sb as i32) >> 1);
            w(x.wrapping_add((t >> 10) as i64))
        };
        let sgn = |v: i64| if v != 0 { ((v >> 30) & 2) - 1 } else { 0 };
        (self.ada[p], self.ada[p - 1]) = (sgn(self.pa[p]), sgn(self.pa[p - 1]));
        (self.adb[p], self.adb[p - 1]) = (sgn(self.pb[p]), sgn(self.pb[p - 1]));
        let dir = (x < 0) as i64 - (x > 0) as i64;
        for i in 0..4 {
            self.ma[i] = w(self.ma[i].wrapping_add(self.ada[p - i] * dir));
        }
        for i in 0..5 {
            self.mb[i] = w(self.mb[i].wrapping_add(self.adb[p - i] * dir));
        }
        // 一阶固定滤波的逆（Stage1FilterA.Decompress）
        self.s1a = cur.wrapping_add(w(self.s1a as i64 * 31) >> 5) as i32;
        self.last_a = cur;
        self.pos += 1;
        self.s1a as i64
    }
}

/// 3.93 ~ 3.94：只有本声道的四阶预测
pub(super) struct Pred3930 {
    nn: Vec<NnFilter<i16>>,
    buf: [i32; WINDOW + HIST],
    pos: usize,
    m: [i32; 4],
    last: i32,
}

impl Pred3930 {
    fn new(level: u16, version: u16) -> Option<Self> {
        if level > 4000 {
            return None;
        }
        let mut p = Pred3930 { nn: filters(level, version)?, buf: [0; WINDOW + HIST], pos: HIST, m: [0; 4], last: 0 };
        p.flush();
        Some(p)
    }

    fn flush(&mut self) {
        self.nn.iter_mut().for_each(NnFilter::flush);
        self.buf[..=HIST].fill(0);
        (self.pos, self.m, self.last) = (HIST, [360, 317, -109, 98], 0);
    }

    fn decompress(&mut self, a: i64) -> i64 {
        if self.pos == WINDOW + HIST {
            self.buf.copy_within(WINDOW.., 0);
            self.pos = HIST;
        }
        let mut x = a as i32 as i64;
        for f in &mut self.nn {
            x = f.decompress(x);
        }
        let x = x as i32;
        let b = &self.buf;
        let p = self.pos;
        let d = [b[p - 1], b[p - 1].wrapping_sub(b[p - 2]), b[p - 2].wrapping_sub(b[p - 3]), b[p - 3].wrapping_sub(b[p - 4])];
        let s = (0..4).fold(0i32, |s, i| s.wrapping_add(d[i].wrapping_mul(self.m[i])));
        self.buf[p] = x.wrapping_add(s >> 9);
        let dir = (x < 0) as i32 - (x > 0) as i32;
        for i in 0..4 {
            self.m[i] = self.m[i].wrapping_add((((d[i] >> 30) & 2) - 1) * dir);
        }
        self.last = self.buf[p].wrapping_add(self.last.wrapping_mul(31) >> 5);
        self.pos += 1;
        self.last as i64
    }
}

pub(super) enum Predictor {
    Narrow(Box<Pred3950<i16>>),
    Wide(Box<Pred3950<i32>>),
    Old(Box<Pred3930>),
}

impl Predictor {
    /// 不支持的压缩级别时返回 None
    pub fn new(level: u16, version: u16, bits: u16) -> Option<Self> {
        Some(if version < 3950 {
            Predictor::Old(Box::new(Pred3930::new(level, version)?))
        } else if bits < 32 {
            Predictor::Narrow(Box::new(Pred3950::new(level, version)?))
        } else {
            Predictor::Wide(Box::new(Pred3950::new(level, version)?))
        })
    }

    pub fn flush(&mut self) {
        match self {
            Predictor::Narrow(p) => p.flush(),
            Predictor::Wide(p) => p.flush(),
            Predictor::Old(p) => p.flush(),
        }
    }

    /// `b` 是立体声时另一声道（3.95 起才用）
    pub fn decompress(&mut self, a: i64, b: i64) -> i32 {
        (match self {
            Predictor::Narrow(p) => p.decompress(a, b),
            Predictor::Wide(p) => p.decompress(a, b),
            Predictor::Old(p) => p.decompress(a),
        }) as i32
    }

    /// 是否用「过渡期」算法（只对 3.95 起的非 32 位样本有意义）
    pub fn set_interim(&mut self, on: bool) {
        if let Predictor::Narrow(p) = self {
            p.interim = on;
            p.nn.iter_mut().for_each(|f| f.interim = on);
        }
    }
}