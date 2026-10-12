//! 原版风格的弹出菜单（自绘分层窗口：深色底、灰色描边、高亮条、右侧勾选、子菜单箭头）。

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::canvas::{Canvas, R, Rgba};
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
}

#[derive(Clone)]
pub enum Item {
    Do { label: String, icon: Option<Cell>, check: bool, act: Act },
    Sub { label: String, items: Vec<Item> },
    Sep,
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
}

fn layout(items: &[Item]) -> (i32, Vec<(i32, i32)>) {
    let mut y = 3;
    let mut rows = Vec::new();
    let mut w = 0f32;
    for it in items {
        let h = if matches!(it, Item::Sep) { SEP_H } else { ITEM_H };
        if let Item::Do { label, .. } | Item::Sub { label, .. } = it {
            w = w.max(text::measure(label, FONT).0);
        }
        rows.push((y, h));
        y += h;
    }
    ((w as i32 + 72).max(125), rows)
}

impl Menu {
    pub fn open(items: Vec<Item>, x: i32, y: i32, owner: HWND) -> Menu {
        let (w, rows) = layout(&items);
        let h = rows.last().map_or(6, |(y, h)| y + h + 3);
        // 不超出鼠标所在显示器
        let a = super::popup::work_area_at((x, y));
        let (x, y) = (x.min(a.right() - w).max(a.x), y.min(a.bottom() - h).max(a.y));
        let hwnd = window::create("", x, y, w, h, false, Some(owner));
        unsafe {
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex | WS_EX_TOPMOST.0 as i32);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        }
        let mut m = Menu { hwnd, items, hover: None, canvas: Canvas::new(w, h), surface: Surface::new(w, h), pos: (x, y), child: None, opened: std::time::Instant::now() };
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

    pub fn present(&mut self) {
        let c = &mut self.canvas;
        c.clear();
        let all = R::new(0, 0, c.w, c.h);
        c.fill(all, Rgba::gray(85));
        c.fill(all.shrink(1), Rgba::gray(34));
        let (_, rows) = layout(&self.items);
        for (i, it) in self.items.iter().enumerate() {
            let (y, h) = rows[i];
            match it {
                Item::Sep => c.fill(R::new(1, y + h / 2, c.w - 2, 1), Rgba::gray(64)),
                Item::Do { label, .. } | Item::Sub { label, .. } => {
                    let (icon, checked, sub) = match it {
                        Item::Do { icon, check, .. } => (*icon, *check, false),
                        _ => (None, false, true),
                    };
                    if self.hover == Some(i) {
                        c.fill(R::new(1, y, c.w - 2, h), Rgba::gray(85));
                    }
                    if let Some(ic) = icon {
                        c.icon(ic, 4, y + 2, Rgba::gray(230));
                    }
                    c.text(label, FONT, 22, y + 2, Rgba::gray(232), Hint::ClearType);
                    if checked {
                        c.icon(icons::CHECK, c.w - 20, y + 3, Rgba::gray(150));
                    }
                    if sub {
                        c.icon(icons::TRI_RIGHT, c.w - 18, y + 3, Rgba::gray(120));
                    }
                }
            }
        }
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
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
