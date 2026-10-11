//! 菜单动作、弹出菜单的消息、文件对话框、热键、置顶。

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::app::{App, Pane, WM_APP_ASYNC};
use super::menu::{Act, Item, Menu, check, item};
use crate::config::{HotkeyAction, PlayMode};
use crate::library::{ListKind, SortBy};
use crate::skin::icons;

pub enum AsyncMsg {
    Files(Vec<std::path::PathBuf>, crate::library::ListId),
}

fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

impl App {
    fn main_menu(&self) -> Vec<Item> {
        let s = &self.ui.snap;
        let order = PlayMode::ALL.iter().map(|m| check(m.label(), None, s.mode == *m, Act::SetMode(*m))).collect();
        vec![
            check("歌词图片", Some(icons::LYRIC_PIC), s.show[0], Act::TogglePane(0)),
            check("音乐列表", Some(icons::LIST), s.show[2], Act::TogglePane(2)),
            check("均衡器", Some(icons::EQ), s.show[1], Act::TogglePane(1)),
            Item::Sep,
            Item::Sub { label: "列表播放顺序".into(), items: order },
            Item::Sep,
            item("配置", Act::OpenConfig),
            Item::Sep,
            item("退出", Act::Quit),
        ]
    }

    fn row_menu(&self) -> Vec<Item> {
        vec![
            item("添加文件", Act::AddFiles),
            item("添加文件夹", Act::AddFolder),
            Item::Sep,
            item("清空当前列表", Act::ClearList),
            Item::Sep,
            item("打开所在文件夹", Act::RevealSelected),
            Item::Sep,
            item("物理删除 (放入回收站)", Act::RecycleSelected),
        ]
    }

    pub fn popup(&mut self, items: Vec<Item>) {
        self.close_menu();
        let (x, y) = cursor();
        self.menu = Some(Menu::open(items, x + 2, y + 2, self.bar_hwnd()));
    }

    pub fn close_menu(&mut self) {
        if let Some(mut m) = self.menu.take() {
            m.close();
        }
    }

