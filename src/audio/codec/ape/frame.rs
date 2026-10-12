//! 解一帧（改写自参考库 APEDecompressCore / Old/UnMACOld / Old/APEDecompressCoreOld，BSD 许可，见 mod.rs）。
//! 3.93 起各声道的残差逐个采样点交错，一帧一段区间编码；以前是每个声道一整串，再整串反预测。

use super::bits::Bits;
use super::header::{FLAG_CRC, Info};
use super::predictor::Predictor;
use super::range::{self, State};
use super::{anti, rice, unprepare};

const SILENCE_L: u32 = 1;
const SILENCE_R: u32 = 2;
const PSEUDO_STEREO: u32 = 4;

pub(super) enum Outcome {
    Ok,
    /// 数据坏了（CRC 不对、越界）
    Bad,
    /// 数据不够（截断的文件）：解出了前面这么多个采样点
    Partial(usize),
}

/// 帧头：CRC 和「特殊帧」标志（静音、假立体声）
fn header(b: &mut Bits, version: u16) -> (u32, u32) {
    let mut crc = b.bits(32);
    let mut special = 0;
    if version > 3820 {
        if crc & 0x8000_0000 != 0 {
            special = b.bits(32);
        }
        crc &= 0x7FFF_FFFF;
    }
    (crc, special)
}

/// CRC 比较（3.82 以后存的是右移一位的值）
fn crc_ok(v: &[i32], bits: u16, version: u16, stored: u32) -> bool {
    let c = unprepare::crc(v, bits);
    (if version > 3820 { c >> 1 } else { c }) == stored
}

/// 3.93 起
pub(super) fn modern(info: &Info, preds: &mut [Predictor], b: &mut Bits, n: usize, complete: bool, out: &mut Vec<i32>) -> Outcome {
    let (version, bits, ch) = (info.version, info.bits, info.channels as usize);
    let (stored, special) = header(b, version);
    preds.iter_mut().for_each(Predictor::flush);
    let mut st = [State::new(); 32];
    b.range_start();
    let new = version >= 3990;
    let val = |b: &mut Bits, s: &mut State| if new { range::value(b, s) } else { range::value_old(b, s, version) };
    let mut last_x = 0i32;
    let mut v = [0i32; 32];
    out.clear();
    for i in 0..n {
        if ch == 2 && special & SILENCE_L != 0 && special & SILENCE_R != 0 || ch == 1 && special & SILENCE_L != 0 {
            v = [0; 32];
        } else if ch == 2 && special & PSEUDO_STEREO != 0 {
            let x = val(b, &mut st[0]);
            (v[0], v[1]) = (preds[0].decompress(x, 0), 0);
        } else if ch == 2 && version >= 3950 {
            let ny = val(b, &mut st[1]);
            let nx = val(b, &mut st[0]);
            let y = preds[1].decompress(ny, last_x as i64);
            let x = preds[0].decompress(nx, y as i64);
            last_x = x;
            (v[0], v[1]) = (x, y);
        } else {
            for c in 0..ch {
                let r = val(b, &mut st[c]);
                v[c] = preds[c].decompress(r, 0);
            }
        }
        if b.over {
            return if complete { Outcome::Bad } else { Outcome::Partial(i) };
        }
        // 24 位的中 / 侧值不可能超过 ±2^24：超过说明算法不对（「过渡期」文件）或数据坏了
        if bits == 24 && v[..ch].iter().any(|x| x.unsigned_abs() > 1 << 24) {
            return Outcome::Bad;
        }
        if !unprepare::modern(&v[..ch], bits, out) {
            return Outcome::Bad;
        }
    }
    b.range_finish(version);
    if crc_ok(out, bits, version, stored) { Outcome::Ok } else { Outcome::Bad }
}

/// 3.93 以前：一个声道一整串残差；返回数据读完以前解出的个数（截断的文件只有这些可信，反预测是因果的）
fn channel(info: &Info, b: &mut Bits, tmp: &mut [i32], dst: &mut [i32]) -> usize {
    let v = info.version;
    let valid = if v < 3860 {
        rice::array_old(b, tmp)
    } else if v <= 3890 {
        rice::array_rice(b, tmp, v)
    } else {
        let mut s = State::new();
        b.range_start();
        let n = tmp.len();
        let mut valid = n;
        for (i, x) in tmp.iter_mut().enumerate() {
            *x = range::value_old(b, &mut s, v) as i32;
            if b.over && valid == n {
                valid = i;
            }
        }
        b.range_finish(v);
        valid
    };
    anti::anti_predict(info.level, v, tmp, dst);
    valid
}

/// 3.93 以前
pub(super) fn old(info: &Info, b: &mut Bits, n: usize, complete: bool, work: &mut [Vec<i32>; 3], out: &mut Vec<i32>) -> Outcome {
    let (version, bits, stereo) = (info.version, info.bits, info.channels == 2);
    let uses_crc = info.flags & FLAG_CRC != 0;
    let (stored, mut special) = if uses_crc { header(b, version) } else { (rice_crc(b), 0) };
    if !uses_crc && stored == 0 {
        special = SILENCE_L | SILENCE_R;
    }
    let [tmp, xs, ys] = work;
    for w in [&mut *tmp, &mut *xs, &mut *ys] {
        w.clear();
        w.resize(n, 0);
    }
    let mut valid = n;
    if special & SILENCE_L != 0 && (!stereo || special & SILENCE_R != 0) {
        // 整帧静音
    } else {
        valid = channel(info, b, tmp, xs);
        if stereo && special & PSEUDO_STEREO == 0 {
            valid = valid.min(channel(info, b, tmp, ys));
        }
    }
    out.clear();
    let ys = stereo.then_some(&ys[..]);
    unprepare::old(xs, ys, bits, version, out);
    if b.over && !complete {
        out.truncate(valid * info.channels as usize);
        return Outcome::Partial(valid);
    }
    let ok = if uses_crc { crc_ok(out, bits, version, stored) } else { unprepare::old_checksum(xs, ys) == stored };
    if ok && !b.over { Outcome::Ok } else { Outcome::Bad }
}

/// 没有 CRC 标志的老文件：帧头是 k = 30 的 Rice 码
fn rice_crc(b: &mut Bits) -> u32 {
    let q = b.zeros();
    q.wrapping_shl(30) | b.bits(30)
}
