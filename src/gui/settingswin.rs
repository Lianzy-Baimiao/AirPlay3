//! 「配置」窗口：切换分页、勾选即生效、下拉选择、文字输入、热键捕获。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::app::App;
use super::menu::{Act, check};
use super::panels::settings::{self as sp, SettingsModel};
use super::popup::Popup;
use super::settings_model::{self as model, Btn, ChoiceKind, Key, Widget};
use crate::config::{HotkeyAction, KeyCombo};

pub struct SettingsWin {
    pub win: Popup,
    pub tab: usize,
    pub focus: Option<usize>,
    pub hover: Option<usize>,
    pub hover_tab: Option<usize>,
    pub capturing: Option<usize>,
    pub devices: Vec<crate::audio::devices::DeviceInfo>,
    pub status: String,
    pub drag: Option<(i32, i32)>,
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
        self.settings_win = Some(SettingsWin { win, tab: 0, focus: None, hover: None, hover_tab: None, capturing: None, devices, status: String::new(), drag: None });
        self.paint_settings();
        if let Some(s) = &self.settings_win {
            s.win.show(true);
        }
    }

    fn settings_items(&self) -> Vec<(Widget, bool, String)> {
        let Some(sw) = self.settings_win.as_ref() else { return Vec::new() };
        let g = self.player.core.lock();
        let s = &g.settings;
        let dev = if s.audio.device_id.is_empty() {
            "系统默认设备".to_string()
        } else {
            sw.devices.iter().find(|d| d.id == s.audio.device_id).map_or("未知设备".to_string(), |d| d.name.clone())
        };
        model::widgets(sw.tab, s, &dev)
            .into_iter()
            .map(|w| {
                let (on, val) = match &w {
                    Widget::Check(k, _) => (model::get(s, *k), String::new()),
                    Widget::Choice(_, v, _) => (false, v.clone()),
                    Widget::Text(f, _) => (false, model::text(s, *f)),
                    Widget::Hotkey(a) => {
                        let text = s.hotkeys.combo(*a).map_or("未设置".to_string(), |c| crate::platform::hotkeys::combo_text(&c));
                        // 注册失败（被其它程序占用）的热键直接标出来，不用等改键时才知道
                        let busy = s.hotkeys.enabled && s.hotkeys.combo(*a).is_some() && self.hotkey_failed.contains(a);
                        (false, if busy { format!("{text}（已被其他程序占用）") } else { text })
                    }
                    _ => (false, String::new()),
                };
                (w, on, val)
            })
            .collect()
    }

    pub fn paint_settings(&mut self) {
        let items = self.settings_items();
        let Some(sw) = self.settings_win.as_mut() else { return };
        let m = SettingsModel { tab: sw.tab, items: &items, focus: sw.focus, hover: sw.hover, hover_tab: sw.hover_tab, capturing: sw.capturing, status: &sw.status };
        sp::paint(&mut sw.win.canvas, &m);
        sw.win.present();
    }

    fn set_status(&mut self, s: impl Into<String>) {
        if let Some(sw) = self.settings_win.as_mut() {
            sw.status = s.into();
        }
    }

    /// 勾选框：修改设置并让它立即生效
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
            Key::ShowLyric | Key::ShowCover | Key::EmbeddedPic | Key::LocalPic | Key::AutoDownload | Key::UseCustomDir => p.refresh_now_playing(&self.now, true),
            Key::TaskbarLyric => self.sync_taskbar_lyric(),
            _ => {}
        }
        p.save(false);
    }

    fn press_button(&mut self, b: Btn) {
        let p = self.player.clone();
        match b {
            Btn::OpenConfigDir => crate::platform::shell::open(&p.core.lock().dirs.root.to_string_lossy()),
            Btn::BrowseLyricDir => self.browse_lyric_dir(),
            Btn::ResetHotkeys => {
                p.core.lock().settings.hotkeys.bindings.clear();
                p.core.lock().touch_settings();
                self.register_hotkeys();
            }
            Btn::Associate => {
                let mut exts: Vec<&str> = crate::media::AUDIO_EXTS.to_vec();
                exts.push("cue");
                let ok = crate::platform::shell::register_associations(&exts);
                self.set_status(if ok { "已关联音乐文件" } else { "关联失败" });
            }
            Btn::Unassociate => {
                let mut exts: Vec<&str> = crate::media::AUDIO_EXTS.to_vec();
                exts.push("cue");
                crate::platform::shell::unregister_associations(&exts);
                self.set_status("已取消关联");
            }
        }
        p.save(false);
    }

    fn open_choice(&mut self, kind: ChoiceKind) {
        let items = match kind {
            ChoiceKind::Device => {
                let cur = self.player.core.lock().settings.audio.device_id.clone();
                let devs = self.settings_win.as_ref().map(|s| s.devices.clone()).unwrap_or_default();
                let mut v = vec![check("系统默认设备", None, cur.is_empty(), Act::SetDevice(String::new()))];
                v.extend(devs.iter().map(|d| check(&d.name, None, d.id == cur, Act::SetDevice(d.id.clone()))));
                v
            }
            ChoiceKind::ProxyKind => {
                let cur = self.player.core.lock().settings.network.kind;
                crate::config::ProxyKind::ALL.iter().map(|k| check(model::proxy_label(*k), None, *k == cur, Act::SetProxy(*k))).collect()
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
}
fn modifiers() -> (bool, bool, bool, bool) {
    let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0 as i32) } < 0;
    (down(VK_CONTROL), down(VK_MENU), down(VK_SHIFT), down(VK_LWIN) || down(VK_RWIN))
}

