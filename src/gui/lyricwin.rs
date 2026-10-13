//! 歌词图片面板的动作：下载专辑图片、关联本地图片 / 歌词文件（「下载歌词」窗口见 `lyricdlwin`）。

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::actions::AsyncMsg;
use super::app::{App, WM_APP_ASYNC};

pub(super) fn clipboard_text() -> Option<String> {
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    unsafe {
        OpenClipboard(None).ok()?;
        let h = GetClipboardData(13).ok(); // CF_UNICODETEXT
        let s = h.and_then(|h| {
            let p = GlobalLock(HGLOBAL(h.0)) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(HGLOBAL(h.0));
            Some(s)
        });
        let _ = CloseClipboard();
        s
    }
}

impl App {
    /// 「下载专辑图片」：后台从网上找图片，保存到本地并关联到当前歌曲
    pub fn download_cover(&mut self) {
        let Some(id) = self.ui.snap.current else { return };
        self.ui.cover_status = Some(("正在下载专辑图片…".into(), std::time::Instant::now()));
        self.dirty(super::app::Pane::Cover);
        let (p, now, tx, hwnd) = (self.player.clone(), self.now.clone(), self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| p.download_cover(id, &now))).unwrap_or_else(|_| Err("内部错误".into()));
            let _ = tx.send(AsyncMsg::CoverDone(r));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
            }
        });
    }

    /// 选择本地图片关联到当前歌曲
    pub fn pick_cover_file(&mut self) {
        let Some(id) = self.ui.snap.current else { return };
        let (tx, hwnd) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            let f = rfd::FileDialog::new()
                .set_parent(&crate::platform::window::Parent(hwnd))
                .set_title("关联图片文件")
                .add_filter("图片文件 (*.jpg;*.png;*.bmp;*.gif;*.webp)", &["jpg", "jpeg", "png", "bmp", "gif", "webp"])
                .pick_file();
            if let Some(f) = f {
                let _ = tx.send(AsyncMsg::CoverFile(id, f));
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
                }
            }
        });
    }

    /// 选择本地 .lrc 文件关联到当前歌曲
    pub fn pick_lyric_file(&mut self) {
        let Some(id) = self.ui.snap.current else { return };
        let (tx, hwnd) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            let f = rfd::FileDialog::new()
                .set_parent(&crate::platform::window::Parent(hwnd))
                .set_title("关联歌词文件")
                .add_filter("歌词文件 (*.lrc;*.txt)", &["lrc", "txt"])
                .pick_file();
            if let Some(f) = f {
                let _ = tx.send(AsyncMsg::LyricFile(id, f));
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
                }
            }
        });
    }
}
