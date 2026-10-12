//! 「配置」窗口（原版风格面板）：顶部分页，下方为勾选框 / 下拉 / 输入框 / 热键 / 按钮。

use super::frame;
use super::search::{button, field};
use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::settings_model::{TAB_DESK, TAB_HOTKEYS, TABS, Widget};
use crate::gui::text::{self, Font, Hint};
use crate::skin::icons;

pub const W: i32 = 470;
pub const H: i32 = 450;
const FONT: Font = Font::px(12.0, false);
const TAB_Y: i32 = 26;
const TOP: i32 = 62;
pub const ORANGE: Rgba = Rgba(234, 115, 43, 255);

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

/// 第 i 个设置项的区域
pub fn row_rect(tab: usize, i: usize) -> R {
    R::new(22, TOP + i as i32 * row_h(tab), W - 44, row_h(tab) - 4)
}

/// 设置项中可点击的部分（勾选框整行；下拉 / 输入框 / 热键为右侧控件；按钮为按钮本身）
pub fn control_rect(tab: usize, i: usize, w: &Widget) -> R {
    let r = row_rect(tab, i);
    match w {
        Widget::Choice(..) | Widget::Text(..) | Widget::Hotkey(_) => R::new(r.x + 110, r.y, r.w - 110, r.h),
        Widget::Button(label, _) => R::new(r.x, r.y, text::measure(label, FONT).0 as i32 + 28, r.h),
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
    pub status: &'a str,
}

pub fn paint(c: &mut Canvas, m: &SettingsModel) {
    let body = frame::panel(c, 0.0);
    c.text("配置", FONT, 10, 4, Rgba::gray(200), Hint::ClearType);
    c.icon(icons::W_CLOSE, W - 22, 4, Rgba::gray(170));
    c.push_clip(body);
    for (i, name) in TABS.iter().enumerate() {
        let r = tab_rect(i);
        let active = i == m.tab;
        if active {
            c.round_rect(r, 3.0, Some(Rgba::gray(56)), Some(Rgba::gray(96)));
        } else if m.hover_tab == Some(i) {
            c.round_rect(r, 3.0, Some(Rgba::gray(42)), None);
        }
        let tw = text::measure(name, FONT).0 as i32;
        c.text(name, FONT, r.x + (r.w - tw) / 2, r.y + 4, if active { ORANGE } else { Rgba::gray(190) }, Hint::ClearType);
    }
    c.fill(R::new(body.x + 8, TAB_Y + 28, body.w - 16, 1), Rgba::gray(64));
    for (i, (w, on, value)) in m.items.iter().enumerate() {
        let r = row_rect(m.tab, i);
        let hot = m.hover == Some(i);
        let ty = r.y + (r.h - 16) / 2 + 1;
        match w {
            Widget::Label(s) => {
                c.text(s, FONT, r.x, ty, Rgba::gray(140), Hint::ClearType);
            }
            Widget::Check(_, label) => {
                let b = R::new(r.x, r.y + (r.h - 13) / 2, 13, 13);
                c.round_rect(b, 2.0, Some(Rgba::gray(24)), Some(Rgba::gray(if hot { 170 } else { 110 })));
                if *on {
                    c.icon(icons::CHECK, b.x - 1, b.y - 1, ORANGE);
                }
                c.text(label, FONT, r.x + 22, ty, Rgba::gray(if hot { 250 } else { 225 }), Hint::ClearType);
            }
            Widget::Choice(label, ..) | Widget::Text(_, label) => {
                c.text(label, FONT, r.x, ty, Rgba::gray(200), Hint::ClearType);
                let cr = control_rect(m.tab, i, w);
                if matches!(w, Widget::Choice(..)) {
                    c.round_rect(cr, 3.0, Some(Rgba::gray(if hot { 52 } else { 40 })), Some(Rgba::gray(100)));
                    c.push_clip(R::new(cr.x, cr.y, cr.w - 20, cr.h));
                    c.text(value, FONT, cr.x + 8, ty, Rgba::gray(230), Hint::ClearType);
                    c.pop_clip();
                    c.icon(icons::TRI_DOWN, cr.right() - 18, cr.y + (cr.h - 15) / 2, Rgba::gray(160));
                } else {
                    field(c, cr, value, m.focus == Some(i));
                }
            }
            Widget::Hotkey(a) => {
                c.text(a.label(), FONT, r.x, ty, Rgba::gray(200), Hint::ClearType);
                let cr = control_rect(m.tab, i, w);
                let shown = if m.capturing == Some(i) { "请按下自定义键盘热键" } else { value.as_str() };
                c.round_rect(cr, 3.0, Some(Rgba::gray(if m.capturing == Some(i) { 60 } else if hot { 46 } else { 34 })), Some(Rgba::gray(90)));
                c.text(shown, FONT, cr.x + 8, ty, if m.capturing == Some(i) { ORANGE } else { Rgba::gray(220) }, Hint::ClearType);
            }
            Widget::Button(label, _) => button(c, control_rect(m.tab, i, w), label, true, hot),
        }
    }
    c.text(m.status, FONT, 14, H - 26, Rgba::gray(140), Hint::ClearType);
    c.pop_clip();
}
