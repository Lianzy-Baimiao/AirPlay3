//! 「配置」窗口（原版风格面板）：顶部分页，下方为勾选框 / 下拉 / 输入框 / 热键 / 按钮。

use super::frame;
use super::search::{button, field};
use crate::gui::theme;
use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::settings_model::{Swatch, TAB_COLOR, TAB_DESK, TAB_HOTKEYS, TABS, Widget};
use crate::gui::text::{self, Font, Hint};
use crate::skin::icons;

pub const W: i32 = 520;
pub const H: i32 = 450;
const FONT: Font = Font::px(12.0, false);
const TAB_Y: i32 = 26;
const TOP: i32 = 62;

/// 分页按钮：7 个分页均分 (W - 24) 的宽度
pub fn tab_rect(i: usize) -> R {
    let slot = (W - 24) / TABS.len() as i32;
    R::new(12 + i as i32 * slot, TAB_Y, slot - 2, 24)
}

pub fn row_h(tab: usize) -> i32 {
    match tab {
        TAB_HOTKEYS => 23,
        TAB_DESK => 24,
        _ => 28,
    }
}

/// 「颜色透明」分页各项的高度（色块、滑块比普通的行高）
const COLOR_ROWS: [i32; 8] = [28, 22, 58, 22, 22, 30, 24, 28];
/// 色块的间距（与原版相同：23 × 18）
pub const SWATCH: (i32, i32) = (23, 18);

/// 第 i 个设置项的区域
pub fn row_rect(tab: usize, i: usize) -> R {
    if tab == TAB_COLOR {
        let y = TOP + COLOR_ROWS.iter().take(i).sum::<i32>();
        let h = COLOR_ROWS.get(i).copied().unwrap_or(28);
        return R::new(22, y, W - 44, h - 4);
    }
    R::new(22, TOP + i as i32 * row_h(tab), W - 44, row_h(tab) - 4)
}

/// 色块区域里第几个色块（行 × 16 + 列）
pub fn swatch_at(r: R, kind: Swatch, x: i32, y: i32) -> Option<usize> {
    let (col, row) = ((x - r.x) / SWATCH.0, (y - r.y) / SWATCH.1);
    (x >= r.x && y >= r.y && col < 16 && row < kind.rows()).then_some((row * 16 + col) as usize)
}

/// 滑块的轨道
pub fn slider_track(r: R) -> R {
    R::new(r.x + 110, r.y + r.h / 2 - 2, 220, 4)
}

