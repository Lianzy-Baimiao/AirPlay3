//! 桌面歌词：置顶的透明分层窗口，两行卡拉 OK 填色；可拖动，锁定后鼠标穿透；悬停显示工具条。

use std::rc::Rc;

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::app::App;
use super::canvas::{R, Rgba};
use super::popup::Popup;
use super::text::{self, Font, Hint, Mask};
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
            let t = if n.title.is_empty() { "AirPlay3".to_string() } else if n.artist.is_empty() { n.title.clone() } else { format!("{} - {}", n.title, n.artist) };
            return LineState { idx: None, text: t, next: String::new(), progress: 0.0 };
        };
        match n.lyric_index(pos) {
            Some(i) => {
                let line = &l.lines[i];
                let next = line.trans.clone().unwrap_or_else(|| l.lines.get(i + 1).map_or(String::new(), |x| x.text.clone()));
                LineState { idx: Some(i), text: line.text.clone(), next, progress: n.lyric_progress(i, pos) }
            }
            None => LineState { idx: None, text: format!("{} - {}", n.title, n.artist), next: l.lines[0].text.clone(), progress: 0.0 },
        }
    }
}

pub struct DeskLyric {
    pub win: Popup,
    pub hover: f32,
    pub hovering: bool,
    pub drag: Option<(i32, i32)>,
    pub last: LineState,
    /// 缓存：(文字, 字号) → 文字遮罩与描边遮罩
    cache: Vec<(String, u16, Rc<Mask>, Rc<Mask>)>,
}

const BAR_H: i32 = 30;
const BTNS: [(&str, i32); 6] = [("prev", 0), ("play", 1), ("next", 2), ("smaller", 3), ("bigger", 4), ("lock", 5)];

fn masks(cache: &mut Vec<(String, u16, Rc<Mask>, Rc<Mask>)>, s: &str, size: f32) -> (Rc<Mask>, Rc<Mask>) {
    let key = (size * 10.0) as u16;
    if let Some(c) = cache.iter().find(|c| c.0 == s && c.1 == key) {
        return (c.2.clone(), c.3.clone());
    }
    let m = Rc::new(text::render(s, Font::px(size, true), Hint::Gray));
    let o = Rc::new(super::draw::outline(&m, 2));
    if cache.len() > 8 {
        cache.remove(0);
    }
    cache.push((s.to_string(), key, m.clone(), o.clone()));
    (m, o)
}
impl App {
    fn desk_size(&self) -> (i32, i32) {
        let s = &self.player.core.lock().settings.desk_lyric;
        let lines = if s.two_lines { 2 } else { 1 };
        let w = s.width.unwrap_or(900).clamp(300, 3000);
        (w, BAR_H + (s.font_size * 1.5) as i32 * lines + 12)
    }

