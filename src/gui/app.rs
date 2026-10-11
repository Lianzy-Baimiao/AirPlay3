//! 原生界面主体：一组吸附在一起的分层窗口（播放条 + 歌词图片 + 均衡器 + 音乐列表），与原版布局一致。

use std::cell::RefCell;
use std::time::Instant;

use crossbeam_channel::Sender;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::canvas::{Canvas, R};
use super::panels::{bar, cover, eq, playlist};
use super::window::{self, Surface};
use crate::controller::Command;
use crate::player::Player;
use crate::player::nowplaying::SharedNow;

pub const WM_APP_REPAINT: u32 = WM_APP + 1;
pub const WM_APP_ASYNC: u32 = WM_APP + 2;
pub const WM_APP_TRAY: u32 = WM_APP + 3;
pub const TIMER_ANIM: usize = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Bar,
    Cover,
    Eq,
    List,
}

pub const PANES: [Pane; 4] = [Pane::Bar, Pane::Cover, Pane::Eq, Pane::List];

/// 可调整的面板尺寸
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    pub list: (i32, i32),
    pub cover_h: i32,
}

impl Sizes {
    pub fn size(&self, p: Pane) -> (i32, i32) {
        match p {
            Pane::Bar => (bar::W, bar::H),
            Pane::Cover => (cover::W, self.cover_h),
            Pane::Eq => (eq::W, eq::H),
            Pane::List => self.list,
        }
    }
    /// 相对播放条窗口左上角的偏移（默认尺寸时与原版布局一致；歌词图片变高时均衡器随之下移）
    pub fn offset(&self, p: Pane) -> (i32, i32) {
        match p {
            Pane::Bar => (0, 0),
            Pane::Cover => (0, 65),
            Pane::Eq => (0, 65 + self.cover_h - 3),
            Pane::List => (301, 0),
        }
    }
    pub fn from_settings(w: &crate::config::WindowState) -> Sizes {
        Sizes {
            list: (w.list_size[0].clamp(playlist::MIN.0, 2000), w.list_size[1].clamp(playlist::MIN.1, 2000)),
            cover_h: w.cover_h.clamp(cover::MIN_H, cover::MAX_H),
        }
    }
}

pub struct Win {
    pub hwnd: HWND,
    pub canvas: Canvas,
    pub surface: Surface,
    pub dirty: bool,
    pub visible: bool,
}

pub struct App {
    pub player: Player,
    pub now: SharedNow,
    pub cmds: Sender<Command>,
    pub wins: Vec<(Pane, Win)>,
    /// 播放条窗口左上角（屏幕坐标）
    pub origin: (i32, i32),
    pub ui: super::state::UiState,
    pub started: Instant,
    pub quitting: bool,
    pub sizes: Sizes,
    pub menu: Option<super::menu::Menu>,
    pub search: Option<super::lyricwin::LyricSearch>,
    pub settings_win: Option<super::settingswin::SettingsWin>,
    pub desk: Option<super::desklyric::DeskLyric>,
    pub taskbar: Option<super::taskbar::TaskbarLyric>,
    pub minibar: Option<super::minibar::Minibar>,
    pub hotkey_failed: Vec<crate::config::HotkeyAction>,
    pub async_tx: crossbeam_channel::Sender<super::actions::AsyncMsg>,
    pub async_rx: crossbeam_channel::Receiver<super::actions::AsyncMsg>,
}

