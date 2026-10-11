//! 鼠标与键盘：悬停、点击、拖动（移动整组窗口 / 进度 / 音量 / 均衡器滑杆）、滚轮、搜索输入。

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::app::{App, Pane};
use super::menu::Act;
use super::panels::bar::{self, Hot};
use super::panels::{eq, frame, playlist};
use super::state::Drag;

fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

impl App {
    fn idx(p: Pane) -> usize {
        match p {
            Pane::Bar => 0,
            Pane::Cover => 1,
            Pane::Eq => 2,
            Pane::List => 3,
        }
    }

    pub fn mouse_move(&mut self, pane: Pane, x: i32, y: i32) {
        match self.ui.drag {
            Drag::Move(dx, dy) => {
                let (cx, cy) = cursor();
                self.move_to(cx - dx, cy - dy);
                return;
            }
            Drag::Seek => return self.seek_at(x),
            Drag::Volume => return self.volume_at(y),
            Drag::EqSlider(i) => return self.eq_at(i, y),
            Drag::Scroll(y0, s0) => {
                let lay = playlist::Layout::of(self.list_size());
                let n = self.ui.list.ids.len();
                let (_, th) = lay.thumb(n, s0);
                let track = (lay.scroll.h - th - 2).max(1);
                let max = lay.max_scroll(n);
                let target = s0 + ((y - y0) as i64 * max as i64 / track as i64) as i32;
                self.ui.list.scroll = target.clamp(0, max);
                return self.dirty(Pane::List);
            }
            Drag::Resize(..) => return self.resize_drag(),
            _ => {}
        }
        self.ui.hover[Self::idx(pane)] = true;
        match pane {
            Pane::Bar => {
                let h = bar::hit(x, y, self.ui.bar_hover > 0.5);
                if h != self.ui.bar_hot {
                    self.ui.bar_hot = h;
                    self.dirty(Pane::Bar);
                }
            }
            Pane::List => {
                let lay = playlist::Layout::of(self.sizes.list);
                let (ty, th) = lay.thumb(self.ui.list.ids.len(), self.ui.list.scroll);
                let thumb = lay.scroll.contains(x, y) && y >= ty && y < ty + th;
                if thumb != self.ui.list.thumb_hot {
                    self.ui.list.thumb_hot = thumb;
                    self.dirty(Pane::List);
                }
                let l = &mut self.ui.list;
                let row = if lay.rows.contains(x, y) { Some(lay.row_at(y, l.scroll)) } else { None };
                let row = row.filter(|r| *r < l.ids.len());
                let tool = playlist::TOOL.iter().position(|r| r.contains(x, y));
                if row != l.hover || tool != l.hot_tool {
                    l.hover = row;
                    l.hot_tool = tool;
                    self.dirty(Pane::List);
                }
            }
            Pane::Eq => {
                let hot = [eq::BTN_ENABLE, eq::BTN_RESET, eq::BTN_PRESET].iter().position(|r| r.contains(x, y));
                if hot != self.ui.eq_hot {
                    self.ui.eq_hot = hot;
                    self.dirty(Pane::Eq);
                }
            }
            Pane::Cover => {}
        }
    }

    pub fn mouse_leave(&mut self, pane: Pane) {
        self.ui.hover[Self::idx(pane)] = false;
        self.ui.bar_hot = Hot::None;
        self.ui.list.hover = None;
        self.ui.list.hot_tool = None;
        self.ui.eq_hot = None;
        self.dirty(pane);
    }

    fn start_move(&mut self) {
        let (cx, cy) = cursor();
        self.ui.drag = Drag::Move(cx - self.origin.0, cy - self.origin.1);
    }

