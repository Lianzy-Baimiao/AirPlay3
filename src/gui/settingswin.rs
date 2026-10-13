//! 「配置」窗口：切换分页、点药丸 / 勾选框即生效、下拉选择、文字输入、热键捕获、文件关联。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::app::App;
use super::menu::{Act, check};
use super::panels::frame;
use super::panels::pill;
use super::panels::settings::{self as sp, SettingsModel, State};
use super::popup::Popup;
use super::settings_layout::{self as lay, Btn, Choice, FORMATS, Item, SHOWN_FORMATS, Toggle, Widget};
use super::settings_model::{self as model, Field, Key, TAB_ASSOC};
use crate::config::{HotkeyAction, KeyCombo, Settings};

pub struct SettingsWin {
    pub win: Popup,
    pub tab: usize,
    /// 正在输入的框
    pub focus: Option<Field>,
    /// 正在编辑的输入框（框, 已输入的文字）：数字输到一半（如 "-"）时照样显示，解析得了才写进设置
    pub edit: Option<(Field, String)>,
    pub hover: Option<usize>,
    pub hover_tab: Option<usize>,
    pub hover_cell: Option<usize>,
    /// 鼠标在窗口里（标题条上的光）
    pub inside: bool,
    /// 正在等按键的热键
    pub capturing: Option<HotkeyAction>,
    pub devices: Vec<crate::audio::devices::DeviceInfo>,
    /// 文件关联页：各格式（FORMATS 的顺序）是否已关联（打开这一页时从注册表读）
    pub assoc: Vec<bool>,
    pub status: String,
    pub drag: Option<(i32, i32)>,
    pub slider_drag: bool,
}

/// 下拉框里显示的当前选项
fn choice_label(sw: &SettingsWin, s: &Settings, c: Choice) -> String {
    match c {
        Choice::Device => {
            if s.audio.device_id.is_empty() {
                "系统默认设备".into()
            } else {
                sw.devices.iter().find(|d| d.id == s.audio.device_id).map_or("未知设备".into(), |d| d.name.clone())
            }
        }
        Choice::ProxyKind => model::proxy_label(s.network.kind).into(),
        Choice::LyricSource => s.lyrics.source.label().into(),
        Choice::TaskbarAlign => model::taskbar_align_label(s.taskbar_lyric.align).into(),
        Choice::DeskStyle => crate::gui::desklyric::style(s.desk_lyric.style).name.into(),
        Choice::DeskLines => format!("{} 行（拖动桌面歌词的上下边也能调整）", s.desk_lyric.line_count()),
    }
}

/// 某个格式（或一组）的关联状态
fn assoc_on(assoc: &[bool], t: Toggle) -> bool {
    match t {
        Toggle::Format(i) => assoc.get(i).copied().unwrap_or(false),
        Toggle::Group(g) => {
            let mut shown = (0..SHOWN_FORMATS).filter(|&i| !FORMATS[i].2.is_empty() && g.is_none_or(|t| t == FORMATS[i].1));
            shown.all(|i| assoc.get(i).copied().unwrap_or(false))
        }
        _ => false,
    }
}

impl App {
    pub fn open_settings(&mut self) {
        if let Some(s) = &self.settings_win {
            s.win.show(true);
            return;
        }
        let (x, y) = Popup::centered(sp::W, sp::H);
        let win = Popup::new(x, y, sp::W, sp::H, Some(self.bar_hwnd()), "配置");
        let devices = crate::audio::devices::list_devices();
        self.settings_win = Some(SettingsWin {
            win,
            tab: 0,
            focus: None,
            edit: None,
            hover: None,
            hover_tab: None,
            hover_cell: None,
            inside: false,
            capturing: None,
            devices,
            assoc: Vec::new(),
            status: String::new(),
            drag: None,
            slider_drag: false,
        });
        self.paint_settings();
        if let Some(s) = &self.settings_win {
            s.win.show(true);
        }
    }

    /// 切到第 t 页
    pub fn settings_tab(&mut self, t: usize) {
        let Some(sw) = self.settings_win.as_mut() else { return };
        (sw.tab, sw.focus, sw.edit, sw.capturing, sw.hover, sw.hover_cell) = (t, None, None, None, None, None);
        sw.status.clear();
        if t == TAB_ASSOC {
            self.refresh_assoc();
        }
        if let Some(sw) = self.settings_win.as_mut() {
            sw.win.dirty = true;
        }
        self.paint_settings();
    }

