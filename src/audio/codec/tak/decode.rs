// TAK 的帧解码：残差（自适应的分段编码）、每个子帧的线性预测、声道间去相关、整帧的积分。
// 改写自 FFmpeg 的 libavcodec/takdec.c（Copyright (c) 2012 Paul B Mahol）与 takdsp.c
// （Copyright (c) 2015 Paul B Mahol），以 GNU LGPL 2.1 或更新版本发布，见 mod.rs 开头的说明。
// 整数运算的顺序、截断与参考实现相同（C 里会回绕的地方用 wrapping_*），所以结果逐位一致。

use super::header::{CODEC_MONO_STEREO, CODEC_MULTICHANNEL, StreamInfo, check_crc};
use crate::audio::codec::bits::LsbReader as Bits;

/// 多声道时的去相关方式
const MC_DMODES: [u8; 4] = [1, 3, 4, 6];
const PREDICTOR_SIZES: [usize; 16] = [4, 8, 12, 16, 24, 32, 48, 64, 80, 96, 128, 160, 192, 224, 256, 0];

/// 残差的编码参数（init, escape, scale, aescape, bias）
#[rustfmt::skip]
const XCODES: [(u32, u32, u32, u32, u32); 50] = [
    (1, 0x1, 0x1, 0x3, 0x8), (2, 0x3, 0x1, 0x7, 0x6),
    (3, 0x5, 0x2, 0xE, 0xD), (3, 0x3, 0x3, 0xD, 0x18),
    (4, 0xB, 0x4, 0x1C, 0x19), (4, 0x6, 0x6, 0x1A, 0x30),
    (5, 0x16, 0x8, 0x38, 0x32), (5, 0xC, 0xC, 0x34, 0x60),
    (6, 0x2C, 0x10, 0x70, 0x64), (6, 0x18, 0x18, 0x68, 0xC0),
    (7, 0x58, 0x20, 0xE0, 0xC8), (7, 0x30, 0x30, 0xD0, 0x180),
    (8, 0xB0, 0x40, 0x1C0, 0x190), (8, 0x60, 0x60, 0x1A0, 0x300),
    (9, 0x160, 0x80, 0x380, 0x320), (9, 0xC0, 0xC0, 0x340, 0x600),
    (10, 0x2C0, 0x100, 0x700, 0x640), (10, 0x180, 0x180, 0x680, 0xC00),
    (11, 0x580, 0x200, 0xE00, 0xC80), (11, 0x300, 0x300, 0xD00, 0x1800),
    (12, 0xB00, 0x400, 0x1C00, 0x1900), (12, 0x600, 0x600, 0x1A00, 0x3000),
    (13, 0x1600, 0x800, 0x3800, 0x3200), (13, 0xC00, 0xC00, 0x3400, 0x6000),
    (14, 0x2C00, 0x1000, 0x7000, 0x6400), (14, 0x1800, 0x1800, 0x6800, 0xC000),
    (15, 0x5800, 0x2000, 0xE000, 0xC800), (15, 0x3000, 0x3000, 0xD000, 0x18000),
    (16, 0xB000, 0x4000, 0x1C000, 0x19000), (16, 0x6000, 0x6000, 0x1A000, 0x30000),
    (17, 0x16000, 0x8000, 0x38000, 0x32000), (17, 0xC000, 0xC000, 0x34000, 0x60000),
    (18, 0x2C000, 0x10000, 0x70000, 0x64000), (18, 0x18000, 0x18000, 0x68000, 0xC0000),
    (19, 0x58000, 0x20000, 0xE0000, 0xC8000), (19, 0x30000, 0x30000, 0xD0000, 0x180000),
    (20, 0xB0000, 0x40000, 0x1C0000, 0x190000), (20, 0x60000, 0x60000, 0x1A0000, 0x300000),
    (21, 0x160000, 0x80000, 0x380000, 0x320000), (21, 0xC0000, 0xC0000, 0x340000, 0x600000),
    (22, 0x2C0000, 0x100000, 0x700000, 0x640000), (22, 0x180000, 0x180000, 0x680000, 0xC00000),
    (23, 0x580000, 0x200000, 0xE00000, 0xC80000), (23, 0x300000, 0x300000, 0xD00000, 0x1800000),
    (24, 0xB00000, 0x400000, 0x1C00000, 0x1900000), (24, 0x600000, 0x600000, 0x1A00000, 0x3000000),
    (25, 0x1600000, 0x800000, 0x3800000, 0x3200000), (25, 0xC00000, 0xC00000, 0x3400000, 0x6000000),
    (26, 0x2C00000, 0x1000000, 0x7000000, 0x6400000), (26, 0x1800000, 0x1800000, 0x6800000, 0xC000000),
];

