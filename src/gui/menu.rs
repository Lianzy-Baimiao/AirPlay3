//! 原版风格的弹出菜单（自绘分层窗口：深色底、灰色描边、高亮条、右侧勾选、子菜单箭头）；
//! 四周 36 像素的柔和阴影是另一个点不到（鼠标穿透）的窗口，与原版的四个阴影窗口相同。

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::theme;
use super::canvas::{Canvas, R};
use super::text::{self, Font, Hint};
use super::window::{self, Surface};
use crate::config::PlayMode;
use crate::library::SortBy;
use crate::skin::{Cell, icons};

const FONT: Font = Font::px(12.0, false);
const ITEM_H: i32 = 20;
const SEP_H: i32 = 9;

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    TogglePane(usize),
    Minimize,
    ToggleTopmost,
    CycleMode,
    SetMode(PlayMode),
    EqToggle,
    EqReset,
    EqPresetMenu,
    EqPreset(String),
    MainMenu,
    RowMenu,
    AddMenu,
    EditMenu,
    SortMenu,
    AddFiles,
    AddFolder,
    RemoveRows(Vec<crate::library::TrackId>),
    RemoveSelected,
    RemoveBroken,
    SelectAll,
    SelectNone,
    SelectInvert,
    Sort(SortBy),
    ClearList,
    RevealSelected,
    RecycleSelected,
    OpenConfig,
    /// 列表面板换到「网络电台」页签（true）/「音乐列表」（false）
    RadioTab(bool),
    RadioMenu,
    RadioPaste,
    RadioPlay(usize),
    RadioCopy(usize),
    RadioRemove(usize),
    RadioRemoveList(usize),
    /// 主界面 / 迷你条回到默认位置（拖到看不见的地方时用）
    ResetPosition,
    Quit,
    LyricMenu,
    LyricSearch,
    LyricFile,
    LyricUnlink,
    LyricOffset(i64),
    CoverDownload,
    CoverFile,
    CoverUnlink,
    LyricAuto,
    LyricDir,
    SetDevice(String),
    SetProxy(crate::config::ProxyKind),
    SetLyricSource(crate::lyrics::online::Provider),
    DeskToggle,
    DeskLock,
    /// 桌面歌词显示几行
    DeskLines(usize),
    /// 桌面歌词配色；None = 换下一种
    DeskStyle(Option<usize>),
    /// 桌面歌词字号增减（行数不变，窗口随之变高 / 变矮）
    DeskFont(f32),
    DeskMenu,
    TaskbarToggle,
    TaskbarMenu,
    TaskbarAlign(u8),
    TaskbarFont(f32),
    /// 任务栏歌词回到当前对齐方式的默认位置
    TaskbarReset,
    TaskbarTwoLines,
    /// 打开「配置」的某个分页
    SettingsTab(usize),
    Minibar,
    /// 迷你条模式：打开 / 关掉歌词条（原版右键菜单「迷你歌词」）
    MiniStrip,
    /// 迷你条模式：单独打开 / 关掉一个面板（app.wins 的下标）
    MiniPane(usize),
}

#[derive(Clone)]
pub enum Item {
    Do { label: String, icon: Option<Cell>, check: bool, act: Act },
    Sub { label: String, items: Vec<Item> },
    Sep,
    /// 界面缩放：「界面缩放 125%」一行，下面是 − ━━●━━ + 的横向滑块（25% 一档，系统的缩放那一档是焦点色）
    Zoom,
}

/// 缩放那一行的高度、滑块各部分的位置（相对这一行，逻辑像素）
const ZOOM_H: i32 = 40;
const ZOOM_MINUS: R = R::new(16, 21, 16, 15);
const ZOOM_TRACK_X: i32 = 40;
const ZOOM_TRACK_Y: i32 = 28;
const ZOOM_BTN_W: i32 = 16;

/// 滑块可以停的档：100% ~ 300% 每 25% 一档，再加上系统的缩放（不是 25 的倍数时）
pub fn zoom_stops() -> Vec<u32> {
    let sys = super::scale::system().clamp(super::scale::MIN, super::scale::MAX);
    let mut v: Vec<u32> = (super::scale::MIN..=super::scale::MAX).step_by(super::scale::STEP as usize).collect();
    if !v.contains(&sys) {
        v.push(sys);
        v.sort_unstable();
    }
    v
}

