//! 原版「配置」窗口的控件画法（照原版截图逐像素量出来的）：
//! 彩色的“药丸”行（两圈暗边、顶上一抹高光、右边一个凹进去的勾选框）、灰色方块勾选框、
//! 细边框的下拉框和输入框、色块、圆点滑块、凹槽分隔线

use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::text::{self, Font, Hint};
use crate::skin::sdf::{S, Shape};

pub const FONT: Font = Font::px(12.0, false);
/// 药丸上的文字
pub const PILL_TEXT: Rgba = Rgba::gray(0x3B);
/// 灰掉的药丸：底色、勾选框按这个比例变暗
const DIM: f32 = 0.28;

/// 药丸的颜色组（原版按功能分组上色）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Grey,
    Khaki,
    Teal,
    Lavender,
    Pink,
}

impl Tone {
    pub fn rgb(self) -> Rgba {
        match self {
            Tone::Grey => Rgba(0xC8, 0xC8, 0xC8, 255),
            Tone::Khaki => Rgba(0xC8, 0xC8, 0xA8, 255),
            Tone::Teal => Rgba(0xA8, 0xC8, 0xC8, 255),
            Tone::Lavender => Rgba(0xB0, 0xB8, 0xC8, 255),
            Tone::Pink => Rgba(0xC8, 0xB0, 0xC8, 255),
        }
    }
}

fn scale(c: Rgba, k: f32) -> Rgba {
    let f = |v: u8| (v as f32 * k).round().min(255.0) as u8;
    Rgba(f(c.0), f(c.1), f(c.2), c.3)
}

fn white(c: Rgba, t: f32) -> Rgba {
    c.mix(Rgba(255, 255, 255, 255), t)
}

/// 一个像素（画布外的忽略）
fn put(c: &mut Canvas, x: i32, y: i32, col: Rgba, cov: f32) {
    if c.clip_rect().contains(x, y) {
        c.blend(x, y, col, cov);
    }
}

/// 1 像素的矩形边框，四个角按 `corner` 的比例混进底色（圆一点）
fn ring(c: &mut Canvas, r: R, col: Rgba, corner: f32) {
    let (x1, y1) = (r.right() - 1, r.bottom() - 1);
    for x in r.x + 1..x1 {
        put(c, x, r.y, col, 1.0);
        put(c, x, y1, col, 1.0);
    }
    for y in r.y + 1..y1 {
        put(c, r.x, y, col, 1.0);
        put(c, x1, y, col, 1.0);
    }
    for (x, y) in [(r.x, r.y), (x1, r.y), (r.x, y1), (x1, y1)] {
        put(c, x, y, col, corner);
    }
}

/// 药丸：r 是最外圈（高 21）。两圈暗边，里面是底色，顶上几行有一点高光
pub fn pill(c: &mut Canvas, r: R, tone: Tone, dim: bool) {
    ring(c, r, Rgba::gray(0x1E), 0.4);
    ring(c, r.shrink(1), Rgba::gray(0x3D), 0.55);
    ring(c, r.shrink(2), Rgba::gray(0x17), 0.75);
    let f = r.shrink(3);
    let base = if dim { scale(tone.rgb(), DIM) } else { tone.rgb() };
    const HL: [f32; 7] = [0.0, 0.127, 0.109, 0.091, 0.091, 0.073, 0.055];
    for (i, y) in (f.y..f.bottom()).enumerate() {
        let t = if dim { 0.0 } else { HL.get(i).copied().unwrap_or(0.0) };
        c.fill(R::new(f.x, y, f.w, 1), white(base, t));
    }
    // 底色与内圈之间的四个角
    let (x1, y1) = (f.right() - 1, f.bottom() - 1);
    for (x, y) in [(f.x, f.y), (x1, f.y), (f.x, y1), (x1, y1)] {
        put(c, x, y, Rgba::gray(0x17), 0.62);
    }
}

/// 灰掉的药丸上文字、图标的颜色：比底色再暗一点（几乎看不出来，与原版相同）
pub fn dim_ink(tone: Tone) -> Rgba {
    scale(tone.rgb(), DIM * 0.72)
}

