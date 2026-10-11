//! 移动与调整面板（原版行为，算法见 `snap.rs`）：
//! - 拖播放条：与它贴着的面板一起移动，整组吸附到屏幕边、任务栏和其它面板；
//! - 拖面板（标题条 / 空白处）：只移动这个面板（脱离），靠近其它面板或屏幕边 12 像素内自动吸附；
//! - 音乐列表、歌词图片可从左、右、下边及左下、右下角调整大小，被拖动的边自动对齐到附近的边。

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::app::{App, Pane};
use super::canvas::{Canvas, R};
use super::panels::{bar, cover, frame, playlist};
use super::snap::{self, BOTTOM, BOTTOM_LEFT, BOTTOM_RIGHT, LEFT, RIGHT};
use super::state::Drag;

/// 原版吸附对象表的顺序（相同距离时取后面的）
pub const SNAP_ORDER: [Pane; 4] = [Pane::Bar, Pane::List, Pane::Cover, Pane::Eq];

/// 原版：左右下三条边各 3 像素、两个下角 6 像素（加上外侧 2 像素透明边）
const EDGE: i32 = 5;
const CORNER: i32 = 8;

/// 可调整的面板尺寸范围（外框，原版 WM_GETMINMAXINFO 的值加上标题条与边）
pub fn limits(p: Pane) -> Option<((i32, i32), (i32, i32))> {
    match p {
        Pane::List => Some((playlist::MIN, playlist::MAX)),
        Pane::Cover => Some((cover::MIN, cover::MAX)),
        Pane::Bar => Some(((bar::MIN_W, bar::H), (bar::MAX_W, bar::H))),
        _ => None,
    }
}

/// 在可调整面板的左右下边缘留出可点击的像素（分层窗口里完全透明的像素收不到鼠标）
pub fn grip_margins(c: &mut Canvas, pane: Pane) {
    let (w, h) = (c.w, c.h);
    if pane == Pane::Bar {
        c.hit_margin(R::new(0, 0, EDGE, h));
        c.hit_margin(R::new(w - EDGE, 0, EDGE, h));
        return;
    }
    let top = frame::STRIP_H + 1;
    c.hit_margin(R::new(0, h - EDGE, w, EDGE));
    c.hit_margin(R::new(0, top, EDGE, h - top));
    c.hit_margin(R::new(w - EDGE, top, EDGE, h - top));
}

fn taskbar_rect() -> Option<R> {
    unsafe {
        let h = FindWindowW(w!("Shell_TrayWnd"), None).ok()?;
        let mut r = RECT::default();
        GetWindowRect(h, &mut r).ok()?;
        Some(R::new(r.left, r.top, r.right - r.left, r.bottom - r.top))
    }
}

/// 矩形中心所在显示器的工作区
fn work_area(r: R) -> R {
    super::popup::work_area_at((r.x + r.w / 2, r.y + r.h / 2))
}

/// w×h 的面板内 (x, y) 处的调整区（原版判断顺序：左下角、右下角、下边、左边、右边）
fn edge_hit(w: i32, h: i32, x: i32, y: i32) -> u32 {
    // 标题条只用于拖动，不能调整
    if y <= frame::STRIP_H || x < 0 || y < 0 || x >= w || y >= h {
        return 0;
    }
    let (l, r, b) = (x < EDGE, x >= w - EDGE, y >= h - EDGE);
    let (cl, cr, cb) = (x < CORNER, x >= w - CORNER, y >= h - CORNER);
    if cb && cl {
        BOTTOM_LEFT
    } else if cb && cr {
        BOTTOM_RIGHT
    } else if b {
        BOTTOM
    } else if l {
        LEFT
    } else if r {
        RIGHT
    } else {
        0
    }
}

impl App {
    /// 鼠标所在位置能调整哪些边（`snap::LEFT` 等的组合，0 = 不能调整）
    pub fn edge_at(&self, pane: Pane, x: i32, y: i32) -> u32 {
        if limits(pane).is_none() {
            return 0;
        }
        let (w, h) = self.sizes.size(pane);
        if pane == Pane::Bar {
            // 原版播放条只能左右调整（整个高度都可以拖）
            return if (0..h).contains(&y) && (0..EDGE).contains(&x) { LEFT } else if (0..h).contains(&y) && (w - EDGE..w).contains(&x) { RIGHT } else { 0 };
        }
        edge_hit(w, h, x, y)
    }