/// 滑块上 x 处对应的值（0 ~ 255）
pub fn slider_value(r: R, x: i32) -> u8 {
    let t = slider_track(r);
    (((x - t.x) as f32 / t.w as f32).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// 设置项中可点击的部分（勾选框整行；下拉 / 输入框 / 热键为右侧控件；按钮为按钮本身）
pub fn control_rect(tab: usize, i: usize, w: &Widget) -> R {
    let r = row_rect(tab, i);
    match w {
        Widget::Choice(..) | Widget::Text(..) | Widget::Hotkey(_) => R::new(r.x + 110, r.y, r.w - 110, r.h),
        Widget::Button(label, _) => R::new(r.x, r.y, text::measure(label, FONT).0 as i32 + 28, r.h),
        Widget::Swatches(k) => R::new(r.x, r.y, 16 * SWATCH.0, k.rows() * SWATCH.1),
        Widget::Slider(_) => {
            let t = slider_track(r);
            R::new(t.x - 6, r.y, t.w + 12, r.h)
        }
        _ => r,
    }
}

pub struct SettingsModel<'a> {
    pub tab: usize,
    pub items: &'a [(Widget, bool, String)],
    pub focus: Option<usize>,
    pub hover: Option<usize>,
    pub hover_tab: Option<usize>,
    pub capturing: Option<usize>,
    /// 鼠标下的色块
    pub hover_cell: Option<usize>,
    pub status: &'a str,
}

pub fn paint(c: &mut Canvas, m: &SettingsModel) {
    let body = frame::panel(c, 0.0);
    c.glow_text("配置", FONT, 10, 4, Rgba::gray(235), frame::TITLE_HALO, 2);
    c.icon(icons::W_CLOSE, W - 22, 4, Rgba::gray(220));
    c.push_clip(body);
    for (i, name) in TABS.iter().enumerate() {
        let r = tab_rect(i);
        let active = i == m.tab;
        if active {
            c.round_rect(r, 3.0, Some(theme::g(56)), Some(theme::g(96)));
        } else if m.hover_tab == Some(i) {
            c.round_rect(r, 3.0, Some(theme::g(42)), None);
        }
        let tw = text::measure(name, FONT).0 as i32;
        c.text(name, FONT, r.x + (r.w - tw) / 2, r.y + 4, if active { theme::focus() } else { theme::g(190) }, Hint::ClearType);
    }
    c.fill(R::new(body.x + 8, TAB_Y + 28, body.w - 16, 1), theme::g(64));
    for (i, (w, on, value)) in m.items.iter().enumerate() {
        let r = row_rect(m.tab, i);
        let hot = m.hover == Some(i);
        let ty = r.y + (r.h - 16) / 2 + 1;
        match w {
            Widget::Label(s) => {
                c.text(s, FONT, r.x, ty, theme::g(140), Hint::ClearType);
            }
            Widget::Check(_, label) => {
                let b = R::new(r.x, r.y + (r.h - 13) / 2, 13, 13);
                c.round_rect(b, 2.0, Some(theme::g(24)), Some(theme::g(if hot { 170 } else { 110 })));
                if *on {
                    c.icon(icons::CHECK, b.x - 1, b.y - 1, theme::focus());
                }
                c.text(label, FONT, r.x + 22, ty, theme::g(if hot { 250 } else { 225 }), Hint::ClearType);
            }
            Widget::Choice(label, ..) | Widget::Text(_, label) => {
                c.text(label, FONT, r.x, ty, theme::g(200), Hint::ClearType);
                let cr = control_rect(m.tab, i, w);
                if matches!(w, Widget::Choice(..)) {
                    c.round_rect(cr, 3.0, Some(theme::g(if hot { 52 } else { 40 })), Some(theme::g(100)));
                    c.push_clip(R::new(cr.x, cr.y, cr.w - 20, cr.h));
                    c.text(value, FONT, cr.x + 8, ty, theme::g(230), Hint::ClearType);
                    c.pop_clip();
                    c.icon(icons::TRI_DOWN, cr.right() - 18, cr.y + (cr.h - 15) / 2, theme::g(160));
                } else {
                    field(c, cr, value, m.focus == Some(i));
                }
            }
            Widget::Hotkey(a) => {
                c.text(a.label(), FONT, r.x, ty, theme::g(200), Hint::ClearType);
                let cr = control_rect(m.tab, i, w);
                let shown = if m.capturing == Some(i) { "请按下自定义键盘热键" } else { value.as_str() };
                c.round_rect(cr, 3.0, Some(theme::g(if m.capturing == Some(i) { 60 } else if hot { 46 } else { 34 })), Some(theme::g(90)));
                c.text(shown, FONT, cr.x + 8, ty, if m.capturing == Some(i) { theme::focus() } else { theme::g(220) }, Hint::ClearType);
            }
            Widget::Button(label, _) => button(c, control_rect(m.tab, i, w), label, true, hot),
            Widget::Swatches(k) => swatches(c, control_rect(m.tab, i, w), *k, if hot { m.hover_cell } else { None }),
            Widget::Slider(label) => slider(c, r, label, hot),
        }
    }
    c.text(m.status, FONT, 14, H - 26, theme::g(140), Hint::ClearType);
    c.pop_clip();
}

/// 色块：当前选中的用焦点颜色描边，鼠标下的用浅色描边
fn swatches(c: &mut Canvas, r: R, kind: Swatch, hover: Option<usize>) {
    let t = theme::get();
    let want = if kind == Swatch::Body { t.body } else { t.focus };
    // 当前颜色不在色块里时（如默认焦点颜色 #FFA040）标出最接近的
    let dist = |c: &[u8; 3]| (0..3).map(|k| (c[k] as i32 - want[k] as i32).pow(2)).sum::<i32>();
    let cur = *kind.presets().iter().min_by_key(|c| dist(c)).unwrap_or(&want);
    for (i, rgb) in kind.presets().iter().enumerate() {
        let cell = R::new(r.x + (i % 16) as i32 * SWATCH.0, r.y + (i / 16) as i32 * SWATCH.1, SWATCH.0 - 3, SWATCH.1 - 3);
        let edge = if *rgb == cur {
            theme::focus()
        } else if hover == Some(i) {
            theme::g(230)
        } else {
            theme::g(90)
        };
        c.fill(cell, edge);
        c.fill(cell.shrink(1), Rgba(rgb[0], rgb[1], rgb[2], 255));
        if *rgb == cur {
            c.fill(R::new(cell.x - 1, cell.y - 1, cell.w + 2, cell.h + 2), theme::focus().alpha(90));
            c.fill(cell.shrink(1), Rgba(rgb[0], rgb[1], rgb[2], 255));
        }
    }
}

/// 边框透明的滑块：轨道左边是当前值，右边是一小块预览（当前不透明度的玻璃）
fn slider(c: &mut Canvas, r: R, label: &str, hot: bool) {
    let ty = r.y + (r.h - 16) / 2 + 1;
    c.text(label, FONT, r.x, ty, theme::g(200), Hint::ClearType);
    let v = theme::border_alpha();
    let t = slider_track(r);
    c.round_rect(t, 2.0, Some(theme::g(22)), Some(theme::g(90)));
    let kx = t.x + (t.w as f32 * v as f32 / 255.0).round() as i32;
    c.round_rect(R::new(t.x, t.y, kx - t.x, t.h), 2.0, Some(theme::focus()), None);
    c.round_rect(R::new(kx - 4, t.y - 5, 9, 14), 3.0, Some(theme::g(if hot { 250 } else { 210 })), None);
    let pct = format!("{}%", (v as u32 * 100 + 127) / 255);
    c.text(&pct, FONT, t.right() + 14, ty, theme::g(220), Hint::ClearType);
}
