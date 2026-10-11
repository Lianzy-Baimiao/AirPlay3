//! 原生界面主体：一组吸附在一起的分层窗口（播放条 + 歌词图片 + 均衡器 + 音乐列表），与原版布局一致。

use std::cell::RefCell;
use std::time::Instant;

use crossbeam_channel::Sender;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::canvas::Canvas;
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

impl Pane {
    pub fn size(self) -> (i32, i32) {
        match self {
            Pane::Bar => (bar::W, bar::H),
            Pane::Cover => (cover::W, cover::H),
            Pane::Eq => (eq::W, eq::H),
            Pane::List => (playlist::W, playlist::H),
        }
    }
    /// 相对播放条窗口左上角的偏移（原版默认布局）
    pub fn offset(self) -> (i32, i32) {
        match self {
            Pane::Bar => (0, 0),
            Pane::Cover => (0, 65),
            Pane::Eq => (0, 287),
            Pane::List => (301, 0),
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
    pub menu: Option<super::menu::Menu>,
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
        let origin = self.origin;
        for i in 0..self.wins.len() {
            let (pane, ref w) = self.wins[i];
            if !w.dirty || !w.visible {
                continue;
            }
            let mut canvas = std::mem::replace(&mut self.wins[i].1.canvas, Canvas::new(0, 0));
            self.paint(pane, &mut canvas);
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
            let (dx, dy) = pane.offset();
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
                let (dx, dy) = p.offset();
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
    let mut wins = Vec::new();
    let bar_hwnd = window::create("AirPlay3", x, y, bar::W, bar::H, true, None);
    for p in PANES {
        let (w, h) = p.size();
        let (dx, dy) = p.offset();
        let hwnd = if p == Pane::Bar { bar_hwnd } else { window::create("AirPlay3", x + dx, y + dy, w, h, false, Some(bar_hwnd)) };
        wins.push((p, Win { hwnd, canvas: Canvas::new(w, h), surface: Surface::new(w, h), dirty: true, visible: true }));
    }
    bar_slot.store(bar_hwnd.0 as isize, std::sync::atomic::Ordering::Release);
    let ui = super::state::UiState::new(&player);
    let (async_tx, async_rx) = crossbeam_channel::unbounded();
    let app = App { player, now, cmds, wins, origin: (x, y), ui, started: Instant::now(), quitting: false, menu: None, async_tx, async_rx };
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