    fn refresh_assoc(&mut self) {
        let Some(sw) = self.settings_win.as_mut() else { return };
        sw.assoc = FORMATS
            .iter()
            .map(|f| !f.2.is_empty() && crate::platform::shell::associated(f.2).iter().all(|&a| a))
            .collect();
    }

    /// 当前分页的控件和它们现在的状态
    fn settings_items(&self) -> Vec<(Item, State)> {
        let Some(sw) = self.settings_win.as_ref() else { return Vec::new() };
        let g = self.player.core.lock();
        let s = &g.settings;
        let hk = &s.hotkeys;
        lay::layout(sw.tab)
            .into_iter()
            .map(|it| {
                let mut st = State { enabled: true, ..Default::default() };
                match &it.w {
                    Widget::Pill { act, .. } => match *act {
                        Toggle::Key(k) => {
                            st.on = model::get(s, k);
                            st.enabled = model::enabled(s, k);
                            st.dim = !st.enabled;
                        }
                        Toggle::Hotkey(a) => {
                            st.on = !hk.disabled.contains(&a);
                            st.enabled = hk.enabled;
                            st.dim = !st.on || !hk.enabled;
                        }
                        Toggle::Format(i) => {
                            st.on = assoc_on(&sw.assoc, *act);
                            st.enabled = !FORMATS[i].2.is_empty();
                            st.dim = !st.enabled;
                        }
                        Toggle::Group(_) => st.on = assoc_on(&sw.assoc, *act),
                        Toggle::Act(Btn::Capture(a)) => {
                            st.enabled = hk.enabled && !hk.disabled.contains(&a);
                            st.dim = !st.enabled;
                            st.value = if sw.capturing == Some(a) {
                                "请按下自定义键盘热键".into()
                            } else {
                                let text = hk.combo(a).map_or("未设置".to_string(), |c| crate::platform::hotkeys::combo_text(&c));
                                // 注册失败（被其它程序占用）的热键直接标出来
                                if hk.enabled && hk.active(a) && self.hotkey_failed.contains(&a) { format!("{text}（已被占用）") } else { text }
                            };
                        }
                        Toggle::Act(_) => {}
                    },
                    Widget::Check(k, _) => st.on = model::get(s, *k),
                    Widget::Drop(c) => {
                        st.value = choice_label(sw, s, *c);
                        st.enabled = *c != Choice::ProxyKind || s.network.proxy_enabled;
                    }
                    Widget::Edit(f, _) | Widget::Path(f, _) => {
                        st.enabled = !f.proxy() || s.network.proxy_enabled;
                        st.focused = sw.focus == Some(*f);
                        st.value = match &sw.edit {
                            Some((e, buf)) if e == f => if f.secret() { "*".repeat(buf.chars().count()) } else { buf.clone() },
                            _ => model::text(s, *f),
                        };
                    }
                    _ => {}
                }
                (it, st)
            })
            .collect()
    }

    pub fn paint_settings(&mut self) {
        let items = self.settings_items();
        let Some(sw) = self.settings_win.as_mut() else { return };
        let m = SettingsModel {
            tab: sw.tab,
            items: &items,
            hover: sw.hover,
            hover_tab: sw.hover_tab,
            hover_cell: sw.hover_cell,
            strip: if sw.inside { 1.0 } else { 0.0 },
        };
        sp::paint(&mut sw.win.canvas, &m);
        // 出错、提示的文字写在标题条上
        if !sw.status.is_empty() {
            sw.win.canvas.glow_text(&sw.status, pill::FONT, 10, 3, crate::gui::canvas::Rgba::gray(235), frame::TITLE_HALO, 2);
        }
        #[cfg(debug_assertions)]
        sw.win.canvas.dump("settings");
        sw.win.present();
    }

    fn set_status(&mut self, s: impl Into<String>) {
        if let Some(sw) = self.settings_win.as_mut() {
            sw.status = s.into();
        }
    }

    /// 开关：修改设置并让它立即生效
    fn toggle_key(&mut self, k: Key) {
        let p = self.player.clone();
        let v = {
            let mut g = p.core.lock();
            let v = !model::get(&g.settings, k);
            if k != Key::Exclusive {
                model::set(&mut g.settings, k, v);
                g.touch_settings();
            }
            v
        };
        match k {
            Key::Autostart => {
                if !crate::platform::shell::set_autostart(v) {
                    self.set_status("设置开机启动失败");
                }
            }
            Key::Topmost => self.apply_topmost(),
            Key::Exclusive => {
                let mut a = p.core.lock().settings.audio.clone();
                a.exclusive = v;
                p.set_audio(&a);
            }
            Key::HotkeysEnabled => self.register_hotkeys(),
            Key::ShowLyric | Key::ShowCover | Key::EmbeddedPic | Key::LocalPic | Key::AutoDownload | Key::DownloadLyric | Key::UseCustomDir => {
                p.refresh_now_playing(&self.now, true)
            }
            Key::TaskbarLyric | Key::TaskbarTwoLines => self.sync_taskbar_lyric(),
            Key::Frost => self.apply_theme(),
            Key::DeskLyric | Key::DeskLyricLocked => self.sync_desk_lyric(),
            _ => {}
        }
        p.save(false);
    }

