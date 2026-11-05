//! 新建 / 重命名列表，复用配置窗口的分层画布、阴影、主题色和按钮。

use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use super::app::App;
use super::canvas::{Canvas, R};
use super::panels::lyricdl::{SHADOW, button_rgba, shadow, soft_rect};
use super::popup::Popup;
use super::text::{self, Font, Hint};
use super::theme;
use crate::library::ListId;

const FONT: Font = Font::px(12.0, false);
const BODY: R = R::new(SHADOW, SHADOW, 340, 154);
const FIELD: R = R::new(SHADOW + 20, SHADOW + 55, 300, 25);
const OK: R = R::new(SHADOW + 166, SHADOW + 113, 74, 23);
const CANCEL: R = R::new(SHADOW + 246, SHADOW + 113, 74, 23);
const CLOSE: R = R::new(SHADOW + 314, SHADOW + 7, 18, 18);
const W: i32 = BODY.w + 2 * SHADOW;
const H: i32 = BODY.h + 2 * SHADOW;

struct Visual<'a> {
    rename: bool,
    text: &'a str,
    focus: usize,
    hot: Option<usize>,
    active: bool,
    text_x: i32,
    selection: (i32, i32),
    caret: Option<i32>,
}

fn draw(c: &mut Canvas, m: &Visual<'_>) {
    c.clear();
    shadow(c, BODY);
    c.fill(BODY, theme::g(75));
    c.fill(BODY.shrink(1), theme::g(32));
    let title = if m.rename { "重命名播放列表" } else { "新建播放列表" };
    let tw = text::measure(title, FONT).0 as i32;
    c.text(title, FONT, BODY.x + (BODY.w - tw) / 2, BODY.y + 10, theme::g(215), Hint::ClearType);
    c.text("×", Font::px(14.0, false), CLOSE.x + 4, CLOSE.y, theme::g(if m.hot == Some(3) { 235 } else { 140 }), Hint::ClearType);
    c.text("列表名称", FONT, FIELD.x, FIELD.y - 21, theme::g(175), Hint::ClearType);
    soft_rect(c, FIELD, if m.focus == 0 && m.active { theme::focus().mix(theme::g(90), 0.45) } else { theme::g(90) }, 0.7);
    c.fill(FIELD.shrink(1), theme::g(24));
    c.push_clip(FIELD.shrink(3));
    let (left, right) = m.selection;
    if right > left && m.focus == 0 {
        c.fill(R::new(m.text_x + left, FIELD.y + 5, right - left, 15), theme::focus().alpha(if m.active { 75 } else { 35 }));
    }
    c.text(m.text, FONT, m.text_x, FIELD.y + 5, theme::g(224), Hint::ClearType);
    if let Some(pos) = m.caret { c.fill(R::new(m.text_x + pos, FIELD.y + 5, 1, 15), theme::focus()); }
    c.pop_clip();
    c.text("Enter 确认 · Esc 取消", FONT, FIELD.x, FIELD.bottom() + 8, theme::g(110), Hint::ClearType);
    for (i, r, label) in [(1, OK, if m.rename { "保存" } else { "创建" }), (2, CANCEL, "取消")] {
        let enabled = i != 1 || !m.text.trim().is_empty();
        let hot = m.hot == Some(i) || (m.focus == i && m.active);
        button_rgba(c, r, label, [theme::g(25), if hot && enabled { theme::focus() } else { theme::g(117) },
            theme::g(if hot && enabled { 72 } else { 55 }), theme::g(if !enabled { 100 } else if hot { 245 } else { 205 })]);
    }
}

