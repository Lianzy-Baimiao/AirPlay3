//! 分层窗口（WS_EX_LAYERED + UpdateLayeredWindow 逐像素 alpha），与原版 AIRPLAY_GUI_CLASS 相同的做法。

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use super::canvas::Canvas;

pub const CLASS: PCWSTR = w!("AIRPLAY_GUI_CLASS");

pub fn register(wndproc: WNDPROC) {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: wndproc,
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: LoadIconW(Some(hinst.into()), PCWSTR(1 as _)).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    }
}

/// 创建分层弹出窗口。`taskbar` 为 true 时出现在任务栏（主窗口），否则为工具窗口。
pub fn create(title: &str, x: i32, y: i32, w: i32, h: i32, taskbar: bool, owner: Option<HWND>) -> HWND {
    let t: Vec<u16> = title.encode_utf16().chain([0]).collect();
    let mut ex = WS_EX_LAYERED;
    if !taskbar {
        ex |= WS_EX_TOOLWINDOW;
    }
    unsafe {
        CreateWindowExW(
            ex,
            CLASS,
            PCWSTR(t.as_ptr()),
            WS_POPUP | WS_MINIMIZEBOX | WS_SYSMENU,
            x,
            y,
            w,
            h,
            owner,
            None,
            GetModuleHandleW(None).ok().map(|h| h.into()),
            None,
        )
        .unwrap_or_default()
    }
}

/// 一块与窗口同尺寸的 DIB，用于 UpdateLayeredWindow
pub struct Surface {
    pub w: i32,
    pub h: i32,
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut [u8; 4],
}

impl Surface {
    pub fn new(w: i32, h: i32) -> Surface {
        unsafe {
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w.max(1),
                    biHeight: -h.max(1),
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
            let old = SelectObject(dc, bmp.into());
            Surface { w, h, dc, bmp, old, bits: bits as *mut [u8; 4] }
        }
    }

    /// 把画布内容提交到分层窗口（位置 x, y；尺寸为画布尺寸）
    pub fn present(&mut self, hwnd: HWND, c: &Canvas, x: i32, y: i32) {
        self.present_at(hwnd, c, Some((x, y)));
    }

    /// pos 为 None 时不改变窗口位置（子窗口的位置由调用方用 SetWindowPos 按父窗口坐标设置）
    pub fn present_at(&mut self, hwnd: HWND, c: &Canvas, pos: Option<(i32, i32)>) {
        if self.w != c.w || self.h != c.h {
            *self = Surface::new(c.w, c.h);
        }
        unsafe {
            if !self.bits.is_null() {
                std::ptr::copy_nonoverlapping(c.px.as_ptr(), self.bits, c.px.len().min((self.w * self.h) as usize));
            }
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let pt = pos.map(|(x, y)| POINT { x, y });
            let size = SIZE { cx: c.w, cy: c.h };
            let src = POINT { x: 0, y: 0 };
            let _ = UpdateLayeredWindow(hwnd, None, pt.as_ref().map(|p| p as *const POINT), Some(&size), Some(self.dc), Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.dc);
        }
    }
}