    pub fn mouse_down(&mut self, pane: Pane, x: i32, y: i32, dbl: bool, ctrl: bool, shift: bool) {
        if let Some(edge) = self.edge_at(pane, x, y) {
            return self.start_resize(pane, edge);
        }
        let (w, _) = self.sizes.size(pane);
        let in_strip = pane != Pane::Bar && y < frame::STRIP_H + 2;
        if in_strip {
            if pane != Pane::List && x >= w - 24 && self.ui.strip_hover[Self::idx(pane)] > 0.5 {
                return self.act(match pane {
                    Pane::Cover => Act::TogglePane(0),
                    Pane::Eq => Act::TogglePane(1),
                    _ => Act::TogglePane(2),
                });
            }
            return self.start_move();
        }
        match pane {
            Pane::Bar => match bar::hit(x, y, self.ui.bar_hover > 0.5) {
                Hot::Prev => self.player.prev(),
                Hot::PlayPause => self.player.toggle_pause(),
                Hot::Next => self.player.next(),
                Hot::Min => self.act(Act::Minimize),
                Hot::Minibar => self.act(Act::Minibar),
                Hot::Pin => self.act(Act::ToggleTopmost),
                Hot::Close => self.quit(),
                Hot::Lyric => self.act(Act::TogglePane(0)),
                Hot::Eq => self.act(Act::TogglePane(1)),
                Hot::List => self.act(Act::TogglePane(2)),
                Hot::Mode => self.act(Act::CycleMode),
                Hot::Track => {
                    self.ui.drag = Drag::Seek;
                    self.seek_at(x);
                }
                Hot::Volume => {
                    self.ui.drag = Drag::Volume;
                    self.volume_at(y);
                }
                _ => self.start_move(),
            },
            Pane::Eq => {
                if eq::BTN_ENABLE.contains(x, y) {
                    return self.act(Act::EqToggle);
                }
                if eq::BTN_RESET.contains(x, y) {
                    return self.act(Act::EqReset);
                }
                if eq::BTN_PRESET.contains(x, y) {
                    return self.act(Act::EqPresetMenu);
                }
                if let Some(i) = eq::SLIDER_X.iter().position(|sx| (x - sx).abs() <= 6) {
                    if y >= eq::RAIL_TOP - 4 && y <= eq::RAIL_TOP + eq::RAIL_H + 4 && self.ui.snap.eq_enabled {
                        self.ui.drag = Drag::EqSlider(i);
                        return self.eq_at(i, y);
                    }
                }
                self.start_move();
            }
            Pane::List => self.list_down(x, y, dbl, ctrl, shift),
            Pane::Cover => self.start_move(),
        }
    }
    pub fn mouse_up(&mut self, _pane: Pane, _x: i32, _y: i32) {
        if matches!(self.ui.drag, Drag::EqSlider(_)) {
            self.player.save(false);
        }
        if matches!(self.ui.drag, Drag::Resize(..)) {
            self.end_resize();
        }
        self.ui.drag = Drag::None;
    }

    fn seek_at(&mut self, x: i32) {
        let t = ((x - bar::TRACK.x) as f64 / bar::TRACK.w as f64).clamp(0.0, 1.0);
        if self.ui.snap.dur > 0.0 {
            self.player.seek(t * self.ui.snap.dur);
            self.dirty(Pane::Bar);
        }
    }

    fn volume_at(&mut self, y: i32) {
        let v = ((bar::VOLUME.bottom() - y) as f32 / bar::VOLUME.h as f32).clamp(0.0, 1.0);
        self.player.set_volume(v);
        self.dirty(Pane::Bar);
    }

    fn eq_at(&mut self, i: usize, y: i32) {
        let db = eq::db_at(y);
        let mut e = self.player.core.lock().settings.eq.clone();
        if i == 0 {
            e.preamp_db = db;
        } else {
            e.bands[i - 1] = db;
        }
        e.preset = crate::audio::presets::CUSTOM.to_string();
        self.player.set_eq(&e);
        self.dirty(Pane::Eq);
    }

