//! 歌词搜索窗口（原版的「下载歌词」）：标题、艺人两个输入框，搜索结果列表，下载 / 关闭按钮。

use super::frame;
use crate::gui::theme;
use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::text::{self, Font, Hint};

pub const W: i32 = 424;
pub const H: i32 = 324;
pub const FIELD_TITLE: R = R::new(52, 28, 150, 22);
pub const FIELD_ARTIST: R = R::new(246, 28, 110, 22);
pub const BTN_SEARCH: R = R::new(362, 28, 52, 22);
pub const LIST: R = R::new(10, 58, 404, 220);
pub const ROW_H: i32 = 22;
pub const BTN_DOWNLOAD: R = R::new(286, 288, 62, 24);
pub const BTN_CLOSE: R = R::new(352, 288, 62, 24);
const FONT: Font = Font::px(12.0, false);

pub struct ResultRow<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    pub duration: &'a str,
    /// 来源（QQ / 网易 / 酷狗 / LRCLIB）
    pub source: crate::lyrics::online::Provider,
}

pub struct SearchModel<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    /// 0 = 标题，1 = 艺人
    pub focus: Option<usize>,
    pub rows: &'a [ResultRow<'a>],
    pub selected: Option<usize>,
    pub hover: Option<usize>,
    pub scroll: i32,
    pub status: &'a str,
    pub hot_button: Option<usize>,
    pub can_download: bool,
}

pub fn field(c: &mut Canvas, r: R, s: &str, focused: bool) {
    c.round_rect(r, 3.0, Some(theme::g(22)), Some(theme::g(if focused { 150 } else { 90 })));
    c.push_clip(r.shrink(3));
    let tw = text::measure(s, FONT).0 as i32;
    // 文字过长时显示末尾
    let x = (r.x + 6).min(r.right() - 8 - tw);
    c.text(s, FONT, x, r.y + 4, theme::g(232), Hint::ClearType);
    if focused {
        c.fill(R::new(x + tw + 1, r.y + 5, 1, r.h - 10), theme::g(220));
    }
    c.pop_clip();
}

pub fn button(c: &mut Canvas, r: R, label: &str, enabled: bool, hot: bool) {
    let (fill, edge, col) = match (enabled, hot) {
        (true, true) => (theme::g(85), theme::g(130), theme::g(250)),
        (true, false) => (theme::g(69), theme::g(117), theme::g(230)),
        _ => (theme::g(40), theme::g(52), theme::g(90)),
    };
    c.fill(r, edge);
    c.fill(r.shrink(1), fill);
    let tw = text::measure(label, FONT).0 as i32;
    c.text(label, FONT, r.x + (r.w - tw) / 2, r.y + (r.h - 16) / 2 + 1, col, Hint::ClearType);
}

pub fn paint(c: &mut Canvas, m: &SearchModel) {
    let body = frame::panel(c, 0.0);
    c.glow_text("搜索歌词", FONT, 10, 4, Rgba::gray(235), frame::TITLE_HALO, 2);
    c.push_clip(body);
    c.text("标题", FONT, 18, 31, theme::g(170), Hint::ClearType);
    field(c, FIELD_TITLE, m.title, m.focus == Some(0));
    c.text("艺人", FONT, 212, 31, theme::g(170), Hint::ClearType);
    field(c, FIELD_ARTIST, m.artist, m.focus == Some(1));
    button(c, BTN_SEARCH, "搜索", true, m.hot_button == Some(0));
    c.fill(LIST, theme::g(26));
    c.push_clip(LIST);
    for (i, r) in m.rows.iter().enumerate() {
        let y = LIST.y + i as i32 * ROW_H - m.scroll;
        if y + ROW_H < LIST.y || y > LIST.bottom() {
            continue;
        }
        let row = R::new(LIST.x, y, LIST.w, ROW_H);
        if m.selected == Some(i) {
            c.fill(row, theme::g(75));
        } else if m.hover == Some(i) {
            c.fill(row, theme::g(48));
        } else if i % 2 == 1 {
            c.fill(row, theme::g(34));
        }
        use crate::lyrics::online::Provider;
        let col = match r.source {
            Provider::LrcApi => Rgba(60, 150, 240, 255),
            _ => theme::g(150),
        };
        c.text(r.source.tag(), FONT, LIST.x + 6, y + 3, col, Hint::ClearType);
        c.push_clip(R::new(LIST.x + 52, y, LIST.w - 102, ROW_H));
        let w = c.text(r.title, FONT, LIST.x + 52, y + 3, theme::g(232), Hint::ClearType);
        let sub = if r.album.is_empty() { r.artist.to_string() } else { format!("{} · {}", r.artist, r.album) };
        c.text(&sub, FONT, LIST.x + 64 + w, y + 3, theme::g(140), Hint::ClearType);
        c.pop_clip();
        let dw = text::measure(r.duration, FONT).0 as i32;
        c.text(r.duration, FONT, LIST.right() - dw - 8, y + 3, theme::g(170), Hint::ClearType);
    }
    c.pop_clip();
    c.text(m.status, FONT, 12, 292, theme::g(150), Hint::ClearType);
    button(c, BTN_DOWNLOAD, "下载", m.can_download, m.hot_button == Some(1));
    button(c, BTN_CLOSE, "关闭", true, m.hot_button == Some(2));
    c.pop_clip();
}
