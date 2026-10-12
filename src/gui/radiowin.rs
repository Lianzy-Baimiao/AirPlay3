//! 「网络电台」窗口（替代原版已关闭的「音乐导航 / 精选电台」）：推荐 / 热门 / 收藏 / 搜索，双击播放。
//! 听电台时「正在播放」显示电台名、当前节目（ICY）和电台图标。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::actions::AsyncMsg;
use super::app::{App, WM_APP_ASYNC};
use super::menu::{Act, check};
use super::panels::radio::{self as rp, RadioModel, Row};
use super::popup::Popup;
use crate::radio::Station;

/// 「推荐」可选的地区（国家 / 地区代码；空 = 全球）
pub const REGIONS: [(&str, &str); 12] = [
    ("CN", "中国大陆"),
    ("TW", "台湾"),
    ("HK", "香港"),
    ("MO", "澳门"),
    ("SG", "新加坡"),
    ("MY", "马来西亚"),
    ("JP", "日本"),
    ("KR", "韩国"),
    ("US", "美国"),
    ("GB", "英国"),
    ("DE", "德国"),
    ("", "全球"),
];

fn region_name(code: &str) -> &'static str {
    REGIONS.iter().find(|r| r.0 == code).map_or("全球", |r| r.1)
}

/// 默认地区：按系统的区域设置（zh-CN → CN）
fn default_region() -> String {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(&mut buf) };
    let s = String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize]);
    let cc = s.rsplit('-').next().unwrap_or("").to_ascii_uppercase();
    if REGIONS.iter().any(|r| r.0 == cc) { cc } else { "CN".into() }
}

pub struct RadioWin {
    pub win: Popup,
    /// 0 推荐，1 热门，2 收藏，3 搜索结果
    pub tab: usize,
    pub region: String,
    pub query: String,
    pub focus: bool,
    pub list: Vec<Station>,
    pub selected: Option<usize>,
    pub hover: Option<usize>,
    pub hot: Option<usize>,
    pub scroll: i32,
    pub status: String,
    pub seq: u64,
    pub drag: Option<(i32, i32)>,
    /// 正在播放的电台地址（变了要重画列表里的标记）
    playing: String,
    /// 状态行在显示播放状态（连接中 / 正在播放），随电台状态更新
    status_playing: bool,
}

impl App {
    pub fn open_radio(&mut self) {
        if let Some(r) = &self.radio_win {
            r.win.show(true);
            return;
        }
        let (region, favs) = {
            let g = self.player.core.lock();
            (g.settings.radio.country.clone(), !g.settings.radio.favorites.is_empty())
        };
        let region = if region.is_empty() { default_region() } else { region };
        let (x, y) = Popup::centered(rp::W, rp::H);
        let win = Popup::new(x, y, rp::W, rp::H, Some(self.bar_hwnd()), "网络电台");
        self.radio_win = Some(RadioWin {
            win,
            tab: if favs { 2 } else { 0 },
            region,
            query: String::new(),
            focus: false,
            list: Vec::new(),
            selected: None,
            hover: None,
            hot: None,
            scroll: 0,
            status: String::new(),
            seq: 0,
            drag: None,
            playing: String::new(),
            status_playing: false,
        });
        self.load_radio();
        self.paint_radio();
        if let Some(r) = &self.radio_win {
            r.win.show(true);
        }
    }

    pub fn close_radio(&mut self) {
        if let Some(mut r) = self.radio_win.take() {
            r.win.close();
        }
    }

