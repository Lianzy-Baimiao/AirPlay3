//! 「网络电台」窗口：推荐 / 热门 / 收藏三个分页、地区、搜索框，电台列表，收藏 / 播放 / 关闭按钮。

use super::frame;
use super::search::{button, field};
use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::text::{self, Font, Hint};

pub const W: i32 = 460;
pub const H: i32 = 420;
pub const TABS: [&str; 3] = ["推荐", "热门", "收藏"];
pub const BTN_REGION: R = R::new(172, 28, 92, 22);
pub const FIELD_SEARCH: R = R::new(270, 28, 124, 22);
pub const BTN_SEARCH: R = R::new(398, 28, 52, 22);
pub const LIST: R = R::new(10, 58, 440, 308);
pub const ROW_H: i32 = 22;
pub const BTN_FAV: R = R::new(244, 382, 78, 24);
pub const BTN_PLAY: R = R::new(326, 382, 60, 24);
pub const BTN_CLOSE: R = R::new(390, 382, 60, 24);
const FONT: Font = Font::px(12.0, false);
const ORANGE: Rgba = Rgba(234, 115, 43, 255);

pub fn tab_rect(i: usize) -> R {
    R::new(10 + i as i32 * 52, 28, 50, 22)
}

pub struct Row<'a> {
    pub name: &'a str,
    pub detail: String,
    pub format: String,
    pub favorite: bool,
    pub playing: bool,
}

pub struct RadioModel<'a> {
    /// 0 推荐，1 热门，2 收藏，3 搜索结果
    pub tab: usize,
    pub region: &'a str,
    pub query: &'a str,
    pub focus: bool,
    pub rows: &'a [Row<'a>],
    pub selected: Option<usize>,
    pub hover: Option<usize>,
    pub scroll: i32,
    pub status: &'a str,
    /// 鼠标所在的按钮：0 地区，1 搜索，2 收藏，3 播放，4 关闭，10+ 分页
    pub hot: Option<usize>,
    pub fav_label: &'a str,
}

pub fn paint(c: &mut Canvas, m: &RadioModel) {
    let body = frame::panel(c, 0.0);
    c.text("网络电台", FONT, 10, 4, Rgba::gray(200), Hint::ClearType);
    c.push_clip(body);
    for (i, name) in TABS.iter().enumerate() {
        let r = tab_rect(i);
        let on = m.tab == i;
        if on {
            c.round_rect(r, 3.0, Some(Rgba::gray(56)), Some(Rgba::gray(96)));
        } else if m.hot == Some(10 + i) {
            c.round_rect(r, 3.0, Some(Rgba::gray(42)), None);
        }
        let tw = text::measure(name, FONT).0 as i32;
        c.text(name, FONT, r.x + (r.w - tw) / 2, r.y + 4, if on { ORANGE } else { Rgba::gray(190) }, Hint::ClearType);
    }
    button(c, BTN_REGION, &format!("{} ▾", m.region), true, m.hot == Some(0));
    field(c, FIELD_SEARCH, m.query, m.focus);
    if m.query.is_empty() && !m.focus {
        c.text("搜索电台名称", FONT, FIELD_SEARCH.x + 6, FIELD_SEARCH.y + 4, Rgba::gray(100), Hint::ClearType);
    }
    button(c, BTN_SEARCH, "搜索", true, m.hot == Some(1));
    c.fill(LIST, Rgba::gray(26));
    c.push_clip(LIST);
    for (i, r) in m.rows.iter().enumerate() {
        let y = LIST.y + i as i32 * ROW_H - m.scroll;
        if y + ROW_H < LIST.y || y > LIST.bottom() {
            continue;
        }
        let row = R::new(LIST.x, y, LIST.w, ROW_H);
        if m.selected == Some(i) {
            c.fill(row, Rgba::gray(75));
        } else if m.hover == Some(i) {
            c.fill(row, Rgba::gray(48));
        } else if i % 2 == 1 {
            c.fill(row, Rgba::gray(34));
        }
        let mark = if r.playing { "▶" } else if r.favorite { "★" } else { "" };
        c.text(mark, FONT, LIST.x + 6, y + 3, if r.playing { ORANGE } else { Rgba(240, 190, 60, 255) }, Hint::ClearType);
        let fw = text::measure(&r.format, FONT).0 as i32;
        c.push_clip(R::new(LIST.x + 24, y, LIST.w - 36 - fw, ROW_H));
        let w = c.text(r.name, FONT, LIST.x + 24, y + 3, if r.playing { ORANGE } else { Rgba::gray(232) }, Hint::ClearType);
        c.text(&r.detail, FONT, LIST.x + 36 + w, y + 3, Rgba::gray(130), Hint::ClearType);
        c.pop_clip();
        c.text(&r.format, FONT, LIST.right() - fw - 8, y + 3, Rgba::gray(150), Hint::ClearType);
    }
    if m.rows.is_empty() {
        let tip = if m.tab == 2 { "还没有收藏的电台：选中电台后点「收藏」" } else { "" };
        c.text(tip, FONT, LIST.x + 12, LIST.y + 12, Rgba::gray(120), Hint::ClearType);
    }
    c.pop_clip();
    c.push_clip(R::new(12, 382, BTN_FAV.x - 20, 24));
    c.text(m.status, FONT, 12, 386, Rgba::gray(150), Hint::ClearType);
    c.pop_clip();
    button(c, BTN_FAV, m.fav_label, m.selected.is_some(), m.hot == Some(2));
    button(c, BTN_PLAY, "播放", m.selected.is_some(), m.hot == Some(3));
    button(c, BTN_CLOSE, "关闭", true, m.hot == Some(4));
    c.pop_clip();
}