thread_local! {
    pub static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

impl App {
    pub fn win(&mut self, p: Pane) -> &mut Win {
        &mut self.wins.iter_mut().find(|(q, _)| *q == p).expect("pane").1
    }

    pub fn pane_of(&self, hwnd: HWND) -> Option<Pane> {
        self.wins.iter().find(|(_, w)| w.hwnd == hwnd).map(|(p, _)| *p)
    }

    pub fn bar_hwnd(&self) -> HWND {
        self.wins[0].1.hwnd
    }

    pub fn dirty(&mut self, p: Pane) {
        self.win(p).dirty = true;
    }

    pub fn dirty_all(&mut self) {
        for (_, w) in &mut self.wins {
            w.dirty = true;
        }
    }

    /// 把所有需要重绘的窗口画好并提交
    pub fn flush(&mut self) {
        if self.search.as_ref().is_some_and(|s| s.win.dirty) {
            self.paint_search();
        }
        if self.settings_win.as_ref().is_some_and(|s| s.win.dirty) {
            self.paint_settings();
        }
        let origin = self.origin;
        for i in 0..self.wins.len() {
            let (pane, ref w) = self.wins[i];
            if !w.dirty || !w.visible {
                continue;
            }
            let (cw, ch) = self.sizes.size(pane);
            let mut canvas = std::mem::replace(&mut self.wins[i].1.canvas, Canvas::new(0, 0));
            if canvas.w != cw || canvas.h != ch {
                canvas = Canvas::new(cw, ch);
            }
            self.paint(pane, &mut canvas);
            if matches!(pane, Pane::List | Pane::Cover) {
                let (w, h) = (canvas.w, canvas.h);
                canvas.hit_margin(R::new(0, h - 5, w, 5));
                if pane == Pane::List {
                    canvas.hit_margin(R::new(w - 5, 0, 5, h));
                }
            }
            // 仅调试构建：AIRPLAY3_DUMP=<目录> 时把各面板（含透明通道）存成 PNG，用于制作说明文档截图
            #[cfg(debug_assertions)]
            if let Some(dir) = std::env::var_os("AIRPLAY3_DUMP") {
                let dir = std::path::PathBuf::from(dir);
                let tmp = dir.join(format!("{pane:?}.tmp.png"));
                if canvas.save_png(&tmp).is_ok() {
                    let _ = std::fs::rename(&tmp, dir.join(format!("{pane:?}.png")));
                }
            }
            let w = &mut self.wins[i].1;
            let (dx, dy) = self.sizes.offset(pane);
            w.surface.present(w.hwnd, &canvas, origin.0 + dx, origin.1 + dy);
            w.canvas = canvas;
            w.dirty = false;
        }
    }

    /// 移动整组窗口
    pub fn move_to(&mut self, x: i32, y: i32) {
        self.origin = (x, y);
        unsafe {
            for (p, w) in &self.wins {
                let (dx, dy) = self.sizes.offset(*p);
                let _ = SetWindowPos(w.hwnd, None, x + dx, y + dy, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
    }

    pub fn set_visible(&mut self, p: Pane, on: bool) {
        let w = self.win(p);
        w.visible = on;
        w.dirty = on;
        unsafe {
            let _ = ShowWindow(w.hwnd, if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }
}

/// 任意线程：请求界面刷新
pub fn post_repaint(bar: isize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(bar as _)), WM_APP_REPAINT, WPARAM(0), LPARAM(0));
    }
}

/// 创建窗口并运行消息循环（阻塞直到退出）。`bar_slot` 用于让后台线程投递重绘消息。
pub fn run(player: Player, now: SharedNow, cmds: Sender<Command>, start_hidden: bool, bar_slot: &std::sync::atomic::AtomicIsize) {
    // 与原版一致：声明 DPI 感知，按 1:1 像素绘制（皮肤图片不缩放）
    unsafe {
        use windows::Win32::UI::HiDpi::*;
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
    }
    window::register(Some(super::wnd::wndproc));
    let (x, y) = super::state::initial_origin(&player);
    let sizes = Sizes::from_settings(&player.core.lock().settings.window);
    let mut wins = Vec::new();
    let bar_hwnd = window::create("AirPlay3", x, y, bar::W, bar::H, true, None);
    for p in PANES {
        let (w, h) = sizes.size(p);
        let (dx, dy) = sizes.offset(p);
        let hwnd = if p == Pane::Bar { bar_hwnd } else { window::create("AirPlay3", x + dx, y + dy, w, h, false, Some(bar_hwnd)) };
        wins.push((p, Win { hwnd, canvas: Canvas::new(w, h), surface: Surface::new(w, h), dirty: true, visible: true }));
    }
    bar_slot.store(bar_hwnd.0 as isize, std::sync::atomic::Ordering::Release);
    let ui = super::state::UiState::new(&player);
    let (async_tx, async_rx) = crossbeam_channel::unbounded();
    let app = App { player, now, cmds, wins, origin: (x, y), ui, started: Instant::now(), quitting: false, sizes, menu: None, search: None, settings_win: None, hotkey_failed: Vec::new(), desk: None, taskbar: None, minibar: None, async_tx, async_rx };
    APP.with(|a| *a.borrow_mut() = Some(app));
    super::wnd::after_create(start_hidden);
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    APP.with(|a| a.borrow_mut().take());
}
