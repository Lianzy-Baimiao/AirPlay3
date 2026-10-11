//! 主窗口句柄相关：显示/隐藏（托盘）、作为文件对话框父窗口。

use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible, SW_HIDE, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow};

/// 文件对话框的父窗口（rfd 需要 raw-window-handle）
pub struct Parent(pub isize);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let nz = std::num::NonZeroIsize::new(self.0).ok_or(HandleError::Unavailable)?;
        let h = Win32WindowHandle::new(nz);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(h)) })
    }
}

impl HasDisplayHandle for Parent {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}

fn hwnd(h: isize) -> HWND {
    HWND(h as *mut _)
}

pub fn hide(h: isize) {
    if h != 0 {
        unsafe {
            let _ = ShowWindow(hwnd(h), SW_HIDE);
        }
    }
}

/// 显示并激活主窗口（从托盘恢复、第二个实例启动时）。
pub fn show(h: isize) {
    if h == 0 {
        return;
    }
    unsafe {
        let w = hwnd(h);
        let _ = ShowWindow(w, if IsIconic(w).as_bool() { SW_RESTORE } else { SW_SHOW });
        let _ = SetForegroundWindow(w);
    }
}

pub fn is_visible(h: isize) -> bool {
    h != 0 && unsafe { IsWindowVisible(hwnd(h)).as_bool() && !IsIconic(hwnd(h)).as_bool() }
}

pub fn toggle(h: isize) {
    if is_visible(h) { hide(h) } else { show(h) }
}

/// 记录的窗口位置是否还在某个显示器上（拔掉副屏后避免窗口跑到屏幕外）。
/// 原生界面声明了 DPI 感知，保存的就是物理像素，不再换算。
pub fn point_on_screen(x: i32, y: i32) -> bool {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint};
    !unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL) }.is_invalid()
}
