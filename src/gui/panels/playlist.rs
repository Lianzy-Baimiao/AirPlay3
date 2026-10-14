//! 音乐列表面板（原版 324×431）：工具栏 + ✎ ⇅、左侧列表分栏、21 像素行（标题 • 艺人、右侧时长）、底部计数与搜索框。
//! 配置里打开网络电台后，标题条上有「音乐列表 / 网络电台」两个页签；电台页签里的分栏是用户粘贴的电台清单。

use super::frame;
use crate::gui::theme;
use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::text::{self, Font, Hint};
use crate::skin::icons;

pub const W: i32 = 324;
pub const H: i32 = 431;
/// 外框尺寸范围：原版主体 260×204 ～ 800×800，加上标题条与边（宽 +4、高 +21）
pub const MIN: (i32, i32) = (264, 225);
pub const MAX: (i32, i32) = (804, 821);
pub const TOOL: [R; 3] = [R::new(18, 24, 15, 15), R::new(67, 24, 15, 15), R::new(116, 24, 15, 15)];
pub const ROW_H: i32 = 21;
/// 电台页签的工具栏：「+ 粘贴电台清单」（图标和文字一起点）
pub const PASTE: R = R::new(14, 22, 110, 19);
const STRIP_TABS: [&str; 2] = ["音乐列表", "网络电台"];

/// 标题条上两个页签的位置（只在打开了网络电台时有）
pub fn strip_tabs() -> [R; 2] {
    let w0 = text::measure(STRIP_TABS[0], FONT).0 as i32;
    let w1 = text::measure(STRIP_TABS[1], FONT).0 as i32;
    [R::new(8, 2, w0 + 12, 18), R::new(8 + w0 + 18, 2, w1 + 12, 18)]
}

/// 左边一栏（原版）的宽度；电台清单的名字长，那一页按最长的名字加宽（见 [`tab_width`]）
pub const TAB_W: i32 = 49;
/// 左边一栏每一项的高度
pub const TAB_H: i32 = 26;

/// 随窗口尺寸变化的布局（默认 324×431 时与原版一致）：左边一栏一直到底，
/// 底部的计数和搜索框在它右边（照原版量的）
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
    pub fn of(size: (i32, i32)) -> Layout {
        Layout::with_tabs(size, TAB_W)
    }

    /// 左边一栏宽 tab_w 的布局
    pub fn with_tabs((w, h): (i32, i32), tab_w: i32) -> Layout {
        let x = 3 + tab_w + 1;
        Layout {
            w,
            h,
            tabs: R::new(3, 43, tab_w, h - 47),
            rows: R::new(x, 44, w - 18 - x, h - 72),
            scroll: R::new(w - 17, 44, 10, h - 72),
            search: R::new(w - 111, h - 24, 103, 19),
            footer_y: h - 27,
        }
    }

    /// 底部计数（及提示）文字的位置和最大宽度（不压到搜索框）
    pub fn footer(&self) -> (i32, i32, i32) {
        let x = self.tabs.right() + 6;
        (x, self.footer_y + 5, self.search.x - 6 - x)
    }

    /// 左边一栏能滚动的最大距离（项太多、放不下时）
    pub fn max_tab_scroll(&self, n: usize) -> i32 {
        (n as i32 * TAB_H + 4 - self.tabs.h).max(0)
    }

    /// 左边一栏里 y 处是第几项（tab_scroll 为这一栏的滚动量）；不在任何一项上时为 None
    pub fn tab_at(&self, y: i32, tab_scroll: i32, n: usize) -> Option<usize> {
        if !self.tabs.contains(self.tabs.x, y) {
            return None;
        }
        let i = (y - self.tabs.y - 1 + tab_scroll).div_euclid(TAB_H);
        (i >= 0 && (i as usize) < n).then_some(i as usize)
    }

    /// 让第 i 项整个露出来的滚动量
    pub fn reveal_tab(&self, i: usize, tab_scroll: i32, n: usize) -> i32 {
        let top = i as i32 * TAB_H;
        let s = tab_scroll.min(top).max(top + TAB_H + 4 - self.tabs.h);
        s.clamp(0, self.max_tab_scroll(n))
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
/// 左边一栏里名字的位置（相对这一栏的左边；前面留着选中项的耳机图标）和右边留的空
const TAB_TEXT_X: i32 = 22;
const TAB_TEXT_PAD: i32 = 3;

/// 左边一栏放得下最长的名字要多宽（不比原版窄；最多占面板宽度的三分之一，再长的名字末尾用「…」）
pub fn tab_width<'a>(names: impl IntoIterator<Item = &'a str>, panel_w: i32) -> i32 {
    let longest = names.into_iter().map(|n| text::measure(n, FONT).0.ceil() as i32).max().unwrap_or(0);
    (TAB_TEXT_X + longest + TAB_TEXT_PAD).clamp(TAB_W, (panel_w / 3).max(TAB_W))
}