    /// 根据设置创建 / 关闭桌面歌词窗口，并同步锁定（鼠标穿透）状态
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
        if self.desk.is_none() {
            let a = super::popup::work_area_at((self.origin.0 + 100, self.origin.1 + 50));
            let (x, y) = pos.map_or((a.x + (a.w - w) / 2, a.bottom() - h - 60), |p| (p[0], p[1]));
            let win = Popup::new(x, y, w, h, None, "AirPlay3 桌面歌词");
            unsafe {
                let ex = GetWindowLongW(win.hwnd, GWL_EXSTYLE);
                SetWindowLongW(win.hwnd, GWL_EXSTYLE, ex | WS_EX_TOPMOST.0 as i32 | WS_EX_NOACTIVATE.0 as i32);
                let _ = SetWindowPos(win.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
            self.desk = Some(DeskLyric { win, hover: 0.0, hovering: false, drag: None, last: LineState::default(), cache: Vec::new() });
        }
        let d = self.desk.as_mut().unwrap();
        d.win.resize(w, h);
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

    /// 每帧：歌词或填色进度变化时重绘
    pub fn tick_desk(&mut self) {
        if self.desk.is_none() {
            return;
        }
        let st = self.line_state();
        let d = self.desk.as_mut().unwrap();
        let hover_changed = super::state::approach(&mut d.hover, if d.hovering { 1.0 } else { 0.0 }, 0.15);
        let changed = st.idx != d.last.idx || st.text != d.last.text || (st.progress - d.last.progress).abs() > 0.004;
        if changed || hover_changed {
            d.last = st;
            d.win.dirty = true;
            self.paint_desk();
        }
    }

    pub fn paint_desk(&mut self) {
        let (size, two, played, unplayed, locked) = {
            let g = self.player.core.lock();
            let s = &g.settings.desk_lyric;
            (s.font_size, s.two_lines, s.played, s.unplayed, s.locked)
        };
        let playing = self.ui.snap.playing;
        let Some(d) = self.desk.as_mut() else { return };
        let st = d.last.clone();
        let c = &mut d.win.canvas;
        c.clear();
        let (w, h) = (c.w, c.h);
        // 悬停时显示半透明底与工具条（锁定时不显示）
        let hv = if locked { 0.0 } else { d.hover };
        if hv > 0.0 {
            c.round_rect(R::new(0, 0, w, h), 8.0, Some(Rgba(20, 20, 20, (150.0 * hv) as u8)), Some(Rgba(255, 255, 255, (40.0 * hv) as u8)));
            let bx = w / 2 - BTNS.len() as i32 * 13;
            let a = (255.0 * hv) as u8;
            for (name, i) in BTNS {
                let x = bx + i * 26;
                let col = Rgba::gray(230).alpha(a);
                match name {
                    "prev" => c.icon(icons::PREV, x, 7, col),
                    "play" => c.icon(if playing { icons::PAUSE } else { icons::PLAY }, x, 7, col),
                    "next" => c.icon(icons::NEXT, x, 7, col),
                    "smaller" => {
                        c.text("A-", Font::px(13.0, true), x, 6, col, Hint::Gray);
                    }
                    "bigger" => {
                        c.text("A+", Font::px(13.0, true), x, 6, col, Hint::Gray);
                    }
                    _ => c.icon(icons::W_PIN, x, 7, col),
                }
            }
        }
        let line_h = (size * 1.5) as i32;
        let y1 = BAR_H;
        let lines: Vec<(String, f32, bool)> = if two { vec![(st.text.clone(), st.progress, true), (st.next.clone(), 0.0, false)] } else { vec![(st.text.clone(), st.progress, true)] };
        for (k, (s, prog, current)) in lines.iter().enumerate() {
            if s.is_empty() {
                continue;
            }
            let (m, o) = masks(&mut d.cache, s, size);
            let tw = m.w as i32;
            // 两行时第一行靠左、第二行靠右（经典桌面歌词排版）
            let x = if !two { (w - tw) / 2 } else if k == 0 { 24 } else { (w - tw - 24).max(24) };
            let y = y1 + k as i32 * line_h;
            c.mask(&o, x - 2, y - 2, Rgba(0, 0, 0, 200));
            let (pr, ur) = (Rgba(played[0], played[1], played[2], 255), Rgba(unplayed[0], unplayed[1], unplayed[2], 255));
            let split = x + (tw as f32 * prog) as i32;
            if *current {
                c.push_clip(R::new(x - 4, y - 4, split - x + 4, m.h as i32 + 8));
                c.mask(&m, x, y, pr);
                c.pop_clip();
                c.push_clip(R::new(split, y - 4, w, m.h as i32 + 8));
                c.mask(&m, x, y, ur);
                c.pop_clip();
            } else {
                c.mask(&m, x, y, ur.alpha(200));
            }
        }
        // 未锁定时整窗可接收鼠标（拖动）
        if !locked {
            c.hit_margin(R::new(0, 0, w, h));
        }
        let _ = h;
        d.win.present();
    }
}
impl App {
    pub fn desk_msg(&mut self, hwnd: HWND, msg: u32, _wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if self.desk.as_ref()?.win.hwnd != hwnd {
            return None;
        }
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        match msg {
            WM_MOUSEMOVE => {
                let d = self.desk.as_mut()?;
                if !d.hovering {
                    d.hovering = true;
                    let mut t = windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT {
                        cbSize: size_of::<windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT>() as u32,
                        dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    unsafe {
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::TrackMouseEvent(&mut t);
                    }
                }
                if let Some((dx, dy)) = d.drag {
                    let (cx, cy) = super::resize::cursor();
                    d.win.move_to(cx - dx, cy - dy);
                }
            }
            0x02A3 => self.desk.as_mut()?.hovering = false, // WM_MOUSELEAVE
            WM_LBUTTONDOWN => {
                let w = self.desk.as_ref()?.win.canvas.w;
                let bx = w / 2 - BTNS.len() as i32 * 13;
                if y < BAR_H && x >= bx - 4 && x < bx + BTNS.len() as i32 * 26 {
                    let i = ((x - bx + 4) / 26).clamp(0, BTNS.len() as i32 - 1);
                    match BTNS[i as usize].0 {
                        "prev" => self.player.prev(),
                        "play" => self.player.toggle_pause(),
                        "next" => self.player.next(),
                        "smaller" => self.desk_font(-2.0),
                        "bigger" => self.desk_font(2.0),
                        _ => self.act(super::menu::Act::DeskLock),
                    }
                    return Some(LRESULT(0));
                }
                let d = self.desk.as_mut()?;
                let (cx, cy) = super::resize::cursor();
                d.drag = Some((cx - d.win.pos.0, cy - d.win.pos.1));
                unsafe { SetCapture(hwnd) };
            }
            WM_LBUTTONUP => {
                let d = self.desk.as_mut()?;
                if d.drag.take().is_some() {
                    let pos = d.win.pos;
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    let mut g = self.player.core.lock();
                    g.settings.desk_lyric.pos = Some([pos.0, pos.1]);
                    g.touch_settings();
                }
            }
            WM_RBUTTONUP => self.act(super::menu::Act::DeskMenu),
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn desk_font(&mut self, d: f32) {
        {
            let mut g = self.player.core.lock();
            let s = &mut g.settings.desk_lyric;
            s.font_size = (s.font_size + d).clamp(16.0, 72.0);
            g.touch_settings();
        }
        self.sync_desk_lyric();
    }
}
