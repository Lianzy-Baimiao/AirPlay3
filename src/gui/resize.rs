//! 调整面板大小：音乐列表可调宽高，歌词图片可调高度（原版四周 2 像素的透明边就是调整区域）。

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::app::{App, Pane};
use super::panels::{cover, playlist};
use super::state::Drag;

/// 调整方向：右边、下边、右下角
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Right,
    Bottom,
    Corner,
}

const GRIP: i32 = 5;

impl App {
    /// 鼠标所在位置能调整哪条边
    pub fn edge_at(&self, pane: Pane, x: i32, y: i32) -> Option<Edge> {
        let (w, h) = self.sizes.size(pane);
        let (r, b) = (x >= w - GRIP, y >= h - GRIP);
        match pane {
            Pane::List if r && b => Some(Edge::Corner),
            Pane::List if r => Some(Edge::Right),
            Pane::List | Pane::Cover if b => Some(Edge::Bottom),
            _ => None,
        }
    }

    pub fn cursor_for(&self, pane: Pane, x: i32, y: i32) -> CursorId {
        match self.edge_at(pane, x, y) {
            Some(Edge::Right) => IDC_SIZEWE,
            Some(Edge::Bottom) => IDC_SIZENS,
            Some(Edge::Corner) => IDC_SIZENWSE,
            None => IDC_ARROW,
        }
    }

    pub fn start_resize(&mut self, pane: Pane, edge: Edge) {
        let (cx, cy) = cursor();
        let (w, h) = self.sizes.size(pane);
        self.ui.drag = Drag::Resize(pane, edge, cx - w, cy - h);
    }

    pub fn resize_drag(&mut self) {
        let Drag::Resize(pane, edge, ox, oy) = self.ui.drag else { return };
        let (cx, cy) = cursor();
        let (nw, nh) = (cx - ox, cy - oy);
        match pane {
            Pane::List => {
                let (mut w, mut h) = self.sizes.list;
                if edge != Edge::Bottom {
                    w = nw.clamp(playlist::MIN.0, 2000);
                }
                if edge != Edge::Right {
                    h = nh.clamp(playlist::MIN.1, 2000);
                }
                if (w, h) != self.sizes.list {
                    self.sizes.list = (w, h);
                    let max = playlist::Layout::of((w, h)).max_scroll(self.ui.list.ids.len());
                    self.ui.list.scroll = self.ui.list.scroll.min(max);
                    self.dirty(Pane::List);
                }
            }
            Pane::Cover => {
                let h = nh.clamp(cover::MIN_H, cover::MAX_H);
                if h != self.sizes.cover_h {
                    self.sizes.cover_h = h;
                    self.dirty(Pane::Cover);
                    // 均衡器跟着下移
                    self.dirty(Pane::Eq);
                }
            }
            _ => {}
        }
    }

    /// 结束调整：保存尺寸
    pub fn end_resize(&mut self) {
        let mut g = self.player.core.lock();
        g.settings.window.list_size = [self.sizes.list.0, self.sizes.list.1];
        g.settings.window.cover_h = self.sizes.cover_h;
        g.touch_settings();
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

pub type CursorId = windows::core::PCWSTR;

pub fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}