/// 索引始终位于 UTF-8 字符边界，鼠标、键盘和粘贴共用同一份编辑状态。
struct Edit {
    text: String,
    caret: usize,
    anchor: usize,
    high_surrogate: Option<u16>,
}
impl Edit {
    fn new(text: &str) -> Self { Self { text: text.into(), caret: text.len(), anchor: 0, high_surrogate: None } }
    fn range(&self) -> std::ops::Range<usize> { self.anchor.min(self.caret)..self.anchor.max(self.caret) }
    fn move_to(&mut self, to: usize, extend: bool) {
        self.caret = to;
        if !extend { self.anchor = to; }
    }
    fn previous(&self) -> usize { self.text[..self.caret].char_indices().last().map_or(0, |(i, _)| i) }
    fn next(&self) -> usize { self.caret + self.text[self.caret..].chars().next().map_or(0, char::len_utf8) }
    fn insert(&mut self, value: &str) {
        let range = self.range();
        let room = 255usize.saturating_sub(self.text.chars().count() - self.text[range.clone()].chars().count());
        let value: String = value.chars().filter(|c| !c.is_control()).take(room).collect();
        self.text.replace_range(range.clone(), &value);
        self.move_to(range.start + value.len(), false);
    }
    fn delete(&mut self, backwards: bool) {
        if self.range().is_empty() { self.anchor = if backwards { self.previous() } else { self.next() }; }
        self.insert("");
    }
    fn character(&mut self, unit: u16) {
        if (0xd800..=0xdbff).contains(&unit) { self.high_surrogate = Some(unit); return; }
        let ch = if (0xdc00..=0xdfff).contains(&unit) {
            self.high_surrogate.take().and_then(|hi| char::from_u32(0x10000 + ((hi as u32 - 0xd800) << 10) + unit as u32 - 0xdc00))
        } else {
            self.high_surrogate = None;
            char::from_u32(unit as u32)
        };
        if let Some(c) = ch.filter(|c| !c.is_control()) { self.insert(&c.to_string()); }
    }
}

pub struct NamePrompt {
    pub win: Popup,
    id: Option<ListId>,
    edit: Edit,
    /// 0 输入框、1 确认、2 取消。
    focus: usize,
    hot: Option<usize>,
    active: bool,
    drag: Option<(i32, i32)>,
    selecting: bool,
    composing: bool,
    caret_t0: Instant,
    caret_drawn: bool,
    offset: i32,
}
impl NamePrompt {
    #[cfg(test)]
    pub(super) fn offscreen(id: Option<ListId>, initial: &str) -> Self {
        Self { win: Popup::offscreen(W, H), id, edit: Edit::new(initial), focus: 0, hot: None,
            active: false, drag: None, selecting: false, composing: false, caret_t0: Instant::now(), caret_drawn: false, offset: 0 }
    }
    fn caret_on(&self) -> bool { self.active && self.focus == 0 && self.caret_t0.elapsed().as_millis() % 1000 < 500 }
    fn text_x(&self) -> i32 { FIELD.x + 7 - self.offset }
    fn width(&self, end: usize) -> i32 { text::measure(&self.edit.text[..end], FONT).0.round() as i32 }
    fn index_at(&self, x: i32) -> usize {
        let x = x - self.text_x();
        let mut prev = 0;
        for (i, ch) in self.edit.text.char_indices() {
            let width = self.width(i + ch.len_utf8());
            if x < (prev + width) / 2 { return i; }
            prev = width;
        }
        self.edit.text.len()
    }
    fn paint(&mut self) {
        let pos = self.width(self.edit.caret);
        self.offset = self.offset.min(pos).max(pos - (FIELD.w - 16)).max(0);
        let x = self.text_x();
        let range = self.edit.range();
        let (left, right) = (self.width(range.start), self.width(range.end));
        let on = self.caret_on();
        self.caret_drawn = on;
        let c = &mut self.win.canvas;
        draw(c, &Visual { rename: self.id.is_some(), text: &self.edit.text, focus: self.focus, hot: self.hot,
            active: self.active, text_x: x, selection: (left, right), caret: on.then_some(pos) });
        #[cfg(debug_assertions)]
        c.dump("playlist-name");
        self.win.present();
        // 系统光标只定位中文输入法候选框，可见光标由画布绘制。
        if self.focus == 0 && self.active {
            unsafe { let _ = SetCaretPos(super::scale::to_px(x + pos), super::scale::to_px(FIELD.y + 5)); }
        }
    }
}

