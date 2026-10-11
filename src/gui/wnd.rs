//! 窗口过程：把 Win32 消息分发给 App。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::app::{APP, App, Pane, TIMER_ANIM, WM_APP_ASYNC, WM_APP_REPAINT, WM_APP_TRAY};

const WM_MOUSELEAVE: u32 = 0x02A3;

pub fn lo(l: LPARAM) -> i32 {
    (l.0 & 0xFFFF) as i16 as i32
}
pub fn hi(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xFFFF) as i16 as i32
}

pub unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // 重入（例如 SetWindowPos 同步发送的消息）时 App 已被借用：交给默认处理
    let r = APP.with(|a| {
        let Ok(mut g) = a.try_borrow_mut() else { return None };
        let app = g.as_mut()?;
        let r = app.handle(hwnd, msg, wp, lp);
        app.flush();
        r
    });
    match r {
        Some(v) => v,
        None => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// 创建后：显示窗口、启动动画定时器、托盘、热键、拖放
pub fn after_create(start_hidden: bool) {
    APP.with(|a| {
        let mut g = a.borrow_mut();
        let Some(app) = g.as_mut() else { return };
        let bar = app.bar_hwnd();
        let show = app.ui.snap.show;
        let s = app.player.core.lock().settings.window.clone();
        app.tick();
        app.flush();
        unsafe {
            for (p, w) in &app.wins {
                DragAcceptFiles(w.hwnd, true);
                let on = match p {
                    Pane::Bar => true,
                    Pane::Cover => s.show_cover,
                    Pane::Eq => s.show_eq,
                    Pane::List => s.show_list,
                };
                if !start_hidden && on {
                    let _ = ShowWindow(w.hwnd, if *p == Pane::Bar { SW_SHOW } else { SW_SHOWNOACTIVATE });
                }
            }
            SetTimer(Some(bar), TIMER_ANIM, 30, None);
        }
        let _ = show;
        for (i, p) in [Pane::Cover, Pane::Eq, Pane::List].into_iter().enumerate() {
            app.win(p).visible = [s.show_cover, s.show_eq, s.show_list][i];
        }
        super::tray::add(bar);
        app.register_hotkeys();
        app.apply_topmost();
    });
}

impl App {
    pub fn handle(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if let Some(r) = self.menu_msg(hwnd, msg, wp, lp) {
            return Some(r);
        }
        let pane = self.pane_of(hwnd)?;
        match msg {
            WM_TIMER if wp.0 == TIMER_ANIM => self.tick(),
            WM_APP_REPAINT => self.tick(),
            WM_APP_ASYNC => self.drain_async(),
            WM_APP_TRAY => self.on_tray(lp.0 as u32),
            WM_MOUSEMOVE => {
                self.track_leave(hwnd);
                self.mouse_move(pane, lo(lp), hi(lp));
            }
            WM_MOUSELEAVE => self.mouse_leave(pane),
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                unsafe { SetCapture(hwnd) };
                let ctrl = wp.0 & 0x0008 != 0;
                let shift = wp.0 & 0x0004 != 0;
                self.mouse_down(pane, lo(lp), hi(lp), msg == WM_LBUTTONDBLCLK, ctrl, shift);
            }
            WM_LBUTTONUP => {
                unsafe {
                    let _ = ReleaseCapture();
                }
                self.mouse_up(pane, lo(lp), hi(lp));
            }
            WM_RBUTTONUP => self.right_click(pane, lo(lp), hi(lp)),
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as i32;
                let mut pt = POINT { x: lo(lp), y: hi(lp) };
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);
                }
                self.wheel(pane, pt.x, pt.y, delta);
            }
            WM_CHAR => self.on_char(wp.0 as u32),
            WM_KEYDOWN => self.on_key(VIRTUAL_KEY(wp.0 as u16)),
            WM_DROPFILES => {
                let files = drop_files(HDROP(wp.0 as _));
                self.drop_files(pane, files);
            }
            WM_HOTKEY => self.on_hotkey(wp.0 as i32),
            WM_SETCURSOR => {
                unsafe { SetCursor(LoadCursorW(None, IDC_ARROW).ok()) };
                return Some(LRESULT(1));
            }
            WM_CLOSE => {
                self.quit();
                return Some(LRESULT(0));
            }
            WM_DESTROY if pane == Pane::Bar => {
                super::tray::remove(hwnd);
                unsafe { PostQuitMessage(0) };
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn track_leave(&mut self, hwnd: HWND) {
        let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
        unsafe {
            let _ = TrackMouseEvent(&mut t);
        }
    }

    pub fn quit(&mut self) {
        if self.quitting {
            return;
        }
        self.quitting = true;
        {
            let mut g = self.player.core.lock();
            let (x, y) = self.origin;
            g.settings.window.main = Some(crate::config::WinRect { x: x as f32, y: y as f32, w: 625.0, h: 431.0 });
            g.touch_settings();
        }
        self.player.snapshot_last();
        self.player.save(true);
        crate::platform::hotkeys::unregister_all(self.bar_hwnd().0 as isize);
        unsafe {
            let _ = DestroyWindow(self.bar_hwnd());
        }
    }
}

fn drop_files(h: HDROP) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    unsafe {
        let n = DragQueryFileW(h, u32::MAX, None);
        for i in 0..n {
            let len = DragQueryFileW(h, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(h, i, Some(&mut buf));
            out.push(std::path::PathBuf::from(String::from_utf16_lossy(&buf[..len])));
        }
        DragFinish(h);
    }
    out
}
