//! 音乐列表面板（原版 324×431）：工具栏 + ✎ ⇅、左侧列表分栏、21 像素行（标题 • 艺人、右侧时长）、底部计数与搜索框。

use super::frame;
use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::text::{self, Font, Hint};
use crate::skin::icons;

pub const W: i32 = 324;
pub const H: i32 = 431;
pub const TOOL: [R; 3] = [R::new(18, 24, 15, 15), R::new(67, 24, 15, 15), R::new(116, 24, 15, 15)];
pub const TABS: R = R::new(3, 43, 49, 352);
pub const ROWS: R = R::new(53, 44, 253, 349);
pub const ROW_H: i32 = 21;
pub const SCROLL: R = R::new(307, 44, 10, 349);
pub const SEARCH: R = R::new(209, 404, 106, 20);
pub const ORANGE: Rgba = Rgba(234, 115, 43, 255);
const FONT: Font = Font::px(12.0, false);
const STRIPE: Rgba = Rgba::gray(40);

pub struct Row<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub duration: &'a str,
    pub playing: bool,
    pub selected: bool,
}

pub struct PlaylistModel<'a> {
    pub tabs: &'a [(&'a str, bool)],
    pub rows: &'a [Row<'a>],
    /// rows[0] 在整个列表中的序号；scroll 为整个列表的像素滚动量
    pub first: usize,
    pub scroll: i32,
    pub total_rows: usize,
    pub hover_row: Option<usize>,
    pub hot_tool: Option<usize>,
    pub footer: &'a str,
    pub search: &'a str,
    pub search_focus: bool,
    pub strip_hover: f32,
}

pub fn paint(c: &mut Canvas, m: &PlaylistModel) {
    let body = frame::panel(c, m.strip_hover);
    c.push_clip(body);
    for (i, (r, cell)) in TOOL.iter().zip([icons::T_ADD, icons::T_EDIT, icons::T_SORT]).enumerate() {
        c.icon(cell, r.x, r.y, Rgba::gray(if m.hot_tool == Some(i) { 200 } else { 93 }));
    }
    c.fill(R::new(body.x, 42, body.w, 1), Rgba::gray(93));
    c.fill(R::new(TABS.right(), TABS.y, 1, TABS.h), Rgba::gray(93));
    c.fill(R::new(body.x, 395, body.w, 1), Rgba::gray(93));
    // 分栏
    for (i, (name, active)) in m.tabs.iter().enumerate() {
        let y = TABS.y + 4 + i as i32 * 26;
        let col = if *active { ORANGE } else { Rgba::gray(180) };
        if *active {
            // 选中分栏：左亮右暗的底色 + 下方淡出的分隔线
            for x in TABS.x..TABS.right() {
                let t = (x - TABS.x) as f32 / TABS.w as f32;
                c.fill(R::new(x, y - 3, 1, 22), Rgba::gray(255).alpha((26.0 * (1.0 - t)) as u8));
            }
            c.icon(icons::HEADPHONE, 8, y + 2, col);
        }
        c.text(name, FONT, 25, y + 1, col, Hint::ClearType);
        c.fill(R::new(TABS.x + 6, y + 24, TABS.w - 12, 1), Rgba::gray(44));
    }
    // 行
    c.push_clip(ROWS);
    for (i, row) in m.rows.iter().enumerate() {
        let idx = m.first + i;
        let y = ROWS.y + idx as i32 * ROW_H - m.scroll;
        let r = R::new(ROWS.x, y, ROWS.w, ROW_H);
        if row.selected {
            c.fill(r, Rgba::gray(75));
        } else if idx % 2 == 1 {
            c.fill(r, STRIPE);
        }
        let hover = m.hover_row == Some(idx);
        let col = if row.playing { ORANGE } else { Rgba::gray(224) };
        let dur_w = text::measure(row.duration, FONT).0 as i32;
        let tx = ROWS.x + 20;
        if hover {
            c.icon(icons::ROW_PLAY, ROWS.x + 2, y + 3, Rgba::gray(200));
        }
        c.push_clip(R::new(ROWS.x, y, ROWS.w - dur_w - 8, ROW_H));
        let w = c.text(row.title, FONT, tx, y + 3, col, Hint::ClearType);
        let dot = tx + w + 8;
        c.round_rect(R::new(dot, y + 9, 4, 4), 2.0, Some(if row.playing { Rgba(150, 80, 40, 255) } else { Rgba::gray(73) }), None);
        c.text(row.artist, FONT, dot + 11, y + 3, col, Hint::ClearType);
        c.pop_clip();
        if hover {
            c.icon(icons::CROSS, ROWS.right() - 17, y + 3, Rgba::gray(150));
        } else {
            c.text(row.duration, FONT, ROWS.right() - dur_w - 1, y + 3, col, Hint::ClearType);
        }
    }
    c.pop_clip();
    scrollbar(c, m);
    c.text(m.footer, FONT, 8, 406, Rgba::gray(150), Hint::ClearType);
    c.round_rect(SEARCH, 9.0, Some(Rgba::gray(26)), Some(Rgba::gray(109)));
    if m.search.is_empty() && !m.search_focus && m.strip_hover > 0.5 {
        c.text("按名称搜索", FONT, SEARCH.x + 12, SEARCH.y + 3, Rgba::gray(110), Hint::ClearType);
    } else {
        c.push_clip(SEARCH.shrink(2));
        c.text(m.search, FONT, SEARCH.x + 9, SEARCH.y + 3, Rgba::gray(230), Hint::ClearType);
        c.pop_clip();
    }
    c.icon(icons::FIND, SEARCH.right() - 18, SEARCH.y + 3, Rgba::gray(150));
    c.pop_clip();
}

fn scrollbar(c: &mut Canvas, m: &PlaylistModel) {
    let content = m.total_rows as i32 * ROW_H;
    if content <= ROWS.h {
        return;
    }
    let th = (ROWS.h * ROWS.h / content).max(20);
    let max_scroll = content - ROWS.h;
    let ty = SCROLL.y + 1 + (m.scroll * (SCROLL.h - th - 2)) / max_scroll.max(1);
    c.round_rect(R::new(SCROLL.x + 2, ty, 6, th), 3.0, Some(Rgba::gray(132)), None);
}