    pub fn cursor_for(&self, pane: Pane, x: i32, y: i32) -> CursorId {
        match self.edge_at(pane, x, y) {
            LEFT | RIGHT => IDC_SIZEWE,
            BOTTOM => IDC_SIZENS,
            BOTTOM_LEFT => IDC_SIZENESW,
            BOTTOM_RIGHT => IDC_SIZENWSE,
            _ => IDC_ARROW,
        }
    }

    /// 各面板当前外框，按 `SNAP_ORDER`
    fn snap_rects(&self) -> [R; 4] {
        SNAP_ORDER.map(|p| self.rect_of(p))
    }

    fn snap_visible(&self) -> [bool; 4] {
        SNAP_ORDER.map(|p| p == Pane::Bar || self.win_ref(p).visible)
    }

    pub fn start_move(&mut self, pane: Pane) {
        self.start_move_at(pane, cursor());
    }

    pub fn move_drag(&mut self) {
        self.move_drag_at(cursor());
    }

    pub fn start_resize(&mut self, pane: Pane, edges: u32) {
        self.start_resize_at(pane, edges, cursor());
    }

    pub fn resize_drag(&mut self) {
        self.resize_drag_at(cursor());
    }

    fn start_move_at(&mut self, pane: Pane, cursor: (i32, i32)) {
        let rects = self.snap_rects();
        let lead = SNAP_ORDER.iter().position(|p| *p == pane).unwrap_or(0);
        let moving: [bool; 4] = if pane == Pane::Bar {
            let g = snap::group(&rects, lead);
            [g[0], g[1], g[2], g[3]]
        } else {
            std::array::from_fn(|i| i == lead)
        };
        self.ui.drag = Drag::Move { lead: pane, cursor, rects, moving, taskbar: taskbar_rect() };
    }

    fn move_drag_at(&mut self, (cx, cy): (i32, i32)) {
        let Drag::Move { lead, cursor: (cx0, cy0), rects, moving, taskbar } = self.ui.drag else { return };
        let li = SNAP_ORDER.iter().position(|p| *p == lead).unwrap_or(0);
        let start = rects[li];
        let p = R::new(start.x + cx - cx0, start.y + cy - cy0, start.w, start.h);
        let env = snap::Env { taskbar, work_area: &work_area };
        let q = snap::group_move(&rects, &self.snap_visible(), &moving, li, p, lead == Pane::Bar, &env);
        let (dx, dy) = (q.x - start.x, q.y - start.y);
        for (i, pane) in SNAP_ORDER.into_iter().enumerate() {
            if moving[i] {
                self.place(pane, rects[i].x + dx, rects[i].y + dy);
            }
        }
    }

    fn start_resize_at(&mut self, pane: Pane, edges: u32, cursor: (i32, i32)) {
        self.ui.drag = Drag::Resize { pane, edges, cursor, rects: self.snap_rects(), taskbar: taskbar_rect() };
    }

    fn resize_drag_at(&mut self, (cx, cy): (i32, i32)) {
        let Drag::Resize { pane, edges, cursor: (cx0, cy0), rects, taskbar } = self.ui.drag else { return };
        let Some(((min_w, min_h), (max_w, max_h))) = limits(pane) else { return };
        let me = SNAP_ORDER.iter().position(|p| *p == pane).unwrap_or(0);
        let (dx, dy) = (cx - cx0, cy - cy0);
        let s = rects[me];
        let mut p = s;
        // 先按鼠标位置并限制尺寸（与系统调整循环一致），再吸附
        if edges & (LEFT | BOTTOM_LEFT) != 0 {
            p.w = (s.w - dx).clamp(min_w, max_w);
            p.x = s.x + s.w - p.w;
        }
        if edges & (RIGHT | BOTTOM_RIGHT) != 0 {
            p.w = (s.w + dx).clamp(min_w, max_w);
        }
        if edges & (BOTTOM | BOTTOM_LEFT | BOTTOM_RIGHT) != 0 {
            p.h = (s.h + dy).clamp(min_h, max_h);
        }
        let env = snap::Env { taskbar, work_area: &work_area };
        let mut q = snap::resize(&rects, &self.snap_visible(), me, p, edges, &env);
        // 吸附后仍不能超出尺寸范围（左边被拉动时保持右边不动）
        let right = q.x + q.w;
        q.w = q.w.clamp(min_w, max_w);
        if edges & (LEFT | BOTTOM_LEFT) != 0 {
            q.x = right - q.w;
        }
        q.h = q.h.clamp(min_h, max_h);
        self.set_rect(pane, q);
    }