/// 药丸上的文字（左边留 5 像素）。原版里带中文的字比纯英文的（格式名、热键）高 1 像素
pub fn pill_text(c: &mut Canvas, r: R, label: &str, tone: Tone, dim: bool) {
    let col = if dim { dim_ink(tone) } else { PILL_TEXT };
    c.push_clip(r.shrink(3));
    let y = if crate::util::text::contains_cjk(label) { r.y + 1 } else { r.y + 2 };
    c.text(label, FONT, r.x + 8, y, col, Hint::ClearType);
    c.pop_clip();
}

/// 药丸里的勾选框：在药丸右边，15 × 13，凹进去的样子；勾上时更暗、有白色的勾
pub fn pill_box(c: &mut Canvas, pill: R, tone: Tone, on: bool, dim: bool) {
    let b = R::new(pill.right() - 23, pill.y + 4, 15, 13);
    let base = if dim { scale(tone.rgb(), DIM) } else { tone.rgb() };
    let edge = scale(base, if on { 0.61 } else { 0.865 });
    ring(c, b, edge, 1.0);
    if on {
        // 左上暗、右下亮
        let i = b.shrink(1);
        for y in i.y..i.bottom() {
            for x in i.x..i.right() {
                let t = ((x - i.x) as f32 / i.w as f32 * 0.5 + (y - i.y) as f32 / i.h as f32) / 1.5;
                put(c, x, y, scale(base, 0.62 + 0.27 * t), 1.0);
            }
        }
        check(c, b.x + 2, b.y + 1, if dim { scale(Rgba::gray(250), DIM) } else { Rgba::gray(253) }, true);
    } else {
        c.fill(R::new(b.x + 1, b.y + 1, b.w - 2, 1), white(base, 0.16));
        for y in b.y + 2..b.bottom() - 1 {
            put(c, b.x + 1, y, white(base, 0.055), 1.0);
            put(c, b.right() - 2, y, white(base, 0.055), 1.0);
        }
    }
}

/// 勾：11 × 10 的区域里，短边从左下到中间、长边到右上；shadow 时右下有一道暗影
pub fn check(c: &mut Canvas, x: i32, y: i32, col: Rgba, shadow: bool) {
    let s = Shape::of(vec![S::Line(1.6, 5.6, 4.4, 8.4, 2.2), S::Line(4.4, 8.4, 9.6, 1.6, 2.2)]);
    let draw = |c: &mut Canvas, dx: i32, dy: i32, col: Rgba| {
        for py in -1..12 {
            for px in -1..13 {
                let cov = (0.5 - s.d(px as f32 + 0.5, py as f32 + 0.5)).clamp(0.0, 1.0);
                if cov > 0.0 {
                    put(c, x + px + dx, y + py + dy, col, cov);
                }
            }
        }
    };
    if shadow {
        draw(c, 1, 1, Rgba(0x50, 0x54, 0x5B, 200));
    }
    draw(c, 0, 0, col);
}

/// 不在药丸里的勾选框：13 × 12 的灰色方块（勾上时有暗色的勾），右边是文字
pub fn plain_check(c: &mut Canvas, r: R, label: &str, on: bool, hot: bool) {
    plain_check_ex(c, r, label, on, hot, true);
}

/// 同上；不能改时方块和文字都很暗
pub fn plain_check_ex(c: &mut Canvas, r: R, label: &str, on: bool, hot: bool, enabled: bool) {
    let b = R::new(r.x, r.y + (r.h - 12) / 2, 13, 12);
    let fill = if !enabled { 0x2D } else if hot { 0x96 } else { 0x81 };
    c.fill(b, Rgba::gray(fill));
    if on {
        check(c, b.x + 1, b.y, Rgba::gray(0x21), false);
    }
    let ink = if enabled { 0xE0 } else { 0x3E };
    c.text(label, FONT, b.x + 19, r.y + (r.h - 16) / 2 + 1, Rgba::gray(ink), Hint::ClearType);
}