    /// 按当前分页加载电台列表（收藏直接取配置，其它后台联网）
    pub fn load_radio(&mut self) {
        let (net, favs) = {
            let g = self.player.core.lock();
            (g.settings.network.clone(), g.settings.radio.favorites.clone())
        };
        let Some(r) = self.radio_win.as_mut() else { return };
        r.seq += 1;
        (r.selected, r.hover, r.scroll, r.status_playing) = (None, None, 0, false);
        if r.tab == 2 {
            r.status = format!("{} 个收藏", favs.len());
            r.list = favs;
            r.win.dirty = true;
            return;
        }
        r.list.clear();
        r.status = "正在加载…".into();
        r.win.dirty = true;
        let (seq, tab, region, query) = (r.seq, r.tab, r.region.clone(), r.query.trim().to_string());
        let (tx, hwnd) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            let res = match tab {
                0 => crate::radio::top(&net, &region, 300),
                1 => crate::radio::top(&net, "", 300),
                _ => crate::radio::search(&net, &query, ""),
            };
            let _ = tx.send(AsyncMsg::Radio(seq, res));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
            }
        });
    }

    pub fn on_radio_list(&mut self, seq: u64, res: Result<Vec<Station>, String>) {
        let Some(r) = self.radio_win.as_mut() else { return };
        if seq != r.seq {
            return; // 已经换了分页 / 重新搜索
        }
        match res {
            Ok(v) => {
                r.status = if v.is_empty() { "没有找到电台".into() } else { format!("{} 个电台，双击播放", v.len()) };
                r.list = v;
            }
            Err(e) => r.status = e,
        }
        r.win.dirty = true;
        // 仅调试构建：自测时列表到了就播放第 N 个（AIRPLAY3_TEST_RADIO=N）
        #[cfg(debug_assertions)]
        if let Some(i) = std::env::var("AIRPLAY3_TEST_RADIO").ok().and_then(|v| v.parse::<usize>().ok()) {
            if let Some(r) = self.radio_win.as_mut().filter(|r| i < r.list.len()) {
                r.selected = Some(i);
                self.play_selected_station();
            }
        }
    }

    fn radio_selected(&self) -> Option<(Station, Vec<Station>)> {
        let r = self.radio_win.as_ref()?;
        Some((r.list.get(r.selected?)?.clone(), r.list.clone()))
    }

    pub fn play_selected_station(&mut self) {
        if let Some((st, list)) = self.radio_selected() {
            if let Some(r) = self.radio_win.as_mut() {
                r.status = format!("正在连接「{}」…", st.name);
                r.status_playing = true;
                r.win.dirty = true;
            }
            self.player.play_station(st, list);
        }
    }

    /// 收藏 / 取消收藏选中的电台
    pub fn toggle_favorite_station(&mut self) {
        let Some((st, _)) = self.radio_selected() else { return };
        let now_fav = {
            let mut g = self.player.core.lock();
            let f = &mut g.settings.radio.favorites;
            let fav = match f.iter().position(|s| s.url == st.url) {
                Some(i) => {
                    f.remove(i);
                    false
                }
                None => {
                    f.push(st.clone());
                    true
                }
            };
            g.touch_settings();
            fav
        };
        self.player.save(false);
        let on_fav_tab = self.radio_win.as_ref().is_some_and(|r| r.tab == 2);
        if on_fav_tab {
            self.load_radio();
        }
        if let Some(r) = self.radio_win.as_mut() {
            r.status = if now_fav { format!("已收藏「{}」", st.name) } else { format!("已取消收藏「{}」", st.name) };
            r.status_playing = false;
            r.win.dirty = true;
        }
    }

    fn open_region_menu(&mut self) {
        let cur = self.radio_win.as_ref().map(|r| r.region.clone()).unwrap_or_default();
        let items = REGIONS.iter().map(|(code, name)| check(name, None, *code == cur, Act::SetRadioRegion(code.to_string()))).collect();
        self.popup(items);
    }

    /// 选了地区：存起来并切到「推荐」
    pub fn set_radio_region(&mut self, code: String) {
        {
            let mut g = self.player.core.lock();
            g.settings.radio.country = code.clone();
            g.touch_settings();
        }
        if let Some(r) = self.radio_win.as_mut() {
            (r.region, r.tab) = (code, 0);
        }
        self.load_radio();
    }

    pub fn paint_radio(&mut self) {
        let (favs, playing) = {
            let g = self.player.core.lock();
            let favs: Vec<String> = g.settings.radio.favorites.iter().map(|s| s.url.clone()).collect();
            (favs, g.radio.as_ref().map(|r| r.station.url.clone()).unwrap_or_default())
        };
        let Some(r) = self.radio_win.as_mut() else { return };
        r.playing = playing.clone();
        let rows: Vec<Row> = r
            .list
            .iter()
            .map(|s| Row { name: &s.name, detail: s.detail(), format: s.format(), favorite: favs.contains(&s.url), playing: !playing.is_empty() && s.url == playing })
            .collect();
        let fav_label = match r.selected.and_then(|i| r.list.get(i)) {
            Some(s) if favs.contains(&s.url) => "取消收藏",
            _ => "收藏",
        };
        let m = RadioModel {
            tab: r.tab,
            region: region_name(&r.region),
            query: &r.query,
            focus: r.focus,
            rows: &rows,
            selected: r.selected,
            hover: r.hover,
            scroll: r.scroll,
            status: &r.status,
            hot: r.hot,
            fav_label,
        };
        rp::paint(&mut r.win.canvas, &m);
        r.win.present();
    }

    /// 每帧：听电台时把电台名、当前节目、连接状态写进「正在播放」；换台时加载电台图标
    pub fn sync_radio(&mut self) {
        let radio = self.player.core.lock().radio.clone();
        let playing = radio.as_ref().map(|r| r.station.url.clone()).unwrap_or_default();
        let st = crate::audio::stream::status();
        if let Some(w) = self.radio_win.as_mut() {
            if w.playing != playing {
                w.win.dirty = true;
            }
            // 状态行：连接中 → 正在播放 / 出错
            if let (true, Some(r)) = (w.status_playing, radio.as_ref()) {
                let s = if st.url != r.station.url || st.buffering {
                    format!("正在连接「{}」…", r.station.name)
                } else if !st.error.is_empty() {
                    format!("「{}」：{}", r.station.name, st.error)
                } else {
                    format!("正在播放「{}」 {}", r.station.name, st.info)
                };
                if s != w.status {
                    w.status = s;
                    w.win.dirty = true;
                }
            }
        }
        let Some(r) = radio else { return };
        let mine = st.url == r.station.url;
        let artist = if !mine || st.buffering && st.title.is_empty() {
            "正在连接…".to_string()
        } else if !st.error.is_empty() {
            st.error.clone()
        } else if st.title.is_empty() {
            "网络电台".to_string()
        } else {
            st.title.clone()
        };
        let album = [r.station.detail(), st.info.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
        let new_station = {
            let mut n = self.now.lock();
            let changed = n.title != r.station.name || n.track.is_some();
            if changed {
                *n = crate::player::nowplaying::NowPlaying { rev: n.rev + 1, loaded: true, ..Default::default() };
                n.title = r.station.name.clone();
            }
            if n.artist != artist || n.album != album {
                (n.artist, n.album) = (artist, album);
                n.rev += 1;
            }
            changed
        };
        if new_station {
            self.player.load_station_icon(&r.station, &self.now);
            self.dirty_all();
        }
    }
}