    pub fn act(&mut self, a: Act) {
        let p = self.player.clone();
        match a {
            Act::TogglePane(i) => {
                let mut g = p.core.lock();
                let w = &mut g.settings.window;
                match i {
                    0 => w.show_cover = !w.show_cover,
                    1 => w.show_eq = !w.show_eq,
                    _ => w.show_list = !w.show_list,
                }
                g.touch_settings();
                drop(g);
                self.tick();
            }
            Act::Minimize => self.minimize(),
            Act::ToggleTopmost => {
                let mut g = p.core.lock();
                g.settings.general.topmost = !g.settings.general.topmost;
                g.touch_settings();
                drop(g);
                self.apply_topmost();
            }
            Act::CycleMode => p.set_mode(self.ui.snap.mode.cycle()),
            Act::SetMode(m) => p.set_mode(m),
            Act::EqToggle | Act::EqReset | Act::EqPreset(_) => {
                let mut e = p.core.lock().settings.eq.clone();
                match a {
                    Act::EqToggle => e.enabled = !e.enabled,
                    Act::EqReset => {
                        e.bands = [0.0; 10];
                        e.preamp_db = 0.0;
                        e.preset = crate::audio::presets::FLAT.into();
                    }
                    Act::EqPreset(name) => {
                        if let Some(b) = crate::audio::presets::preset(&name) {
                            e.bands = b;
                        }
                        e.preset = name;
                    }
                    _ => {}
                }
                p.set_eq(&e);
                self.dirty(Pane::Eq);
            }
            Act::EqPresetMenu => {
                let cur = self.ui.snap.eq_preset.clone();
                let items = crate::audio::presets::PRESETS.iter().map(|(n, _)| check(n, None, cur == *n, Act::EqPreset(n.to_string()))).collect();
                self.popup(items);
            }
            Act::MainMenu => self.popup(self.main_menu()),
            Act::RowMenu => self.popup(self.row_menu()),
            Act::AddMenu => self.popup(vec![item("添加文件", Act::AddFiles), item("添加文件夹", Act::AddFolder)]),
            Act::EditMenu => self.popup(vec![
                item("全选", Act::SelectAll),
                item("不选", Act::SelectNone),
                item("反选", Act::SelectInvert),
                Item::Sep,
                item("删除所选内容", Act::RemoveSelected),
                item("删除不能播放的内容", Act::RemoveBroken),
            ]),
            Act::SortMenu => self.popup(vec![
                item("按名称排序", Act::Sort(SortBy::Title)),
                item("按艺人排序", Act::Sort(SortBy::Artist)),
                item("按专辑排序", Act::Sort(SortBy::Album)),
                item("按时间长度排序", Act::Sort(SortBy::Duration)),
            ]),
            Act::AddFiles | Act::AddFolder => self.add_dialog(a == Act::AddFolder),
            other => self.list_act(other),
        }
        self.dirty_all();
    }
    fn list_act(&mut self, a: Act) {
        let p = self.player.clone();
        let list = self.ui.list.list.unwrap_or_else(|| p.core.lock().lib.special_id(ListKind::Audition));
        let sel: Vec<_> = self.ui.list.selected.iter().copied().collect();
        match a {
            Act::RemoveRows(ids) => p.core.lock().lib.remove_from_list(list, &ids.into_iter().collect()),
            Act::RemoveSelected => p.core.lock().lib.remove_from_list(list, &sel.iter().copied().collect()),
            Act::RemoveBroken => {
                p.core.lock().lib.remove_broken(list);
            }
            Act::SelectAll => self.ui.list.selected = self.ui.list.ids.iter().copied().collect(),
            Act::SelectNone => self.ui.list.selected.clear(),
            Act::SelectInvert => {
                let all: std::collections::HashSet<_> = self.ui.list.ids.iter().copied().collect();
                self.ui.list.selected = all.difference(&self.ui.list.selected).copied().collect();
            }
            Act::Sort(by) => p.core.lock().lib.sort_list(list, by),
            Act::ClearList => p.core.lock().lib.clear_list(list),
            Act::RevealSelected => {
                let path = sel.first().and_then(|id| p.core.lock().lib.track(*id).map(|t| t.path.clone()));
                if let Some(path) = path {
                    crate::platform::shell::reveal(&path);
                }
            }
            Act::RecycleSelected => {
                let paths: Vec<_> = sel.iter().filter_map(|id| p.core.lock().lib.track(*id).map(|t| t.path.clone())).collect();
                let refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
                if !refs.is_empty() && crate::platform::shell::recycle(&refs) {
                    p.core.lock().lib.remove_from_list(list, &sel.iter().copied().collect());
                }
            }
            Act::OpenConfig => crate::platform::shell::open(&p.core.lock().dirs.root.to_string_lossy()),
            Act::Quit => self.quit(),
            _ => {}
        }
        self.ui.list.ids_key = None;
        p.save(false);
    }