/// 细边框的框（下拉框、输入框）
fn frame(c: &mut Canvas, r: R, enabled: bool, focused: bool) {
    let col = if focused { 0x8F } else if enabled { 0x51 } else { 0x39 };
    ring(c, r, Rgba::gray(col), 1.0);
}

/// 实心的向下小三角：左上角 (x, y)，宽 w、高 h（抗锯齿）
pub fn tri_down(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, col: Rgba) {
    let tri = Shape::of(vec![S::Poly(vec![(0.0, 0.0), (w, 0.0), (w / 2.0, h)], 0.0)]);
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    for py in -1..h.ceil() as i32 + 2 {
        for px in -1..w.ceil() as i32 + 2 {
            let (cx, cy) = ((x0 + px) as f32 + 0.5 - x, (y0 + py) as f32 + 0.5 - y);
            let cov = (0.5 - tri.d(cx, cy)).clamp(0.0, 1.0);
            put(c, x0 + px, y0 + py, col, cov);
        }
    }
}

/// 下拉框：左边是当前的选项，右边一个小三角
pub fn dropdown(c: &mut Canvas, r: R, value: &str, enabled: bool, hot: bool) {
    frame(c, r, enabled, hot && enabled);
    let ty = r.y + (r.h - 16) / 2 + 1;
    c.push_clip(R::new(r.x + 1, r.y + 1, r.w - 18, r.h - 2));
    c.text(value, FONT, r.x + 4, ty, Rgba::gray(if enabled { 0xE0 } else { 0x41 }), Hint::ClearType);
    c.pop_clip();
    let (ax, ay) = (r.right() - 12.0f32.max(0.0) as i32, r.y + r.h / 2 - 2);
    let tri = Shape::of(vec![S::Poly(vec![(0.0, 0.0), (7.0, 0.0), (3.5, 4.0)], 0.0)]);
    for py in -1..6 {
        for px in -1..9 {
            let cov = (0.5 - tri.d(px as f32 + 0.5, py as f32 + 0.5)).clamp(0.0, 1.0);
            put(c, ax + px, ay + py, Rgba::gray(if enabled { 0x6A } else { 0x39 }), cov);
        }
    }
}

/// 输入框：有内容时靠左显示，没有时居中显示灰色的提示文字；focused 时有光标
pub fn edit(c: &mut Canvas, r: R, value: &str, hint: &str, enabled: bool, focused: bool) {
    frame(c, r, enabled, focused);
    let ty = r.y + (r.h - 16) / 2 + 1;
    c.push_clip(r.shrink(2));
    if value.is_empty() && !focused {
        let tw = text::measure(hint, FONT).0 as i32;
        c.text(hint, FONT, r.x + (r.w - tw) / 2, ty, Rgba::gray(if enabled { 0x8F } else { 0x41 }), Hint::ClearType);
    } else {
        let tw = text::measure(value, FONT).0 as i32;
        // 文字过长时显示末尾
        let x = (r.x + 4).min(r.right() - 6 - tw);
        c.text(value, FONT, x, ty, Rgba::gray(if enabled { 0xE0 } else { 0x41 }), Hint::ClearType);
        if focused {
            c.fill(R::new(x + tw + 1, r.y + 4, 1, r.h - 8), Rgba::gray(0xE0));
        }
    }
    c.pop_clip();
}

/// 选文件夹的框：像输入框，右边一个「+」
pub fn path_box(c: &mut Canvas, r: R, value: &str, hint: &str, enabled: bool, hot: bool) {
    edit(c, R::new(r.x, r.y, r.w, r.h), value, hint, enabled, false);
    if hot && enabled {
        frame(c, r, true, true);
    }
    let (cx, cy) = (r.right() - 12, r.y + r.h / 2);
    let col = Rgba::gray(if enabled { 0x8F } else { 0x41 });
    c.fill(R::new(cx - 4, cy, 9, 2), col);
    c.fill(R::new(cx, cy - 4, 2, 9), col);
}

