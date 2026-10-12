//! SV7 / SV8 一帧的位流（改写自参考库 mpc_decoder.c 的 read_bitstream_sv7 / sv8，BSD 许可，见 mod.rs）：
//! 各子带的量化级别 → 比例因子的分组方式（SCFI）→ 比例因子（差分）→ 样本。

use super::bits::Bits;
use super::decoder::{DC, Decoder, RES_BIT};
use super::tables::{CAN_BANDS, CAN_DSCF, CAN_Q, CAN_Q1, CAN_Q9UP, CAN_RES, CAN_SCFI, HUFF_DSCF, HUFF_HDR, HUFF_Q, HUFF_SCFI, HUFFQ2_VAR};

impl Decoder {
    pub fn read_sv7(&mut self, r: &mut Bits) {
        let mut used = 0;
        for n in 0..=self.max_band {
            for c in 0..2 {
                self.res[c][n] = if n == 0 {
                    r.read(4) as i32
                } else {
                    let idx = r.huff(HUFF_HDR);
                    if idx != 4 { self.res[c][n - 1] + idx } else { r.read(4) as i32 }
                };
            }
            if self.res[0][n] != 0 || self.res[1][n] != 0 {
                if self.ms {
                    self.ms_flag[n] = r.read(1) != 0;
                }
                used = n + 1;
            }
        }
        for n in 0..used {
            for c in 0..2 {
                if self.res[c][n] != 0 {
                    self.scfi[c][n] = r.huff(HUFF_SCFI);
                }
            }
        }
        for n in 0..used {
            for c in 0..2 {
                if self.res[c][n] == 0 {
                    continue;
                }
                let d = |r: &mut Bits, prev: i32| {
                    let idx = r.huff(HUFF_DSCF);
                    if idx != 8 { prev + idx } else { r.read(6) as i32 }
                };
                let s = &mut self.scf_i[c][n];
                match self.scfi[c][n] {
                    1 => {
                        s[0] = d(r, s[2]);
                        s[1] = d(r, s[0]);
                        s[2] = s[1];
                    }
                    3 => {
                        s[0] = d(r, s[2]);
                        (s[1], s[2]) = (s[0], s[0]);
                    }
                    2 => {
                        s[0] = d(r, s[2]);
                        s[1] = s[0];
                        s[2] = d(r, s[1]);
                    }
                    0 => {
                        s[0] = d(r, s[2]);
                        s[1] = d(r, s[0]);
                        s[2] = d(r, s[1]);
                    }
                    _ => return,
                }
                for x in s.iter_mut() {
                    if *x > 1024 {
                        *x = 0x8080;
                    }
                }
            }
        }
        for n in 0..used {
            for c in 0..2 {
                let res = self.res[c][n];
                let mut q = self.q[c][n];
                match res {
                    -17..=-2 | 0 => {}
                    -1 => self.noise(&mut q),
                    1 => {
                        let t = HUFF_Q[0][r.read(1) as usize];
                        for k in (0..36).step_by(3) {
                            let i = r.huff(t).clamp(0, 26);
                            (q[k], q[k + 1], q[k + 2]) = ((i % 3 - 1) as i16, (i / 3 % 3 - 1) as i16, (i / 9 - 1) as i16);
                        }
                    }
                    2 => {
                        let t = HUFF_Q[1][r.read(1) as usize];
                        for k in (0..36).step_by(2) {
                            let i = r.huff(t).clamp(0, 24);
                            (q[k], q[k + 1]) = ((i % 5 - 2) as i16, (i / 5 - 2) as i16);
                        }
                    }
                    3..=7 => {
                        let t = HUFF_Q[res as usize - 1][r.read(1) as usize];
                        q.iter_mut().for_each(|x| *x = r.huff(t) as i16);
                    }
                    8..=17 => q.iter_mut().for_each(|x| *x = (r.read(RES_BIT[res as usize]) as i32 - DC[res as usize + 1]) as i16),
                    _ => return,
                }
                self.q[c][n] = q;
            }
        }
    }