impl App {
    pub fn settings_msg(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if self.settings_win.as_ref()?.win.hwnd != hwnd {
            return None;
        }
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        let items = self.settings_items();
        let tab = self.settings_win.as_ref()?.tab;
        let hit = items.iter().enumerate().position(|(i, (w, ..))| sp::control_rect(tab, i, w).contains(x, y) && !matches!(w, Widget::Label(_)));
        match msg {
            WM_MOUSEMOVE => {
                let sw = self.settings_win.as_mut()?;
                if let Some((dx, dy)) = sw.drag {
                    let (cx, cy) = super::resize::cursor();
                    sw.win.move_to(cx - dx, cy - dy);
                    return Some(LRESULT(0));
                }
                let ht = (0..model::TABS.len()).find(|i| sp::tab_rect(*i).contains(x, y));
                if hit != sw.hover || ht != sw.hover_tab {
                    sw.hover = hit;
                    sw.hover_tab = ht;
                    sw.win.dirty = true;
                }
            }
            WM_LBUTTONDOWN => {
                if let Some(t) = (0..model::TABS.len()).find(|i| sp::tab_rect(*i).contains(x, y)) {
                    let sw = self.settings_win.as_mut()?;
                    (sw.tab, sw.focus, sw.capturing, sw.hover) = (t, None, None, None);
                    sw.status.clear();
                } else if y < 20 && x > sp::W - 26 {
                    return self.close_settings();
                } else if let Some(i) = hit {
                    let w = items[i].0.clone();
                    if let Some(sw) = self.settings_win.as_mut() {
                        sw.focus = None;
                        sw.capturing = None;
                    }
                    match w {
                        Widget::Check(k, _) => self.toggle_key(k),
                        Widget::Choice(_, _, kind) => self.open_choice(kind),
                        Widget::Text(..) => self.settings_win.as_mut()?.focus = Some(i),
                        Widget::Hotkey(_) => self.settings_win.as_mut()?.capturing = Some(i),
                        Widget::Button(_, b) => self.press_button(b),
                        Widget::Label(_) => {}
                    }
                } else {
                    let sw = self.settings_win.as_mut()?;
                    let (cx, cy) = super::resize::cursor();
                    sw.drag = Some((cx - sw.win.pos.0, cy - sw.win.pos.1));
                    unsafe { SetCapture(hwnd) };
                }
                self.settings_win.as_mut()?.win.dirty = true;
            }
            WM_LBUTTONUP => {
                self.settings_win.as_mut()?.drag = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            WM_CHAR => {
                let sw = self.settings_win.as_ref()?;
                let Some(i) = sw.focus else { return Some(LRESULT(0)) };
                let Widget::Text(f, _) = items.get(i)?.0 else { return Some(LRESULT(0)) };
                let mut g = self.player.core.lock();
                let mut cur = match f {
                    model::Field::ProxyPass => g.settings.network.pass.clone(),
                    _ => model::text(&g.settings, f),
                };
                match wp.0 as u32 {
                    8 => {
                        cur.pop();
                    }
                    13 | 27 => {
                        drop(g);
                        self.settings_win.as_mut()?.focus = None;
                        self.settings_win.as_mut()?.win.dirty = true;
                        return Some(LRESULT(0));
                    }
                    c if c >= 32 => cur.extend(char::from_u32(c)),
                    _ => {}
                }
                model::set_text(&mut g.settings, f, &cur);
                g.touch_settings();
                drop(g);
                self.settings_win.as_mut()?.win.dirty = true;
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let Some(i) = self.settings_win.as_ref()?.capturing else { return None };
                let Some(Widget::Hotkey(a)) = items.get(i).map(|t| t.0.clone()) else { return None };
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

    fn bind_hotkey(&mut self, a: HotkeyAction, c: Option<KeyCombo>) {
        {
            let mut g = self.player.core.lock();
            g.settings.hotkeys.bindings.insert(a, c);
            g.touch_settings();
        }
        self.register_hotkeys();
        let failed = self.hotkey_failed.contains(&a);
        self.set_status(if failed { "该热键已被其他程序占用，请换一个" } else { "" });
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