/// 参考实现支持的编码：单声道 / 立体声编码最多 2 声道，多声道编码最多 6 声道；8 / 16 / 24 位整数
pub(super) fn supported(info: &StreamInfo) -> bool {
    let codec_ok = match info.codec {
        CODEC_MONO_STEREO => info.channels <= 2,
        CODEC_MULTICHANNEL => info.channels <= 6,
        _ => false,
    };
    codec_ok && info.data_type == 0 && matches!(info.bps, 8 | 16 | 24)
}

/// 1 位标志，为 1 时再读 4 位 + 1
fn esc4(r: &mut Bits) -> u32 {
    if r.bit() { r.bits(4) + 1 } else { 0 }
}

/// 限制在 13 位有符号数的范围
fn clip13(v: i32) -> i32 {
    v.clamp(-(1 << 13), (1 << 13) - 1)
}

/// 16 位整数的点积（32 位回绕）
fn dot(a: &[i16], b: &[i16]) -> i32 {
    a.iter().zip(b).fold(0i32, |s, (&x, &y)| s.wrapping_add(x as i32 * y as i32))
}

/// 两个不同声道的可变引用
fn two_mut(ch: &mut [Vec<i32>], a: usize, b: usize) -> (&mut [i32], &mut [i32]) {
    if a < b {
        let (x, y) = ch.split_at_mut(b);
        (&mut x[a], &mut y[0])
    } else {
        let (x, y) = ch.split_at_mut(a);
        (&mut y[0], &mut x[b])
    }
}

/// 积分（1 ~ 3 阶，32 位回绕）：用于整个声道，也用于子帧开头的预测样本
fn lpc(c: &mut [i32], mode: u32) {
    let n = c.len();
    if n < 2 {
        return;
    }
    match mode {
        1 => {
            let mut a1 = c[0];
            let mut k = 1;
            for _ in 0..(n - 1) >> 1 {
                c[k] = c[k].wrapping_add(a1);
                c[k + 1] = c[k + 1].wrapping_add(c[k]);
                a1 = c[k + 1];
                k += 2;
            }
            if (n - 1) & 1 != 0 {
                c[k] = c[k].wrapping_add(a1);
            }
        }
        2 => {
            let mut a1 = c[1];
            let mut a2 = a1.wrapping_add(c[0]);
            c[1] = a2;
            if n > 2 {
                let mut k = 2;
                for _ in 0..(n - 2) >> 1 {
                    let a3 = c[k].wrapping_add(a1);
                    let a4 = a3.wrapping_add(a2);
                    c[k] = a4;
                    a1 = c[k + 1].wrapping_add(a3);
                    a2 = a1.wrapping_add(a4);
                    c[k + 1] = a2;
                    k += 2;
                }
                if n & 1 != 0 {
                    c[k] = c[k].wrapping_add(a1.wrapping_add(a2));
                }
            }
        }
        3 => {
            let a1 = c[1];
            let a2 = a1.wrapping_add(c[0]);
            c[1] = a2;
            if n > 2 {
                let mut a3 = c[2];
                let mut a4 = a3.wrapping_add(a1);
                let mut a5 = a4.wrapping_add(a2);
                c[2] = a5;
                for v in &mut c[3..] {
                    a3 = a3.wrapping_add(*v);
                    a4 = a4.wrapping_add(a3);
                    a5 = a5.wrapping_add(a4);
                    *v = a5;
                }
            }
        }
        _ => {}
    }
}