    pub fn read_sv8(&mut self, r: &mut Bits, key: bool) {
        let used = if key {
            r.log_dec(self.max_band as u32 + 1) as usize
        } else {
            let u = self.last_max_band + r.can(CAN_BANDS).max(0) as usize;
            if u > 32 { u - 33 } else { u }
        }
        .min(32);
        self.last_max_band = used;
        if used > 0 {
            for c in 0..2 {
                let v = r.can(CAN_RES[0]);
                self.res[c][used - 1] = if v > 15 { v - 17 } else { v };
            }
            for n in (0..used - 1).rev() {
                for c in 0..2 {
                    let v = r.can(CAN_RES[(self.res[c][n + 1] > 2) as usize]) + self.res[c][n + 1];
                    self.res[c][n] = if v > 15 { v - 17 } else { v };
                }
            }
            if self.ms {
                let active = |d: &Self, n: usize| d.res[0][n] != 0 || d.res[1][n] != 0;
                let tot = (0..used).filter(|&n| active(self, n)).count() as u32;
                let cnt = r.log_dec(tot);
                let mut tmp = if cnt != 0 && cnt != tot { r.enum_dec(cnt.min(tot - cnt), tot) } else { 0 };
                if cnt * 2 > tot {
                    tmp = !tmp;
                }
                for n in (0..used).rev() {
                    if active(self, n) {
                        self.ms_flag[n] = tmp & 1 != 0;
                        tmp >>= 1;
                    }
                }
            }
        }
        for n in used..=self.max_band.min(31) {
            (self.res[0][n], self.res[1][n]) = (0, 0);
        }
        if key {
            self.dscf_flag = [[true; 32]; 2];
        }
        for n in 0..used {
            let (l, rr) = (self.res[0][n] != 0, self.res[1][n] != 0);
            if l || rr {
                let cnt = (l && rr) as usize;
                let t = r.can(CAN_SCFI[cnt]);
                if l {
                    self.scfi[0][n] = t >> (2 * cnt);
                }
                if rr {
                    self.scfi[1][n] = t & 3;
                }
            }
        }
        for n in 0..used {
            for c in 0..2 {
                if self.res[c][n] == 0 {
                    continue;
                }
                let s = &mut self.scf_i[c][n];
                if self.dscf_flag[c][n] {
                    s[0] = r.read(7) as i32 - 6;
                    self.dscf_flag[c][n] = false;
                } else {
                    let mut t = r.can(CAN_DSCF[1]);
                    if t == 64 {
                        t += r.read(6) as i32;
                    }
                    s[0] = ((s[2] - 25 + t) & 127) - 6;
                }
                for m in 0..2 {
                    if (self.scfi[c][n] << m) & 2 == 0 {
                        let mut t = r.can(CAN_DSCF[0]);
                        if t == 31 {
                            t = 64 + r.read(6) as i32;
                        }
                        s[m + 1] = ((s[m] - 25 + t) & 127) - 6;
                    } else {
                        s[m + 1] = s[m];
                    }
                }
            }
        }
        for n in 0..used {
            for c in 0..2 {
                let res = self.res[c][n];
                let mut q = self.q[c][n];
                self.sv8_samples(r, res, &mut q);
                self.q[c][n] = q;
            }
        }
    }

    fn sv8_samples(&mut self, r: &mut Bits, res: i32, q: &mut [i16; 36]) {
        const THRES: [i32; 9] = [0, 0, 3, 0, 0, 1, 3, 4, 8];
        match res {
            0 => {}
            -1 => self.noise(q),
            1 => {
                for half in 0..2 {
                    let cnt = r.can(CAN_Q1) as u32;
                    let mut idx = if cnt > 0 && cnt < 18 { r.enum_dec(if cnt <= 9 { cnt } else { 18 - cnt }, 18) } else { 0 };
                    if cnt > 9 {
                        idx = !idx;
                    }
                    for x in &mut q[half * 18..half * 18 + 18] {
                        *x = if idx & (1 << 17) != 0 { ((r.read(1) << 1) as i32 - 1) as i16 } else { 0 };
                        idx <<= 1;
                    }
                }
            }
            2 => {
                let mut idx = 2 * THRES[2];
                for k in (0..36).step_by(3) {
                    let t = r.can(CAN_Q[0][(idx > THRES[2]) as usize]).clamp(0, 124);
                    (q[k], q[k + 1], q[k + 2]) = ((t % 5 - 2) as i16, (t / 5 % 5 - 2) as i16, (t / 25 - 2) as i16);
                    idx = (idx >> 1) + HUFFQ2_VAR[t as usize] as i32;
                }
            }
            3 | 4 => {
                let t = CAN_Q[1][res as usize - 3];
                for k in (0..36).step_by(2) {
                    let s = r.can(t) as i8;
                    (q[k], q[k + 1]) = (((s << 4) >> 4) as i16, (s >> 4) as i16);
                }
            }
            5..=8 => {
                let th = THRES[res as usize];
                let mut idx = 2 * th;
                for x in q.iter_mut() {
                    *x = r.can(CAN_Q[res as usize - 3][(idx > th) as usize]) as i16;
                    idx = (idx >> 1) + (*x as i32).abs();
                }
            }
            9..=17 => {
                for x in q.iter_mut() {
                    let mut v = r.can(CAN_Q9UP) as u8 as i32;
                    if res != 9 {
                        v = (v << (res - 9)) | r.read(res as u32 - 9) as i32;
                    }
                    *x = (v as i16).wrapping_sub(DC[res as usize + 1] as i16);
                }
            }
            _ => {}
        }
    }
}