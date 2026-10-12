//! Musepack 帧解码的状态与反量化（改写自参考库 mpc_decoder.c / requant.c，BSD 许可，见 mod.rs）。
//! 每帧 36 × 32 个子带样本：按量化级别（Res）解出整数，乘以「级别系数 × 比例因子」，中 / 侧时还原左右。

/// 每帧的采样点数；合成滤波器的延迟
pub(super) const FRAME: usize = 36 * 32;
pub(super) const DELAY: u64 = 481;

/// 各量化级别（Res = −1 ~ 17）的反量化系数，下标 Res + 1
const CC: [f32; 19] = [
    111.285962475327, 65536.000000000000, 21845.333333333332, 13107.200000000001, 9362.285714285713, 7281.777777777777,
    4369.066666666666, 2114.064516129032, 1040.253968253968, 516.031496062992, 257.003921568627, 128.250489236790,
    64.062561094819, 32.015632633121, 16.003907203907, 8.000976681723, 4.000244155527, 2.000061037018, 1.000015259021,
];
/// 各量化级别的零点偏移，下标 Res + 1
pub(super) const DC: [i32; 19] = [2, 0, 1, 2, 3, 4, 7, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767];
/// 直接存的量化级别（8 ~ 17）每个样本的位数，下标 Res
pub(super) const RES_BIT: [u32; 18] = [0, 0, 0, 0, 0, 0, 0, 0, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

pub(super) struct Decoder {
    pub version: u8,
    pub max_band: usize,
    pub ms: bool,
    pub channels: usize,
    pub samples: u64,
    pub decoded: u64,
    pub skip: u64,
    pub last_max_band: usize,
    r1: u32,
    r2: u32,
    pub scf_i: [[[i32; 3]; 32]; 2],
    pub q: [[[i16; 36]; 32]; 2],
    pub res: [[i32; 32]; 2],
    pub dscf_flag: [[bool; 32]; 2],
    pub scfi: [[i32; 32]; 2],
    pub ms_flag: [bool; 32],
    pub v: [Vec<f32>; 2],
    pub y: [[[f32; 32]; 36]; 2],
    scf: [f32; 256],
}

impl Decoder {
    pub fn new(version: u8, max_band: usize, ms: bool, channels: usize, samples: u64, beg_silence: u64) -> Self {
        let mut d = Decoder {
            version,
            max_band,
            ms,
            channels,
            samples,
            decoded: 0,
            skip: DELAY + beg_silence,
            last_max_band: 0,
            r1: 1,
            r2: 1,
            scf_i: [[[0; 3]; 32]; 2],
            q: [[[0; 36]; 32]; 2],
            res: [[0; 32]; 2],
            dscf_flag: [[false; 32]; 2],
            scfi: [[0; 32]; 2],
            ms_flag: [false; 32],
            v: [vec![0.0; 2304 + 960], vec![0.0; 2304 + 960]],
            y: [[[0.0; 32]; 36]; 2],
            scf: [0.0; 256],
        };
        d.scale_output(1.0);
        d
    }

    /// 比例因子表：相邻两档差 1.2005 倍（参考库 mpc_decoder_scale_output，下标按 8 位回绕）
    fn scale_output(&mut self, factor: f64) {
        let factor = factor * (1.0 / (1u32 << 15) as f64);
        let (mut f1, mut f2) = (factor, factor);
        self.scf[1] = factor as f32;
        const K: f64 = 0.83298066476582673961;
        const INV: f64 = 1.0 / 0.83298066476582673961;
        f1 *= K;
        f2 *= INV;
        for n in 1..=128u32 {
            self.scf[(1 + n) as u8 as usize] = f1 as f32;
            self.scf[(1u32.wrapping_sub(n)) as u8 as usize] = f2 as f32;
            f1 *= K;
            f2 *= INV;
        }
    }

    /// 噪声替代用的伪随机数（两个反向的线性反馈移位寄存器）
    pub fn random(&mut self) -> u32 {
        let parity = |x: u32| x.count_ones() & 1;
        let t1 = parity(self.r1 & 0xF5) << 31;
        let t2 = parity((self.r2 >> 25) & 0x63);
        self.r1 = (self.r1 >> 1) | t1;
        self.r2 = self.r2.wrapping_add(self.r2) | t2;
        self.r1 ^ self.r2
    }

    /// 噪声：四个字节之和 − 510
    pub fn noise(&mut self, q: &mut [i16; 36]) {
        for x in q.iter_mut() {
            let t = self.random();
            *x = ((t >> 24) + ((t >> 16) & 0xFF) + ((t >> 8) & 0xFF) + (t & 0xFF)) as i32 as i16 - 510;
        }
    }

    fn fac(&self, ch: usize, band: usize, part: usize) -> f32 {
        let r = self.res[ch][band];
        CC[(r + 1).clamp(0, 18) as usize] * self.scf[(self.scf_i[ch][band][part] & 0xFF) as usize]
    }

    /// 反量化：子带样本 → Y（按 12 个样本一段用三个比例因子）
    pub fn requantize(&mut self) {
        for band in 0..=self.max_band.min(31) {
            let (rl, rr) = (self.res[0][band] != 0, self.res[1][band] != 0);
            for part in 0..3 {
                let fl = if rl { self.fac(0, band, part) } else { 0.0 };
                let fr = if rr { self.fac(1, band, part) } else { 0.0 };
                for n in part * 12..part * 12 + 12 {
                    let l = self.q[0][band][n] as f32;
                    let r = self.q[1][band][n] as f32;
                    let (yl, yr) = match (self.ms_flag[band], rl, rr) {
                        (true, true, true) => {
                            let (a, b) = (fl * l, fr * r);
                            (a + b, a - b)
                        }
                        (true, true, false) => (fl * l, fl * l),
                        (true, false, true) => {
                            let a = fr * r;
                            (a, -a)
                        }
                        (false, true, true) => (fl * l, fr * r),
                        (false, true, false) => (fl * l, 0.0),
                        (false, false, true) => (0.0, fr * r),
                        (_, false, false) => (0.0, 0.0),
                    };
                    self.y[0][n][band] = yl;
                    self.y[1][n][band] = yr;
                }
            }
        }
    }
}
