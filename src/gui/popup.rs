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
    /// 不跟随界面缩放（桌面歌词）：位置、尺寸都是屏幕上的实际像素，画布一直按 100% 画
    fixed: bool,
}

impl Popup {
    /// 在屏幕坐标 (x, y) 创建 w×h 的窗口；owner 为播放条（随主窗口最小化）
    pub fn new(x: i32, y: i32, w: i32, h: i32, owner: Option<HWND>, title: &str) -> Popup {
        let hwnd = window::create(title, x, y, w, h, false, owner);
        Popup { hwnd, canvas: Canvas::new(w, h), surface: Surface::new(w, h), pos: (x, y), dirty: true, fixed: false }
    }

    /// 不跟随界面缩放的窗口（桌面歌词有自己的字号）：(x, y)、w×h 是实际像素
    pub fn new_fixed(x: i32, y: i32, w: i32, h: i32, owner: Option<HWND>, title: &str) -> Popup {
        let hwnd = window::create_px(title, x, y, w, h, false, owner);
        Popup { hwnd, canvas: Canvas::with_scale(w, h, 1.0), surface: Surface::new(w, h), pos: (x, y), dirty: true, fixed: true }
    }

    /// 画布的缩放：跟随界面缩放，或一直 100%
    fn scale(&self) -> f32 {
        if self.fixed { 1.0 } else { super::scale::get() }
    }

    /// 坐标 → 屏幕上的实际像素
    fn px(&self, v: i32) -> i32 {
        if self.fixed { v } else { super::scale::to_px(v) }
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
        let scale = self.scale();
        if self.canvas.w != w || self.canvas.h != h || self.canvas.scale != scale {
            self.canvas = Canvas::with_scale(w, h, scale);
            self.dirty = true;
        }
    }

    pub fn present(&mut self) {
        let pos = (self.px(self.pos.0), self.px(self.pos.1));
        self.surface.present_at(self.hwnd, &self.canvas, Some(pos));
        self.dirty = false;
    }

    pub fn move_to(&mut self, x: i32, y: i32) {
        self.pos = (x, y);
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, self.px(x), self.px(y), 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
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

/// 某个屏幕点所在显示器的整个屏幕范围（含任务栏；点和结果都是逻辑像素）
pub fn monitor_at(p: (i32, i32)) -> super::canvas::R {
    monitor_rect(p, false)
}

/// 显示器的整个范围或工作区（逻辑像素）
fn monitor_rect((x, y): (i32, i32), work: bool) -> super::canvas::R {
    use super::scale::{rect_logical, to_px};
    rect_logical(monitor_rect_px((to_px(x), to_px(y)), work))
}

/// 显示器的整个范围或工作区（点和结果都是实际像素）
fn monitor_rect_px((x, y): (i32, i32), work: bool) -> super::canvas::R {
    use windows::Win32::Graphics::Gdi::*;
    unsafe {
        let m = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(m, &mut mi).as_bool() {
            let r = if work { mi.rcWork } else { mi.rcMonitor };
            return super::canvas::R::new(r.left, r.top, r.right - r.left, r.bottom - r.top);
        }
        super::canvas::R::new(0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
    }
}

/// 某个屏幕点所在显示器的工作区（多显示器时不能只用主屏尺寸；逻辑像素）
pub fn work_area_at(p: (i32, i32)) -> super::canvas::R {
    monitor_rect(p, true)
}

/// 同 `work_area_at`，点和结果都是实际像素（不跟随界面缩放的桌面歌词用）
pub fn work_area_px(p: (i32, i32)) -> super::canvas::R {
    monitor_rect_px(p, true)
}
