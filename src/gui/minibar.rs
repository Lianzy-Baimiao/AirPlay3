//! 迷你条（原版 minibar）：两条 304×24 的窄条，上面是音量 / 标题 / 上一首·暂停·下一首 / 还原·关闭，下面是滚动歌词。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::app::App;
use super::canvas::{Canvas, R, Rgba};
use super::desklyric::LineState;
use super::popup::Popup;
use super::text::{self, Font, Hint};
use crate::skin::icons;

pub const W: i32 = 304;
pub const H: i32 = 24;
const GAP: i32 = 41;
const FONT: Font = Font::px(12.0, false);
const BTN_PREV: R = R::new(123, 6, 16, 12);
const BTN_PLAY: R = R::new(144, 6, 16, 12);
const BTN_NEXT: R = R::new(165, 6, 16, 12);
const BTN_RESTORE: R = R::new(272, 5, 10, 10);
const BTN_CLOSE: R = R::new(284, 5, 10, 10);

pub struct Minibar {
    pub top: Popup,
    pub bottom: Popup,
    pub hover: [bool; 2],
    pub drag: Option<(i32, i32)>,
    pub last: LineState,
}

fn strip(c: &mut Canvas, hover: bool) {
    c.clear();
    let r = R::new(2, 2, W - 4, H - 4);
    c.push_clip(r.shrink(1));
    c.vgrad(r.shrink(1), Rgba::gray(8), Rgba::gray(22));
    if hover {
        for x in r.x..r.right() {
            let d = ((x - W / 2) as f32).abs() / (W as f32 / 2.0);
            c.fill(R::new(x, r.y, 1, r.h), Rgba::gray(255).alpha((70.0 * (1.0 - d)) as u8));
        }
    }
    c.pop_clip();
    c.round_rect(r, 3.0, None, Some(Rgba::gray(85)));
}

