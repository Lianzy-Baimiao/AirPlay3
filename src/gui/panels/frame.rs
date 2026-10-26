//! 面板通用外框（按原版截图实测的效果自己画的）：四周 2 像素柔和阴影；1 像素浅色亮线（顶部两角是圆角）；
//! 顶部是半透明的玻璃标题条（空闲时几乎完全透明，悬停时更不透明，关闭按钮后面有一团光）；
//! 标题条下一条暗线；主体为「整体颜色」，两侧和底边是主体与白色混合的亮线。

use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::theme;

pub const STRIP_H: i32 = 18;
/// 顶部两角的圆角半径
const RADIUS: f32 = 4.0;
/// 玻璃上的亮线（顶边、标题条两侧、标题条底边）
pub const HIGHLIGHT: Rgba = Rgba(0xE2, 0xE2, 0xE2, 96);
/// 窗口外 1、2 像素的阴影
const SHADOW: [f32; 2] = [112.0, 16.0];

/// 主体颜色（整体颜色）
pub fn body_bg() -> Rgba {
    theme::body()
}

/// 玻璃上的文字（标题条上的标题）后面的暗色光晕：玻璃几乎透明，放在什么背景上都看得清
pub const TITLE_HALO: Rgba = Rgba(16, 16, 16, 190);

/// 画外框，返回主体区域（不含标题条与边）
pub fn panel(c: &mut Canvas, strip_hover: f32) -> R {
    panel_ex(c, strip_hover)
}

/// 外框形状（左上右下 l t rr b，顶部 `r_top`、底部 `r_bottom` 圆角）在点 (px, py) 的有符号距离（内部为负；都是实际像素）；
/// 底部是直角时，两条边外侧的阴影在角上减半（与原版相同）
#[allow(clippy::too_many_arguments)]
fn sdf(l: f32, t: f32, rr: f32, b: f32, px: f32, py: f32, r_top: f32, r_bottom: f32) -> (f32, bool) {
    // 很小的框（出现动画刚开始）：圆角不超过一半宽高
    let r = (if py < (t + b) / 2.0 { r_top } else { r_bottom }).min((rr - l) / 2.0).min((b - t) / 2.0);
    if r > 0.0 && (px < l + r || px > rr - r) && (py < t + r || py > b - r) {
        let (dx, dy) = (px - px.clamp(l + r, rr - r), py - py.clamp(t + r, b - r));
        ((dx * dx + dy * dy).sqrt() - r, false)
    } else {
        let (ex, ey) = ((l - px).max(px - rr), (t - py).max(py - b));
        (ex.max(ey), ex > 0.0 && ey > 0.0 && (ex - ey).abs() < 0.01)
    }
}

/// 外框形状外侧的阴影不透明度
fn shadow(d: f32) -> f32 {
    if d <= 0.5 {
        SHADOW[0]
    } else if d <= 1.5 {
        SHADOW[0] + (SHADOW[1] - SHADOW[0]) * (d - 0.5)
    } else {
        (SHADOW[1] * (2.5 - d)).max(0.0)
    }
}

