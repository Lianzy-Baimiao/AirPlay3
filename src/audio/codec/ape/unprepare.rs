//! 预测器输出 → PCM（改写自参考库 Prepare.cpp 的 Unprepare / UnprepareOld，BSD 许可，见 mod.rs）。
//! 结果按容器位数存成有符号整数（与参考库写出的字节一一对应），帧的 CRC 按参考库的字节格式算。

/// 中 / 侧还原：第一个 = X − Y/2，第二个 = 它 + Y
fn pair(x: i32, y: i32) -> (i32, i32) {
    let r = x.wrapping_sub(y / 2);
    (r, r.wrapping_add(y))
}

/// 24 位（单声道 / 立体声）：负数按参考库的写法转成 3 字节
fn s24(v: i32) -> i32 {
    let t = if v < 0 { (v.wrapping_add(0x80_0000) as u32) | 0x80_0000 } else { v as u32 };
    ((t << 8) as i32) >> 8
}

/// 24 位（多声道）：直接取低 3 字节
fn t24(v: i32) -> i32 {
    (((v as u32) << 8) as i32) >> 8
}

fn set_pair(e: &mut [(i32, bool); 32], v: &[i32], i: usize) {
    let (a, b) = pair(v[i], v[i + 1]);
    (e[i], e[i + 1]) = ((a, true), (b, true));
}

/// 3.93 起，一个采样点（`v` 是各声道的值）。16 位的声道对超出范围时返回 false（数据坏了）
pub(super) fn modern(v: &[i32], bits: u16, out: &mut Vec<i32>) -> bool {
    let ch = v.len();
    let mut o = [0i32; 32];
    match bits {
        32 if ch == 2 => (o[0], o[1]) = pair(v[0], v[1]),
        32 => o[..ch].copy_from_slice(v),
        8 if ch == 2 => {
            let (r, l) = pair(v[0], v[1]);
            (o[0], o[1]) = (r as i8 as i32, l as i8 as i32);
        }
        8 => o.iter_mut().zip(v).for_each(|(d, &x)| *d = x as i8 as i32),
        _ => {
            // 多声道按位置成对：前左右、（中置、低音）、环绕、后置；其余单独。7 声道时第 7 个声道参考库没有写
            let mut e = [(0i32, false); 32];
            match ch {
                1 => e[0] = (v[0], false),
                2 | 4 => (0..ch).step_by(2).for_each(|i| set_pair(&mut e, v, i)),
                c if c >= 6 => {
                    set_pair(&mut e, v, 0);
                    (e[2], e[3]) = ((v[2], true), (v[3], true));
                    set_pair(&mut e, v, 4);
                    if c >= 8 {
                        set_pair(&mut e, v, 6);
                    }
                    for i in if c == 7 { 7 } else { 8 }..c {
                        e[i] = (v[i], false);
                    }
                }
                _ => (0..ch).for_each(|i| e[i] = (v[i], false)),
            }
            for i in 0..ch {
                let (x, checked) = e[i];
                o[i] = if bits == 24 {
                    if ch <= 2 { s24(x) } else { t24(x) }
                } else {
                    if checked && !(-32768..=32767).contains(&x) {
                        return false;
                    }
                    x as i16 as i32
                };
            }
        }
    }
    out.extend_from_slice(&o[..ch]);
    true
}

/// 3.93 以前（单声道 / 立体声），整帧
pub(super) fn old(xs: &[i32], ys: Option<&[i32]>, bits: u16, version: u16, out: &mut Vec<i32>) {
    // 3.83 以前 8 位没有加 128
    let off = if version > 3830 { 128 } else { 0 };
    match (bits, ys) {
        (8, Some(ys)) => xs.iter().zip(ys).for_each(|(&x, &y)| {
            let r = x.wrapping_sub(y / 2).wrapping_add(off) as u8;
            let l = (r as i32).wrapping_add(y) as u8;
            out.extend([r as i32 - 128, l as i32 - 128]);
        }),
        (24, Some(ys)) => xs.iter().zip(ys).for_each(|(&x, &y)| {
            let (r, l) = pair(x, y);
            out.extend([s24(r), s24(l)]);
        }),
        (_, Some(ys)) => xs.iter().zip(ys).for_each(|(&x, &y)| {
            let (r, l) = pair(x, y);
            out.extend([r as i16 as i32, l as i16 as i32]);
        }),
        (8, None) => out.extend(xs.iter().map(|&x| x.wrapping_add(off) as u8 as i32 - 128)),
        (24, None) => out.extend(xs.iter().map(|&x| s24(x))),
        (_, None) => out.extend(xs.iter().map(|&x| x as i16 as i32)),
    }
}

/// 参考库写出的字节（小端 PCM，8 位无符号）上的 CRC-32
pub(super) fn crc(v: &[i32], bits: u16) -> u32 {
    let mut b = Vec::with_capacity(v.len() * bits as usize / 8);
    match bits {
        8 => b.extend(v.iter().map(|&x| (x + 128) as u8)),
        16 => v.iter().for_each(|&x| b.extend_from_slice(&(x as i16).to_le_bytes())),
        24 => v.iter().for_each(|&x| b.extend_from_slice(&x.to_le_bytes()[..3])),
        _ => v.iter().for_each(|&x| b.extend_from_slice(&x.to_le_bytes())),
    }
    crate::audio::codec::crc32(&b)
}

/// 没有 CRC 的老文件：各声道绝对值之和
pub(super) fn old_checksum(xs: &[i32], ys: Option<&[i32]>) -> u32 {
    let abs = |v: i32| (v as i64).unsigned_abs() as u32;
    match ys {
        Some(ys) => xs.iter().zip(ys).fold(0u32, |s, (&x, &y)| {
            let (r, l) = pair(x, y);
            s.wrapping_add(abs(r)).wrapping_add(abs(l))
        }),
        None => xs.iter().fold(0u32, |s, &x| s.wrapping_add(abs(x))),
    }
}

/// 浮点文件：整数的位按参考库 CFloatTransform 变回 IEEE 单精度
pub(super) fn float(x: i32) -> f32 {
    let i = x as u32;
    let mut o = (i & 0xC3FF_FFFF) | (!(i & 0x3C00_0000) ^ 0xC3FF_FFFF);
    if o & 0x8000_0000 != 0 {
        o = !o | 0x8000_0000;
    }
    f32::from_bits(o)
}