    fn toggle_hotkey(&mut self, a: HotkeyAction) {
        {
            let mut g = self.player.core.lock();
            let d = &mut g.settings.hotkeys.disabled;
            if !d.remove(&a) {
                d.insert(a);
            }
            g.touch_settings();
        }
        self.register_hotkeys();
        self.player.save(false);
    }

    /// 关联 / 取消关联这些扩展名
    fn associate(&mut self, exts: &[&str], on: bool) {
        if on {
            if !crate::platform::shell::register_associations(exts) {
                self.set_status("关联失败");
            }
        } else {
            crate::platform::shell::unassociate(exts);
        }
        self.refresh_assoc();
        // 一个都不关联了：把程序在注册表里的登记也删掉，不留痕迹
        if !on && self.settings_win.as_ref().is_some_and(|s| !s.assoc.iter().any(|&a| a)) {
            crate::platform::shell::unregister_associations(&lay::group_exts(None));
        }
    }

    fn press_button(&mut self, b: Btn) {
        let p = self.player.clone();
        match b {
            Btn::DataDir => self.open_cfgdir(),
            Btn::BrowseLyricDir => self.browse_lyric_dir(),
            Btn::ResetTaskbarPos => self.act(Act::TaskbarReset),
            Btn::ResetColors => self.set_theme(|t| {
                let frost = t.frost;
                *t = crate::config::ThemeSettings { frost, ..Default::default() };
            }),
            Btn::Capture(a) => {
                if let Some(sw) = self.settings_win.as_mut() {
                    sw.capturing = Some(a);
                }
                return;
            }
        }
        p.save(false);
    }

    /// 改「颜色透明」的设置，立即应用到所有窗口
    fn set_theme(&mut self, f: impl FnOnce(&mut crate::config::ThemeSettings)) {
        {
            let mut g = self.player.core.lock();
            f(&mut g.settings.theme);
            g.touch_settings();
        }
        self.apply_theme();
    }

    fn open_choice(&mut self, kind: Choice) {
        let items = match kind {
            Choice::Device => {
                let cur = self.player.core.lock().settings.audio.device_id.clone();
                let devs = self.settings_win.as_ref().map(|s| s.devices.clone()).unwrap_or_default();
                let mut v = vec![check("系统默认设备", None, cur.is_empty(), Act::SetDevice(String::new()))];
                v.extend(devs.iter().map(|d| check(&d.name, None, d.id == cur, Act::SetDevice(d.id.clone()))));
                v
            }
            Choice::ProxyKind => {
                let cur = self.player.core.lock().settings.network.kind;
                crate::config::ProxyKind::ALL.iter().map(|k| check(model::proxy_label(*k), None, *k == cur, Act::SetProxy(*k))).collect()
            }
            Choice::LyricSource => {
                use crate::lyrics::online::Provider;
                let cur = self.player.core.lock().settings.lyrics.source;
                Provider::ALL.iter().map(|p| check(p.label(), None, *p == cur, Act::SetLyricSource(*p))).collect()
            }
            Choice::TaskbarAlign => {
                let cur = self.player.core.lock().settings.taskbar_lyric.align;
                (0..3u8).map(|a| check(model::taskbar_align_label(a), None, a == cur, Act::TaskbarAlign(a))).collect()
            }
            // 与桌面歌词右键菜单的「配色」「显示行数」相同
            Choice::DeskStyle | Choice::DeskLines => {
                let want = if kind == Choice::DeskStyle { "配色" } else { "显示行数" };
                self.desk_menu()
                    .into_iter()
                    .find_map(|it| match it {
                        super::menu::Item::Sub { label, items } if label == want => Some(items),
                        _ => None,
                    })
                    .unwrap_or_default()
            }
        };
        self.popup(items);
    }

