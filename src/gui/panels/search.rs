//! 网络电台窗口用的输入框、按钮。

use crate::gui::theme;
use crate::gui::canvas::{Canvas, R};
use crate::gui::text::{self, Font, Hint};

const FONT: Font = Font::px(12.0, false);

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