/// 太长的文字截短，末尾加「…」，不超过 max_w
pub fn ellipsize(s: &str, max_w: i32) -> String {
    if text::measure(s, FONT).0 <= max_w as f32 {
        return s.to_string();
    }
    let mut v: Vec<char> = s.chars().collect();
    while !v.is_empty() {
        v.pop();
        let t: String = v.iter().collect::<String>() + "…";
        if text::measure(&t, FONT).0 <= max_w as f32 {
            return t;
        }
    }
    String::new()
}

/// 搜索框里文字能用的宽度（右边是放大镜 / 清除图标）
fn search_room(r: R) -> i32 {
    r.w - 9 - 20
}

/// 搜索框里文字的起点：字太长时整体左移，让末尾（光标）留在框里
fn search_text_x(r: R, s: &str) -> i32 {
    let tw = text::measure(s, FONT).0.round() as i32;
    r.x + 9 - (tw - search_room(r)).max(0)
}

/// 搜索框里光标的左上角（光标 2×13，在文字末尾）
pub fn search_caret(r: R, s: &str) -> (i32, i32) {
    let tw = text::measure(s, FONT).0.round() as i32;
    (search_text_x(r, s) + tw, r.y + 3)
}

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
    /// 标题条上的页签：None = 没打开网络电台；Some(true) = 现在是电台页签
    pub strip_tabs: Option<bool>,
    /// 电台页签：「粘贴电台清单」在鼠标下、还一个电台都没有（显示怎么用）
    pub paste_hot: bool,
    pub radio_empty: bool,
    /// 左边一栏的宽度（[`TAB_W`] 或 [`tab_width`]）和它的滚动量
    pub tab_w: i32,
    pub tab_scroll: i32,
    /// 鼠标在搜索框上；输入时光标现在亮着（一闪一闪）
    pub search_hover: bool,
    pub caret_on: bool,
}