/// 当前缩放往大 / 往小一档
pub fn zoom_step(cur: u32, up: bool) -> u32 {
    let stops = zoom_stops();
    if up { stops.iter().copied().find(|&s| s > cur).unwrap_or(cur) } else { stops.iter().rev().copied().find(|&s| s < cur).unwrap_or(cur) }
}

/// 滑块轨道（菜单宽 w 时）：左右端的 x
fn zoom_track(w: i32) -> (i32, i32) {
    (ZOOM_TRACK_X, w - ZOOM_TRACK_X)
}

/// 缩放 p 在轨道上的 x
fn zoom_x(w: i32, p: u32) -> i32 {
    let (x0, x1) = zoom_track(w);
    let t = (p - super::scale::MIN) as f32 / (super::scale::MAX - super::scale::MIN) as f32;
    x0 + ((x1 - x0) as f32 * t).round() as i32
}

/// 轨道上 x 处最近的一档
fn zoom_at(w: i32, x: i32) -> u32 {
    zoom_stops().into_iter().min_by_key(|&p| (zoom_x(w, p) - x).abs()).unwrap_or(super::scale::MIN)
}

/// 缩放那一行里点到了什么
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZoomHit {
    Minus,
    Plus,
    /// 轨道上（按住可以拖），值是最近的一档
    Track(u32),
}

pub fn item(label: &str, act: Act) -> Item {
    Item::Do { label: label.into(), icon: None, check: false, act }
}

pub fn check(label: &str, icon: Option<Cell>, on: bool, act: Act) -> Item {
    Item::Do { label: label.into(), icon, check: on, act }
}

pub struct Menu {
    pub hwnd: HWND,
    pub items: Vec<Item>,
    pub hover: Option<usize>,
    pub canvas: Canvas,
    surface: Surface,
    pub pos: (i32, i32),
    pub child: Option<Box<Menu>>,
    pub opened: std::time::Instant,
    /// 菜单下面的阴影窗口（鼠标穿透）
    shadow: HWND,
    shadow_surface: Surface,
    /// 正在拖缩放滑块：滑块现在指着的档（松手才换缩放，拖的时候菜单不变大小）
    pub zoom_drag: Option<u32>,
    /// 鼠标在缩放那一行的 − / + 上
    pub zoom_hover: Option<ZoomHit>,
    /// 在缩放那一行按下的：松手时不当作点了菜单项（按下时缩放已经变了，菜单的行也挪了）
    pub zoom_press: bool,
}

/// 阴影比菜单每边宽多少（原版的四个阴影窗口都是 36 像素）
const SHADOW: i32 = 36;

/// 在菜单 (x, y, w, h) 四周画阴影的窗口：鼠标点上去穿透到下面（原版的阴影窗口是 WS_EX_TRANSPARENT）
fn shadow_window(x: i32, y: i32, w: i32, h: i32, owner: HWND) -> (HWND, Surface) {
    let hwnd = window::create("", x - SHADOW, y - SHADOW, w + 2 * SHADOW, h + 2 * SHADOW, false, Some(owner));
    window::set_frost(hwnd, false);
    let mut s = Surface::new(1, 1);
    unsafe {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex | (WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE).0 as i32);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        draw_shadow(hwnd, &mut s, x, y, w, h);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    (hwnd, s)
}

/// 画菜单 (x, y, w, h) 的阴影并提交到阴影窗口（换了缩放时也用它重画）
fn draw_shadow(hwnd: HWND, s: &mut Surface, x: i32, y: i32, w: i32, h: i32) {
    let mut c = Canvas::new(w + 2 * SHADOW, h + 2 * SHADOW);
    super::panels::lyricdl::shadow(&mut c, R::new(SHADOW, SHADOW, w, h));
    s.present(hwnd, &c, x - SHADOW, y - SHADOW);
}

fn layout(items: &[Item]) -> (i32, Vec<(i32, i32)>) {
    let mut y = 3;
    let mut rows = Vec::new();
    let mut w = 0f32;
    let mut min_w = 125;
    for it in items {
        let h = match it {
            Item::Sep => SEP_H,
            Item::Zoom => ZOOM_H,
            _ => ITEM_H,
        };
        match it {
            Item::Do { label, .. } | Item::Sub { label, .. } => w = w.max(text::measure(label, FONT).0),
            // 滑块要宽一点才好拖（每档约 10 像素）
            Item::Zoom => min_w = 160,
            Item::Sep => {}
        }
        rows.push((y, h));
        y += h;
    }
    ((w as i32 + 72).max(min_w), rows)
}