    fn browse_lyric_dir(&mut self) {
        let (tx, hwnd) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            if let Some(d) = rfd::FileDialog::new().set_parent(&crate::platform::window::Parent(hwnd)).set_title("歌词文件存放位置").pick_folder() {
                let _ = tx.send(super::actions::AsyncMsg::LyricDir(d));
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd as _)), super::app::WM_APP_ASYNC, WPARAM(0), LPARAM(0));
                }
            }
        });
    }

    /// 点了一个控件
    fn settings_click(&mut self, it: &Item, st: &State, x: i32, y: i32) {
        match &it.w {
            Widget::Pill { act, .. } => match *act {
                Toggle::Key(k) if st.enabled => self.toggle_key(k),
                Toggle::Hotkey(a) if st.enabled => self.toggle_hotkey(a),
                Toggle::Format(i) if st.enabled => self.associate(FORMATS[i].2, !st.on),
                Toggle::Group(g) => self.associate(&lay::group_exts(g), !st.on),
                Toggle::Act(b) if st.enabled => self.press_button(b),
                _ => {}
            },
            Widget::Check(k, _) => self.toggle_key(*k),
            Widget::Drop(c) if st.enabled => self.open_choice(*c),
            Widget::Edit(f, _) if st.enabled => {
                if let Some(sw) = self.settings_win.as_mut() {
                    sw.focus = Some(*f);
                }
            }
            Widget::Path(..) => self.browse_lyric_dir(),
            Widget::Swatches(k) => {
                let Some(n) = pill::swatch_at(it.r, k.presets().len(), x, y) else { return };
                if let Some(rgb) = k.presets().get(n).copied() {
                    self.set_theme(|t| match k {
                        model::Swatch::Body => t.body = rgb,
                        model::Swatch::Focus => t.focus = rgb,
                    });
                    self.player.save(false);
                }
            }
            Widget::Slider => {
                let v = pill::slider_value(it.r, x);
                self.set_theme(|t| t.border_alpha = v);
                if let Some(sw) = self.settings_win.as_mut() {
                    sw.slider_drag = true;
                    unsafe { SetCapture(sw.win.hwnd) };
                }
            }
            _ => {}
        }
    }
}

fn modifiers() -> (bool, bool, bool, bool) {
    let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0 as i32) } < 0;
    (down(VK_CONTROL), down(VK_MENU), down(VK_SHIFT), down(VK_LWIN) || down(VK_RWIN))
}

/// 能点的控件（文字、分隔线不算）
fn clickable(w: &Widget) -> bool {
    !matches!(w, Widget::Label(_) | Widget::Title(_) | Widget::Groove(_))
}

