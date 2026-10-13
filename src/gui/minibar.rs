//! 迷你条（原版 minibar）：两条 304×24 的窄条，上面是音量 / 标题 / 上一首·暂停·下一首 / 还原·关闭，下面是滚动歌词。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::anim::{self, After, Anim, Dir};
use super::app::App;
use super::canvas::{Canvas, R, Rgba};
use super::desklyric::LineState;
use super::popup::Popup;
use super::text::{self, Font};
use crate::skin::icons;

pub const W: i32 = 304;
pub const H: i32 = 24;
/// 歌词条默认在控制条下方 65 像素（原版位置）
pub const GAP: i32 = 65;
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
    ticks: u32,
    /// 正在淡入 / 淡出（打开迷你条、缩到托盘、退出）
    pub fade: Option<Anim>,
}

impl Minibar {
    pub fn new(top: Popup, bottom: Popup, fade: Option<Anim>) -> Minibar {
        Minibar { top, bottom, hover: [false; 2], drag: None, last: LineState::default(), ticks: 0, fade }
    }
}

/// 所有任务栏（主屏 Shell_TrayWnd，其它显示器 Shell_SecondaryTrayWnd）的位置
fn taskbars() -> Vec<R> {
    use windows::core::w;
    let mut v = Vec::new();
    unsafe {
        let mut push = |h: HWND| {
            let mut r = RECT::default();
            if GetWindowRect(h, &mut r).is_ok() {
                v.push(R::new(r.left, r.top, r.right - r.left, r.bottom - r.top));
            }
        };
        if let Ok(h) = FindWindowW(w!("Shell_TrayWnd"), None) {
            push(h);
        }
        let mut after = None;
        while let Ok(h) = FindWindowExW(None, after, w!("Shell_SecondaryTrayWnd"), None) {
            push(h);
            after = Some(h);
        }
    }
    v
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

/// 一条迷你条：与播放条相同的半透明玻璃
fn strip(c: &mut Canvas, hover: bool) {
    c.clear();
    super::panels::bar::glass(c, R::new(2, 2, W - 4, H - 4), 3.0, if hover { 1.0 } else { 0.0 });
}

/// 文字后面的暗色光晕：玻璃几乎透明，放在什么背景上都看得清
const HALO: Rgba = Rgba(16, 16, 16, 190);

impl App {
    /// 主界面 ⇄ 迷你条：主界面先播关闭动画再换成迷你条（淡入）；回来时迷你条直接消失，面板从中心长出来（原版）
    pub fn toggle_minibar(&mut self) {
        if self.quitting || self.after == Some(After::Minibar) {
            return;
        }
        if let Some(mut m) = self.minibar.take() {
            m.top.close();
            m.bottom.close();
            self.show_main(false);
            return;
        }
        if self.away.is_some() || self.after.is_some() {
            return;
        }
        self.close_all(After::Minibar);
    }

    pub fn tick_minibar(&mut self) {
        // 缩到托盘时不画
        if self.minibar.is_none() || self.away.is_some() {
            return;
        }
        let st = self.line_state();
        let m = self.minibar.as_mut().unwrap();
        m.ticks = m.ticks.wrapping_add(1);
        let check_z = m.ticks % 15 == 0;
        // 淡入淡出期间每一帧都画；淡入完了去掉（淡出的留着，等 tick_anim 隐藏）
        let fading = m.fade.is_some_and(|a| a.dir == Dir::In || !a.out_done(true));
        if m.fade.is_some_and(|a| a.dir == Dir::In && a.t() >= anim::BAR_IN) {
            m.fade = None;
        }
        if st != m.last || fading {
            m.last = st;
            self.paint_minibar();
        }
        if check_z {
            self.keep_minibar_on_top();
        }
    }

    /// 迷你条放在任务栏上时，点任务栏（开始菜单、切换窗口）会让任务栏压到迷你条上面，迷你条就点不到了：
    /// 盖着任务栏时每半秒把迷你条重新放到最上层（关了「始终置顶」也一样，离开任务栏后再取消置顶）
    fn keep_minibar_on_top(&mut self) {
        let on_top = self.player.core.lock().settings.general.always_on_top;
        let Some(m) = self.minibar.as_ref() else { return };
        if m.drag.is_some() {
            return;
        }
        let strips = [(m.top.hwnd, m.top.pos), (m.bottom.hwnd, m.bottom.pos)];
        let bars = taskbars();
        for (h, (x, y)) in strips {
            let r = R::new(x, y, W, H);
            let over_bar = bars.iter().any(|b| b.inter(&r).w > 0 && b.inter(&r).h > 0);
            let now_top = unsafe { GetWindowLongW(h, GWL_EXSTYLE) } & WS_EX_TOPMOST.0 as i32 != 0;
            let want = on_top || over_bar;
            if over_bar || want != now_top {
                unsafe {
                    let _ = SetWindowPos(h, Some(if want { HWND_TOPMOST } else { HWND_NOTOPMOST }), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
            }
        }
    }

    /// 「恢复默认位置」：迷你条回到播放条的位置（控制条在上，歌词条在下方 65 像素）
    pub fn reset_minibar(&mut self) {
        let (x, y) = self.bar_pos();
        if let Some(m) = self.minibar.as_mut() {
            m.top.move_to(x, y);
            m.bottom.move_to(x, y + GAP);
        }
        self.keep_minibar_on_top();
    }

    pub fn paint_minibar(&mut self) {
        let (playing, vol, muted) = (self.ui.snap.playing, self.ui.snap.volume, self.ui.snap.muted);
        let title = self.now.lock().title.clone();
        let Some(m) = self.minibar.as_mut() else { return };
        let alpha = m.fade.map_or(1.0, |a| anim::fade_alpha(&a, a.t().max(0.0)));
        let c = &mut m.top.canvas;
        strip(c, m.hover[0]);
        let v = if muted { 0.0 } else { vol };
        c.round_rect(R::new(7, 6, 5, 12), 2.0, Some(Rgba(0x9F, 0x9F, 0x9F, 107)), None);
        let fh = (12.0 * v) as i32;
        c.push_clip(R::new(7, 18 - fh, 5, fh));
        c.round_rect(R::new(7, 6, 5, 12), 2.0, Some(Rgba(0xE3, 0xE3, 0xE3, 208)), None);
        c.pop_clip();
        let name = if title.is_empty() { "AIRPLAY".to_string() } else { title };
        c.push_clip(R::new(20, 3, 98, 18));
        c.glow_text(&name, FONT, 22, 4, Rgba::gray(if m.hover[0] { 235 } else { 200 }), HALO, 2);
        c.pop_clip();
        let a = if m.hover[0] { 255 } else { 200 };
        c.icon(icons::PREV, BTN_PREV.x, BTN_PREV.y - 2, Rgba::gray(230).alpha(a));
        c.icon(if playing { icons::PAUSE } else { icons::PLAY }, BTN_PLAY.x, BTN_PLAY.y - 2, Rgba::gray(230).alpha(a));
        c.icon(icons::NEXT, BTN_NEXT.x, BTN_NEXT.y - 2, Rgba::gray(230).alpha(a));
        c.icon(icons::S_RESTORE, BTN_RESTORE.x, BTN_RESTORE.y, Rgba::gray(220));
        c.icon(icons::S_CLOSE, BTN_CLOSE.x, BTN_CLOSE.y, Rgba::gray(220));
        anim::fade(c, alpha);
        #[cfg(debug_assertions)]
        if m.fade.is_some() {
            c.dump_seq("MiniTop", self.started.elapsed().as_millis());
        }
        m.top.present();
        // 下方：迷你歌词，长句随进度向左滚动
        let c = &mut m.bottom.canvas;
        strip(c, m.hover[1]);
        let inner = R::new(8, 3, W - 16, 18);
        let tw = text::measure(&m.last.text, FONT).0 as i32;
        let x = if tw <= inner.w { inner.x + (inner.w - tw) / 2 } else { inner.x - ((tw - inner.w) as f32 * m.last.progress) as i32 };
        c.push_clip(inner);
        c.glow_text(&m.last.text, FONT, x, 4, Rgba::gray(235), HALO, 2);
        c.pop_clip();
        anim::fade(c, alpha);
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
        // 可以盖住任务栏，但不能拖出屏幕（限制在光标所在显示器的整个屏幕范围里）
        let group: Vec<R> = (0..2).filter(|&i| moving[i]).map(|i| d.rects[i]).collect();
        let (dx, dy) = super::snap::keep_inside(&group, (q.x - s.x, q.y - s.y), super::popup::monitor_at((cx, cy)));
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
                    self.request_close();
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
            // Alt+F4 等：与主界面一样（完全退出，或开了「点关闭按钮缩到托盘」时缩到托盘）
            WM_CLOSE => self.request_close(),
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
