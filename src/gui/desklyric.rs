//! 桌面歌词：置顶的透明分层窗口，卡拉 OK 填色；可拖动、拖边调整大小（窗口越高显示的行越多），
//! 预设配色；锁定后鼠标穿透；悬停显示工具条。原版没有桌面歌词，这里按 AirPlay3 的深色极简风格设计。
//! - 1 行：当前句居中
//! - 2 行：经典双行，上一行靠左、下一行靠右交替（有翻译时第二行显示翻译）
//! - 3 行及以上：当前句在中间，前后句上下排列、越远越淡，换句时平滑滚动

use std::rc::Rc;
use std::sync::Arc;

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::app::App;
use super::canvas::{Canvas, R, Rgba};
use super::menu::{Act, Item, check, item};
use super::popup::Popup;
use super::text::{self, Font, Hint, Mask};
use crate::config::DeskLyricSettings;
use crate::lyrics::Lyrics;
use crate::skin::icons;

/// 当前歌词状态（桌面歌词与任务栏歌词共用）
#[derive(Clone, Default, PartialEq)]
pub struct LineState {
    pub idx: Option<usize>,
    pub text: String,
    pub next: String,
    pub progress: f32,
}

impl App {
    pub fn line_state(&self) -> LineState {
        let n = self.now.lock();
        let pos = self.ui.snap.pos;
        let Some(l) = n.lyrics.as_ref().filter(|l| l.synced && !l.lines.is_empty()) else {
            return LineState { idx: None, text: title_of(&n.title, &n.artist), next: String::new(), progress: 0.0 };
        };
        match n.lyric_index(pos) {
            Some(i) => {
                let line = &l.lines[i];
                let next = line.trans.clone().unwrap_or_else(|| l.lines.get(i + 1).map_or(String::new(), |x| x.text.clone()));
                LineState { idx: Some(i), text: line.text.clone(), next, progress: n.lyric_progress(i, pos) }
            }
            None => LineState { idx: None, text: title_of(&n.title, &n.artist), next: l.lines[0].text.clone(), progress: 0.0 },
        }
    }
}

/// 预设配色：名称、已唱（上 → 下渐变）、未唱（上 → 下渐变）
pub struct Style {
    pub name: &'static str,
    pub played: (Rgba, Rgba),
    pub unplayed: (Rgba, Rgba),
}

const fn c(r: u8, g: u8, b: u8) -> Rgba {
    Rgba(r, g, b, 255)
}

/// 第一个是旧版本的配色（金色已唱、蓝色未唱），老用户升级后观感不变
pub const STYLES: [Style; 8] = [
    Style { name: "经典蓝金", played: (c(255, 236, 140), c(255, 186, 52)), unplayed: (c(150, 222, 255), c(58, 160, 240)) },
    Style { name: "活力橙", played: (c(255, 214, 120), c(240, 110, 30)), unplayed: (c(255, 255, 255), c(205, 205, 205)) },
    Style { name: "清新绿", played: (c(222, 255, 160), c(110, 205, 60)), unplayed: (c(255, 255, 255), c(205, 205, 205)) },
    Style { name: "浪漫粉", played: (c(255, 205, 225), c(240, 90, 150)), unplayed: (c(255, 255, 255), c(220, 210, 214)) },
    Style { name: "冰雪蓝", played: (c(225, 250, 255), c(60, 190, 250)), unplayed: (c(185, 205, 225), c(120, 145, 175)) },
    Style { name: "典雅紫", played: (c(240, 215, 255), c(160, 90, 235)), unplayed: (c(255, 255, 255), c(205, 205, 205)) },
    Style { name: "热情红", played: (c(255, 225, 120), c(235, 60, 50)), unplayed: (c(255, 255, 255), c(205, 205, 205)) },
    Style { name: "简约白", played: (c(255, 255, 255), c(255, 255, 255)), unplayed: (c(160, 160, 160), c(125, 125, 125)) },
];

pub fn style(i: usize) -> &'static Style {
    &STYLES[i.min(STYLES.len() - 1)]
}