/// 缩放那一行：上面「界面缩放」和百分比（跟随系统时是焦点色），下面 − 轨道 +
fn draw_zoom(c: &mut Canvas, y: i32, cur: u32, hover: Option<ZoomHit>) {
    use super::canvas::Rgba;
    let (w, sys) = (c.w, super::scale::system());
    let follow = cur == sys;
    c.text("界面缩放", FONT, 22, y + 2, theme::g(232), Hint::ClearType);
    let pct = format!("{cur}%");
    let pw = text::measure(&pct, FONT).0.round() as i32;
    c.text(&pct, FONT, w - 16 - pw, y + 2, if follow { theme::focus() } else { theme::g(200) }, Hint::ClearType);
    // − 和 +：小方块按钮，鼠标在上面时亮一点
    let plus = R::new(w - ZOOM_MINUS.x - ZOOM_BTN_W, ZOOM_MINUS.y, ZOOM_BTN_W, ZOOM_MINUS.h);
    for (r, hot, is_plus) in [(ZOOM_MINUS, hover == Some(ZoomHit::Minus), false), (plus, hover == Some(ZoomHit::Plus), true)] {
        let r = R::new(r.x, y + r.y, r.w, r.h);
        c.round_rect(r, 3.0, Some(theme::g(if hot { 85 } else { 52 })), Some(theme::g(if hot { 120 } else { 90 })));
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        c.fill(R::new(cx - 4, cy, 8, 1), theme::g(225));
        if is_plus {
            c.fill(R::new(cx, cy - 4, 1, 8), theme::g(225));
        }
    }
    // 轨道：每档一个刻度，系统的缩放那一档是焦点色、长一点；左边走过的部分亮一点
    let (x0, x1) = zoom_track(w);
    let ty = y + ZOOM_TRACK_Y;
    let kx = zoom_x(w, cur);
    c.round_rect(R::new(x0 - 1, ty - 1, x1 - x0 + 3, 3), 1.5, Some(theme::g(70)), None);
    c.round_rect(R::new(x0 - 1, ty - 1, kx - x0 + 2, 3), 1.5, Some(theme::g(150)), None);
    for p in zoom_stops() {
        let x = zoom_x(w, p);
        if p == sys {
            c.fill(R::new(x, ty - 5, 1, 4), theme::focus());
        } else {
            c.fill(R::new(x, ty - 3, 1, 2), theme::g(110));
        }
    }
    c.round_rect(R::new(kx - 4, ty - 4, 9, 9), 4.5, Some(theme::g(235)), Some(Rgba(0, 0, 0, 90)));
}

