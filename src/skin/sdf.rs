//! 自己画图标用的小工具：用有符号距离（像素，内部为负）描述形状，按距离抗锯齿。
//! 图标是白色图形（绘制时再按颜色着色），可选外面一圈淡淡的暗边（玻璃上也看得清）。

use super::Sprite;

#[derive(Clone)]
pub enum S {
    /// 矩形（左、上、右、下），圆角半径
    Rect(f32, f32, f32, f32, f32),
    /// 圆（圆心、半径）
    Circle(f32, f32, f32),
    /// 圆环（圆心、中线半径、线宽）
    Ring(f32, f32, f32, f32),
    /// 线段（两端、线宽；圆头）
    Line(f32, f32, f32, f32, f32),
    /// 圆弧（圆心、中线半径、起止角（度；0 = 右、90 = 下，沿角度增大的方向）、线宽；圆头）
    Arc(f32, f32, f32, f32, f32, f32),
    /// 多边形（顶点），外扩半径（圆角）
    Poly(Vec<(f32, f32)>, f32),
}

fn seg(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (pax, pay, bax, bay) = (px - ax, py - ay, bx - ax, by - ay);
    let h = ((pax * bax + pay * bay) / (bax * bax + bay * bay).max(1e-6)).clamp(0.0, 1.0);
    (pax - bax * h).hypot(pay - bay * h)
}

fn poly(px: f32, py: f32, v: &[(f32, f32)]) -> f32 {
    let mut d = f32::MAX;
    let mut sign = 1.0;
    let n = v.len();
    for i in 0..n {
        let (a, b) = (v[i], v[(i + n - 1) % n]);
        d = d.min(seg(px, py, a.0, a.1, b.0, b.1));
        let (ex, ey, wx, wy) = (b.0 - a.0, b.1 - a.1, px - a.0, py - a.1);
        let c = [py >= a.1, py < b.1, ex * wy > ey * wx];
        if c.iter().all(|&k| k) || c.iter().all(|&k| !k) {
            sign = -sign;
        }
    }
    sign * d
}

impl S {
    pub fn d(&self, x: f32, y: f32) -> f32 {
        match *self {
            S::Rect(x0, y0, x1, y1, r) => {
                let (hw, hh) = ((x1 - x0) / 2.0 - r, (y1 - y0) / 2.0 - r);
                let (qx, qy) = ((x - (x0 + x1) / 2.0).abs() - hw, (y - (y0 + y1) / 2.0).abs() - hh);
                qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
            }
            S::Circle(cx, cy, r) => (x - cx).hypot(y - cy) - r,
            S::Ring(cx, cy, r, w) => ((x - cx).hypot(y - cy) - r).abs() - w / 2.0,
            S::Line(ax, ay, bx, by, w) => seg(x, y, ax, ay, bx, by) - w / 2.0,
            S::Arc(cx, cy, r, a0, a1, w) => {
                let ang = (y - cy).atan2(x - cx).to_degrees().rem_euclid(360.0);
                let on = if a0 <= a1 { ang >= a0 && ang <= a1 } else { ang >= a0 || ang <= a1 };
                if on {
                    ((x - cx).hypot(y - cy) - r).abs() - w / 2.0
                } else {
                    let end = |a: f32| (cx + r * a.to_radians().cos(), cy + r * a.to_radians().sin());
                    let ((ex, ey), (fx, fy)) = (end(a0), end(a1));
                    (x - ex).hypot(y - ey).min((x - fx).hypot(y - fy)) - w / 2.0
                }
            }
            S::Poly(ref v, r) => poly(x, y, v) - r,
        }
    }
}

/// 若干形状的并集，减去另一些形状
pub struct Shape {
    pub add: Vec<S>,
    pub sub: Vec<S>,
}

impl Shape {
    pub fn of(add: Vec<S>) -> Shape {
        Shape { add, sub: Vec::new() }
    }

    pub fn minus(mut self, sub: Vec<S>) -> Shape {
        self.sub.extend(sub);
        self
    }

    pub fn d(&self, x: f32, y: f32) -> f32 {
        let a = self.add.iter().map(|s| s.d(x, y)).fold(f32::MAX, f32::min);
        let b = self.sub.iter().map(|s| s.d(x, y)).fold(f32::MAX, f32::min);
        a.max(-b)
    }
}

/// 把形状画进精灵图 (ox, oy) 起的 w×h 区域：白色图形；`rim` 为 true 时外面一圈约 1 像素的淡淡暗边
pub fn draw(sp: &mut Sprite, ox: u32, oy: u32, w: u32, h: u32, shape: &Shape, rim: bool) {
    for y in 0..h {
        for x in 0..w {
            let d = shape.d(x as f32 + 0.5, y as f32 + 0.5);
            let cov = (0.5 - d).clamp(0.0, 1.0);
            let shadow = if rim { (1.4 - d).clamp(0.0, 1.0) * 0.55 } else { 0.0 };
            let a = cov + shadow * (1.0 - cov);
            if a <= 0.0 {
                continue;
            }
            let v = (cov / a * 255.0).round() as u8;
            sp.px[((oy + y) * sp.w + ox + x) as usize] = [v, v, v, (a * 255.0).round() as u8];
        }
    }
}
