//! 鼠标与键盘：悬停、点击、拖动（移动整组窗口 / 进度 / 音量 / 均衡器滑杆）、滚轮、搜索输入。

use windows::Win32::UI::Input::KeyboardAndMouse::*;

use super::app::{App, Pane};
use super::menu::Act;
use super::panels::bar::{self, Hot};
use super::panels::{eq, frame, playlist};
use super::state::Drag;

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
            Drag::Move { .. } => return self.move_drag(),
            Drag::Seek => return self.seek_at(x),
            Drag::Volume => return self.volume_at(y),
            Drag::EqSlider(i) => return self.eq_at(i, y),
            Drag::Scroll(y0, s0) => {
                let lay = playlist::Layout::of(self.list_size());
                let n = self.ui.list.rows_len();
                let (_, th) = lay.thumb(n, s0);
                let track = (lay.scroll.h - th - 2).max(1);
                let max = lay.max_scroll(n);
                let target = s0 + ((y - y0) as i64 * max as i64 / track as i64) as i32;
                self.ui.list.scroll = target.clamp(0, max);
                return self.dirty(Pane::List);
            }
            Drag::Resize { .. } => return self.resize_drag(),
            _ => {}
        }
        self.ui.hover[Self::idx(pane)] = true;
        match pane {
            Pane::Bar => {
                let h = bar::hit(self.sizes.bar_w, x, y, self.ui.bar_hover > 0.5);
                if h != self.ui.bar_hot {
                    self.ui.bar_hot = h;
                    self.dirty(Pane::Bar);
                }
            }
            Pane::List => {
                let lay = self.list_layout();
                let (ty, th) = lay.thumb(self.ui.list.rows_len(), self.ui.list.scroll);
                let thumb = lay.scroll.contains(x, y) && y >= ty && y < ty + th;
                if thumb != self.ui.list.thumb_hot {
                    self.ui.list.thumb_hot = thumb;
                    self.dirty(Pane::List);
                }
                let l = &mut self.ui.list;
                let row = if lay.rows.contains(x, y) { Some(lay.row_at(y, l.scroll)) } else { None };
                let row = row.filter(|r| *r < l.rows_len());
                let tool = playlist::TOOL.iter().position(|r| r.contains(x, y)).filter(|_| !l.radio);
                let paste = l.radio && playlist::PASTE.contains(x, y);
                let search = lay.search.contains(x, y);
                if row != l.hover || tool != l.hot_tool || paste != l.radio_paste_hot || search != l.search_hover {
                    l.hover = row;
                    l.hot_tool = tool;
                    l.radio_paste_hot = paste;
                    l.search_hover = search;
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
        self.ui.list.search_hover = false;
        self.ui.eq_hot = None;
        self.dirty(pane);
    }

    pub fn mouse_down(&mut self, pane: Pane, x: i32, y: i32, dbl: bool, ctrl: bool, shift: bool) {
        // 点到搜索框以外的地方：搜索框不再接收输入
        if self.ui.list.search_focus && !(pane == Pane::List && self.list_layout().search.contains(x, y)) {
            self.set_search_focus(false);
        }
        let edges = self.edge_at(pane, x, y);
        if edges != 0 {
            return self.start_resize(pane, edges);
        }
        let (w, _) = self.sizes.size(pane);
        let in_strip = pane != Pane::Bar && y < frame::STRIP_H + 2;
        // 列表面板标题条上的「音乐列表 / 网络电台」页签
        if in_strip && pane == Pane::List && self.radio_enabled() {
            if let Some(i) = playlist::strip_tabs().iter().position(|r| r.contains(x, y)) {
                return self.show_radio_tab(i == 1);
            }
        }
        if in_strip {
            let close = frame::close_rect(w);
            if crate::gui::canvas::R::new(close.x - 3, 0, close.w + 6, frame::STRIP_H + 2).contains(x, y) && self.ui.strip_hover[Self::idx(pane)] > 0.5 {
                return self.act(match pane {
                    Pane::Cover => Act::TogglePane(0),
                    Pane::Eq => Act::TogglePane(1),
                    _ => Act::TogglePane(2),
                });
            }
            // 拖标题条：只移动这个面板（原版：子面板单独拖动即脱离）
            return self.start_move(pane);
        }
        match pane {
            Pane::Bar => match bar::hit(self.sizes.bar_w, x, y, self.ui.bar_hover > 0.5) {
                Hot::Prev => self.player.prev(),
                Hot::PlayPause => self.player.toggle_pause(),
                Hot::Next => self.player.next(),
                Hot::Min => self.act(Act::Minimize),
                Hot::Minibar => self.act(Act::Minibar),
                Hot::Pin => self.act(Act::ToggleTopmost),
                Hot::Close => self.request_close(),
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
                // 播放条空白处：与贴着的面板一起移动
                _ => self.start_move(Pane::Bar),
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
                self.start_move(Pane::Eq);
            }
            Pane::List => self.list_down(x, y, dbl, ctrl, shift),
            Pane::Cover => self.start_move(Pane::Cover),
        }
    }
    pub fn mouse_up(&mut self, _pane: Pane, _x: i32, _y: i32) {
        if matches!(self.ui.drag, Drag::EqSlider(_)) {
            self.player.save(false);
        }
        if matches!(self.ui.drag, Drag::Resize { .. } | Drag::Move { .. }) {
            self.end_drag();
        }
        self.ui.drag = Drag::None;
    }

    fn seek_at(&mut self, x: i32) {
        let track = bar::Layout::of(self.sizes.bar_w).track;
        let t = ((x - track.x) as f64 / track.w as f64).clamp(0.0, 1.0);
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
        let radio = self.ui.list.radio;
        if radio && playlist::PASTE.contains(x, y) {
            return self.radio_paste();
        }
        if let Some(i) = playlist::TOOL.iter().position(|r| r.contains(x, y)).filter(|_| !radio) {
            return self.act([Act::AddMenu, Act::EditMenu, Act::SortMenu][i].clone());
        }
        let lay = self.list_layout();
        if lay.search.contains(x, y) {
            // 有字时点右边的「清除」：清空，继续输入
            if x >= lay.search.right() - 20 && !self.ui.list.search.is_empty() {
                self.ui.list.search.clear();
                self.ui.list.scroll = 0;
            }
            self.set_search_focus(true);
            return self.dirty(Pane::List);
        }
        if lay.scroll.contains(x, y) {
            // 滚动条：按住滑块拖动；点空白处翻页
            let (ty, th) = lay.thumb(self.ui.list.rows_len(), self.ui.list.scroll);
            if y >= ty && y < ty + th {
                self.ui.drag = Drag::Scroll(y, self.ui.list.scroll);
            } else {
                let page = lay.rows.h - playlist::ROW_H;
                self.scroll_list(if y < ty { -page } else { page });
            }
            return;
        }
        if radio && self.radio_list_down(x, y, dbl) {
            return;
        }
        if !lay.rows.contains(x, y) {
            return self.start_move(Pane::List);
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
            Pane::List if self.ui.list.radio => {
                let lay = self.list_layout();
                let l = &mut self.ui.list;
                l.radio_sel = if lay.rows.contains(x, y) { l.radio_rows.get(lay.row_at(y, l.scroll)).copied() } else { None };
                self.dirty(Pane::List);
                self.act(Act::RadioMenu);
            }
            Pane::List if self.list_layout().rows.contains(x, y) => {
                let lay = self.list_layout();
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

    pub fn wheel(&mut self, pane: Pane, x: i32, y: i32, delta: i32) {
        // Ctrl + 滚轮：界面缩放（一次一档）
        if unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0 {
            self.set_zoom(Some(super::menu::zoom_step(super::scale::percent(), delta > 0)));
            return;
        }
        match pane {
            // 左边一栏放不下时，在它上面滚动的是这一栏
            Pane::List if self.list_layout().tabs.contains(x, y) && self.max_tab_scroll() > 0 => {
                let max = self.max_tab_scroll();
                let l = &mut self.ui.list;
                l.tab_scroll = (l.tab_scroll - delta * playlist::TAB_H / 120).clamp(0, max);
                self.dirty(Pane::List);
            }
            Pane::List => self.scroll_list(-delta * playlist::ROW_H * 3 / 120),
            _ => {
                self.player.volume_step(if delta > 0 { 0.05 } else { -0.05 });
                self.dirty(Pane::Bar);
            }
        }
    }

    pub fn on_char(&mut self, ch: u32) {
        // 电台页签里 Ctrl+V：粘贴电台清单
        if ch == 22 && self.ui.list.radio && !self.ui.list.search_focus {
            return self.radio_paste();
        }
        if !self.ui.list.search_focus {
            return;
        }
        if ch == 27 {
            // Esc：清空并结束输入
            self.ui.list.search.clear();
            self.ui.list.scroll = 0;
            return self.set_search_focus(false);
        }
        let l = &mut self.ui.list;
        match ch {
            8 => {
                l.search.pop();
            }
            // Ctrl+V：粘贴到搜索框（只取第一行）
            22 => {
                if let Some(t) = super::lyricwin::clipboard_text() {
                    l.search.push_str(t.lines().next().unwrap_or("").trim());
                }
            }
            c if c >= 32 => {
                if let Some(c) = char::from_u32(c) {
                    l.search.push(c);
                }
            }
            _ => {}
        }
        // 打字时光标一直亮着（停下来才开始闪）
        self.ui.list.caret_t0 = Some(std::time::Instant::now());
        self.ui.list.scroll = 0;
        self.dirty(Pane::List);
    }

    /// 搜索框开始 / 停止接收输入。系统光标只用来告诉输入法和读屏软件位置（分层窗口上不会画出来），
    /// 看得见的光标是 playlist.rs 自己画的
    pub fn set_search_focus(&mut self, on: bool) {
        let l = &mut self.ui.list;
        let was = l.search_focus;
        l.search_focus = on;
        l.caret_t0 = on.then(std::time::Instant::now);
        let hwnd = self.win(Pane::List).hwnd;
        unsafe {
            if on && !was {
                let _ = windows::Win32::UI::WindowsAndMessaging::CreateCaret(hwnd, None, 2, 13);
            } else if !on && was {
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyCaret();
            }
        }
        if on {
            self.place_caret();
        }
        self.dirty(Pane::List);
    }

    /// 系统光标放到搜索框里文字的末尾（输入法的候选窗口跟着它）
    pub fn place_caret(&self) {
        if self.ui.list.search_focus {
            let (x, y) = playlist::search_caret(self.list_layout().search, &self.ui.list.search);
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetCaretPos(x, y);
            }
        }
    }

    /// 搜索框的光标现在亮不亮：按系统的光标闪烁速度，从获得输入 / 最后一次打字时算起
    pub fn caret_on(&self) -> bool {
        let Some(t0) = self.ui.list.caret_t0 else { return false };
        let ms = unsafe { windows::Win32::UI::WindowsAndMessaging::GetCaretBlinkTime() };
        // 0xFFFFFFFF = 设置成不闪
        if ms == 0 || ms == u32::MAX {
            return true;
        }
        (t0.elapsed().as_millis() / ms as u128) % 2 == 0
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
            VK_DELETE if self.ui.list.radio => {
                if let Some(i) = self.ui.list.radio_sel {
                    self.radio_remove(i);
                }
            }
            VK_RETURN if self.ui.list.radio => {
                if let Some(i) = self.ui.list.radio_sel {
                    self.play_radio(i);
                }
            }
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
