//! 均衡器面板（原版 304×146）：前置放大 + 10 段滑杆、像素字刻度，底部 启用 / 重置 / 预设 三个按钮。

use super::eq_labels as L;
use super::frame;
use crate::gui::theme;
use crate::gui::canvas::{Canvas, R};

pub const W: i32 = 304;
pub const H: i32 = 144;
/// 滑杆中心 x（第 0 个为前置放大）
pub const SLIDER_X: [i32; 11] = [36, 89, 109, 129, 149, 169, 189, 209, 229, 249, 269];
pub const RAIL_TOP: i32 = 29;
pub const RAIL_H: i32 = 54;
pub const BTN_ENABLE: R = R::new(12, 111, 80, 23);
pub const BTN_RESET: R = R::new(92, 111, 80, 23);
pub const BTN_PRESET: R = R::new(172, 111, 120, 23);

pub struct EqModel<'a> {
    pub enabled: bool,
    pub preamp: f32,
    pub bands: [f32; 10],
    pub preset: &'a str,
    pub hot_button: Option<usize>,
    pub strip_hover: f32,
}

/// 增益（dB，±12）→ 滑块 y
pub fn knob_y(db: f32) -> i32 {
    RAIL_TOP + ((1.0 - (db.clamp(-12.0, 12.0) + 12.0) / 24.0) * (RAIL_H - 1) as f32).round() as i32
}

pub fn db_at(y: i32) -> f32 {
    let t = (y - RAIL_TOP) as f32 / (RAIL_H - 1) as f32;
    ((1.0 - t.clamp(0.0, 1.0)) * 24.0 - 12.0).round()
}

pub fn paint(c: &mut Canvas, m: &EqModel) {
    let body = frame::panel(c, m.strip_hover);
    c.push_clip(body);
    let label = theme::g(L::LABEL_GRAY);
    let mut dot = |x: i32, y: i32| c.blend(x, y, label, 1.0);
    L::draw(L::PREAMP.0, L::PREAMP.1, L::FREQ_Y, &mut dot);
    for (s, cx) in L::FREQS {
        L::draw(s, cx - L::width(s) / 2, L::FREQ_Y, &mut dot);
    }
    for (s, right, y) in L::DB {
        L::draw(s, right - L::width(s), y, &mut dot);
    }
    // 前级放大的 0 位置的短横线
    c.fill(R::new(40, 59, 3, 1), label);
    for (i, &sx) in SLIDER_X.iter().enumerate() {
        // 刻度：左右两列短横线
        for k in 0..=8 {
            let y = RAIL_TOP + k * (RAIL_H - 1) / 8;
            let w = if k % 4 == 0 { 3 } else { 2 };
            if i > 0 {
                c.fill(R::new(sx - 6, y, w, 1), label);
                c.fill(R::new(sx + 4, y, w, 1), label);
            }
        }
        c.fill(R::new(sx - 1, RAIL_TOP - 1, 3, RAIL_H + 2), theme::g(22));
        if m.enabled {
            let db = if i == 0 { m.preamp } else { m.bands[i - 1] };
            let ky = knob_y(db);
            c.fill(R::new(sx - 1, ky, 3, RAIL_TOP + RAIL_H - ky), theme::g(110));
            c.round_rect(R::new(sx - 4, ky - 2, 9, 5), 2.0, Some(theme::g(210)), None);
        }
    }
    button(c, BTN_ENABLE, if m.enabled { "禁用" } else { "启用" }, true, m.hot_button == Some(0));
    button(c, BTN_RESET, "重置", m.enabled, m.hot_button == Some(1));
    button(c, BTN_PRESET, m.preset, m.enabled, m.hot_button == Some(2));
    // 预设右边的小三角（原版是 6 × 5.5 的实心三角）
    let (tx, ty) = (BTN_PRESET.right() as f32 - 12.5, BTN_PRESET.y as f32 + 9.6);
    super::pill::tri_down(c, tx, ty, 6.0, 5.5, theme::g(if m.enabled { 150 } else { 65 }));
    c.pop_clip();
}

/// 按钮：和「配置文件存放位置」窗口的按钮一样（外圈暗、里圈亮的两道边），颜色照原版量的
fn button(c: &mut Canvas, r: R, label: &str, active: bool, hot: bool) {
    let [o, i, f, t] = match (active, hot) {
        (true, true) => [24, 143, 87, 255],
        (true, false) => [25, 117, 69, 192],
        (false, _) => [29, 61, 45, 75],
    };
    super::lyricdl::button_rgba(c, r, label, [theme::g(o), theme::g(i), theme::g(f), theme::g(t)]);
}
