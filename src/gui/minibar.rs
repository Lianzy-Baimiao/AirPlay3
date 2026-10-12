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
/// 歌词条默认在控制条下方 65 像素（原版位置）
const GAP: i32 = 65;
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
    pub drag: Option<MiniDrag>,
    pub last: LineState,
}

/// 拖动迷你条（原版：拖控制条时歌词条一起移动；拖歌词条只移动歌词条；都会吸附到屏幕边、任务栏和另一条）
#[derive(Clone, Copy)]
pub struct MiniDrag {
    /// 0 = 控制条，1 = 歌词条
    lead: usize,
    cursor: (i32, i32),
    rects: [R; 2],
    taskbar: Option<R>,
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
        let (x, y) = self.bar_pos();
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

    /// 按鼠标位置移动迷你条（吸附算法与主界面面板相同，见 snap.rs）
    fn minibar_drag(&mut self, d: MiniDrag) {
        self.minibar_drag_at(d, super::resize::cursor());
    }

    fn minibar_drag_at(&mut self, d: MiniDrag, (cx, cy): (i32, i32)) {
        let s = d.rects[d.lead];
        let p = R::new(s.x + cx - d.cursor.0, s.y + cy - d.cursor.1, s.w, s.h);
        let moving = if d.lead == 0 { [true, true] } else { [false, true] };
        let env = super::snap::Env { taskbar: d.taskbar, work_area: &super::resize::work_area };
        let q = super::snap::group_move(&d.rects, &[true, true], &moving, d.lead, p, d.lead == 0, &env);
        let (dx, dy) = (q.x - s.x, q.y - s.y);
        if let Some(m) = self.minibar.as_mut() {
            if moving[0] {
                m.top.move_to(d.rects[0].x + dx, d.rects[0].y + dy);
            }
            m.bottom.move_to(d.rects[1].x + dx, d.rects[1].y + dy);
        }
        // 拖控制条时，隐藏的主界面跟着移动（还原时回到迷你条的位置）
        if moving[0] {
            self.move_to(d.rects[0].x + dx, d.rects[0].y + dy);
        }
    }

    /// 仅调试构建：模拟拖动迷你条（0 = 控制条，1 = 歌词条），返回两条的位置
    #[cfg(debug_assertions)]
    pub fn test_minibar_drag(&mut self, lead: usize, dx: i32, dy: i32) -> Option<[(i32, i32); 2]> {
        if self.minibar.is_none() {
            self.toggle_minibar();
        }
        let m = self.minibar.as_ref()?;
        let rect = |p: &Popup| R::new(p.pos.0, p.pos.1, W, H);
        let c0 = (10_000, 10_000);
        let d = MiniDrag { lead, cursor: c0, rects: [rect(&m.top), rect(&m.bottom)], taskbar: super::resize::taskbar_rect() };
        self.minibar_drag_at(d, (c0.0 + dx, c0.1 + dy));
        let m = self.minibar.as_ref()?;
        Some([m.top.pos, m.bottom.pos])
    }

    pub fn minibar_msg(&mut self, hwnd: HWND, msg: u32, _wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let m = self.minibar.as_mut()?;
        let which = if m.top.hwnd == hwnd { 0 } else if m.bottom.hwnd == hwnd { 1 } else { return None };
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        match msg {
            WM_MOUSEMOVE => {
                if let Some(d) = m.drag {
                    self.minibar_drag(d);
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
                    let rect = |p: &Popup| R::new(p.pos.0, p.pos.1, W, H);
                    m.drag = Some(MiniDrag { lead: which, cursor: super::resize::cursor(), rects: [rect(&m.top), rect(&m.bottom)], taskbar: super::resize::taskbar_rect() });
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
            // Alt+F4 等：与主界面一样完全退出（否则只关掉迷你条，进程留在后台）
            WM_CLOSE => self.quit(),
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