/// 工具条高度（悬停时显示按钮）
const BAR_H: i32 = 30;
/// 歌词区上下留白之和
const PAD: i32 = 12;
/// 歌词左右留白
const MARGIN: i32 = 24;
pub const MIN_W: i32 = 300;
const MAX_W: i32 = 3000;

fn line_h(size: f32) -> i32 {
    (size * 1.5) as i32
}

/// 显示 n 行时窗口的高度
pub fn height_for(lines: usize, size: f32) -> i32 {
    BAR_H + line_h(size) * lines.max(1) as i32 + PAD
}

/// 窗口高 h 时放得下几行（拖动窗口上下边时行数跟着变）
pub fn lines_for(h: i32, size: f32) -> usize {
    (((h - BAR_H - PAD) / line_h(size).max(1)).max(1) as usize).min(DeskLyricSettings::MAX_LINES)
}

pub struct DeskLyric {
    pub win: Popup,
    pub hover: f32,
    pub hovering: bool,
    drag: Option<Drag>,
    pub last: LineState,
    /// 多行时的滚动位置（行号，换句时平滑趋近当前行）
    scroll: f32,
    /// 缓存：(文字, 字号) → 文字遮罩与描边遮罩
    cache: Vec<(String, u16, Rc<Mask>, Rc<Mask>)>,
}

/// 拖动：移动窗口，或拖边调整大小（按下时的光标、窗口位置与大小）
#[derive(Clone, Copy)]
enum Drag {
    Move(i32, i32),
    Resize { edges: u32, cursor: (i32, i32), rect: R },
}

/// 可以拖动调整大小的边（距边缘 6 像素以内）
const EDGE: i32 = 6;
const LEFT: u32 = 1;
const RIGHT: u32 = 2;
const TOP: u32 = 4;
const BOTTOM: u32 = 8;

fn edges_at(w: i32, h: i32, x: i32, y: i32) -> u32 {
    let mut e = 0;
    if x < EDGE {
        e |= LEFT;
    } else if x >= w - EDGE {
        e |= RIGHT;
    }
    if y < EDGE {
        e |= TOP;
    } else if y >= h - EDGE {
        e |= BOTTOM;
    }
    e
}

fn edge_cursor(e: u32) -> windows::core::PCWSTR {
    match e {
        LEFT | RIGHT => IDC_SIZEWE,
        TOP | BOTTOM => IDC_SIZENS,
        x if x == LEFT | TOP || x == RIGHT | BOTTOM => IDC_SIZENWSE,
        0 => IDC_ARROW,
        _ => IDC_SIZENESW,
    }
}

/// 拖边后的新窗口：被拖的边跟着光标，对边不动；宽、高限制在范围内
fn resized(r: R, edges: u32, dx: i32, dy: i32, (min_h, max_h): (i32, i32)) -> R {
    let (mut l, mut t, mut rt, mut b) = (r.x, r.y, r.right(), r.bottom());
    if edges & LEFT != 0 {
        l = (l + dx).clamp(rt - MAX_W, rt - MIN_W);
    }
    if edges & RIGHT != 0 {
        rt = (rt + dx).clamp(l + MIN_W, l + MAX_W);
    }
    if edges & TOP != 0 {
        t = (t + dy).clamp(b - max_h, b - min_h);
    }
    if edges & BOTTOM != 0 {
        b = (b + dy).clamp(t + min_h, t + max_h);
    }
    R::new(l, t, rt - l, b - t)
}