impl App {
    /// 电台窗口的消息；不是该窗口时返回 None
    pub fn radio_msg(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let r = self.radio_win.as_mut()?;
        if r.win.hwnd != hwnd {
            return None;
        }
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        let row_at = |r: &RadioWin, y: i32| {
            let i = ((y - rp::LIST.y + r.scroll) / rp::ROW_H) as usize;
            (rp::LIST.contains(rp::LIST.x + 1, y) && i < r.list.len()).then_some(i)
        };
        let hot_at = |x: i32, y: i32| {
            let btn = [rp::BTN_REGION, rp::BTN_SEARCH, rp::BTN_FAV, rp::BTN_PLAY, rp::BTN_CLOSE].iter().position(|b| b.contains(x, y));
            btn.or_else(|| (0..rp::TABS.len()).find(|&i| rp::tab_rect(i).contains(x, y)).map(|i| 10 + i))
        };
        match msg {
            WM_MOUSEMOVE => {
                if let Some((dx, dy)) = r.drag {
                    let (cx, cy) = super::resize::cursor();
                    r.win.move_to(cx - dx, cy - dy);
                    return Some(LRESULT(0));
                }
                let hover = if rp::LIST.contains(x, y) { row_at(r, y) } else { None };
                let hot = hot_at(x, y);
                if hover != r.hover || hot != r.hot {
                    (r.hover, r.hot) = (hover, hot);
                    r.win.dirty = true;
                }
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let dbl = msg == WM_LBUTTONDBLCLK;
                r.focus = rp::FIELD_SEARCH.contains(x, y);
                r.win.dirty = true;
                match hot_at(x, y) {
                    Some(0) => self.open_region_menu(),
                    Some(1) => self.radio_search(),
                    Some(2) => self.toggle_favorite_station(),
                    Some(3) => self.play_selected_station(),
                    Some(4) => self.close_radio(),
                    Some(t) if t >= 10 => {
                        r.tab = t - 10;
                        self.load_radio();
                    }
                    _ if y < 20 && x > rp::W - 24 => self.close_radio(),
                    _ if rp::LIST.contains(x, y) => {
                        if let Some(i) = row_at(r, y) {
                            r.selected = Some(i);
                            if dbl {
                                self.play_selected_station();
                            }
                        }
                    }
                    _ if !rp::FIELD_SEARCH.contains(x, y) => {
                        let (cx, cy) = super::resize::cursor();
                        r.drag = Some((cx - r.win.pos.0, cy - r.win.pos.1));
                        unsafe { SetCapture(hwnd) };
                    }
                    _ => {}
                }
            }
            WM_LBUTTONUP => {
                r.drag = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            WM_MOUSEWHEEL => {
                let d = ((wp.0 >> 16) & 0xFFFF) as i16 as i32;
                let max = (r.list.len() as i32 * rp::ROW_H - rp::LIST.h).max(0);
                r.scroll = (r.scroll - d / 120 * rp::ROW_H * 3).clamp(0, max);
                r.win.dirty = true;
            }
            WM_CHAR if r.focus => {
                match wp.0 as u32 {
                    8 => {
                        r.query.pop();
                    }
                    13 => self.radio_search(),
                    27 => self.close_radio(),
                    22 => {
                        if let Some(t) = super::lyricwin::clipboard_text() {
                            r.query.push_str(t.lines().next().unwrap_or("").trim());
                        }
                    }
                    c if c >= 32 => r.query.extend(char::from_u32(c)),
                    _ => {}
                }
                if let Some(r) = self.radio_win.as_mut() {
                    r.win.dirty = true;
                }
            }
            WM_KEYDOWN if wp.0 as u32 == 0x0D && !r.focus => self.play_selected_station(),
            WM_KEYDOWN if wp.0 as u32 == 0x1B => self.close_radio(),
            WM_CLOSE => self.close_radio(),
            _ => return None,
        }
        if self.radio_win.as_ref().is_some_and(|r| r.win.dirty) {
            self.paint_radio();
        }
        Some(LRESULT(0))
    }

    /// 搜索框有内容：搜索；空的：回到推荐
    fn radio_search(&mut self) {
        let Some(r) = self.radio_win.as_mut() else { return };
        r.tab = if r.query.trim().is_empty() { 0 } else { 3 };
        r.focus = false;
        self.load_radio();
    }
}