/// 画一个外框：阴影、1 像素描边（颜色由 `stroke(行)` 给出）、内部（`fill(x, 行)`）。o、x、行都是逻辑像素；
/// 放大时外框的边按实际像素取整、按实际像素抗锯齿（边缘清楚），颜色按每个实际像素所在的逻辑像素取
pub fn shape(c: &mut Canvas, o: R, r_top: f32, r_bottom: f32, stroke: impl Fn(i32) -> Rgba, fill: impl Fn(i32, i32) -> Rgba) {
    let s = c.scale;
    let p = c.tr(o);
    let (l, t, rr, b) = (p.x as f32, p.y as f32, p.right() as f32, p.bottom() as f32);
    let (rt, rb, bw) = (r_top * s, r_bottom * s, c.line_px());
    let cols: Vec<i32> = (0..c.pw).map(|x| c.lp(x)).collect();
    for y in 0..c.ph {
        let row = c.lp(y);
        for x in 0..c.pw {
            let (d, corner) = sdf(l, t, rr, b, x as f32 + 0.5, y as f32 + 0.5, rt, rb);
            // 阴影的宽度是逻辑像素
            let dl = d / s;
            if dl >= 2.5 {
                continue;
            }
            let outer = (0.5 - d).clamp(0.0, 1.0);
            let inner = (0.5 - bw - d).clamp(0.0, 1.0);
            if outer < 1.0 {
                let a = shadow(dl.max(0.5)) * if corner { 0.5 } else { 1.0 };
                c.blend_px(x, y, Rgba(0, 0, 0, a.round() as u8), 1.0 - outer);
            }
            if inner > 0.0 {
                c.blend_px(x, y, fill(cols[x as usize], row), inner);
            }
            if outer > inner {
                c.blend_px(x, y, stroke(row), outer - inner);
            }
        }
    }
}

fn panel_ex(c: &mut Canvas, strip_hover: f32) -> R {
    c.clear();
    let o = R::new(2, 2, c.w - 4, c.h - 4);
    let (body, edge, sep) = (theme::body(), theme::edge(), theme::separator());
    let ba = theme::border_alpha() as f32;
    let hover = strip_hover.clamp(0.0, 1.0);
    let close = strip_close(c);
    let gx = close.x as f32 + close.w as f32 / 2.0;
    // 行：o.y 顶边亮线；o.y+1 ..= o.y+15 玻璃；o.y+16 亮线；o.y+17 暗线；以下为主体
    let (glass0, line_y, sep_y) = (o.y + 1, o.y + 16, o.y + 17);
    let stroke = |y: i32| if y < sep_y { HIGHLIGHT } else { edge };
    let fill = |x: i32, y: i32| {
        if y < glass0 || y == line_y {
            HIGHLIGHT
        } else if y < line_y {
            let t = (y - glass0) as f32;
            let idle = 2.0 + 2.0 * t;
            let glow = 126.4 + 6.8 * t - 2.1 * (x as f32 + 0.5 - gx).abs();
            let hov = (5.0 + 6.0 * t).max(glow);
            let a = (ba + idle + (hov - idle) * hover).clamp(0.0, 255.0);
            let (r, g, b) = theme::tint((130.0 + 2.8 * t).round() as u8);
            Rgba(r, g, b, a.round() as u8)
        } else if y == sep_y {
            sep
        } else {
            body
        }
    };
    shape(c, o, RADIUS, 0.0, stroke, fill);
    // 悬停时在光的中心显示关闭按钮
    if hover > 0.0 {
        c.icon(crate::skin::icons::W_CLOSE, close.x, close.y, Rgba::gray(235).alpha((hover * 255.0) as u8));
    }
    R::new(o.x + 1, sep_y + 1, o.w - 2, o.bottom() - 1 - (sep_y + 1))
}

/// 宽 w、高 h 的外框里「主体窗口」的区域：标题条下面的暗线以下，连同两侧和底边的亮线
/// （原版的主体是单独的一个窗口，开着「配置文件存放位置」时整块变暗；504×386 的配置窗口是 500×365）
pub fn body_rect(w: i32, h: i32) -> R {
    R::new(2, 2 + 17, w - 4, h - 4 - 17)
}

/// 标题条上的关闭按钮区域（悬停时显示；后面的光以它为中心）
pub fn strip_close(c: &Canvas) -> R {
    close_rect(c.w)
}

/// 宽 `w` 的面板标题条上的关闭按钮区域
pub fn close_rect(w: i32) -> R {
    R::new(w - 39, 3, 15, 15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_rect_matches_original_settings() {
        // 原版「配置」的主体窗口 500×365，在外框 (2, 19)（实测变暗的范围）
        assert_eq!(body_rect(504, 386), R::new(2, 19, 500, 365));
    }
}
