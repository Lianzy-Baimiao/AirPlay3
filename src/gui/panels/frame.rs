//! 原版面板通用外框：2 像素透明边 + 1 像素灰色圆角描边，顶部 19 像素渐变标题条，主体底色 #202020。

use crate::gui::canvas::{Canvas, R, Rgba};

pub const STRIP_H: i32 = 18;
pub const BODY_BG: Rgba = Rgba::gray(32);
pub const BORDER: Rgba = Rgba::gray(85);
pub const LINE: Rgba = Rgba::gray(88);

/// 画外框，返回主体区域（不含标题条与描边）
pub fn panel(c: &mut Canvas, strip_hover: f32) -> R {
    c.clear();
    let outer = R::new(2, 2, c.w - 4, c.h - 4);
    let inner = outer.shrink(1);
    c.push_clip(inner);
    let strip = R::new(inner.x, inner.y, inner.w, STRIP_H - 1);
    c.vgrad(strip, Rgba::gray(1), Rgba::gray(20));
    if strip_hover > 0.0 {
        let a = (strip_hover * 255.0) as u8;
        for x in strip.x..strip.right() {
            let d = ((x - strip.x - strip.w / 2) as f32).abs() / (strip.w as f32 / 2.0);
            let v = (75.0 - 60.0 * d) as u8;
            for y in strip.y..strip.bottom() {
                let t = (y - strip.y) as f32 / strip.h as f32;
                c.blend(x, y, Rgba::gray((v as f32 * (0.4 + 0.6 * t)) as u8).alpha(a), 1.0);
            }
        }
    }
    c.fill(R::new(inner.x, strip.bottom(), inner.w, 1), LINE);
    c.fill(R::new(inner.x, strip.bottom() + 1, inner.w, 1), Rgba::gray(23));
    let body = R::new(inner.x, strip.bottom() + 2, inner.w, inner.bottom() - strip.bottom() - 2);
    c.fill(body, BODY_BG);
    c.pop_clip();
    c.round_rect(outer, 4.0, None, Some(BORDER));
    body
}

/// 标题条上的关闭按钮区域（悬停时显示）
pub fn strip_close(c: &Canvas) -> R {
    R::new(c.w - 22, 4, 15, 15)
}
