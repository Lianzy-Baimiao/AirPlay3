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

/// 可调整的面板尺寸（外框）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    pub list: (i32, i32),
    pub cover: (i32, i32),
    /// 播放条宽度（原版可调，高度固定）
    pub bar_w: i32,
}

impl Sizes {
    pub fn size(&self, p: Pane) -> (i32, i32) {
        match p {
            Pane::Bar => (self.bar_w, bar::H),
            Pane::Cover => self.cover,
            Pane::Eq => (eq::W, eq::H),
            Pane::List => self.list,
        }
    }
    /// 默认位置：相对播放条左上角（默认尺寸时与原版布局一致）
    pub fn default_offset(&self, p: Pane) -> (i32, i32) {
        match p {
            Pane::Bar => (0, 0),
            Pane::Cover => (0, 65),
            Pane::Eq => (0, 65 + self.cover.1 - 3),
            Pane::List => (self.bar_w - 3, 0),
        }
    }
    pub fn from_settings(w: &crate::config::WindowState) -> Sizes {
        let fit = |v: [i32; 2], (min, max): ((i32, i32), (i32, i32))| (v[0].clamp(min.0, max.0), v[1].clamp(min.1, max.1));
        Sizes { list: fit(w.list_size, (playlist::MIN, playlist::MAX)), cover: fit([w.cover_w, w.cover_h], (cover::MIN, cover::MAX)), bar_w: w.bar_w.clamp(bar::MIN_W, bar::MAX_W) }
    }
}

pub struct Win {
    pub hwnd: HWND,
    pub canvas: Canvas,
    pub surface: Surface,
    pub dirty: bool,
    pub visible: bool,
    /// 窗口左上角（屏幕坐标）
    pub pos: (i32, i32),
}

pub struct App {
    pub player: Player,
    pub now: SharedNow,
    pub cmds: Sender<Command>,
    pub wins: Vec<(Pane, Win)>,
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
    /// 系统媒体控制（「启用全局多媒体键盘支持」开启时）
    pub smtc: Option<crate::platform::smtc::Smtc>,
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

    pub fn win_ref(&self, p: Pane) -> &Win {
        &self.wins.iter().find(|(q, _)| *q == p).expect("pane").1
    }

    /// 播放条窗口左上角（屏幕坐标）
    pub fn bar_pos(&self) -> (i32, i32) {
        self.wins[0].1.pos
    }

    /// 面板外框（屏幕坐标）
    pub fn rect_of(&self, p: Pane) -> R {
        let (x, y) = self.win_ref(p).pos;
        let (w, h) = self.sizes.size(p);
        R::new(x, y, w, h)
    }

    /// 只移动一个面板
    pub fn place(&mut self, p: Pane, x: i32, y: i32) {
        let w = self.win(p);
        if w.pos == (x, y) {
            return;
        }
        w.pos = (x, y);
        unsafe {
            let _ = SetWindowPos(w.hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    /// 保存播放条位置、各面板相对播放条的位置与尺寸
    pub fn save_layout(&mut self) {
        let (bx, by) = self.bar_pos();
        let off = [Pane::Cover, Pane::Eq, Pane::List].map(|p| {
            let (x, y) = self.win_ref(p).pos;
            [x - bx, y - by]
        });
        let mut g = self.player.core.lock();
        let w = &mut g.settings.window;
        w.main = Some(crate::config::WinRect { x: bx as f32, y: by as f32, w: 625.0, h: 431.0 });
        w.list_size = [self.sizes.list.0, self.sizes.list.1];
        (w.cover_w, w.cover_h) = self.sizes.cover;
        w.bar_w = self.sizes.bar_w;
        w.pane_offsets = Some(off);
        g.touch_settings();
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
        // 配置窗口里的值可能被菜单、拖动改了
        if let Some(s) = self.settings_win.as_mut() {
            s.win.dirty = true;
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
            if super::resize::limits(pane).is_some() {
                super::resize::grip_margins(&mut canvas, pane);
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
            w.surface.present(w.hwnd, &canvas, w.pos.0, w.pos.1);
            w.canvas = canvas;
            w.dirty = false;
        }
    }

    /// 「恢复默认位置」：播放条回到主显示器中央，各面板回到默认的相对位置（大小不变）；迷你条一起回来
    pub fn reset_layout(&mut self) {
        let (x, y) = super::state::default_origin();
        for p in [Pane::Bar, Pane::Cover, Pane::Eq, Pane::List] {
            let (ox, oy) = self.sizes.default_offset(p);
            self.place(p, x + ox, y + oy);
        }
        self.reset_minibar();
        self.save_layout();
        self.player.save(false);
        self.dirty_all();
    }

    /// 把播放条移到 (x, y)，所有面板（含已脱离的）平移同样的距离（迷你条拖动时用）
    pub fn move_to(&mut self, x: i32, y: i32) {
        let (bx, by) = self.bar_pos();
        let (dx, dy) = (x - bx, y - by);
        for (_, w) in &mut self.wins {
            w.pos = (w.pos.0 + dx, w.pos.1 + dy);
            unsafe {
                let _ = SetWindowPos(w.hwnd, None, w.pos.0, w.pos.1, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
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

/// 矩形是否有一部分在某个显示器上（拔掉显示器后，留在那边的面板回到默认位置）
pub(super) fn on_screen(r: R) -> bool {
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromRect};
    let rc = RECT { left: r.x, top: r.y, right: r.right(), bottom: r.bottom() };
    !unsafe { MonitorFromRect(&rc, MONITOR_DEFAULTTONULL) }.is_invalid()
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
    let (sizes, saved) = {
        let g = player.core.lock();
        (Sizes::from_settings(&g.settings.window), g.settings.window.pane_offsets)
    };
    let mut wins = Vec::new();
    let bar_hwnd = window::create("AirPlay3", x, y, sizes.bar_w, bar::H, true, None);
    for p in PANES {
        let (w, h) = sizes.size(p);
        // 上次保存的位置（面板可能已脱离）；不在任何显示器上时回到默认位置
        let saved = match p {
            Pane::Cover => saved.map(|o| o[0]),
            Pane::Eq => saved.map(|o| o[1]),
            Pane::List => saved.map(|o| o[2]),
            Pane::Bar => None,
        };
        let def = sizes.default_offset(p);
        let (dx, dy) = saved.map(|o| (o[0], o[1])).filter(|&(dx, dy)| on_screen(R::new(x + dx, y + dy, w, h))).unwrap_or(def);
        let (px, py) = (x + dx, y + dy);
        let hwnd = if p == Pane::Bar { bar_hwnd } else { window::create("AirPlay3", px, py, w, h, false, Some(bar_hwnd)) };
        wins.push((p, Win { hwnd, canvas: Canvas::new(w, h), surface: Surface::new(w, h), dirty: true, visible: true, pos: (px, py) }));
    }
    bar_slot.store(bar_hwnd.0 as isize, std::sync::atomic::Ordering::Release);
    let ui = super::state::UiState::new(&player);
    let (async_tx, async_rx) = crossbeam_channel::unbounded();
    let app = App { player, now, cmds, wins, ui, started: Instant::now(), quitting: false, sizes, menu: None, search: None, settings_win: None, hotkey_failed: Vec::new(), desk: None, taskbar: None, minibar: None, smtc: None, async_tx, async_rx };
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