impl Menu {
    pub fn open(items: Vec<Item>, x: i32, y: i32, owner: HWND) -> Menu {
        let (w, rows) = layout(&items);
        let h = rows.last().map_or(6, |(y, h)| y + h + 3);
        // 不超出鼠标所在显示器
        let a = super::popup::work_area_at((x, y));
        let (x, y) = (x.min(a.right() - w).max(a.x), y.min(a.bottom() - h).max(a.y));
        // 阴影先出来，菜单盖在它上面
        let (shadow, shadow_surface) = shadow_window(x, y, w, h, owner);
        let hwnd = window::create("", x, y, w, h, false, Some(owner));
        unsafe {
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex | WS_EX_TOPMOST.0 as i32);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        }
        let mut m = Menu {
            hwnd,
            items,
            hover: None,
            canvas: Canvas::new(w, h),
            surface: Surface::new(w, h),
            pos: (x, y),
            child: None,
            opened: std::time::Instant::now(),
            shadow,
            shadow_surface,
            zoom_drag: None,
            zoom_hover: None,
            zoom_press: false,
        };
        m.present();
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        m
    }

    pub fn row_at(&self, y: i32) -> Option<usize> {
        let (_, rows) = layout(&self.items);
        rows.iter().position(|(ry, h)| y >= *ry && y < ry + h).filter(|i| !matches!(self.items[*i], Item::Sep))
    }

    pub fn row_rect(&self, i: usize) -> R {
        let (w, rows) = layout(&self.items);
        R::new(1, rows[i].0, w - 2, rows[i].1)
    }

    /// (x, y)（菜单里的逻辑坐标）在缩放那一行的哪里
    pub fn zoom_hit(&self, x: i32, y: i32) -> Option<ZoomHit> {
        let i = self.row_at(y).filter(|&i| matches!(self.items[i], Item::Zoom))?;
        let ry = y - self.row_rect(i).y;
        let w = self.canvas.w;
        let plus = R::new(w - ZOOM_MINUS.x - ZOOM_BTN_W, ZOOM_MINUS.y, ZOOM_BTN_W, ZOOM_MINUS.h);
        let (x0, x1) = zoom_track(w);
        if ZOOM_MINUS.contains(x, ry) {
            Some(ZoomHit::Minus)
        } else if plus.contains(x, ry) {
            Some(ZoomHit::Plus)
        } else if ry >= ZOOM_MINUS.y - 2 && x >= x0 - 6 && x <= x1 + 6 {
            Some(ZoomHit::Track(zoom_at(w, x)))
        } else {
            None
        }
    }

    /// 拖滑块时滑块指着的档（不改缩放）
    pub fn zoom_drag_to(&mut self, x: i32) {
        let p = zoom_at(self.canvas.w, x);
        if self.zoom_drag != Some(p) {
            self.zoom_drag = Some(p);
            self.present();
        }
    }

    /// 界面缩放从 old 换成了现在的：菜单留在屏幕上原来的地方，按新缩放重新排、重画（子菜单关掉）
    pub fn rescale(&mut self, old: f32) {
        if let Some(mut c) = self.child.take() {
            c.close();
        }
        use super::scale::{rnd, to_logical, to_px};
        let (px, py) = (rnd(self.pos.0 as f32 * old), rnd(self.pos.1 as f32 * old));
        let (w, rows) = layout(&self.items);
        let h = rows.last().map_or(6, |(y, h)| y + h + 3);
        let a = super::popup::work_area_at((to_logical(px), to_logical(py)));
        let (x, y) = (to_logical(px).min(a.right() - w).max(a.x), to_logical(py).min(a.bottom() - h).max(a.y));
        self.pos = (x, y);
        self.canvas = Canvas::new(w, h);
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, to_px(x), to_px(y), 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        draw_shadow(self.shadow, &mut self.shadow_surface, x, y, w, h);
        self.present();
    }

    pub fn present(&mut self) {
        let c = &mut self.canvas;
        c.clear();
        let all = R::new(0, 0, c.w, c.h);
        c.fill(all, theme::g(85));
        c.fill(all.shrink(1), theme::g(34));
        let (_, rows) = layout(&self.items);
        for (i, it) in self.items.iter().enumerate() {
            let (y, h) = rows[i];
            match it {
                Item::Sep => c.fill(R::new(1, y + h / 2, c.w - 2, 1), theme::g(64)),
                Item::Zoom => draw_zoom(c, y, self.zoom_drag.unwrap_or_else(super::scale::percent), self.zoom_hover),
                Item::Do { label, .. } | Item::Sub { label, .. } => {
                    let (icon, checked, sub) = match it {
                        Item::Do { icon, check, .. } => (*icon, *check, false),
                        _ => (None, false, true),
                    };
                    if self.hover == Some(i) {
                        c.fill(R::new(1, y, c.w - 2, h), theme::g(85));
                    }
                    if let Some(ic) = icon {
                        c.icon(ic, 4, y + 2, theme::g(230));
                    }
                    c.text(label, FONT, 22, y + 2, theme::g(232), Hint::ClearType);
                    if checked {
                        c.icon(icons::CHECK, c.w - 20, y + 3, theme::g(150));
                    }
                    if sub {
                        c.icon(icons::TRI_RIGHT, c.w - 18, y + 3, theme::g(120));
                    }
                }
            }
        }
        #[cfg(debug_assertions)]
        self.canvas.dump("menu");
        self.surface.present(self.hwnd, &self.canvas, self.pos.0, self.pos.1);
    }

    /// 屏幕坐标是否落在本菜单或其子菜单上
    pub fn contains(&self, x: i32, y: i32) -> bool {
        R::new(self.pos.0, self.pos.1, self.canvas.w, self.canvas.h).contains(x, y) || self.child.as_ref().is_some_and(|c| c.contains(x, y))
    }

    pub fn close(&mut self) {
        if let Some(mut c) = self.child.take() {
            c.close();
        }
        unsafe {
            for h in [std::mem::take(&mut self.hwnd), std::mem::take(&mut self.shadow)] {
                if !h.is_invalid() {
                    let _ = DestroyWindow(h);
                }
            }
        }
    }
}

/// 忘了 close 也不留下窗口（菜单、阴影）
impl Drop for Menu {
    fn drop(&mut self) {
        self.close();
    }
}
