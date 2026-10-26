//! 独立的分层工具窗口（歌词搜索、配置、桌面歌词、任务栏歌词共用）。

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::canvas::Canvas;
use super::window::{self, Surface};

pub struct Popup {
    pub hwnd: HWND,
    pub canvas: Canvas,
    surface: Surface,
    pub pos: (i32, i32),
    pub dirty: bool,
}

impl Popup {
    /// 在屏幕坐标 (x, y) 创建 w×h 的窗口；owner 为播放条（随主窗口最小化）
    pub fn new(x: i32, y: i32, w: i32, h: i32, owner: Option<HWND>, title: &str) -> Popup {
        let hwnd = window::create(title, x, y, w, h, false, owner);
        Popup { hwnd, canvas: Canvas::new(w, h), surface: Surface::new(w, h), pos: (x, y), dirty: true }
    }

    pub fn show(&self, activate: bool) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if activate { SW_SHOW } else { SW_SHOWNOACTIVATE });
            if activate {
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        if self.canvas.w != w || self.canvas.h != h || self.canvas.scale != super::scale::get() {
            self.canvas = Canvas::new(w, h);
            self.dirty = true;
        }
    }

    pub fn present(&mut self) {
        self.surface.present(self.hwnd, &self.canvas, self.pos.0, self.pos.1);
        self.dirty = false;
    }

    pub fn move_to(&mut self, x: i32, y: i32) {
        self.pos = (x, y);
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    pub fn close(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    /// 主窗口所在显示器的中央
    pub fn centered(w: i32, h: i32) -> (i32, i32) {
        let a = work_area_at(super::resize::cursor());
        (a.x + (a.w - w) / 2, a.y + (a.h - h) / 2)
    }
}

/// 某个屏幕点所在显示器的整个屏幕范围（含任务栏）
pub fn monitor_at((x, y): (i32, i32)) -> super::canvas::R {
    use windows::Win32::Graphics::Gdi::*;
    unsafe {
        let m = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(m, &mut mi).as_bool() {
            let r = mi.rcMonitor;
            return super::canvas::R::new(r.left, r.top, r.right - r.left, r.bottom - r.top);
        }
        super::canvas::R::new(0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
    }
}

/// 某个屏幕点所在显示器的工作区（多显示器时不能只用主屏尺寸）
pub fn work_area_at((x, y): (i32, i32)) -> super::canvas::R {
    use windows::Win32::Graphics::Gdi::*;
    unsafe {
        let m = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(m, &mut mi).as_bool() {
            let r = mi.rcWork;
            return super::canvas::R::new(r.left, r.top, r.right - r.left, r.bottom - r.top);
        }
        super::canvas::R::new(0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
    }
}