/// 一排色块（每格 20 × 15，间距 23 × 18）；cur 那一格打勾，hover 那一格边框亮一点
pub fn swatches(c: &mut Canvas, r: R, colors: &[[u8; 3]], cur: Option<usize>, hover: Option<usize>) {
    for (i, rgb) in colors.iter().enumerate() {
        let cell = R::new(r.x + (i % 16) as i32 * 23, r.y + (i / 16) as i32 * 18, 20, 15);
        let base = Rgba(rgb[0], rgb[1], rgb[2], 255);
        ring(c, cell, Rgba::gray(if hover == Some(i) { 0x9A } else { 0x3D }), 1.0);
        ring(c, cell.shrink(1), scale(base, 0.35), 1.0);
        ring(c, cell.shrink(2), white(base, 0.15), 1.0);
        let f = cell.shrink(3);
        c.vgrad(f, white(base, 0.06), base);
        if cur == Some(i) {
            check(c, cell.x + 5, cell.y + 3, Rgba::gray(253), true);
        }
    }
}

/// 色块区域里的第几格
pub fn swatch_at(r: R, n: usize, x: i32, y: i32) -> Option<usize> {
    if x < r.x || y < r.y {
        return None;
    }
    let (col, row) = ((x - r.x) / 23, (y - r.y) / 18);
    let i = (row * 16 + col) as usize;
    (col < 16 && (x - r.x) % 23 < 20 && (y - r.y) % 18 < 15 && i < n).then_some(i)
}

/// 滑块的轨道（r 是整个滑块的区域）：圆点中心从 left 走到 right
pub fn slider_span(r: R) -> (i32, i32) {
    (r.x + 6, r.right() - 6)
}

/// 圆点滑块：4 像素高的凹槽，圆点按 v（0 ~ 255）放（中间一片亮、边上一圈暗环）
pub fn slider(c: &mut Canvas, r: R, v: u8, hot: bool) {
    let (l, rr) = slider_span(r);
    let cy = r.y + r.h / 2;
    for (i, g) in [0x42u8, 0x49, 0x49, 0x3D].iter().enumerate() {
        c.fill(R::new(l, cy - 2 + i as i32, rr - l, 1), Rgba::gray(*g));
    }
    let kx = l as f32 + (rr - l) as f32 * v as f32 / 255.0;
    let (fx, fy) = (kx + 0.5, cy as f32 + 0.5);
    for py in cy - 9..cy + 10 {
        for px in kx as i32 - 9..kx as i32 + 10 {
            let d = (px as f32 + 0.5 - fx).hypot(py as f32 + 0.5 - fy);
            let cov = (8.0 - d).clamp(0.0, 1.0);
            let g = if d < 3.6 { 0xCE as f32 } else if d < 5.0 { 0xCE as f32 - (d - 3.6) * 63.0 } else { (0x75 as f32 - (d - 5.0) * 18.0).max(0x28 as f32) };
            put(c, px, py, Rgba::gray((g + if hot { 18.0 } else { 0.0 }).min(255.0) as u8), cov);
        }
    }
}

/// 滑块上 x 处对应的值
pub fn slider_value(r: R, x: i32) -> u8 {
    let (l, rr) = slider_span(r);
    (((x - l) as f32 / (rr - l) as f32).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// 凹槽分隔线：3 像素高，中间亮两头淡
pub fn groove(c: &mut Canvas, r: R, peak: u8) {
    let bg = Rgba::gray(0x20);
    for x in r.x..r.right() {
        let t = (x - r.x) as f32 / r.w as f32;
        let k = (t.min(1.0 - t) * 5.0).clamp(0.0, 1.0);
        let mid = bg.mix(Rgba::gray(peak), k);
        let side = bg.mix(Rgba::gray(peak.saturating_sub(5)), k * 0.8);
        put(c, x, r.y, side, 1.0);
        put(c, x, r.y + 1, mid, 1.0);
        put(c, x, r.y + 2, side, 1.0);
    }
}
