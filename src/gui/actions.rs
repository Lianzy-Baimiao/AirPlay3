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
    /// 歌词搜索结果：(结果, 是否放宽为只按标题)
    LyricResults(u64, Result<(Vec<crate::lyrics::online::Candidate>, bool), String>),
    /// 搜索窗口下载并保存歌词的结果
    /// 「下载歌词」窗口下载完了：(歌曲, 保存的歌词文件（没下载歌词时为 None）, 各项出的错)
    LyricSaved(crate::library::TrackId, Option<std::path::PathBuf>, Vec<String>),
    LyricFile(crate::library::TrackId, std::path::PathBuf),
    CoverFile(crate::library::TrackId, std::path::PathBuf),
    /// 手动下载专辑图片的结果：Ok(是否找到)
    CoverDone(Result<crate::player::nowplaying::CoverFound, String>),
    LyricDir(std::path::PathBuf),
    /// 电台列表（请求序号, 结果）
    Radio(u64, Result<Vec<crate::radio::Station>, String>),
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
        // 先取出设置再拼菜单：同一个表达式里两次 core.lock() 时第一个锁要到表达式结束才释放，第二次加锁会自己等自己（死锁）
        let (desk, desk_locked, taskbar) = {
            let g = self.player.core.lock();
            (g.settings.desk_lyric.visible, g.settings.desk_lyric.locked, g.settings.taskbar_lyric.visible)
        };
        let mut v = vec![
            check("歌词图片", Some(icons::LYRIC_PIC), s.show[0], Act::TogglePane(0)),
            check("音乐列表", Some(icons::LIST), s.show[2], Act::TogglePane(2)),
            check("均衡器", Some(icons::EQ), s.show[1], Act::TogglePane(1)),
            Item::Sep,
            check("迷你歌词", None, self.minibar.is_some(), Act::Minibar),
            check("桌面歌词", None, desk, Act::DeskToggle),
        ];
        // 锁定后桌面歌词点不到（鼠标穿透），从这里解锁
        if desk && desk_locked {
            v.push(item("解锁桌面歌词", Act::DeskLock));
        }
        v.extend([
            check("任务栏歌词", None, taskbar, Act::TaskbarToggle),
            Item::Sep,
            Item::Sub { label: "列表播放顺序".into(), items: order },
            Item::Sep,
            item("网络电台…", Act::Radio),
            item("配置", Act::OpenConfig),
            item("恢复默认位置", Act::ResetPosition),
            Item::Sep,
            item("退出", Act::Quit),
        ]);
        v
    }

    fn lyric_menu(&self) -> Vec<Item> {
        let auto = self.player.core.lock().settings.lyrics.auto_download;
        vec![
            item("下载歌词…", Act::LyricSearch),
            item("关联本地歌词…", Act::LyricFile),
            item("取消歌词关联", Act::LyricUnlink),
            Item::Sep,
            item("歌词提前 0.5 秒", Act::LyricOffset(500)),
            item("歌词延后 0.5 秒", Act::LyricOffset(-500)),
            Item::Sep,
            item("下载专辑图片", Act::CoverDownload),
            item("关联本地图片…", Act::CoverFile),
            item("取消图片关联", Act::CoverUnlink),
            Item::Sep,
            check("启用自动下载歌词图片", None, auto, Act::LyricAuto),
            item("打开歌词文件夹", Act::LyricDir),
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

    /// 每帧检查：在菜单外按下鼠标、或前台切到别的程序时关闭菜单（不依赖 WM_ACTIVATE，避免刚弹出就被关掉）
    pub fn poll_menu(&mut self) {
        let Some(m) = self.menu.as_ref() else { return };
        if m.opened.elapsed() < std::time::Duration::from_millis(150) {
            return;
        }
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON};
        let down = unsafe { [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|k| GetAsyncKeyState(k.0 as i32) as u16 & 0x8000 != 0) };
        let (cx, cy) = cursor();
        let outside_click = down && !m.contains(cx, cy);
        let fg = unsafe { GetForegroundWindow() };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
        let other_app = !fg.is_invalid() && pid != std::process::id();
        // 自动化自测时测试程序本身占着前台，忽略这一条（仅调试构建）
        #[cfg(debug_assertions)]
        let other_app = other_app && std::env::var_os("AIRPLAY3_TEST_NOFG").is_none();
        if outside_click || other_app {
            self.close_menu();
        }
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
            Act::LyricMenu => self.popup(self.lyric_menu()),
            Act::LyricSearch => self.open_lyric_search(),
            Act::LyricFile => self.pick_lyric_file(),
            Act::CoverDownload => self.download_cover(),
            Act::CoverFile => self.pick_cover_file(),
            Act::LyricUnlink | Act::CoverUnlink | Act::LyricOffset(_) => {
                if let Some(id) = self.ui.snap.current {
                    use crate::player::nowplaying::Link;
                    match a {
                        Act::LyricUnlink => p.set_lyric_link(id, Link::Off, &self.now),
                        Act::CoverUnlink => p.set_cover_link(id, Link::Off, &self.now),
                        Act::LyricOffset(ms) => p.adjust_lyric_offset(id, ms, &self.now),
                        _ => {}
                    }
                }
            }
            Act::LyricAuto => {
                let mut g = p.core.lock();
                g.settings.lyrics.auto_download = !g.settings.lyrics.auto_download;
                g.touch_settings();
                drop(g);
                p.refresh_now_playing(&self.now, true);
            }
            Act::SetDevice(id) => {
                let mut a = p.core.lock().settings.audio.clone();
                a.device_id = id;
                p.set_audio(&a);
                p.save(false);
                if let Some(s) = self.settings_win.as_mut() {
                    s.win.dirty = true;
                }
            }
            Act::SetLyricSource(src) => {
                let mut g = p.core.lock();
                g.settings.lyrics.source = src;
                g.touch_settings();
                drop(g);
                p.save(false);
                if let Some(s) = self.settings_win.as_mut() {
                    s.win.dirty = true;
                }
            }
            Act::SetProxy(k) => {
                let mut g = p.core.lock();
                g.settings.network.kind = k;
                g.touch_settings();
                drop(g);
                p.save(false);
                if let Some(s) = self.settings_win.as_mut() {
                    s.win.dirty = true;
                }
            }
            Act::DeskToggle | Act::DeskLock | Act::DeskLines(_) | Act::DeskStyle(_) | Act::DeskFont(_) => {
                {
                    use crate::config::DeskLyricSettings as D;
                    let mut g = p.core.lock();
                    let d = &mut g.settings.desk_lyric;
                    match a {
                        Act::DeskToggle => d.visible = !d.visible,
                        Act::DeskLock => d.locked = !d.locked,
                        Act::DeskLines(n) => d.lines = n.clamp(1, D::MAX_LINES),
                        Act::DeskStyle(Some(i)) => d.style = i.min(super::desklyric::STYLES.len() - 1),
                        Act::DeskStyle(None) => d.style = (d.style + 1) % super::desklyric::STYLES.len(),
                        Act::DeskFont(df) => {
                            // 先把旧配置的行数定下来，改字号时行数不变
                            d.lines = d.line_count();
                            d.font_size = (d.font_size + df).clamp(16.0, 72.0);
                        }
                        _ => {}
                    }
                    g.touch_settings();
                }
                self.sync_desk_lyric();
                p.save(false);
            }
            Act::DeskMenu => self.popup(self.desk_menu()),
            Act::TaskbarToggle | Act::TaskbarAlign(_) | Act::TaskbarFont(_) | Act::TaskbarReset | Act::TaskbarTwoLines => {
                {
                    use crate::config::TaskbarLyricSettings as T;
                    let mut g = p.core.lock();
                    let t = &mut g.settings.taskbar_lyric;
                    match a {
                        Act::TaskbarToggle => t.visible = !t.visible,
                        // 换对齐方式时水平偏移回到默认，垂直偏移保留
                        Act::TaskbarAlign(al) => (t.align, t.offset) = (al, T::default_offset(al)),
                        Act::TaskbarFont(d) => t.font_size = (t.font_size + d).clamp(9.0, 24.0),
                        Act::TaskbarReset => (t.offset, t.offset_y) = (T::default_offset(t.align), 0),
                        Act::TaskbarTwoLines => t.two_lines = !t.two_lines,
                        _ => {}
                    }
                    g.touch_settings();
                }
                self.sync_taskbar_lyric();
                p.save(false);
            }
            Act::TaskbarMenu => self.popup(self.taskbar_menu()),
            Act::SettingsTab(t) => {
                self.open_settings();
                self.settings_tab(t.min(super::settings_model::TABS.len() - 1));
            }
            Act::Minibar => self.toggle_minibar(),
            Act::LyricDir => {
                let dir = p.core.lock().dirs.lyric_dir();
                crate::platform::shell::open(&dir.to_string_lossy());
            }
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
            Act::OpenConfig => self.open_settings(),
            Act::Radio => self.open_radio(),
            Act::SetRadioRegion(c) => self.set_radio_region(c),
            Act::ResetPosition => self.reset_layout(),
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
                AsyncMsg::Radio(seq, r) => self.on_radio_list(seq, r),
                AsyncMsg::LyricResults(seq, r) => {
                    self.on_lyric_results(seq, r);
                    self.paint_search();
                }
                AsyncMsg::LyricSaved(id, path, errors) => {
                    self.on_lyric_saved(id, path, errors);
                    if self.search.is_some() {
                        self.paint_search();
                    }
                }
                AsyncMsg::LyricDir(d) => {
                    let mut g = self.player.core.lock();
                    g.settings.lyrics.custom_dir = Some(d.to_string_lossy().into_owned());
                    g.touch_settings();
                    drop(g);
                    self.player.refresh_now_playing(&self.now, true);
                    self.player.save(false);
                    if let Some(s) = self.settings_win.as_mut() {
                        s.win.dirty = true;
                    }
                }
                AsyncMsg::LyricFile(id, path) => {
                    self.player.set_lyric_link(id, crate::player::nowplaying::Link::File(path), &self.now);
                    self.dirty(Pane::Cover);
                }
                AsyncMsg::CoverFile(id, path) => {
                    self.player.set_cover_link(id, crate::player::nowplaying::Link::File(path), &self.now);
                    self.dirty(Pane::Cover);
                }
                AsyncMsg::CoverDone(r) => {
                    use crate::player::nowplaying::CoverFound;
                    let msg = match r {
                        Ok(CoverFound::Album) => "专辑图片已下载并保存到本地".to_string(),
                        Ok(CoverFound::Artist) => "没有找到专辑图片，已下载艺人图片".to_string(),
                        Ok(CoverFound::None) => "没有找到这首歌的专辑图片和艺人图片".to_string(),
                        Err(e) => format!("下载图片失败：{e}"),
                    };
                    self.ui.cover_status = Some((msg, std::time::Instant::now()));
                    self.dirty(Pane::Cover);
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
        let failed = if hk.enabled { crate::platform::hotkeys::register_all(h, &hk) } else { Vec::new() };
        if !failed.is_empty() {
            log::warn!("热键注册失败: {failed:?}");
        }
        self.hotkey_failed = failed;
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
