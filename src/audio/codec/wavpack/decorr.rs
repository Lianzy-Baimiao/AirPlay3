//! WavPack 的解相关（其实是「相关」：把预测加回残差），改写自参考库 unpack.c（BSD 许可，见 mod.rs）。
//! 每一遍是一个自适应预测器：term 1 ~ 8 用 term 个样本以前的值，17 / 18 用前两个样本外推，
//! -1 ~ -3 用另一个声道（只有立体声）；权重按符号 LMS 每个样本调整。

#[derive(Clone, Copy, Default)]
pub(super) struct Pass {
    pub term: i32,
    pub delta: i32,
    pub weight_a: i32,
    pub weight_b: i32,
    pub a: [i32; 8],
    pub b: [i32; 8],
}

/// 权重（±1024 = ±1）乘样本，四舍五入；与参考库一样，16 位以内的样本用 32 位算
fn apply(w: i32, s: i32) -> i32 {
    if s == s as i16 as i32 {
        w.wrapping_mul(s).wrapping_add(512) >> 10
    } else {
        let lo = (s & 0xffff).wrapping_mul(w) >> 9;
        let hi = ((s & !0xffff) >> 9).wrapping_mul(w);
        lo.wrapping_add(hi).wrapping_add(1) >> 1
    }
}

/// 预测和残差同号时权重加 delta，异号时减
fn update(w: &mut i32, delta: i32, src: i32, res: i32) {
    if src != 0 && res != 0 {
        let s = (src ^ res) >> 31;
        *w = (delta ^ s).wrapping_add(w.wrapping_sub(s));
    }
}

/// 同上，但权重限制在 ±1024 以内（声道间预测用）
fn update_clip(w: &mut i32, delta: i32, src: i32, res: i32) {
    if src != 0 && res != 0 {
        let s = (src ^ res) >> 31;
        let v = ((*w ^ s).wrapping_add(delta.wrapping_sub(s))).min(1024);
        *w = (v ^ s).wrapping_sub(s);
    }
}

fn extrapolate17(h: &[i32; 8]) -> i32 {
    h[0].wrapping_mul(2).wrapping_sub(h[1])
}

/// 单声道的一遍（每块开头状态都从元数据重新读，所以不必像参考库那样在结尾把历史样本转回原位）
pub(super) fn mono(p: &mut Pass, buf: &mut [i32]) {
    let (delta, mut w) = (p.delta, p.weight_a);
    if p.term > 8 {
        for x in buf.iter_mut() {
            let s = if p.term == 17 { extrapolate17(&p.a) } else { p.a[0].wrapping_mul(3).wrapping_sub(p.a[1]) >> 1 };
            p.a[1] = p.a[0];
            p.a[0] = apply(w, s).wrapping_add(*x);
            update(&mut w, delta, s, *x);
            *x = p.a[0];
        }
    } else {
        let (mut m, mut k) = (0, (p.term & 7) as usize);
        for x in buf.iter_mut() {
            let s = p.a[m];
            p.a[k] = apply(w, s).wrapping_add(*x);
            update(&mut w, delta, s, *x);
            *x = p.a[k];
            m = (m + 1) & 7;
            k = (k + 1) & 7;
        }
    }
    p.weight_a = w;
}

/// 立体声的一遍（`buf` 左右交错）
pub(super) fn stereo(p: &mut Pass, buf: &mut [i32]) {
    let d = p.delta;
    match p.term {
        17 | 18 => {
            let t = p.term;
            let predict = move |h: &[i32; 8]| {
                if t == 17 { extrapolate17(h) } else { h[0].wrapping_add(h[0].wrapping_sub(h[1]) >> 1) }
            };
            for x in buf.chunks_exact_mut(2) {
                let s = predict(&p.a);
                p.a[1] = p.a[0];
                p.a[0] = apply(p.weight_a, s).wrapping_add(x[0]);
                update(&mut p.weight_a, d, s, x[0]);
                x[0] = p.a[0];
                let s = predict(&p.b);
                p.b[1] = p.b[0];
                p.b[0] = apply(p.weight_b, s).wrapping_add(x[1]);
                update(&mut p.weight_b, d, s, x[1]);
                x[1] = p.b[0];
            }
        }
        1..=8 => {
            let (mut m, mut k) = (0, (p.term & 7) as usize);
            for x in buf.chunks_exact_mut(2) {
                let s = p.a[m];
                p.a[k] = apply(p.weight_a, s).wrapping_add(x[0]);
                update(&mut p.weight_a, d, s, x[0]);
                x[0] = p.a[k];
                let s = p.b[m];
                p.b[k] = apply(p.weight_b, s).wrapping_add(x[1]);
                update(&mut p.weight_b, d, s, x[1]);
                x[1] = p.b[k];
                m = (m + 1) & 7;
                k = (k + 1) & 7;
            }
        }
        -1 => {
            for x in buf.chunks_exact_mut(2) {
                let s = x[0].wrapping_add(apply(p.weight_a, p.a[0]));
                update_clip(&mut p.weight_a, d, p.a[0], x[0]);
                x[0] = s;
                p.a[0] = x[1].wrapping_add(apply(p.weight_b, s));
                update_clip(&mut p.weight_b, d, s, x[1]);
                x[1] = p.a[0];
            }
        }
        -2 => {
            for x in buf.chunks_exact_mut(2) {
                let s = x[1].wrapping_add(apply(p.weight_b, p.b[0]));
                update_clip(&mut p.weight_b, d, p.b[0], x[1]);
                x[1] = s;
                p.b[0] = x[0].wrapping_add(apply(p.weight_a, s));
                update_clip(&mut p.weight_a, d, s, x[0]);
                x[0] = p.b[0];
            }
        }
        _ => {
            // -3：两个声道互相用对方上一个样本
            for x in buf.chunks_exact_mut(2) {
                let sa = x[0].wrapping_add(apply(p.weight_a, p.a[0]));
                update_clip(&mut p.weight_a, d, p.a[0], x[0]);
                let sb = x[1].wrapping_add(apply(p.weight_b, p.b[0]));
                update_clip(&mut p.weight_b, d, p.b[0], x[1]);
                p.b[0] = sa;
                p.a[0] = sb;
                x[0] = sa;
                x[1] = sb;
            }
        }
    }
}
