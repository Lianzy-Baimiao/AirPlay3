//! 窗口过程：把 Win32 消息分发给 App。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::app::{APP, App, Pane, TIMER_ANIM, WM_APP_ASYNC, WM_APP_REPAINT, WM_APP_TRAY};

/// 调试构建的自测：等有歌曲后打开「下载歌词」窗口
#[cfg(debug_assertions)]
const TIMER_TEST_SEARCH: usize = 7;

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
        app.sync_desk_lyric();
        app.sync_taskbar_lyric();
        // 仅调试构建：自动化自测时直接打开某个窗口
        #[cfg(debug_assertions)]
        match std::env::var("AIRPLAY3_TEST_OPEN").as_deref() {
            Ok("settings") => app.open_settings(),
            Ok(s) if s.starts_with("settings:") => app.act(super::menu::Act::SettingsTab(s[9..].parse().unwrap_or(0))),
            Ok("minibar") => app.toggle_minibar(),
            Ok("search") => unsafe {
                SetTimer(Some(app.bar_hwnd()), TIMER_TEST_SEARCH, 200, None);
            },
            Ok("radio") => app.open_radio(),
            _ => {}
        }
        #[cfg(debug_assertions)]
        if let Ok(spec) = std::env::var("AIRPLAY3_TEST_DRAG") {
            app.test_drags(&spec);
        }
    });
}

impl App {
    pub fn handle(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if let Some(r) = self.menu_msg(hwnd, msg, wp, lp) {
            return Some(r);
        }
        if let Some(r) = self.search_msg(hwnd, msg, wp, lp) {
            return Some(r);
        }
        if let Some(r) = self.settings_msg(hwnd, msg, wp, lp) {
            return Some(r);
        }
        if let Some(r) = self.radio_msg(hwnd, msg, wp, lp) {
            return Some(r);
        }
        if let Some(r) = self.desk_msg(hwnd, msg, wp, lp) {
            return Some(r);
        }
        if let Some(r) = self.taskbar_msg(hwnd, msg, wp, lp) {
            return Some(r);
        }
        if msg == super::taskbar::taskbar_created_msg() && hwnd == self.bar_hwnd() {
            // 资源管理器重启：重新加托盘图标、重新嵌入任务栏
            super::tray::add(hwnd);
            self.taskbar = None;
            self.sync_taskbar_lyric();
            return Some(LRESULT(0));
        }
        if let Some(r) = self.minibar_msg(hwnd, msg, wp, lp) {
            return Some(r);
        }
        let pane = self.pane_of(hwnd)?;
        match msg {
            WM_TIMER if wp.0 == TIMER_ANIM => self.tick(),
            // 仅调试构建：等开始播放后再打开「下载歌词」窗口（AIRPLAY3_TEST_OPEN=search）
            #[cfg(debug_assertions)]
            WM_TIMER if wp.0 == TIMER_TEST_SEARCH => {
                if self.ui.snap.current.is_some() {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), TIMER_TEST_SEARCH);
                    }
                    self.open_lyric_search();
                }
            }
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
                // 全局鼠标滚轮关着时，只有本程序在前台才响应滚轮
                if !self.player.core.lock().settings.general.global_wheel && !super::window::foreground_is_ours() {
                    return Some(LRESULT(0));
                }
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
            crate::platform::mediakeys::WM_APP_MEDIAKEY => {
                use crate::controller::Command as C;
                let c = match wp.0 {
                    0 => C::Mute,
                    1 => C::VolumeDown,
                    _ => C::VolumeUp,
                };
                let _ = self.cmds.send(c);
            }
            WM_COPYDATA => {
                // 第二个实例转交的文件
                let Some(files) = crate::platform::instance::parse(lp) else { return None };
                if !files.is_empty() {
                    crate::controller::open_files(&self.player, files);
                }
                self.restore();
                return Some(LRESULT(1));
            }
            WM_SETCURSOR => {
                let (cx, cy) = super::resize::cursor();
                let mut pt = POINT { x: cx, y: cy };
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);
                    SetCursor(LoadCursorW(None, self.cursor_for(pane, pt.x, pt.y)).ok());
                }
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

    pub(super) fn track_leave(&mut self, hwnd: HWND) {
        // 仅调试构建：自测脚本用投递的鼠标消息模拟悬停，此时真实光标不在窗口上，不能注册离开通知
        #[cfg(debug_assertions)]
        if std::env::var_os("AIRPLAY3_TEST_NOLEAVE").is_some() {
            return;
        }
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
        self.save_layout();
        self.player.snapshot_last();
        self.player.stop();
        self.player.save(true);
        let bar = self.bar_hwnd();
        crate::platform::hotkeys::unregister_all(bar.0 as isize);
        super::tray::remove(bar);
        self.close_menu();
        // 注意：此时仍在窗口过程中（App 已借用），DestroyWindow 同步发送的 WM_DESTROY 会走默认处理，
        // 所以退出消息必须在这里直接投递，否则消息循环不结束、进程留在后台继续播放。
        unsafe {
            for (_, w) in &self.wins {
                let _ = ShowWindow(w.hwnd, SW_HIDE);
            }
            PostQuitMessage(0);
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
