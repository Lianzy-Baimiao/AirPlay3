//! 音乐列表面板（原版 324×431）：工具栏 + ✎ ⇅、左侧列表分栏、21 像素行（标题 • 艺人、右侧时长）、底部计数与搜索框。

use super::frame;
use crate::gui::theme;
use crate::gui::canvas::{Canvas, R};
use crate::gui::text::{self, Font, Hint};
use crate::skin::icons;

pub const W: i32 = 324;
pub const H: i32 = 431;
/// 外框尺寸范围：原版主体 260×204 ～ 800×800，加上标题条与边（宽 +4、高 +21）
pub const MIN: (i32, i32) = (264, 225);
pub const MAX: (i32, i32) = (804, 821);
pub const TOOL: [R; 3] = [R::new(18, 24, 15, 15), R::new(67, 24, 15, 15), R::new(116, 24, 15, 15)];
pub const ROW_H: i32 = 21;

/// 随窗口尺寸变化的布局（默认 324×431 时与原版一致）
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub w: i32,
    pub h: i32,
    pub tabs: R,
    pub rows: R,
    pub scroll: R,
    pub search: R,
    pub footer_y: i32,
}

impl Layout {
    pub fn of((w, h): (i32, i32)) -> Layout {
        Layout {
            w,
            h,
            tabs: R::new(3, 43, 49, h - 79),
            rows: R::new(53, 44, w - 71, h - 82),
            scroll: R::new(w - 17, 44, 10, h - 82),
            search: R::new(w - 115, h - 27, 106, 20),
            footer_y: h - 36,
        }
    }

    pub fn max_scroll(&self, n: usize) -> i32 {
        (n as i32 * ROW_H - self.rows.h).max(0)
    }

    /// 滚动条滑块（y, 高度）；不需要滚动时高度为 0
    pub fn thumb(&self, n: usize, scroll: i32) -> (i32, i32) {
        let content = n as i32 * ROW_H;
        if content <= self.rows.h {
            return (self.scroll.y, 0);
        }
        let th = (self.rows.h * self.rows.h / content).max(20);
        let max = self.max_scroll(n).max(1);
        (self.scroll.y + 1 + scroll.clamp(0, max) * (self.scroll.h - th - 2) / max, th)
    }

    pub fn row_at(&self, y: i32, scroll: i32) -> usize {
        ((y - self.rows.y + scroll) / ROW_H).max(0) as usize
    }
}

const FONT: Font = Font::px(12.0, false);

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
    pub thumb_hot: bool,
}

pub fn paint(c: &mut Canvas, m: &PlaylistModel) {
    let lay = Layout::of((c.w, c.h));
    let (rows_r, tabs_r, search_r) = (lay.rows, lay.tabs, lay.search);
    let body = frame::panel(c, m.strip_hover);
    c.push_clip(body);
    for (i, (r, cell)) in TOOL.iter().zip([icons::T_ADD, icons::T_EDIT, icons::T_SORT]).enumerate() {
        c.icon(cell, r.x, r.y, theme::g(if m.hot_tool == Some(i) { 200 } else { 93 }));
    }
    c.fill(R::new(body.x, 42, body.w, 1), theme::g(93));
    c.fill(R::new(tabs_r.right(), tabs_r.y, 1, tabs_r.h), theme::g(93));
    c.fill(R::new(body.x, lay.footer_y, body.w, 1), theme::g(93));
    // 分栏
    for (i, (name, active)) in m.tabs.iter().enumerate() {
        let y = tabs_r.y + 4 + i as i32 * 26;
        let col = if *active { theme::focus() } else { theme::g(180) };
        if *active {
            // 选中分栏：左亮右暗的底色 + 下方淡出的分隔线
            for x in tabs_r.x..tabs_r.right() {
                let t = (x - tabs_r.x) as f32 / tabs_r.w as f32;
                c.fill(R::new(x, y - 3, 1, 22), theme::g(255).alpha((26.0 * (1.0 - t)) as u8));
            }
            c.icon(icons::HEADPHONE, 8, y + 2, col);
        }
        c.text(name, FONT, 25, y + 1, col, Hint::ClearType);
        c.fill(R::new(tabs_r.x + 6, y + 24, tabs_r.w - 12, 1), theme::g(44));
    }
    // 行
    c.push_clip(rows_r);
    for (i, row) in m.rows.iter().enumerate() {
        let idx = m.first + i;
        let y = rows_r.y + idx as i32 * ROW_H - m.scroll;
        let r = R::new(rows_r.x, y, rows_r.w, ROW_H);
        if row.selected {
            c.fill(r, theme::g(75));
        } else if idx % 2 == 1 {
            c.fill(r, theme::g(40));
        }
        let hover = m.hover_row == Some(idx);
        let col = if row.playing { theme::focus() } else { theme::g(224) };
        let dur_w = text::measure(row.duration, FONT).0 as i32;
        let tx = rows_r.x + 20;
        if hover {
            c.icon(icons::ROW_PLAY, rows_r.x + 2, y + 3, theme::g(200));
        }
        c.push_clip(R::new(rows_r.x, y, rows_r.w - dur_w - 8, ROW_H));
        let w = c.text(row.title, FONT, tx, y + 3, col, Hint::ClearType);
        let dot = tx + w + 8;
        c.round_rect(R::new(dot, y + 9, 4, 4), 2.0, Some(if row.playing { theme::focus().mix(theme::body(), 0.4) } else { theme::g(73) }), None);
        c.text(row.artist, FONT, dot + 11, y + 3, col, Hint::ClearType);
        c.pop_clip();
        if hover {
            c.icon(icons::CROSS, rows_r.right() - 17, y + 3, theme::g(150));
        } else {
            c.text(row.duration, FONT, rows_r.right() - dur_w - 1, y + 3, col, Hint::ClearType);
        }
    }
    c.pop_clip();
    scrollbar(c, m, &lay);
    c.text(m.footer, FONT, 8, lay.footer_y + 11, theme::g(143), Hint::ClearType);
    c.round_rect(search_r, 9.0, Some(theme::g(26)), Some(theme::g(109)));
    if m.search.is_empty() && !m.search_focus && m.strip_hover > 0.5 {
        c.text("按名称搜索", FONT, search_r.x + 12, search_r.y + 3, theme::g(110), Hint::ClearType);
    } else {
        c.push_clip(search_r.shrink(2));
        c.text(m.search, FONT, search_r.x + 9, search_r.y + 3, theme::g(230), Hint::ClearType);
        c.pop_clip();
    }
    c.icon(icons::FIND, search_r.right() - 18, search_r.y + 3, theme::g(150));
    c.pop_clip();
}

fn scrollbar(c: &mut Canvas, m: &PlaylistModel, lay: &Layout) {
    let (ty, th) = lay.thumb(m.total_rows, m.scroll);
    if th > 0 {
        c.round_rect(R::new(lay.scroll.x + 2, ty, 6, th), 3.0, Some(theme::g(if m.thumb_hot { 170 } else { 132 })), None);
    }
}