pub fn paint(c: &mut Canvas, m: &PlaylistModel) {
    let lay = Layout::with_tabs((c.w, c.h), m.tab_w);
    let (rows_r, tabs_r, search_r) = (lay.rows, lay.tabs, lay.search);
    let body = frame::panel(c, m.strip_hover);
    if let Some(radio) = m.strip_tabs {
        for (i, r) in strip_tabs().iter().enumerate() {
            let on = (i == 1) == radio;
            let col = if on { Rgba::gray(235) } else { Rgba::gray(150) };
            c.glow_text(STRIP_TABS[i], FONT, r.x + 6, r.y + 1, col, frame::TITLE_HALO, 2);
            if on {
                c.fill(R::new(r.x + 6, r.bottom() - 2, r.w - 12, 1), theme::focus());
            }
        }
    }
    c.push_clip(body);
    if m.strip_tabs == Some(true) {
        let col = theme::g(if m.paste_hot { 200 } else { 120 });
        c.icon(icons::T_ADD, TOOL[0].x, TOOL[0].y, col);
        c.text("粘贴电台清单", FONT, TOOL[0].x + 21, TOOL[0].y, col, Hint::ClearType);
    } else {
        for (i, (r, cell)) in TOOL.iter().zip([icons::T_ADD, icons::T_EDIT, icons::T_SORT]).enumerate() {
            c.icon(cell, r.x, r.y, theme::g(if m.hot_tool == Some(i) { 200 } else { 93 }));
        }
    }
    c.fill(R::new(body.x, 42, body.w, 1), theme::g(93));
    c.fill(R::new(tabs_r.right(), tabs_r.y, 1, tabs_r.h), theme::g(93));
    c.fill(R::new(tabs_r.right(), lay.footer_y, body.right() - tabs_r.right(), 1), theme::g(93));
    // 分栏（项太多时这一栏可以滚动，不会画到别处）
    c.push_clip(tabs_r);
    for (i, (name, active)) in m.tabs.iter().enumerate() {
        let y = tabs_r.y + 4 + i as i32 * TAB_H - m.tab_scroll;
        if y + TAB_H < tabs_r.y || y - 3 > tabs_r.bottom() {
            continue;
        }
        let col = if *active { theme::focus() } else { theme::g(180) };
        if *active {
            // 选中分栏：左亮右暗的底色 + 下方淡出的分隔线
            for x in tabs_r.x..tabs_r.right() {
                let t = (x - tabs_r.x) as f32 / tabs_r.w as f32;
                c.fill(R::new(x, y - 3, 1, 22), theme::g(255).alpha((26.0 * (1.0 - t)) as u8));
            }
            c.icon(icons::HEADPHONE, tabs_r.x + 5, y + 2, col);
        }
        let x = tabs_r.x + TAB_TEXT_X;
        c.text(&ellipsize(name, tabs_r.right() - TAB_TEXT_PAD - x), FONT, x, y + 1, col, Hint::ClearType);
        c.fill(R::new(tabs_r.x + 6, y + 24, tabs_r.w - 12, 1), theme::g(44));
    }
    // 放不下一整项以上时，这一栏右边一条细的滑块指出现在的位置（只差几个像素时不画，滚轮照样能滚）
    let max = lay.max_tab_scroll(m.tabs.len());
    if max >= TAB_H {
        let th = (tabs_r.h * tabs_r.h / (tabs_r.h + max)).max(16);
        let ty = tabs_r.y + 2 + m.tab_scroll.clamp(0, max) * (tabs_r.h - th - 4) / max;
        c.round_rect(R::new(tabs_r.right() - 4, ty, 3, th), 1.5, Some(theme::g(110)), None);
    }
    c.pop_clip();
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
    // 电台页签还一个电台都没有：说明怎么加（程序里没有电台地址）
    if m.radio_empty {
        let lines = [
            ("复制电台清单后点上面的「粘贴电台清单」", theme::g(170)),
            ("（M3U、PLS，或每行「名字,地址」）", theme::g(130)),
            ("本程序不含任何电台地址", theme::g(100)),
        ];
        c.push_clip(rows_r);
        for (i, (s, col)) in lines.iter().enumerate() {
            let tw = text::measure(s, FONT).0 as i32;
            c.text(s, FONT, rows_r.x + (rows_r.w - tw).max(0) / 2, rows_r.y + rows_r.h / 2 - 34 + i as i32 * 22, *col, Hint::ClearType);
        }
        c.pop_clip();
    }
    scrollbar(c, m, &lay);
    let (fx, fy, fw) = lay.footer();
    c.text(&ellipsize(m.footer, fw), FONT, fx, fy, theme::g(143), Hint::ClearType);
    search_box(c, search_r, m);
    c.pop_clip();
}

/// 搜索框（照原版量的几种样子）：
/// 平时只有一圈暗边；鼠标在面板上时显示「按名称搜索」；鼠标在框上、或正在输入时框亮起来（有底色）；
/// 正在输入时提示变暗、光标闪；有字时放大镜换成「清除」
fn search_box(c: &mut Canvas, r: R, m: &PlaylistModel) {
    let lit = m.search_focus || m.search_hover;
    let has = !m.search.is_empty();
    c.round_rect(r, r.h as f32 / 2.0, lit.then(|| theme::g(57)), Some(theme::g(if lit { 110 } else { 101 })));
    if has {
        c.push_clip(R::new(r.x + 2, r.y, search_room(r) + 7, r.h));
        c.text(m.search, FONT, search_text_x(r, m.search), r.y + 2, theme::g(if lit { 255 } else { 224 }), Hint::ClearType);
        c.pop_clip();
    } else if m.search_focus || m.strip_hover > 0.5 {
        let col = match (m.search_focus, m.search_hover) {
            (true, true) => 89,
            (true, false) => 83,
            (false, true) => 169,
            (false, false) => 143,
        };
        c.text("按名称搜索", FONT, r.x + 15, r.y + 1, theme::g(col), Hint::ClearType);
    }
    if m.search_focus && m.caret_on {
        let (x, y) = search_caret(r, m.search);
        c.fill(R::new(x, y, 2, 13), theme::g(198));
    }
    let (icon, col) = if has { (icons::CLEAR, if lit { 203 } else { 197 }) } else { (icons::FIND, if lit { 129 } else { 113 }) };
    c.icon(icon, r.right() - 18, r.y + 2, theme::g(col));
}