    /// 改变面板位置与尺寸（下一帧按新尺寸重画并提交）
    fn set_rect(&mut self, pane: Pane, r: R) {
        if self.rect_of(pane) == r {
            return;
        }
        match pane {
            Pane::List => {
                self.sizes.list = (r.w, r.h);
                let max = playlist::Layout::of((r.w, r.h)).max_scroll(self.ui.list.ids.len());
                self.ui.list.scroll = self.ui.list.scroll.min(max);
            }
            Pane::Cover => self.sizes.cover = (r.w, r.h),
            Pane::Bar => self.sizes.bar_w = r.w,
            _ => {}
        }
        self.place(pane, r.x, r.y);
        self.dirty(pane);
    }

    /// 结束移动 / 调整：保存布局
    pub fn end_drag(&mut self) {
        self.save_layout();
    }

    /// 仅调试构建：模拟拖动，走与鼠标拖动相同的代码，用于自测吸附。
    /// `AIRPLAY3_TEST_DRAG="cover:0,7;list@8:0,-7"`：面板名（bar/list/cover/eq）、`@边` 表示调整大小（snap 的位值）、鼠标位移
    #[cfg(debug_assertions)]
    pub fn test_drags(&mut self, spec: &str) {
        for op in spec.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let Some((who, d)) = op.split_once(':') else { continue };
            let (name, edges) = who.split_once('@').map_or((who, 0), |(n, e)| (n, e.parse().unwrap_or(0)));
            let pane = match name {
                "bar" => Pane::Bar,
                "list" => Pane::List,
                "cover" => Pane::Cover,
                "eq" => Pane::Eq,
                _ => continue,
            };
            let Some((dx, dy)) = d.split_once(',').and_then(|(a, b)| Some((a.trim().parse::<i32>().ok()?, b.trim().parse::<i32>().ok()?))) else {
                continue;
            };
            let c0 = (10_000, 10_000);
            let c1 = (c0.0 + dx, c0.1 + dy);
            if edges == 0 {
                self.start_move_at(pane, c0);
                self.move_drag_at(c1);
            } else {
                self.start_resize_at(pane, edges, c0);
                self.resize_drag_at(c1);
            }
            self.ui.drag = Drag::None;
            self.end_drag();
            self.flush();
            log::info!("自测拖动 {op} -> {}", SNAP_ORDER.map(|p| format!("{p:?}={:?}", self.rect_of(p))).join(" "));
        }
    }

    pub fn list_size(&self) -> (i32, i32) {
        self.sizes.list
    }

    pub fn scroll_list(&mut self, delta: i32) {
        let max = playlist::Layout::of(self.sizes.list).max_scroll(self.ui.list.ids.len());
        self.ui.list.scroll = (self.ui.list.scroll + delta).clamp(0, max);
        self.dirty(Pane::List);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_zones() {
        let (w, h) = (324, 431);
        assert_eq!(edge_hit(w, h, 2, 200), LEFT);
        assert_eq!(edge_hit(w, h, 321, 200), RIGHT);
        assert_eq!(edge_hit(w, h, 150, 428), BOTTOM);
        assert_eq!(edge_hit(w, h, 6, 425), BOTTOM_LEFT);
        assert_eq!(edge_hit(w, h, 318, 425), BOTTOM_RIGHT);
        // 标题条、面板中间、角区外的边不触发
        assert_eq!(edge_hit(w, h, 2, 10), 0);
        assert_eq!(edge_hit(w, h, 321, 18), 0);
        assert_eq!(edge_hit(w, h, 150, 200), 0);
        assert_eq!(edge_hit(w, h, 6, 300), 0);
        assert_eq!(edge_hit(w, h, -1, 200), 0);
    }
}

pub type CursorId = windows::core::PCWSTR;

pub fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}