impl App {
    pub fn toggle_minibar(&mut self) {
        if let Some(mut m) = self.minibar.take() {
            m.top.close();
            m.bottom.close();
            self.restore();
            return;
        }
        let (x, y) = self.origin;
        let top = Popup::new(x, y, W, H, None, "AirPlay3 迷你条");
        let bottom = Popup::new(x, y + GAP, W, H, Some(top.hwnd), "AirPlay3 迷你歌词");
        unsafe {
            for (_, w) in &self.wins {
                let _ = ShowWindow(w.hwnd, SW_HIDE);
            }
            for h in [top.hwnd, bottom.hwnd] {
                let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
        self.minibar = Some(Minibar { top, bottom, hover: [false; 2], drag: None, last: LineState::default() });
        self.paint_minibar();
        if let Some(m) = &self.minibar {
            m.top.show(false);
            m.bottom.show(false);
        }
    }

    pub fn tick_minibar(&mut self) {
        if self.minibar.is_none() {
            return;
        }
        let st = self.line_state();
        let m = self.minibar.as_mut().unwrap();
        if st != m.last {
            m.last = st;
            self.paint_minibar();
        }
    }

    pub fn paint_minibar(&mut self) {
        let (playing, vol, muted) = (self.ui.snap.playing, self.ui.snap.volume, self.ui.snap.muted);
        let title = self.now.lock().title.clone();
        let Some(m) = self.minibar.as_mut() else { return };
        let c = &mut m.top.canvas;
        strip(c, m.hover[0]);
        let v = if muted { 0.0 } else { vol };
        c.round_rect(R::new(7, 6, 5, 12), 2.0, Some(Rgba::gray(60)), None);
        let fh = (12.0 * v) as i32;
        c.push_clip(R::new(7, 18 - fh, 5, fh));
        c.round_rect(R::new(7, 6, 5, 12), 2.0, Some(Rgba::gray(200)), None);
        c.pop_clip();
        let name = if title.is_empty() { "AIRPLAY".to_string() } else { title };
        c.push_clip(R::new(20, 3, 98, 18));
        c.text(&name, FONT, 22, 4, Rgba::gray(if m.hover[0] { 230 } else { 120 }), Hint::Gray);
        c.pop_clip();
        let a = if m.hover[0] { 255 } else { 200 };
        c.icon(icons::PREV, BTN_PREV.x, BTN_PREV.y - 2, Rgba::gray(230).alpha(a));
        c.icon(if playing { icons::PAUSE } else { icons::PLAY }, BTN_PLAY.x, BTN_PLAY.y - 2, Rgba::gray(230).alpha(a));
        c.icon(icons::NEXT, BTN_NEXT.x, BTN_NEXT.y - 2, Rgba::gray(230).alpha(a));
        c.icon(icons::S_RESTORE, BTN_RESTORE.x, BTN_RESTORE.y, Rgba::gray(220));
        c.icon(icons::S_CLOSE, BTN_CLOSE.x, BTN_CLOSE.y, Rgba::gray(220));
        m.top.present();
        // 下方：迷你歌词，长句随进度向左滚动
        let c = &mut m.bottom.canvas;
        strip(c, m.hover[1]);
        let inner = R::new(8, 3, W - 16, 18);
        let tw = text::measure(&m.last.text, FONT).0 as i32;
        let x = if tw <= inner.w { inner.x + (inner.w - tw) / 2 } else { inner.x - ((tw - inner.w) as f32 * m.last.progress) as i32 };
        c.push_clip(inner);
        c.glow_text(&m.last.text, FONT, x, 4, Rgba::gray(225), Rgba(255, 255, 255, 40), 2);
        c.pop_clip();
        m.bottom.present();
    }

    pub fn minibar_msg(&mut self, hwnd: HWND, msg: u32, _wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let m = self.minibar.as_mut()?;
        let which = if m.top.hwnd == hwnd { 0 } else if m.bottom.hwnd == hwnd { 1 } else { return None };
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        match msg {
            WM_MOUSEMOVE => {
                if let Some((dx, dy)) = m.drag {
                    let (cx, cy) = super::resize::cursor();
                    m.top.move_to(cx - dx, cy - dy);
                    m.bottom.move_to(cx - dx, cy - dy + GAP);
                    self.origin = (cx - dx, cy - dy);
                    self.move_to(cx - dx, cy - dy);
                } else if !m.hover[which] {
                    m.hover[which] = true;
                    let mut t = windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT {
                        cbSize: size_of::<windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT>() as u32,
                        dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    unsafe {
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::TrackMouseEvent(&mut t);
                    }
                    self.paint_minibar();
                }
            }
            0x02A3 => {
                m.hover[which] = false;
                self.paint_minibar();
            }
            WM_LBUTTONDOWN => {
                if which == 0 && BTN_PREV.contains(x, y) {
                    self.player.prev();
                } else if which == 0 && BTN_PLAY.contains(x, y) {
                    self.player.toggle_pause();
                } else if which == 0 && BTN_NEXT.contains(x, y) {
                    self.player.next();
                } else if which == 0 && BTN_RESTORE.shrink(-2).contains(x, y) {
                    self.toggle_minibar();
                } else if which == 0 && BTN_CLOSE.shrink(-2).contains(x, y) {
                    self.quit();
                } else {
                    let (cx, cy) = super::resize::cursor();
                    m.drag = Some((cx - m.top.pos.0, cy - m.top.pos.1));
                    unsafe { SetCapture(hwnd) };
                }
                self.paint_minibar();
            }
            WM_LBUTTONUP => {
                m.drag = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            WM_LBUTTONDBLCLK => self.toggle_minibar(),
            WM_RBUTTONUP => self.act(super::menu::Act::MainMenu),
            WM_MOUSEWHEEL => {
                let d = ((_wp.0 >> 16) & 0xFFFF) as i16 as i32;
                self.player.volume_step(if d > 0 { 0.05 } else { -0.05 });
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }
}