/// 一段同一编码方式的残差（`mode` 为 0 时全是 0）
fn segment(r: &mut Bits, mode: i32, out: &mut [i32]) -> Option<()> {
    if mode == 0 {
        out.fill(0);
        return Some(());
    }
    let &(init, escape, scale, aescape, bias) = XCODES.get(usize::try_from(mode - 1).ok()?)?;
    for v in out {
        let mut x = r.bits(init);
        if x >= escape && r.bit() {
            x |= 1 << init;
            if x >= aescape {
                let s = r.zeros(9);
                if s == 9 {
                    let mut n = r.bits(3);
                    if n > 0 {
                        if n == 7 {
                            n += r.bits(5);
                            if n > 29 {
                                return None;
                            }
                        }
                        x = x.wrapping_add(scale.wrapping_mul(r.bits(n) + 1));
                    }
                    x = x.wrapping_add(bias);
                } else {
                    x = x.wrapping_add(scale.wrapping_mul(s)).wrapping_sub(escape);
                }
            } else {
                x = x.wrapping_sub(escape);
            }
        }
        *v = ((x >> 1) ^ (x & 1).wrapping_neg()) as i32;
    }
    Some(())
}

/// 一段残差：整段一种编码方式，或者按 `uval` 分成若干小段、每段的编码方式相对上一段变化
fn residues(r: &mut Bits, uval: usize, nb: usize, modes: &mut [i8; 128], out: &mut [i32]) -> Option<()> {
    let len = out.len();
    if len > nb {
        return None;
    }
    if !r.bit() {
        let mode = r.bits(6) as i32;
        return segment(r, mode, out);
    }
    let mut wlen = len / uval;
    let mut rval = len - wlen * uval;
    if rval < uval / 2 {
        rval += uval;
    } else {
        wlen += 1;
    }
    if wlen <= 1 || wlen > 128 {
        return None;
    }
    let mut mode = r.bits(6) as i32;
    modes[0] = mode as i8;
    for m in &mut modes[1..wlen] {
        match r.zeros(6) {
            6 => mode = r.bits(6) as i32,
            c @ 3..=5 => {
                // 有符号位：sign ? 1 - c : c - 1
                let sign = r.bits(1) as i32;
                mode += (-sign ^ (c as i32 - 1)) + sign;
            }
            2 => mode += 1,
            1 => mode -= 1,
            _ => {}
        }
        *m = mode as i8;
    }
    let (mut i, mut at) = (0, 0);
    while i < wlen {
        let mode = modes[i] as i32;
        let mut n = 0;
        loop {
            n += if i >= wlen - 1 { rval } else { uval };
            i += 1;
            if i == wlen || modes[i] as i32 != mode {
                break;
            }
        }
        segment(r, mode, out.get_mut(at..at + n)?)?;
        at += n;
    }
    Some(())
}