impl App {
    pub fn settings_msg(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if self.settings_win.as_ref()?.win.hwnd != hwnd {
            return None;
        }
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        let items = self.settings_items();
        let hit = items.iter().position(|(it, _)| clickable(&it.w) && it.r.contains(x, y));
        match msg {
            WM_MOUSEMOVE => {
                if self.settings_win.as_ref()?.slider_drag {
                    if let Some((it, _)) = items.iter().find(|(it, _)| it.w == Widget::Slider) {
                        let v = pill::slider_value(it.r, x);
                        self.set_theme(|t| t.border_alpha = v);
                    }
                    self.settings_win.as_mut()?.win.dirty = true;
                    self.paint_settings();
                    return Some(LRESULT(0));
                }
                let sw = self.settings_win.as_mut()?;
                if let Some((dx, dy)) = sw.drag {
                    let (cx, cy) = super::resize::cursor();
                    sw.win.move_to(cx - dx, cy - dy);
                    return Some(LRESULT(0));
                }
                if !sw.inside {
                    sw.inside = true;
                    sw.win.dirty = true;
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    unsafe {
                        let _ = TrackMouseEvent(&mut tme);
                    }
                }
                let ht = (0..model::TABS.len()).find(|i| sp::tab_rect(*i).contains(x, y));
                let cell = hit.and_then(|i| match &items[i].0.w {
                    Widget::Swatches(k) => pill::swatch_at(items[i].0.r, k.presets().len(), x, y),
                    _ => None,
                });
                if hit != sw.hover || ht != sw.hover_tab || cell != sw.hover_cell {
                    (sw.hover, sw.hover_tab, sw.hover_cell) = (hit, ht, cell);
                    sw.win.dirty = true;
                }
            }
            0x02A3 => {
                // WM_MOUSELEAVE
                let sw = self.settings_win.as_mut()?;
                (sw.inside, sw.hover, sw.hover_tab, sw.hover_cell) = (false, None, None, None);
                sw.win.dirty = true;
            }
            WM_LBUTTONDOWN => {
                if let Some(t) = (0..model::TABS.len()).find(|i| sp::tab_rect(*i).contains(x, y)) {
                    self.settings_tab(t);
                    return Some(LRESULT(0));
                } else if frame::close_rect(sp::W).contains(x, y) {
                    return self.close_settings();
                } else if let Some(i) = hit {
                    let (it, st) = items[i].clone();
                    if let Some(sw) = self.settings_win.as_mut() {
                        sw.capturing = None;
                        sw.status.clear();
                        // 点回正在编辑的输入框时保留已输入的文字
                        if !matches!(&it.w, Widget::Edit(f, _) if sw.focus == Some(*f)) {
                            (sw.focus, sw.edit) = (None, None);
                        }
                    }
                    self.settings_click(&it, &st, x, y);
                } else {
                    let sw = self.settings_win.as_mut()?;
                    (sw.focus, sw.edit, sw.capturing) = (None, None, None);
                    let (cx, cy) = super::resize::cursor();
                    sw.drag = Some((cx - sw.win.pos.0, cy - sw.win.pos.1));
                    unsafe { SetCapture(hwnd) };
                }
                self.settings_win.as_mut()?.win.dirty = true;
            }
            WM_LBUTTONUP => {
                let sw = self.settings_win.as_mut()?;
                sw.drag = None;
                if std::mem::take(&mut sw.slider_drag) {
                    self.player.save(false);
                }
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            WM_CHAR => self.settings_char(wp.0 as u32),
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let a = self.settings_win.as_ref()?.capturing?;
                let vk = wp.0 as u32;
                if matches!(VIRTUAL_KEY(vk as u16), VK_CONTROL | VK_MENU | VK_SHIFT | VK_LWIN | VK_RWIN) {
                    return Some(LRESULT(0));
                }
                let combo = match VIRTUAL_KEY(vk as u16) {
                    VK_ESCAPE => None,
                    VK_BACK | VK_DELETE => Some(None),
                    _ => {
                        let (ctrl, alt, shift, win) = modifiers();
                        Some(Some(KeyCombo { ctrl, alt, shift, win, vk }))
                    }
                };
                if let Some(c) = combo {
                    self.bind_hotkey(a, c);
                }
                let sw = self.settings_win.as_mut()?;
                sw.capturing = None;
                sw.win.dirty = true;
            }
            WM_CLOSE => return self.close_settings(),
            _ => return None,
        }
        if self.settings_win.as_ref().is_some_and(|s| s.win.dirty) {
            self.paint_settings();
        }
        Some(LRESULT(0))
    }

    /// 在输入框里打字
    fn settings_char(&mut self, ch: u32) {
        let Some(f) = self.settings_win.as_ref().and_then(|s| s.focus) else { return };
        // 第一次按键时从设置里取出当前内容，之后都在编辑缓冲里改
        let mut cur = match &self.settings_win.as_ref().and_then(|s| s.edit.clone()) {
            Some((e, buf)) if *e == f => buf.clone(),
            _ => model::raw_text(&self.player.core.lock().settings, f),
        };
        match ch {
            8 => {
                cur.pop();
            }
            13 | 27 => {
                if let Some(sw) = self.settings_win.as_mut() {
                    (sw.focus, sw.edit) = (None, None);
                    sw.win.dirty = true;
                }
                self.player.save(false);
                return;
            }
            // 数字框只接受数字、负号、小数点
            c if f.numeric() && !char::from_u32(c).is_some_and(|ch| ch.is_ascii_digit() || ch == '-' || ch == '.') => return,
            c if c >= 32 => cur.extend(char::from_u32(c)),
            _ => {}
        }
        let applied = {
            let mut g = self.player.core.lock();
            let ok = model::set_text(&mut g.settings, f, &cur);
            if ok {
                g.touch_settings();
            }
            ok
        };
        if let Some(sw) = self.settings_win.as_mut() {
            sw.edit = Some((f, cur));
            sw.win.dirty = true;
        }
        if applied && f.taskbar() {
            self.sync_taskbar_lyric();
        }
        if applied && f == Field::DeskFont {
            self.sync_desk_lyric();
        }
    }

    fn bind_hotkey(&mut self, a: HotkeyAction, c: Option<KeyCombo>) {
        {
            let mut g = self.player.core.lock();
            g.settings.hotkeys.bindings.insert(a, c);
            g.touch_settings();
        }
        self.register_hotkeys();
        let failed = self.hotkey_failed.contains(&a);
        self.set_status(if failed { "这个热键已被其他程序占用，请换一个" } else { "" });
        self.player.save(false);
    }

    pub fn close_settings(&mut self) -> Option<LRESULT> {
        if let Some(mut s) = self.settings_win.take() {
            s.win.close();
        }
        self.player.save(false);
        Some(LRESULT(0))
    }
}