    fn add_dialog(&mut self, folder: bool) {
        let list = self.ui.list.list.unwrap_or_else(|| self.player.core.lock().lib.special_id(ListKind::Audition));
        let (tx, hwnd) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            let mut d = rfd::FileDialog::new().set_parent(&crate::platform::window::Parent(hwnd));
            let files = if folder {
                d.set_title("添加文件夹").pick_folder().map(|f| vec![f])
            } else {
                d = d.set_title("添加文件");
                for (name, exts) in crate::media::dialog_filters() {
                    d = d.add_filter(name, &exts);
                }
                d.pick_files()
            };
            if let Some(files) = files.filter(|f| !f.is_empty()) {
                let _ = tx.send(AsyncMsg::Files(files, list));
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
                }
            }
        });
    }

    pub fn drain_async(&mut self) {
        while let Ok(m) = self.async_rx.try_recv() {
            match m {
                AsyncMsg::Files(files, list) => {
                    self.player.import(files, list, false);
                }
            }
        }
    }

    /// 弹出菜单窗口的消息；不是菜单窗口时返回 None
    pub fn menu_msg(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let m = self.menu.as_mut()?;
        let is_child = m.child.as_ref().is_some_and(|c| c.hwnd == hwnd);
        if m.hwnd != hwnd && !is_child {
            return None;
        }
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        match msg {
            WM_MOUSEMOVE => {
                let target = if is_child { m.child.as_deref_mut().unwrap() } else { &mut *m };
                let row = target.row_at(y);
                if row != target.hover {
                    target.hover = row;
                    target.present();
                }
                if !is_child {
                    let sub = row.and_then(|i| match &m.items[i] {
                        Item::Sub { items, .. } => Some((i, items.clone())),
                        _ => None,
                    });
                    match sub {
                        Some((i, items)) if m.child.is_none() => {
                            let r = m.row_rect(i);
                            let child = Menu::open(items, m.pos.0 + m.canvas.w - 2, m.pos.1 + r.y - 3, self.wins[0].1.hwnd);
                            m.child = Some(Box::new(child));
                        }
                        None if row.is_some() => {
                            if let Some(mut c) = m.child.take() {
                                c.close();
                            }
                        }
                        _ => {}
                    }
                }
            }
            WM_LBUTTONUP => {
                let target = if is_child { m.child.as_deref().unwrap() } else { &*m };
                if let Some(Item::Do { act, .. }) = target.row_at(y).map(|i| target.items[i].clone()) {
                    let _ = x;
                    self.close_menu();
                    self.act(act);
                }
            }
            WM_ACTIVATE if (wp.0 & 0xFFFF) == 0 => {
                let other = HWND(lp.0 as _);
                let keep = other == m.hwnd || m.child.as_ref().is_some_and(|c| c.hwnd == other);
                if !keep {
                    self.close_menu();
                }
            }
            WM_KEYDOWN if wp.0 == 0x1B => self.close_menu(),
            _ => return None,
        }
        Some(LRESULT(0))
    }

    pub fn minimize(&mut self) {
        let to_tray = self.player.core.lock().settings.general.minimize_to_tray;
        unsafe {
            if to_tray {
                for (_, w) in &self.wins {
                    let _ = ShowWindow(w.hwnd, SW_HIDE);
                }
            } else {
                let _ = ShowWindow(self.bar_hwnd(), SW_MINIMIZE);
            }
        }
    }

    pub fn restore(&mut self) {
        unsafe {
            for (p, w) in &self.wins {
                if *p == Pane::Bar || w.visible {
                    let _ = ShowWindow(w.hwnd, if *p == Pane::Bar { SW_RESTORE } else { SW_SHOWNOACTIVATE });
                }
            }
            let _ = SetForegroundWindow(self.bar_hwnd());
        }
        self.dirty_all();
    }

    pub fn on_tray(&mut self, ev: u32) {
        match ev {
            WM_LBUTTONUP => {
                let shown = unsafe { IsWindowVisible(self.bar_hwnd()).as_bool() && !IsIconic(self.bar_hwnd()).as_bool() };
                if shown { self.minimize() } else { self.restore() }
            }
            WM_RBUTTONUP => {
                unsafe {
                    let _ = SetForegroundWindow(self.bar_hwnd());
                }
                self.act(Act::MainMenu);
            }
            _ => {}
        }
    }

    pub fn apply_topmost(&mut self) {
        let top = self.player.core.lock().settings.general.topmost;
        unsafe {
            for (_, w) in &self.wins {
                let _ = SetWindowPos(w.hwnd, Some(if top { HWND_TOPMOST } else { HWND_NOTOPMOST }), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
    }

    pub fn register_hotkeys(&mut self) {
        let hk = self.player.core.lock().settings.hotkeys.clone();
        let h = self.bar_hwnd().0 as isize;
        crate::platform::hotkeys::unregister_all(h);
        let failed = crate::platform::hotkeys::register_all(h, &hk);
        if !failed.is_empty() {
            log::warn!("热键注册失败: {failed:?}");
        }
    }

    pub fn on_hotkey(&mut self, id: i32) {
        use crate::controller::Command as C;
        let c = match crate::platform::hotkeys::action_of(id) {
            Some(HotkeyAction::PlayPause) => C::PlayPause,
            Some(HotkeyAction::Prev) => C::Prev,
            Some(HotkeyAction::Next) => C::Next,
            Some(HotkeyAction::VolumeUp) => C::VolumeUp,
            Some(HotkeyAction::VolumeDown) => C::VolumeDown,
            Some(HotkeyAction::Mute) => C::Mute,
            Some(HotkeyAction::Favorite) => C::Favorite,
            Some(HotkeyAction::Ban) => C::Ban,
            Some(HotkeyAction::DeskLyric) => C::DeskLyric,
            Some(HotkeyAction::ShowWindow) => return self.restore(),
            None => return,
        };
        let _ = self.cmds.send(c);
    }
}