impl App {
    pub(super) fn open_list_name(&mut self, id: Option<ListId>, initial: &str) {
        self.set_search_focus(false);
        if let Some(n) = &self.name_prompt { n.win.show(true); return; }
        let (x, y) = Popup::centered(W, H);
        let win = Popup::new(x, y, W, H, Some(self.bar_hwnd()), if id.is_some() { "重命名播放列表" } else { "新建播放列表" });
        super::window::set_frost(win.hwnd, false);
        self.name_prompt = Some(NamePrompt { win, id, edit: Edit::new(initial), focus: 0, hot: None,
            active: true, drag: None, selecting: false, composing: false, caret_t0: Instant::now(), caret_drawn: true, offset: 0 });
        self.paint_list_name();
        if let Some(n) = &self.name_prompt {
            n.win.show(true);
            unsafe {
                let _ = SetFocus(Some(n.win.hwnd));
                let _ = CreateCaret(n.win.hwnd, None, 1, super::scale::to_px(15));
            }
        }
        self.paint_list_name();
    }
    pub(super) fn paint_list_name(&mut self) { if let Some(n) = self.name_prompt.as_mut() { n.paint(); } }
    pub(super) fn tick_list_name(&mut self) {
        if let Some(n) = self.name_prompt.as_mut() {
            if n.caret_drawn != n.caret_on() { n.win.dirty = true; }
        }
    }
    pub(super) fn close_list_name(&mut self) {
        if let Some(mut n) = self.name_prompt.take() {
            unsafe {
                if n.active { let _ = DestroyCaret(); }
                let _ = ReleaseCapture();
            }
            n.win.close();
        }
    }
    fn confirm_list_name(&mut self) {
        let Some(n) = self.name_prompt.as_ref().filter(|n| !n.edit.text.trim().is_empty()) else { return };
        let (id, name) = (n.id, n.edit.text.clone());
        self.close_list_name();
        self.finish_list_name(id, &name);
    }
    pub(super) fn list_name_msg(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if self.name_prompt.as_ref()?.win.hwnd != hwnd { return None; }
        let (x, y) = (super::scale::lx(lp), super::scale::ly(lp));
        let n = self.name_prompt.as_mut()?;
        let hit = [FIELD, OK, CANCEL, CLOSE].iter().position(|r| r.contains(x, y));
        match msg {
            // 组合输入交给系统输入法，选候选词时的 Enter / Esc 不提交或关闭弹框。
            WM_IME_STARTCOMPOSITION => { n.composing = true; return None; }
            WM_IME_ENDCOMPOSITION => { n.composing = false; return None; }
            WM_KEYDOWN if n.composing => return None,
            WM_SETFOCUS => {
                n.active = true;
                n.caret_t0 = Instant::now();
                unsafe { let _ = CreateCaret(hwnd, None, 1, super::scale::to_px(15)); }
            }
            WM_KILLFOCUS => { n.active = false; unsafe { let _ = DestroyCaret(); } }
            WM_MOUSEMOVE => {
                if let Some((dx, dy)) = n.drag {
                    let (cx, cy) = super::resize::cursor();
                    n.win.move_to(cx - dx, cy - dy);
                } else if n.selecting { n.edit.move_to(n.index_at(x), true); }
                n.hot = hit;
                self.track_leave(hwnd);
            }
            0x02A3 => n.hot = None,
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                n.caret_t0 = Instant::now();
                match hit {
                    Some(0) => {
                        n.focus = 0;
                        n.edit.move_to(n.index_at(x), unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0);
                        if msg == WM_LBUTTONDBLCLK { n.edit.anchor = 0; n.edit.caret = n.edit.text.len(); }
                        n.selecting = true;
                        unsafe { SetCapture(hwnd); }
                    }
                    Some(1) => self.confirm_list_name(),
                    Some(2 | 3) => self.close_list_name(),
                    _ if BODY.contains(x, y) => {
                        let (cx, cy) = super::resize::cursor();
                        n.drag = Some((cx - n.win.pos.0, cy - n.win.pos.1));
                        unsafe { SetCapture(hwnd); }
                    }
                    _ => {}
                }
            }
            WM_LBUTTONUP => { n.drag = None; n.selecting = false; unsafe { let _ = ReleaseCapture(); } }
            WM_CAPTURECHANGED => { n.drag = None; n.selecting = false; }
            WM_KEYDOWN => {
                let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
                let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
                n.caret_t0 = Instant::now();
                match VIRTUAL_KEY(wp.0 as u16) {
                    VK_ESCAPE => self.close_list_name(),
                    VK_RETURN if n.focus == 2 => self.close_list_name(),
                    VK_RETURN => self.confirm_list_name(),
                    VK_TAB => n.focus = (n.focus + if shift { 2 } else { 1 }) % 3,
                    VK_SPACE if n.focus == 1 => self.confirm_list_name(),
                    VK_SPACE if n.focus == 2 => self.close_list_name(),
                    key if n.focus == 0 => match key {
                        VK_A if ctrl => { n.edit.anchor = 0; n.edit.caret = n.edit.text.len(); }
                        VK_C | VK_X if ctrl => {
                            let range = n.edit.range();
                            if !range.is_empty() {
                                super::lyricwin::set_clipboard_text(&n.edit.text[range]);
                                if key == VK_X { n.edit.insert(""); }
                            }
                        }
                        VK_V if ctrl => { if let Some(t) = super::lyricwin::clipboard_text() { n.edit.insert(t.lines().next().unwrap_or("")); } }
                        VK_LEFT => {
                            let to = if !shift && !n.edit.range().is_empty() { n.edit.range().start } else { n.edit.previous() };
                            n.edit.move_to(to, shift);
                        }
                        VK_RIGHT => {
                            let to = if !shift && !n.edit.range().is_empty() { n.edit.range().end } else { n.edit.next() };
                            n.edit.move_to(to, shift);
                        }
                        VK_HOME => n.edit.move_to(0, shift),
                        VK_END => n.edit.move_to(n.edit.text.len(), shift),
                        VK_BACK => n.edit.delete(true),
                        VK_DELETE => n.edit.delete(false),
                        _ => {}
                    },
                    _ => {}
                }
            }
            WM_CHAR if n.focus == 0 => { if wp.0 >= 32 { n.edit.character(wp.0 as u16); } n.caret_t0 = Instant::now(); }
            WM_CLOSE => self.close_list_name(),
            _ => return None,
        }
        if let Some(n) = self.name_prompt.as_mut() { n.win.dirty = true; }
        Some(LRESULT(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playlist_name_edit_handles_selection_chinese_and_surrogates() {
        let mut e = Edit::new("新建列表");
        e.insert("通勤音乐");
        e.move_to("通勤".len(), false);
        e.insert("夜间");
        e.delete(true);
        e.delete(false);
        assert_eq!(e.text, "通勤夜乐");
        e.move_to(e.text.len(), false);
        e.character(0xd83c);
        e.character(0xdfb5);
        assert_eq!(e.text, "通勤夜乐🎵");
        e.delete(true);
        assert_eq!(e.text, "通勤夜乐");
        e.anchor = 0;
        e.insert("  ");
        assert!(e.text.trim().is_empty());
        e.insert(&"歌".repeat(300));
        assert_eq!(e.text.chars().count(), 255);
    }

    /// 离屏渲染用于视觉检查，不创建或操作任何桌面窗口。
    #[test]
    #[ignore]
    fn playlist_name_render() {
        let dir = std::path::PathBuf::from(std::env::var_os("AIRPLAY3_DUMP").expect("设置渲染输出目录"));
        std::fs::create_dir_all(&dir).unwrap();
        for scale in [1.0, 1.5, 2.0] {
            for (name, rename, value, hot) in [("create", false, "新建列表", None), ("rename", true, "通勤音乐", Some(1)), ("empty", false, "", None)] {
                let mut canvas = Canvas::with_scale(W, H, scale);
                let width = text::measure(value, FONT).0 as i32;
                draw(&mut canvas, &Visual { rename, text: value, focus: 0, hot, active: true, text_x: FIELD.x + 7,
                    selection: (0, width), caret: Some(width) });
                canvas.save_png(&dir.join(format!("playlist-{name}-{}.png", (scale * 100.0) as u32))).unwrap();
            }
        }
        let original = theme::get();
        for (name, body, focus) in [
            ("dark-blue", [0x10, 0x20, 0x40], [0x64, 0xb4, 0xff]),
            ("light", [0xd0, 0xd0, 0xd0], [0x28, 0x6e, 0xbe]),
            ("rose", [0xd0, 0xb0, 0xb0], [0xa0, 0x30, 0x5a]),
        ] {
            theme::set(theme::Theme { body, focus, ..original });
            for empty in [false, true] {
                let value = if empty { "" } else { "通勤音乐" };
                let width = text::measure(value, FONT).0 as i32;
                let mut canvas = Canvas::with_scale(W, H, 1.5);
                draw(&mut canvas, &Visual { rename: true, text: value, focus: 0, hot: Some(1), active: true,
                    text_x: FIELD.x + 7, selection: (0, width), caret: Some(width) });
                canvas.save_png(&dir.join(format!("playlist-theme-{name}-{}.png", if empty { "empty" } else { "filled" }))).unwrap();
            }
        }
        theme::set(original);
    }
}