    fn list_down(&mut self, x: i32, y: i32, dbl: bool, ctrl: bool, shift: bool) {
        if let Some(i) = playlist::TOOL.iter().position(|r| r.contains(x, y)) {
            return self.act([Act::AddMenu, Act::EditMenu, Act::SortMenu][i].clone());
        }
        let search = playlist::Layout::of(self.sizes.list).search;
        if search.contains(x, y) {
            self.ui.list.search_focus = true;
            if x > search.right() - 20 && !self.ui.list.search.is_empty() {
                self.ui.list.search.clear();
            }
            return self.dirty(Pane::List);
        }
        if self.ui.list.search_focus {
            self.ui.list.search_focus = false;
            self.dirty(Pane::List);
        }
        let lay = playlist::Layout::of(self.list_size());
        if lay.scroll.contains(x, y) {
            // 滚动条：按住滑块拖动；点空白处翻页
            let (ty, th) = lay.thumb(self.ui.list.ids.len(), self.ui.list.scroll);
            if y >= ty && y < ty + th {
                self.ui.drag = Drag::Scroll(y, self.ui.list.scroll);
            } else {
                let page = lay.rows.h - playlist::ROW_H;
                self.scroll_list(if y < ty { -page } else { page });
            }
            return;
        }
        if !lay.rows.contains(x, y) {
            return self.start_move();
        }
        let l = &mut self.ui.list;
        let row = lay.row_at(y, l.scroll);
        let Some(&id) = l.ids.get(row) else { return };
        if dbl || (x < lay.rows.x + 18 && l.hover == Some(row)) {
            let list = l.list;
            if let Some(list) = list {
                self.player.play_at(crate::player::Source::List(list), id, 0.0, false);
            }
        } else if x > lay.rows.right() - 20 && l.hover == Some(row) {
            return self.act(Act::RemoveRows(vec![id]));
        } else if shift && l.anchor.is_some() {
            let a = l.anchor.unwrap_or(row);
            let (s, e) = (a.min(row), a.max(row));
            l.selected = l.ids[s..=e].iter().copied().collect();
        } else if ctrl {
            if !l.selected.remove(&id) {
                l.selected.insert(id);
            }
            l.anchor = Some(row);
        } else {
            l.selected = [id].into_iter().collect();
            l.anchor = Some(row);
        }
        self.dirty(Pane::List);
    }

    pub fn right_click(&mut self, pane: Pane, x: i32, y: i32) {
        match pane {
            Pane::List if playlist::Layout::of(self.sizes.list).rows.contains(x, y) => {
                let lay = playlist::Layout::of(self.sizes.list);
                let l = &mut self.ui.list;
                let row = lay.row_at(y, l.scroll);
                if let Some(&id) = l.ids.get(row) {
                    if !l.selected.contains(&id) {
                        l.selected = [id].into_iter().collect();
                        l.anchor = Some(row);
                    }
                }
                self.dirty(Pane::List);
                self.act(Act::RowMenu);
            }
            Pane::List => self.act(Act::RowMenu),
            Pane::Cover => self.act(Act::LyricMenu),
            _ => self.act(Act::MainMenu),
        }
    }

    pub fn wheel(&mut self, pane: Pane, _x: i32, _y: i32, delta: i32) {
        match pane {
            Pane::List => self.scroll_list(-delta * playlist::ROW_H * 3 / 120),
            _ => {
                self.player.volume_step(if delta > 0 { 0.05 } else { -0.05 });
                self.dirty(Pane::Bar);
            }
        }
    }

    pub fn on_char(&mut self, ch: u32) {
        if !self.ui.list.search_focus {
            return;
        }
        let l = &mut self.ui.list;
        match ch {
            8 => {
                l.search.pop();
            }
            27 => {
                l.search.clear();
                l.search_focus = false;
            }
            c if c >= 32 => {
                if let Some(c) = char::from_u32(c) {
                    l.search.push(c);
                }
            }
            _ => {}
        }
        l.scroll = 0;
        self.dirty(Pane::List);
    }

    pub fn on_key(&mut self, vk: VIRTUAL_KEY) {
        if self.ui.list.search_focus {
            return;
        }
        match vk {
            VK_SPACE => self.player.toggle_pause(),
            VK_LEFT => self.player.seek((self.ui.snap.pos - 5.0).max(0.0)),
            VK_RIGHT => self.player.seek(self.ui.snap.pos + 5.0),
            VK_UP => self.player.volume_step(0.05),
            VK_DOWN => self.player.volume_step(-0.05),
            VK_DELETE => {
                let ids: Vec<_> = self.ui.list.selected.iter().copied().collect();
                self.act(Act::RemoveRows(ids));
            }
            VK_RETURN => {
                if let (Some(a), Some(list)) = (self.ui.list.anchor, self.ui.list.list) {
                    if let Some(&id) = self.ui.list.ids.get(a) {
                        self.player.play_at(crate::player::Source::List(list), id, 0.0, false);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn drop_files(&mut self, _pane: Pane, files: Vec<std::path::PathBuf>) {
        if files.is_empty() {
            return;
        }
        let list = self.ui.list.list.unwrap_or_else(|| self.player.core.lock().lib.special_id(crate::library::ListKind::Audition));
        let play = self.player.core.lock().settings.general.play_after_drop;
        self.player.import(files, list, play);
    }
}
