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

/// 「透明雾化」：窗口半透明部分后面的桌面变模糊（毛玻璃）。原版用的 DwmEnableBlurBehindWindow 在
/// Windows 8 以后不再模糊，这里用 Windows 10 起的 SetWindowCompositionAttribute（ACCENT_ENABLE_BLURBEHIND），
/// 系统不支持时什么也不做
/// 前台窗口是本程序的
pub fn foreground_is_ours() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        pid == windows::Win32::System::Threading::GetCurrentProcessId()
    }
}

pub fn set_frost(hwnd: HWND, on: bool) {
    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient: u32,
        animation: u32,
    }
    #[repr(C)]
    struct CompositionData {
        attrib: u32,
        data: *mut AccentPolicy,
        size: usize,
    }
    type Swca = unsafe extern "system" fn(HWND, *mut CompositionData) -> i32;
    static FUNC: std::sync::OnceLock<Option<Swca>> = std::sync::OnceLock::new();
    let f = FUNC.get_or_init(|| unsafe {
        let user32 = windows::Win32::System::LibraryLoader::GetModuleHandleW(w!("user32.dll")).ok()?;
        let p = windows::Win32::System::LibraryLoader::GetProcAddress(user32, windows::core::s!("SetWindowCompositionAttribute"))?;
        Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, Swca>(p))
    });
    let Some(f) = f else { return };
    // ACCENT_DISABLED = 0，ACCENT_ENABLE_BLURBEHIND = 3；WCA_ACCENT_POLICY = 19
    let mut accent = AccentPolicy { state: if on { 3 } else { 0 }, flags: 0, gradient: 0, animation: 0 };
    let mut data = CompositionData { attrib: 19, data: &mut accent, size: size_of::<AccentPolicy>() };
    unsafe {
        let _ = f(hwnd, &mut data);
    }
}

/// 创建分层弹出窗口（位置、尺寸是逻辑像素）。`taskbar` 为 true 时出现在任务栏（主窗口），否则为工具窗口。
/// 「透明雾化」打开时新窗口也跟着雾化（桌面歌词自己再关掉）
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
            super::scale::to_px(x),
            super::scale::to_px(y),
            super::scale::to_px(w),
            super::scale::to_px(h),
            owner,
            None,
            GetModuleHandleW(None).ok().map(|h| h.into()),
            None,
        )
        .inspect(|&h| {
            if super::theme::get().frost {
                set_frost(h, true);
            }
        })
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

    /// 把画布内容提交到分层窗口（位置 x, y 是逻辑像素；尺寸为画布的实际像素）
    pub fn present(&mut self, hwnd: HWND, c: &Canvas, x: i32, y: i32) {
        self.present_at(hwnd, c, Some((super::scale::to_px(x), super::scale::to_px(y))));
    }

    /// pos 是实际像素；为 None 时不改变窗口位置（子窗口的位置由调用方用 SetWindowPos 按父窗口坐标设置）
    pub fn present_at(&mut self, hwnd: HWND, c: &Canvas, pos: Option<(i32, i32)>) {
        if self.w != c.pw || self.h != c.ph {
            *self = Surface::new(c.pw, c.ph);
        }
        unsafe {
            if !self.bits.is_null() {
                std::ptr::copy_nonoverlapping(c.px.as_ptr(), self.bits, c.px.len().min((self.w * self.h) as usize));
            }
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let pt = pos.map(|(x, y)| POINT { x, y });
            let size = SIZE { cx: c.pw, cy: c.ph };
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
