//! 3.80 ~ 3.92 的反预测（改写自参考库 Old/AntiPredictor*Old.cpp 里 3800To3929 / 3320To3930 的部分，
//! BSD 许可，见 mod.rs）。一帧里一个声道的残差整串处理，没有跨帧状态。
//! 普通 / 高 / 超高三档共用同一条预测链，高和超高多一级前置滤波（只改输入、不看输出，所以可以先整串做完）。

/// 预测链：二阶自适应 + 交叉项 + 一阶固定滤波；前 `first` 个样本只做累加
fn chain(input: &[i32], out: &mut [i32], first: usize, shift_c: u32) {
    let n = input.len();
    out[..first].copy_from_slice(&input[..first]);
    let (mut m2, mut m3, mut m4, mut m5, mut m6) = (64i32, 115i32, 64i32, 740i32, 0i32);
    let ip = |i: usize| input[i];
    let mut p4 = ip(first - 1);
    let mut p3 = ip(first - 1).wrapping_sub(ip(first - 2)).wrapping_shl(1);
    let mut p2 = ip(first - 1).wrapping_add(ip(first - 3).wrapping_sub(ip(first - 2)).wrapping_shl(3));
    let mut ipp2 = ip(first - 2);
    let mut p7 = ip(first - 1).wrapping_mul(2).wrapping_sub(ip(first - 2));
    let mut opp = out[first - 1];
    for q in 1..first {
        out[q] = out[q].wrapping_add(out[q - 1]);
    }
    let sign = |v: i32, s: u32, mask: i32, sub: i32| ((v >> s) & mask) - sub;
    for i in first..n {
        let x = input[i];
        let mut o = x.wrapping_add(p2.wrapping_mul(m2).wrapping_add(p3.wrapping_mul(m3)).wrapping_add(p4.wrapping_mul(m4)) >> 11);
        if x > 0 {
            m2 = m2.wrapping_sub(sign(p2, 30, 2, 1));
            m3 = m3.wrapping_sub(sign(p3, 28, 8, 4));
            m4 = m4.wrapping_sub(sign(p4, 28, 8, 4));
        } else if x < 0 {
            m2 = m2.wrapping_add(sign(p2, 30, 2, 1));
            m3 = m3.wrapping_add(sign(p3, 28, 8, 4));
            m4 = m4.wrapping_add(sign(p4, 28, 8, 4));
        }
        p2 = o.wrapping_add(ipp2.wrapping_sub(p4).wrapping_shl(3));
        p3 = o.wrapping_sub(p4).wrapping_shl(1);
        ipp2 = p4;
        p4 = o;
        o = o.wrapping_add(p7.wrapping_mul(m5).wrapping_sub(opp.wrapping_mul(m6)) >> shift_c);
        if p4 > 0 {
            m5 = m5.wrapping_sub(sign(p7, 29, 4, 2));
            m6 = m6.wrapping_add(sign(opp, 30, 2, 1));
        } else if p4 < 0 {
            m5 = m5.wrapping_add(sign(p7, 29, 4, 2));
            m6 = m6.wrapping_sub(sign(opp, 30, 2, 1));
        }
        p7 = o.wrapping_mul(2).wrapping_sub(opp);
        opp = o;
        out[i] = o.wrapping_add(out[i - 1].wrapping_mul(31) >> 5);
    }
}

/// 高：16 阶符号 LMS（用已经改过的输入）
fn high_pre(x: &mut [i32]) {
    let mut bm = [0i32; 16];
    for i in 16..x.len() {
        let cur = x[i];
        let mut dot = 0i32;
        for (k, b) in bm.iter_mut().enumerate() {
            let p = x[i - 16 + k];
            dot = dot.wrapping_add(p.wrapping_mul(*b));
            let adj = ((p >> 30) & 2) - 1;
            if cur > 0 {
                *b = b.wrapping_add(adj);
            } else if cur < 0 {
                *b = b.wrapping_sub(adj);
            }
        }
        x[i] = cur.wrapping_sub(dot >> 9);
    }
}

/// 超高：（3.83 起）8 阶符号 LMS，再 128 / 256 阶 16 位符号 LMS
fn extra_pre(x: &mut [i32], version: u16) {
    let (taps, shift) = if version < 3830 { (128usize, 11u32) } else { (256, 12) };
    let first = taps;
    let n = x.len();
    let mut short = vec![0i16; n];
    let mut adapt = vec![0i16; n];
    for q in 0..first {
        adapt[q] = (((x[q] >> 30) & 2) - 1) as i16;
        short[q] = x[q] as i16;
    }
    let mut bm = vec![0i16; taps];
    let (mut fm, mut fp) = ([0i32; 9], [0i32; 9]);
    for q in first..n {
        if version >= 3830 {
            fp[0] = x[q];
            let mut dot = 0i32;
            for k in (1..=8).rev() {
                dot = dot.wrapping_add(fp[k].wrapping_mul(fm[k]));
                if fp[0] != 0 {
                    let adj = ((fp[k] >> 30) & 2) - 1;
                    fm[k] = if fp[0] > 0 { fm[k].wrapping_add(adj) } else { fm[k].wrapping_sub(adj) };
                }
                fp[k] = fp[k - 1];
            }
            x[q] = x[q].wrapping_sub(dot >> 9);
        }
        let original = x[q];
        let mut dot = 0i32;
        for k in 0..taps {
            dot = dot.wrapping_add(short[q - first + k] as i32 * bm[k] as i32);
            if original > 0 {
                bm[k] = bm[k].wrapping_add(adapt[q - first + k]);
            } else if original < 0 {
                bm[k] = bm[k].wrapping_sub(adapt[q - first + k]);
            }
        }
        x[q] = x[q].wrapping_sub(dot >> shift);
        short[q] = x[q] as i16;
        adapt[q] = (((x[q] >> 30) & 2) - 1) as i16;
    }
}

/// 快速：二阶自适应 + 一阶累加，原地
fn fast(x: &mut [i32]) {
    if x.len() < 3 {
        return;
    }
    let (mut m, mut ip2, mut ip3, mut op1) = (375i32, x[1], x[0], x[1]);
    for v in &mut x[2..] {
        let p = ip2.wrapping_add(ip2).wrapping_sub(ip3);
        ip3 = ip2;
        ip2 = v.wrapping_add(p.wrapping_mul(m) >> 9);
        m = if (*v ^ p) > 0 { m.wrapping_add(1) } else { m.wrapping_sub(1) };
        *v = ip2.wrapping_add(op1);
        op1 = *v;
    }
}

/// 一个声道一帧：`x` 是残差（会被改动），结果写到 `out`
pub(super) fn anti_predict(level: u16, version: u16, x: &mut [i32], out: &mut [i32]) {
    let n = x.len();
    let (first, min_len, shift_c) = match level {
        1000 => {
            fast(x);
            out.copy_from_slice(x);
            return;
        }
        2000 => (4, 8, 10),
        3000 => (16, 20, 10),
        _ if version < 3830 => (128, 134, 10),
        _ => (256, 262, 11),
    };
    if n < min_len {
        out.copy_from_slice(x);
        return;
    }
    match level {
        3000 => high_pre(x),
        4000 => extra_pre(x, version),
        _ => {}
    }
    chain(x, out, first, shift_c);
}