/// 文字遮罩与描边遮罩（缓存最近用过的 cap 条）
fn masks(cache: &mut Vec<(String, u16, Rc<Mask>, Rc<Mask>)>, s: &str, size: f32, cap: usize) -> (Rc<Mask>, Rc<Mask>) {
    let key = (size * 10.0) as u16;
    if let Some(i) = cache.iter().position(|c| c.0 == s && c.1 == key) {
        // 移到末尾：一直在用的行不会被挤掉
        let e = cache.remove(i);
        let r = (e.2.clone(), e.3.clone());
        cache.push(e);
        return r;
    }
    let m = Rc::new(text::render(s, Font::px(size, true), Hint::Gray));
    let o = Rc::new(super::draw::outline(&m, 2));
    while cache.len() >= cap.max(4) {
        cache.remove(0);
    }
    cache.push((s.to_string(), key, m.clone(), o.clone()));
    (m, o)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Align {
    Left,
    Center,
    Right,
}

/// 要画的一行：文字、所在的行位（0 = 第一行，可以是小数：滚动中）、对齐、
/// 填色进度（只有当前句是 Some）、不透明度（越远越淡）
#[derive(Clone, Debug, PartialEq)]
struct Row {
    text: String,
    slot: f32,
    align: Align,
    progress: Option<f32>,
    alpha: f32,
}

fn row(text: &str, slot: f32, align: Align, progress: Option<f32>, alpha: f32) -> Row {
    Row { text: text.to_string(), slot, align, progress, alpha }
}

/// 排版：n 行时每一行画什么。`lyr` = 有时间轴的歌词（没有时只显示标题），
/// `idx` / `progress` = 当前句与填色进度，`scroll` = 多行时的滚动位置（行号）
fn rows(n: usize, lyr: Option<&Lyrics>, title: &str, idx: Option<usize>, progress: f32, scroll: f32) -> Vec<Row> {
    let Some(l) = lyr else { return vec![row(title, (n as f32 - 1.0) / 2.0, Align::Center, None, 1.0)] };
    let text = |i: usize| l.lines.get(i).map_or("", |x| x.text.as_str());
    match (n, idx) {
        (1, Some(i)) => vec![row(text(i), 0.0, Align::Center, Some(progress), 1.0)],
        (1, None) => vec![row(title, 0.0, Align::Center, None, 1.0)],
        // 两行：有翻译时第二行是翻译；否则单数句在上靠左、双数句在下靠右，交替显示
        (2, Some(i)) => match l.lines[i].trans.as_deref() {
            Some(t) => vec![row(text(i), 0.0, Align::Center, Some(progress), 1.0), row(t, 1.0, Align::Center, None, 0.8)],
            None => {
                let (cur, other) = if i % 2 == 0 { (0.0, 1.0) } else { (1.0, 0.0) };
                let al = |s: f32| if s == 0.0 { Align::Left } else { Align::Right };
                vec![row(text(i), cur, al(cur), Some(progress), 1.0), row(text(i + 1), other, al(other), None, 0.8)]
            }
        },
        (2, None) => vec![row(title, 0.0, Align::Left, None, 1.0), row(text(0), 1.0, Align::Right, None, 0.8)],
        // 多行：当前句在中间一行（偶数行时在中线上面一行，后面的句子多显示一句，不会有被切掉一半的行），
        // 前后句上下排列、越远越淡；歌词开始前，标题在第一句的位置之前
        _ => {
            let mid = ((n - 1) / 2) as f32;
            let lo = (scroll - mid).floor() as i64 - 1;
            let hi = (scroll + (n as f32 - 1.0 - mid)).ceil() as i64 + 1;
            (lo.max(-1)..=hi.min(l.lines.len() as i64 - 1))
                .map(|k| {
                    let d = k as f32 - scroll;
                    let s = if k < 0 { title } else { text(k as usize) };
                    let cur = idx.is_some_and(|i| i as i64 == k);
                    let alpha = if cur { 1.0 } else { (0.75 - d.abs() * 0.12).max(0.25) };
                    row(s, mid + d, Align::Center, cur.then_some(progress), alpha)
                })
                .collect()
        }
    }
}

/// 没有歌词（或歌词开始前）显示的标题
fn title_of(title: &str, artist: &str) -> String {
    match (title.is_empty(), artist.is_empty()) {
        (true, _) => "AirPlay3".to_string(),
        (false, true) => title.to_string(),
        _ => format!("{title} - {artist}"),
    }
}

impl App {
    /// 当前设置下窗口的大小（高度由行数和字号决定）
    fn desk_size(&self) -> (i32, i32) {
        let g = self.player.core.lock();
        let s = &g.settings.desk_lyric;
        (s.width.unwrap_or(900).clamp(MIN_W, MAX_W), height_for(s.line_count(), s.font_size))
    }

    /// 根据设置创建 / 关闭桌面歌词窗口，同步大小、位置与锁定（鼠标穿透）状态
    pub fn sync_desk_lyric(&mut self) {
        let (visible, locked, pos) = {
            let g = self.player.core.lock();
            (g.settings.desk_lyric.visible, g.settings.desk_lyric.locked, g.settings.desk_lyric.pos)
        };
        if !visible {
            if let Some(mut d) = self.desk.take() {
                d.win.close();
            }
            return;
        }
        let (w, h) = self.desk_size();
        // 保存的位置不在任何显示器上（拔掉了显示器）时回到默认位置
        let (bx, by) = self.bar_pos();
        let a = super::popup::work_area_at((bx + 100, by + 50));
        let pos = pos.map(|p| (p[0], p[1])).filter(|&(x, y)| super::app::on_screen(R::new(x, y, w, h)));
        let (x, y) = pos.unwrap_or((a.x + (a.w - w) / 2, a.bottom() - h - 60));
        if self.desk.is_none() {
            let win = Popup::new(x, y, w, h, None, "AirPlay3 桌面歌词");
            // 桌面歌词只有文字，不雾化
            super::window::set_frost(win.hwnd, false);
            unsafe {
                let ex = GetWindowLongW(win.hwnd, GWL_EXSTYLE);
                SetWindowLongW(win.hwnd, GWL_EXSTYLE, ex | WS_EX_TOPMOST.0 as i32 | WS_EX_NOACTIVATE.0 as i32);
                let _ = SetWindowPos(win.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
            self.desk = Some(DeskLyric { win, hover: 0.0, hovering: false, drag: None, last: LineState::default(), scroll: -1.0, cache: Vec::new() });
        }
        let d = self.desk.as_mut().unwrap();
        if d.drag.is_none() {
            // 行数变多、窗口变高时不超出所在显示器的下边
            let wa = super::popup::work_area_at((x + w / 2, y));
            let y = if y + h > wa.bottom() { (wa.bottom() - h).max(wa.y) } else { y };
            d.win.pos = (x, y);
            d.win.resize(w, h);
        }
        unsafe {
            let ex = GetWindowLongW(d.win.hwnd, GWL_EXSTYLE) as u32;
            let want = if locked { ex | WS_EX_TRANSPARENT.0 } else { ex & !WS_EX_TRANSPARENT.0 };
            if want != ex {
                SetWindowLongW(d.win.hwnd, GWL_EXSTYLE, want as i32);
            }
        }
        d.win.dirty = true;
        self.paint_desk();
        if let Some(d) = &self.desk {
            d.win.show(false);
        }
    }

    /// 每帧：歌词、填色进度、滚动或悬停变化时重绘
    pub fn tick_desk(&mut self) {
        if self.desk.is_none() {
            return;
        }
        let st = self.line_state();
        let d = self.desk.as_mut().unwrap();
        let hover_changed = super::state::approach(&mut d.hover, if d.hovering { 1.0 } else { 0.0 }, 0.15);
        // 换句时平滑滚动（约 0.2 秒）；跳得远（拖进度、换歌）时直接到位
        let target = st.idx.map_or(-1.0, |i| i as f32);
        if (d.scroll - target).abs() > 3.0 {
            d.scroll = target;
        }
        let scrolled = super::state::approach(&mut d.scroll, target, 0.18);
        let changed = st.idx != d.last.idx || st.text != d.last.text || (st.progress - d.last.progress).abs() > 0.004;
        if changed || hover_changed || scrolled {
            d.last = st;
            d.win.dirty = true;
            self.paint_desk();
        }
    }

    pub fn paint_desk(&mut self) {
        let (size, sty, locked) = {
            let g = self.player.core.lock();
            let s = &g.settings.desk_lyric;
            (s.font_size, style(s.style), s.locked)
        };
        let (lyr, title): (Option<Arc<Lyrics>>, String) = {
            let n = self.now.lock();
            (n.lyrics.clone().filter(|l| l.synced && !l.lines.is_empty()), title_of(&n.title, &n.artist))
        };
        let playing = self.ui.snap.playing;
        let Some(d) = self.desk.as_mut() else { return };
        let (w, h) = (d.win.canvas.w, d.win.canvas.h);
        // 行数跟着窗口当前高度走：拖边调整大小时实时增减
        let n = lines_for(h, size);
        let st = d.last.clone();
        let list = rows(n, lyr.as_deref(), &title, st.idx, st.progress, d.scroll);
        let c = &mut d.win.canvas;
        c.clear();
        // 悬停时显示半透明底与工具条（锁定时不显示）
        let hv = if locked { 0.0 } else { d.hover };
        if hv > 0.0 {
            c.round_rect(R::new(0, 0, w, h), 8.0, Some(Rgba(20, 20, 20, (150.0 * hv) as u8)), Some(Rgba(255, 255, 255, (40.0 * hv) as u8)));
            toolbar(c, w, h, hv, playing, sty);
        }
        let lh = line_h(size);
        let top = BAR_H + (h - BAR_H - lh * n as i32) / 2;
        c.push_clip(R::new(0, BAR_H, w, h - BAR_H));
        for r in &list {
            if r.text.is_empty() {
                continue;
            }
            let (m, o) = masks(&mut d.cache, &r.text, size, n * 2 + 6);
            let y = top + (r.slot * lh as f32).round() as i32 + (lh - m.h as i32) / 2;
            paint_row(c, &m, &o, r, y, sty);
        }
        c.pop_clip();
        // 未锁定时整窗可接收鼠标（拖动、调整大小）
        if !locked {
            c.hit_margin(R::new(0, 0, w, h));
        }
        d.win.present();
    }
}

/// 画一行：描边 + 渐变填色。当前句左边是已唱颜色、右边是未唱颜色；
/// 比窗口宽的当前句随进度向左滚动，保证正在唱的位置看得见
fn paint_row(c: &mut Canvas, m: &Mask, o: &Mask, r: &Row, y: i32, sty: &Style) {
    let w = c.w;
    let tw = m.w as i32;
    let room = w - MARGIN * 2;
    let x = if tw > room {
        let p = r.progress.unwrap_or(0.0);
        MARGIN - ((tw as f32 * p - room as f32 / 2.0).clamp(0.0, (tw - room) as f32)) as i32
    } else {
        match r.align {
            Align::Left => MARGIN,
            Align::Right => w - MARGIN - tw,
            Align::Center => (w - tw) / 2,
        }
    };
    let a = (r.alpha.clamp(0.0, 1.0) * 255.0) as u8;
    c.push_clip(R::new(MARGIN - 4, 0, room + 8, c.h));
    c.mask(o, x - 2, y - 2, Rgba(0, 0, 0, (200.0 * r.alpha.clamp(0.0, 1.0)) as u8));
    let (u0, u1) = (sty.unplayed.0.alpha(a), sty.unplayed.1.alpha(a));
    match r.progress {
        Some(p) => {
            let split = x + (tw as f32 * p) as i32;
            c.push_clip(R::new(x - 4, y - 4, split - x + 4, m.h as i32 + 8));
            c.mask_grad(m, x, y, sty.played.0, sty.played.1);
            c.pop_clip();
            c.push_clip(R::new(split, y - 4, w, m.h as i32 + 8));
            c.mask_grad(m, x, y, u0, u1);
            c.pop_clip();
        }
        None => c.mask_grad(m, x, y, u0, u1),
    }
    c.pop_clip();
}

/// 工具条按钮（悬停时显示在窗口顶部居中）
const BTNS: [&str; 8] = ["prev", "play", "next", "smaller", "bigger", "style", "lock", "close"];
const BTN_W: i32 = 26;

fn btn_x0(w: i32) -> i32 {
    w / 2 - BTNS.len() as i32 * BTN_W / 2
}

/// 点到了哪个工具条按钮
fn btn_at(w: i32, x: i32, y: i32) -> Option<&'static str> {
    let x0 = btn_x0(w);
    if !(0..BAR_H).contains(&y) || x < x0 - 4 || x >= x0 + BTNS.len() as i32 * BTN_W {
        return None;
    }
    Some(BTNS[((x - x0 + 4) / BTN_W).clamp(0, BTNS.len() as i32 - 1) as usize])
}

fn toolbar(c: &mut Canvas, w: i32, h: i32, hv: f32, playing: bool, sty: &Style) {
    let a = (255.0 * hv) as u8;
    let col = Rgba::gray(230).alpha(a);
    let f = Font::px(13.0, true);
    for (i, name) in BTNS.iter().enumerate() {
        let x = btn_x0(w) + i as i32 * BTN_W;
        match *name {
            "prev" => c.icon(icons::PREV, x, 7, col),
            "play" => c.icon(if playing { icons::PAUSE } else { icons::PLAY }, x, 7, col),
            "next" => c.icon(icons::NEXT, x, 7, col),
            "smaller" => {
                c.text("A-", f, x, 6, col, Hint::Gray);
            }
            "bigger" => {
                c.text("A+", f, x, 6, col, Hint::Gray);
            }
            // 配色：当前配色的已唱 / 未唱两色色块，点一下换下一种
            "style" => {
                c.round_rect(R::new(x + 1, 8, 7, 14), 2.0, Some(sty.played.1.alpha(a)), None);
                c.round_rect(R::new(x + 8, 8, 7, 14), 2.0, Some(sty.unplayed.1.alpha(a)), None);
            }
            "lock" => c.icon(icons::W_PIN, x, 7, col),
            _ => c.icon(icons::W_CLOSE, x, 7, col),
        }
    }
    // 右下角的调整大小提示
    for k in 0..3 {
        let d = 4 + k * 4;
        for t in 0..=d {
            if (t + k) % 2 == 0 {
                c.blend(w - 4 - t, h - 4 - (d - t), Rgba::gray(200), hv * 0.6);
            }
        }
    }
}
impl App {
    pub fn desk_msg(&mut self, hwnd: HWND, msg: u32, _wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if self.desk.as_ref()?.win.hwnd != hwnd {
            return None;
        }
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        match msg {
            WM_SETCURSOR => {
                let d = self.desk.as_ref()?;
                let (cx, cy) = super::resize::cursor();
                let (px, py) = (cx - d.win.pos.0, cy - d.win.pos.1);
                let (w, h) = (d.win.canvas.w, d.win.canvas.h);
                let e = if btn_at(w, px, py).is_some() { 0 } else { edges_at(w, h, px, py) };
                unsafe {
                    SetCursor(LoadCursorW(None, edge_cursor(e)).ok());
                }
                return Some(LRESULT(1));
            }
            WM_MOUSEMOVE => {
                if !self.desk.as_ref()?.hovering {
                    self.desk.as_mut()?.hovering = true;
                    self.track_leave(hwnd);
                }
                let d = self.desk.as_mut()?;
                match d.drag {
                    Some(Drag::Move(dx, dy)) => {
                        let (cx, cy) = super::resize::cursor();
                        d.win.move_to(cx - dx, cy - dy);
                    }
                    Some(Drag::Resize { edges, cursor, rect }) => {
                        let (cx, cy) = super::resize::cursor();
                        let size = self.player.core.lock().settings.desk_lyric.font_size;
                        let d = self.desk.as_mut()?;
                        let r = resized(rect, edges, cx - cursor.0, cy - cursor.1, (height_for(1, size), height_for(DeskLyricSettings::MAX_LINES, size)));
                        // 大小和位置一起交给 UpdateLayeredWindow，拖左 / 上边时不闪
                        d.win.pos = (r.x, r.y);
                        d.win.resize(r.w, r.h);
                        self.paint_desk();
                    }
                    None => {}
                }
            }
            0x02A3 => self.desk.as_mut()?.hovering = false, // WM_MOUSELEAVE
            WM_LBUTTONDOWN => {
                let (w, h) = {
                    let c = &self.desk.as_ref()?.win.canvas;
                    (c.w, c.h)
                };
                if let Some(b) = btn_at(w, x, y) {
                    match b {
                        "prev" => self.player.prev(),
                        "play" => self.player.toggle_pause(),
                        "next" => self.player.next(),
                        "smaller" => self.act(Act::DeskFont(-2.0)),
                        "bigger" => self.act(Act::DeskFont(2.0)),
                        "style" => self.act(Act::DeskStyle(None)),
                        "lock" => self.act(Act::DeskLock),
                        _ => self.act(Act::DeskToggle),
                    }
                    return Some(LRESULT(0));
                }
                let d = self.desk.as_mut()?;
                let (cx, cy) = super::resize::cursor();
                let edges = edges_at(w, h, x, y);
                d.drag = Some(if edges != 0 {
                    Drag::Resize { edges, cursor: (cx, cy), rect: R::new(d.win.pos.0, d.win.pos.1, w, h) }
                } else {
                    Drag::Move(cx - d.win.pos.0, cy - d.win.pos.1)
                });
                unsafe { SetCapture(hwnd) };
            }
            WM_LBUTTONUP => {
                let drag = self.desk.as_mut()?.drag.take();
                let Some(drag) = drag else { return Some(LRESULT(0)) };
                unsafe {
                    let _ = ReleaseCapture();
                }
                let size = self.player.core.lock().settings.desk_lyric.font_size;
                let d = self.desk.as_ref()?;
                let (mut pos, w, h) = (d.win.pos, d.win.canvas.w, d.win.canvas.h);
                // 松手时高度对齐到整行；拖的是上边时保持下边不动
                let n = lines_for(h, size);
                if let Drag::Resize { edges, .. } = drag {
                    if edges & TOP != 0 {
                        pos.1 += h - height_for(n, size);
                    }
                }
                {
                    let mut g = self.player.core.lock();
                    let s = &mut g.settings.desk_lyric;
                    s.pos = Some([pos.0, pos.1]);
                    if matches!(drag, Drag::Resize { .. }) {
                        (s.width, s.lines) = (Some(w), n);
                    }
                    g.touch_settings();
                }
                self.sync_desk_lyric();
                self.player.save(false);
                if let Some(s) = self.settings_win.as_mut() {
                    s.win.dirty = true;
                }
            }
            WM_RBUTTONUP => self.popup(self.desk_menu()),
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            // Alt+F4 等：关掉桌面歌词（同「关闭桌面歌词」），不能只销毁窗口
            WM_CLOSE => self.act(Act::DeskToggle),
            _ => return None,
        }
        Some(LRESULT(0))
    }

    /// 桌面歌词右键菜单
    pub fn desk_menu(&self) -> Vec<Item> {
        let d = self.player.core.lock().settings.desk_lyric.clone();
        let n = d.line_count();
        let lines = [1, 2, 3, 4, 5, 6, 8, 10, 12].iter().map(|&k| check(&format!("{k} 行"), None, n == k, Act::DeskLines(k))).collect();
        let styles = STYLES.iter().enumerate().map(|(i, s)| check(s.name, None, d.style == i, Act::DeskStyle(Some(i)))).collect();
        vec![
            check("锁定桌面歌词", None, d.locked, Act::DeskLock),
            Item::Sep,
            Item::Sub { label: "显示行数".into(), items: lines },
            Item::Sub { label: "配色".into(), items: styles },
            item("字号增大", Act::DeskFont(2.0)),
            item("字号减小", Act::DeskFont(-2.0)),
            Item::Sep,
            item("桌面歌词设置…", Act::SettingsTab(super::settings_model::TAB_DESK)),
            item("关闭桌面歌词", Act::DeskToggle),
        ]
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::Line;

    fn lyr(texts: &[&str], trans: Option<&str>) -> Lyrics {
        let lines = texts.iter().enumerate().map(|(i, t)| Line { time: i as f64 * 4.0, text: t.to_string(), trans: trans.map(String::from) }).collect();
        Lyrics { lines, synced: true, ..Default::default() }
    }

    #[test]
    fn lines_and_height() {
        for n in 1..=DeskLyricSettings::MAX_LINES {
            assert_eq!(lines_for(height_for(n, 34.0), 34.0), n);
            // 拖到两行之间时取放得下的行数
            assert_eq!(lines_for(height_for(n, 34.0) + 20, 34.0), n);
        }
        assert_eq!(lines_for(10, 34.0), 1);
        assert_eq!(lines_for(99_999, 34.0), DeskLyricSettings::MAX_LINES);
    }

    #[test]
    fn resize_edges() {
        let r = R::new(100, 100, 600, 200);
        let lim = (60, 900);
        // 拖右边：左边不动；拖左边：右边不动；都不能小于最小宽度
        assert_eq!(resized(r, RIGHT, 50, 0, lim), R::new(100, 100, 650, 200));
        assert_eq!(resized(r, LEFT, -40, 0, lim), R::new(60, 100, 640, 200));
        assert_eq!(resized(r, LEFT, 9999, 0, lim), R::new(700 - MIN_W, 100, MIN_W, 200));
        assert_eq!(resized(r, TOP | RIGHT, 10, -30, lim), R::new(100, 70, 610, 230));
        assert_eq!(resized(r, BOTTOM, 0, -9999, lim), R::new(100, 100, 600, 60));
        assert_eq!(edges_at(600, 200, 2, 100), LEFT);
        assert_eq!(edges_at(600, 200, 598, 198), RIGHT | BOTTOM);
        assert_eq!(edges_at(600, 200, 300, 100), 0);
        assert_eq!(edge_cursor(LEFT | TOP), IDC_SIZENWSE);
        assert_eq!(edge_cursor(RIGHT | TOP), IDC_SIZENESW);
    }

    #[test]
    fn layouts() {
        let l = lyr(&["一", "二", "三", "四", "五"], None);
        // 没有歌词：只有标题，居中
        let r = rows(3, None, "歌名 - 歌手", None, 0.0, -1.0);
        assert_eq!((r.len(), r[0].slot, r[0].align), (1, 1.0, Align::Center));
        // 1 行
        assert_eq!(rows(1, Some(&l), "t", Some(2), 0.5, 2.0), vec![row("三", 0.0, Align::Center, Some(0.5), 1.0)]);
        // 2 行：单数句在上靠左、双数句在下靠右，当前句不换位置
        let r = rows(2, Some(&l), "t", Some(2), 0.3, 2.0);
        assert_eq!((r[0].text.as_str(), r[0].slot, r[0].align, r[0].progress), ("三", 0.0, Align::Left, Some(0.3)));
        assert_eq!((r[1].text.as_str(), r[1].slot, r[1].align, r[1].progress), ("四", 1.0, Align::Right, None));
        let r = rows(2, Some(&l), "t", Some(3), 0.3, 3.0);
        assert_eq!((r[0].text.as_str(), r[0].slot), ("四", 1.0));
        assert_eq!((r[1].text.as_str(), r[1].slot), ("五", 0.0));
        // 2 行 + 翻译：第二行是翻译
        let t = lyr(&["one", "two"], Some("译文"));
        let r = rows(2, Some(&t), "t", Some(0), 0.0, 0.0);
        assert_eq!((r[1].text.as_str(), r[1].align), ("译文", Align::Center));
        // 5 行：当前句在中间（第 2 行），前后各两句，越远越淡
        let r = rows(5, Some(&l), "t", Some(2), 0.4, 2.0);
        let cur = r.iter().find(|x| x.progress.is_some()).unwrap();
        assert_eq!((cur.text.as_str(), cur.slot, cur.alpha), ("三", 2.0, 1.0));
        let first = r.iter().find(|x| x.text == "一").unwrap();
        assert!(first.slot == 0.0 && first.alpha < r.iter().find(|x| x.text == "二").unwrap().alpha);
        // 滚动到一半时各行在两行之间
        let r = rows(5, Some(&l), "t", Some(3), 0.0, 2.5);
        assert_eq!(r.iter().find(|x| x.text == "四").unwrap().slot, 2.5);
        // 歌词开始前：标题在第一句前面，居中
        let r = rows(3, Some(&l), "歌名", None, 0.0, -1.0);
        assert_eq!(r.iter().find(|x| x.text == "歌名").unwrap().slot, 1.0);
        assert_eq!(r.iter().find(|x| x.text == "一").unwrap().slot, 2.0);
        // 偶数行：当前句在中线上面一行，所有行都在整行位置上（没有切掉一半的行），下面能多看一句
        let many = lyr(&["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"], None);
        let r = rows(8, Some(&many), "t", Some(5), 0.0, 5.0);
        let cur = r.iter().find(|x| x.progress.is_some()).unwrap();
        assert_eq!(cur.slot, 3.0);
        let shown: Vec<_> = r.iter().filter(|x| (0.0..=7.0).contains(&x.slot)).map(|x| x.slot).collect();
        assert_eq!(shown, (0..8).map(|s| s as f32).collect::<Vec<_>>());
    }
}