fn scrollbar(c: &mut Canvas, m: &PlaylistModel, lay: &Layout) {
    let (ty, th) = lay.thumb(m.total_rows, m.scroll);
    if th > 0 {
        c.round_rect(R::new(lay.scroll.x + 2, ty, 6, th), 3.0, Some(theme::g(if m.thumb_hot { 170 } else { 132 })), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认尺寸下与原版量的一致：左边一栏到底、底部分隔线在 404、搜索框 213..315 × 407..425
    #[test]
    fn layout_matches_original() {
        let l = Layout::of((W, H));
        assert_eq!(l.tabs, R::new(3, 43, 49, 384));
        assert_eq!((l.tabs.right(), l.tabs.bottom()), (52, 427));
        assert_eq!(l.footer_y, 404);
        assert_eq!(l.search, R::new(213, 407, 103, 19));
        assert_eq!(l.rows, R::new(53, 44, 253, 359));
        assert_eq!(l.rows.bottom(), 403);
        assert_eq!(l.scroll.right(), l.rows.right() + 11);
        // 底部文字从左边一栏右边开始，不压到搜索框
        let (x, y, w) = l.footer();
        assert_eq!((x, y), (58, 409));
        assert!(x + w < l.search.x);
    }

    /// 左边一栏按名字加宽，但不比原版窄、不超过面板的三分之一
    #[test]
    fn tab_width_fits_names() {
        assert!(tab_width(["日本"], W) <= TAB_W + 8);
        let w = tab_width(["收藏", "中国大陆", "澳大利亚"], W);
        let need = TAB_TEXT_X + text::measure("澳大利亚", FONT).0.ceil() as i32 + TAB_TEXT_PAD;
        assert!(w > TAB_W && w == need && w <= W / 3, "{w}");
        // 算出来的宽度里，名字刚好不用截短
        assert_eq!(ellipsize("澳大利亚", w - TAB_TEXT_PAD - TAB_TEXT_X), "澳大利亚");
        assert_eq!(tab_width(["一个特别特别特别特别长的电台清单名字"], W), W / 3);
        assert_eq!(tab_width(["一个特别特别特别特别长的电台清单名字"], 120), TAB_W);
        let e = ellipsize("一个特别特别特别特别长的电台清单名字", 60);
        assert!(e.ends_with('…') && text::measure(&e, FONT).0 <= 60.0);
        assert_eq!(ellipsize("日本", 60), "日本");
    }

    /// 项太多时左边一栏能滚动：点到的项、让某一项露出来
    #[test]
    fn tab_scroll_and_hit() {
        let l = Layout::of((W, H));
        assert_eq!(l.max_tab_scroll(2), 0);
        let n = 15;
        let max = l.max_tab_scroll(n);
        assert_eq!(max, 15 * TAB_H + 4 - 384);
        assert_eq!(l.tab_at(44, 0, n), Some(0));
        assert_eq!(l.tab_at(43 + 1 + TAB_H, 0, n), Some(1));
        assert_eq!(l.tab_at(44, TAB_H, n), Some(1));
        assert_eq!(l.tab_at(30, 0, n), None);
        assert_eq!(l.tab_at(44 + 3 * TAB_H, 0, 2), None);
        assert_eq!(l.reveal_tab(14, 0, n), max);
        assert_eq!(l.reveal_tab(0, max, n), 0);
        assert_eq!(l.reveal_tab(5, 3, n), 3);
    }

    /// 光标在文字末尾；字太长时文字左移，光标还在框里
    #[test]
    fn caret_stays_in_box() {
        let r = Layout::of((W, H)).search;
        assert_eq!(search_caret(r, ""), (r.x + 9, r.y + 3));
        let long = "很长很长很长很长很长很长很长的搜索";
        let (x, _) = search_caret(r, long);
        assert!(x <= r.x + 9 + search_room(r) && x > r.x + 9, "{x}");
    }
}