/// 一帧的解码状态（帧与帧之间互不依赖，只是重复使用这些缓冲）
pub(super) struct FrameDecoder {
    /// 各声道解出的样本
    ch: Vec<Vec<i32>>,
    nb: usize,
    uval: usize,
    scale: i32,
    lpc_mode: [u32; 16],
    shift: [u32; 16],
    modes: [i8; 128],
    filter: [i16; 256],
    res: Vec<i16>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        FrameDecoder { ch: Vec::new(), nb: 0, uval: 1, scale: 1, lpc_mode: [0; 16], shift: [0; 16], modes: [0; 128], filter: [0; 256], res: Vec::new() }
    }

    /// 解一帧：`b` 从帧头开始（可以比这一帧长），帧头 `hsize` 字节，这一帧 `nb` 个采样点。
    /// 返回这一帧的字节数；数据不对（包括 CRC）时返回 None
    pub fn decode(&mut self, b: &[u8], hsize: usize, info: &StreamInfo, nb: usize) -> Option<usize> {
        let (chs, bps) = (info.channels, info.bps);
        if !supported(info) || nb == 0 {
            return None;
        }
        let shift = match info.rate {
            ..11025 => 3,
            11025..22050 => 2,
            22050..44100 => 1,
            _ => 0,
        };
        let a = ((info.rate as usize + 511) >> 9).next_multiple_of(4);
        self.uval = a << shift;
        self.scale = (a << 1) as i32;
        self.nb = nb;
        self.ch.resize_with(chs, Vec::new);
        for c in &mut self.ch {
            c.clear();
            c.resize(nb, 0);
        }
        self.lpc_mode = [0; 16];
        self.shift = [0; 16];
        let body = b.get(hsize..)?;
        let mut r = Bits::new(body);
        if nb < 16 {
            // 很短的帧：不压缩
            for c in &mut self.ch {
                for v in c.iter_mut() {
                    *v = r.sbits(bps);
                }
            }
        } else {
            if info.codec == CODEC_MONO_STEREO {
                for c in 0..chs {
                    self.channel(&mut r, c, bps)?;
                }
                if chs == 2 {
                    if r.bit() {
                        r.bits(6);
                    }
                    let dmode = r.bits(3) as u8;
                    self.decorrelate(&mut r, 0, 1, dmode)?;
                }
            } else {
                self.multichannel(&mut r, chs, bps)?;
            }
            for (c, d) in self.ch.iter_mut().enumerate() {
                if self.lpc_mode[c] != 0 {
                    lpc(d, self.lpc_mode[c]);
                }
                let s = self.shift[c];
                if s > 0 {
                    for v in d.iter_mut() {
                        *v = v.wrapping_shl(s);
                    }
                }
            }
        }
        r.align();
        r.bits(24);
        if r.overrun {
            return None;
        }
        let end = hsize + r.position() / 8;
        check_crc(b.get(hsize..end)?).then_some(end)
    }

    /// 一个声道：无用低位数、第一个样本、整体积分的阶数、子帧划分，然后各子帧
    fn channel(&mut self, r: &mut Bits, c: usize, bps: u32) -> Option<()> {
        let shift = esc4(r);
        if shift >= bps {
            return None;
        }
        self.shift[c] = shift;
        self.ch[c][0] = r.sbits(bps - shift);
        self.lpc_mode[c] = r.bits(2);
        let subs = r.bits(3) as usize + 1;
        let mut lens = [0usize; 8];
        let mut left = self.nb as i32 - 1;
        if subs > 1 {
            if r.left() < (subs as isize - 1) * 6 {
                return None;
            }
            let mut prev = 0;
            for l in &mut lens[..subs - 1] {
                let v = r.bits(6) as i32;
                // 参考实现存成 16 位整数
                let n = ((v - prev) * self.scale) as i16;
                if n <= 0 {
                    return None;
                }
                *l = n as usize;
                left -= n as i32;
                prev = v;
            }
            if left <= 0 {
                return None;
            }
        }
        lens[subs - 1] = left as usize;
        let (mut at, mut prev) = (1, 0);
        for &n in &lens[..subs] {
            self.subframe(r, c, at, n, prev)?;
            at += n;
            prev = n;
        }
        Some(())
    }

    /// 一个子帧（声道 `c` 的 `at..at + size`）：没有预测时只有残差；否则是预测样本（新编码的，或者
    /// 沿用上一子帧末尾的）、格型系数，再按转成直接型的滤波器从残差恢复
    fn subframe(&mut self, r: &mut Bits, c: usize, at: usize, size: usize, prev: usize) -> Option<()> {
        let (uval, nb) = (self.uval, self.nb);
        let d = &mut self.ch[c];
        if !r.bit() {
            return residues(r, uval, nb, &mut self.modes, d.get_mut(at..at + size)?);
        }
        let order = PREDICTOR_SIZES[r.bits(4) as usize];
        // 预测样本的开头、子帧（含预测样本）的长度
        let (start, size) = if prev > 0 && r.bit() {
            if order > prev {
                return None;
            }
            (at - order, size + order)
        } else {
            if order > size {
                return None;
            }
            let mode = r.bits(2);
            if mode > 2 {
                return None;
            }
            let w = d.get_mut(at..at + order)?;
            residues(r, uval, nb, &mut self.modes, w)?;
            if mode != 0 {
                lpc(w, mode);
            }
            (at, size)
        };
        let ds = esc4(r);
        let bits = r.bits(1) + 6;
        let mut quant = 10u32;
        if r.bit() {
            quant -= r.bits(3) + 1;
            if quant < 3 {
                return None;
            }
        }
        if r.left() < 2 * 10 + 2 * bits as isize {
            return None;
        }
        let mut pred = [0i32; 256];
        pred[0] = r.sbits(10);
        pred[1] = r.sbits(10);
        pred[2] = r.sbits(bits) << (10 - bits);
        pred[3] = r.sbits(bits) << (10 - bits);
        if order > 4 {
            let tmp = bits - r.bits(1);
            let mut x = 0;
            for (i, p) in pred[4..order].iter_mut().enumerate() {
                if i & 3 == 0 {
                    x = tmp - r.bits(2);
                }
                *p = r.sbits(x) << (10 - bits);
            }
        }
        // 格型系数 → 直接型
        let mut tf = [0i32; 256];
        tf[0] = pred[0] * 64;
        for i in 1..order {
            let p = pred[i];
            let (mut a, mut b) = (0, i - 1);
            for _ in 0..i.div_ceil(2) {
                let x = tf[a].wrapping_add(p.wrapping_mul(tf[b]).wrapping_add(256) >> 9);
                tf[b] = tf[b].wrapping_add(p.wrapping_mul(tf[a]).wrapping_add(256) >> 9);
                tf[a] = x;
                a += 1;
                b = b.wrapping_sub(1);
            }
            tf[i] = p * 64;
        }
        let qs = 15 - quant;
        let (x, y) = (1i32 << (32 - qs), 1i32 << (qs - 1));
        for i in 0..order / 2 {
            let j = order - 1 - i;
            self.filter[j] = x.wrapping_sub(tf[i].wrapping_add(y) >> qs) as i16;
            self.filter[i] = x.wrapping_sub(tf[j].wrapping_add(y) >> qs) as i16;
        }
        let d = d.get_mut(start..start + size)?;
        residues(r, uval, nb, &mut self.modes, &mut d[order..])?;
        let res = &mut self.res;
        res.clear();
        res.extend(d[..order].iter().map(|&v| (v >> ds) as i16));
        let (filt, round) = (&self.filter[..order], 1i32 << (quant - 1));
        for k in order..size {
            let v = round.wrapping_add(dot(&res[k - order..k], filt));
            let v = (clip13(v >> quant) << ds).wrapping_sub(d[k]);
            d[k] = v;
            res.push((v >> ds) as i16);
        }
        Some(())
    }

    /// 两个声道之间的去相关。1 ~ 5 种不动第 0 个样本；6、7 种是 8 / 16 阶的滤波器，从第 1 个样本开始
    fn decorrelate(&mut self, r: &mut Bits, c1: usize, c2: usize, dmode: u8) -> Option<()> {
        if c1 == c2 {
            return None;
        }
        let (p1, p2) = two_mut(&mut self.ch, c1, c2);
        match dmode {
            0 => {}
            // 左 / 侧
            1 => p2[1..].iter_mut().zip(&p1[1..]).for_each(|(b, &a)| *b = b.wrapping_add(a)),
            // 侧 / 右
            2 => p1[1..].iter_mut().zip(&p2[1..]).for_each(|(a, &b)| *a = b.wrapping_sub(*a)),
            // 侧 / 中
            3 => {
                for (a, b) in p1[1..].iter_mut().zip(&mut p2[1..]) {
                    let v = a.wrapping_sub(*b >> 1);
                    *a = v;
                    *b = v.wrapping_add(*b);
                }
            }
            // 带比例系数的侧 / 左、侧 / 右
            4 | 5 => {
                let (a, b) = if dmode == 4 { (p2, p1) } else { (p1, p2) };
                let ds = esc4(r);
                let f = r.sbits(10);
                for (a, &b) in a[1..].iter_mut().zip(&b[1..]) {
                    let v = ((f as u32).wrapping_mul((b >> ds) as u32).wrapping_add(128) as i32 >> 8) << ds;
                    *a = v.wrapping_sub(*a);
                }
            }
            6 | 7 => {
                let (a, b) = if dmode == 6 { (&mut p2[1..], &p1[1..]) } else { (&mut p1[1..], &p2[1..]) };
                let len = a.len();
                if len < 256 {
                    return None;
                }
                let ds = esc4(r);
                let order = 8 << r.bits(1);
                let (dval1, dval2) = (r.bit(), r.bit());
                let mut cs = 0;
                for i in 0..order {
                    if i & 3 == 0 {
                        cs = 14 - r.bits(3);
                    }
                    self.filter[i] = r.sbits(cs) as i16;
                }
                let (half, len2) = (order / 2, len - (order - 1));
                if dval1 {
                    a[..half].iter_mut().zip(b).for_each(|(x, &y)| *x = x.wrapping_add(y));
                }
                if dval2 {
                    a[len2 + half..].iter_mut().zip(&b[len2 + half..]).for_each(|(x, &y)| *x = x.wrapping_add(y));
                }
                let res = &mut self.res;
                res.clear();
                res.extend(b.iter().map(|&v| (v >> ds) as i16));
                let filt = &self.filter[..order];
                for (g, out) in a[half..half + len2].iter_mut().enumerate() {
                    let v = (1i32 << 9).wrapping_add(dot(&res[g..g + order], filt));
                    *out = ((clip13(v >> 10) as u32) << ds).wrapping_sub(*out as u32) as i32;
                }
            }
            _ => {}
        }
        Some(())
    }

    /// 多声道：可选的去相关参数（每个声道与之前解出的一个声道配对），然后逐个声道解码
    fn multichannel(&mut self, r: &mut Bits, chs: usize, bps: u32) -> Option<()> {
        // (有去相关, 方式序号, 声道, 配对的声道)
        let mut mcd = [(false, 0usize, 0usize, 0usize); 16];
        let n = if r.bit() {
            let n = r.bits(4) as usize + 1;
            if n > chs {
                return None;
            }
            let mut mask = 0u32;
            for p in &mut mcd[..n] {
                let c1 = r.bits(4) as usize;
                if c1 >= chs || mask & (1 << c1) != 0 {
                    return None;
                }
                p.0 = r.bit();
                if p.0 {
                    p.1 = r.bits(2) as usize;
                    p.3 = r.bits(4) as usize;
                    if p.3 >= chs {
                        return None;
                    }
                    if p.1 == 1 {
                        if c1 == p.3 || mask & (1 << p.3) != 0 {
                            return None;
                        }
                        mask |= 1 << p.3;
                    } else if mask & (1 << p.3) == 0 {
                        return None;
                    }
                }
                p.2 = c1;
                mask |= 1 << c1;
            }
            n
        } else {
            for (i, p) in mcd[..chs].iter_mut().enumerate() {
                *p = (false, 0, i, 0);
            }
            chs
        };
        for &(present, index, c1, c2) in &mcd[..n] {
            if present && index == 1 {
                self.channel(r, c2, bps)?;
            }
            self.channel(r, c1, bps)?;
            if present {
                self.decorrelate(r, c2, c1, MC_DMODES[index])?;
            }
        }
        Some(())
    }

    /// 解出的样本交错排列到 `out`，按位数截断（与参考实现输出 8 / 16 / 24 位时相同）
    pub fn interleave(&self, bps: u32, out: &mut Vec<i32>) {
        let s = 32 - bps;
        for i in 0..self.nb {
            out.extend(self.ch.iter().map(|c| (c[i] << s) >> s));
        }
    }
}
