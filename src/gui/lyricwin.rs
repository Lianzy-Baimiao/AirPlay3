//! 歌词：歌词图片面板右键菜单 + 搜索 / 下载歌词窗口（LRCLIB），手动关联本地歌词、调整歌词快慢。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::actions::AsyncMsg;
use super::app::{App, WM_APP_ASYNC};
use super::panels::search::{self as sp, ResultRow, SearchModel};
use super::popup::Popup;
use crate::library::TrackId;
use crate::lyrics::lrclib::Record;
use crate::player::nowplaying::Link;

pub struct LyricSearch {
    pub win: Popup,
    pub track: TrackId,
    pub fields: [String; 2],
    pub focus: Option<usize>,
    pub results: Vec<Record>,
    pub selected: Option<usize>,
    pub hover: Option<usize>,
    pub hot_button: Option<usize>,
    pub scroll: i32,
    pub status: String,
    pub busy: bool,
    pub seq: u64,
    pub drag: Option<(i32, i32)>,
    /// 当前歌曲时长（秒），用于把时长接近的结果排在前面
    pub duration: f64,
}

impl App {
    /// 打开搜索窗口（预填清理过的标题与艺人，可再修改）并立即搜索
    pub fn open_lyric_search(&mut self) {
        let Some(id) = self.ui.snap.current else { return };
        let (title, artist, duration) = {
            let g = self.player.core.lock();
            g.lib.track(id).map(|t| (t.display_title(), t.artist.clone(), t.duration)).unwrap_or_default()
        };
        let (title, artist) = (crate::lyrics::query::clean_title(&title), crate::lyrics::query::clean_artist(&artist));
        if let Some(mut old) = self.search.take() {
            old.win.close();
        }
        let (x, y) = Popup::centered(sp::W, sp::H);
        let win = Popup::new(x, y, sp::W, sp::H, Some(self.bar_hwnd()), "搜索歌词");
        self.search = Some(LyricSearch {
            win,
            track: id,
            fields: [title, artist],
            focus: Some(0),
            results: Vec::new(),
            selected: None,
            hover: None,
            hot_button: None,
            scroll: 0,
            status: String::new(),
            busy: false,
            seq: 0,
            drag: None,
            duration,
        });
        self.paint_search();
        if let Some(s) = &self.search {
            s.win.show(true);
        }
        self.run_search();
    }

    pub fn run_search(&mut self) {
        let net = self.player.core.lock().settings.network.clone();
        let Some(s) = self.search.as_mut() else { return };
        s.seq += 1;
        s.busy = true;
        s.status = "正在搜索…".into();
        s.win.dirty = true;
        let (seq, title, artist) = (s.seq, s.fields[0].clone(), s.fields[1].clone());
        let (tx, hwnd) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            // 后台线程出错也要让界面得到结果，不能一直显示“正在搜索”
            let (title, artist) = (title.trim().to_string(), artist.trim().to_string());
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::lyrics::lrclib::search_loose(&net, &title, &artist)))
                .unwrap_or_else(|_| Err("内部错误".into()));
            let _ = tx.send(AsyncMsg::LyricResults(seq, r));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
            }
        });
    }

    pub fn on_lyric_results(&mut self, seq: u64, r: Result<(Vec<Record>, bool), String>) {
        let Some(s) = self.search.as_mut() else { return };
        if s.seq != seq {
            return;
        }
        s.busy = false;
        match r {
            Ok((mut v, loose)) => {
                // 带时间轴的在前；同类里时长与当前歌曲接近的在前（只按标题搜时同名歌很多）
                let dur = s.duration;
                let dist = |r: &Record| if dur > 0.0 && r.duration > 0.0 { (r.duration - dur).abs() } else { f64::MAX };
                v.sort_by(|a, b| b.has_synced().cmp(&a.has_synced()).then(dist(a).total_cmp(&dist(b))));
                s.status = if v.is_empty() {
                    "没有找到，试试修改标题或艺人".into()
                } else if loose {
                    format!("按标题找到 {} 条，艺人可能不同", v.len())
                } else {
                    format!("找到 {} 条，双击或点「下载」使用", v.len())
                };
                s.selected = if v.is_empty() { None } else { Some(0) };
                s.results = v;
                s.scroll = 0;
            }
            Err(e) => s.status = format!("搜索失败：{e}"),
        }
        s.win.dirty = true;
    }

    /// 保存所选歌词到歌词目录并关联到这首歌
    pub fn download_selected(&mut self) {
        let Some(s) = self.search.as_ref() else { return };
        let Some(rec) = s.selected.and_then(|i| s.results.get(i)).cloned() else { return };
        let id = s.track;
        let (dir, title, artist) = {
            let g = self.player.core.lock();
            let t = g.lib.track(id);
            (crate::player::nowplaying::save_lyric_dir(&g), t.map(|t| t.display_title()).unwrap_or_default(), t.map(|t| t.artist.clone()).unwrap_or_default())
        };
        match crate::lyrics::lrclib::save(&rec, &dir, &title, &artist) {
            Ok(path) => {
                self.player.set_lyric_link(id, Link::File(path), &self.now);
                self.close_search();
                self.dirty(super::app::Pane::Cover);
            }
            Err(e) => {
                if let Some(s) = self.search.as_mut() {
                    s.status = format!("保存失败：{e}");
                    s.win.dirty = true;
                }
            }
        }
    }

    pub fn close_search(&mut self) {
        if let Some(mut s) = self.search.take() {
            s.win.close();
        }
    }

    pub fn paint_search(&mut self) {
        let Some(s) = self.search.as_mut() else { return };
        let durs: Vec<String> = s.results.iter().map(|r| super::panels::bar::fmt_time(r.duration)).collect();
        let rows: Vec<ResultRow> = s
            .results
            .iter()
            .zip(&durs)
            .map(|(r, d)| ResultRow { title: &r.track_name, artist: &r.artist_name, album: &r.album_name, duration: d, synced: r.has_synced() })
            .collect();
        let m = SearchModel {
            title: &s.fields[0],
            artist: &s.fields[1],
            focus: s.focus,
            rows: &rows,
            selected: s.selected,
            hover: s.hover,
            scroll: s.scroll,
            status: &s.status,
            hot_button: s.hot_button,
            can_download: s.selected.is_some() && !s.busy,
        };
        sp::paint(&mut s.win.canvas, &m);
        s.win.present();
    }
}
fn clipboard_text() -> Option<String> {
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    unsafe {
        OpenClipboard(None).ok()?;
        let h = GetClipboardData(13).ok(); // CF_UNICODETEXT
        let s = h.and_then(|h| {
            let p = GlobalLock(HGLOBAL(h.0)) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(HGLOBAL(h.0));
            Some(s)
        });
        let _ = CloseClipboard();
        s
    }
}

impl App {
    /// 歌词搜索窗口的消息；不是该窗口时返回 None
    pub fn search_msg(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let s = self.search.as_mut()?;
        if s.win.hwnd != hwnd {
            return None;
        }
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        let row_at = |s: &LyricSearch, y: i32| {
            if sp::LIST.contains(sp::LIST.x + 1, y) {
                let i = ((y - sp::LIST.y + s.scroll) / sp::ROW_H) as usize;
                (i < s.results.len()).then_some(i)
            } else {
                None
            }
        };
        match msg {
            WM_MOUSEMOVE => {
                if let Some((dx, dy)) = s.drag {
                    let (cx, cy) = super::resize::cursor();
                    s.win.move_to(cx - dx, cy - dy);
                    return Some(LRESULT(0));
                }
                let hover = if sp::LIST.contains(x, y) { row_at(s, y) } else { None };
                let hot = [sp::BTN_SEARCH, sp::BTN_DOWNLOAD, sp::BTN_CLOSE].iter().position(|r| r.contains(x, y));
                if hover != s.hover || hot != s.hot_button {
                    s.hover = hover;
                    s.hot_button = hot;
                    s.win.dirty = true;
                }
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let dbl = msg == WM_LBUTTONDBLCLK;
                if sp::FIELD_TITLE.contains(x, y) {
                    s.focus = Some(0);
                } else if sp::FIELD_ARTIST.contains(x, y) {
                    s.focus = Some(1);
                } else if sp::BTN_SEARCH.contains(x, y) {
                    self.run_search();
                } else if sp::BTN_DOWNLOAD.contains(x, y) {
                    self.download_selected();
                } else if sp::BTN_CLOSE.contains(x, y) || (y < 20 && x > sp::W - 24) {
                    self.close_search();
                } else if sp::LIST.contains(x, y) {
                    if let Some(i) = row_at(s, y) {
                        s.selected = Some(i);
                        s.win.dirty = true;
                        if dbl {
                            self.download_selected();
                        }
                    }
                } else {
                    let (cx, cy) = super::resize::cursor();
                    s.drag = Some((cx - s.win.pos.0, cy - s.win.pos.1));
                    unsafe { SetCapture(hwnd) };
                }
                if let Some(s) = self.search.as_mut() {
                    s.win.dirty = true;
                }
            }
            WM_LBUTTONUP => {
                s.drag = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            WM_MOUSEWHEEL => {
                let d = ((wp.0 >> 16) & 0xFFFF) as i16 as i32;
                let max = (s.results.len() as i32 * sp::ROW_H - sp::LIST.h).max(0);
                s.scroll = (s.scroll - d / 120 * sp::ROW_H * 3).clamp(0, max);
                s.win.dirty = true;
            }
            WM_CHAR => {
                let Some(f) = s.focus else { return Some(LRESULT(0)) };
                match wp.0 as u32 {
                    8 => {
                        s.fields[f].pop();
                    }
                    9 => s.focus = Some(1 - f),
                    13 => self.run_search(),
                    27 => self.close_search(),
                    22 => {
                        if let Some(t) = clipboard_text() {
                            s.fields[f].push_str(t.lines().next().unwrap_or("").trim());
                        }
                    }
                    c if c >= 32 => {
                        if let Some(ch) = char::from_u32(c) {
                            s.fields[f].push(ch);
                        }
                    }
                    _ => {}
                }
                if let Some(s) = self.search.as_mut() {
                    s.win.dirty = true;
                }
            }
            WM_CLOSE => self.close_search(),
            _ => return None,
        }
        if self.search.as_ref().is_some_and(|s| s.win.dirty) {
            self.paint_search();
        }
        Some(LRESULT(0))
    }

    /// 「下载专辑图片」：后台从网上找图片，保存到本地并关联到当前歌曲
    pub fn download_cover(&mut self) {
        let Some(id) = self.ui.snap.current else { return };
        self.ui.cover_status = Some(("正在下载专辑图片…".into(), std::time::Instant::now()));
        self.dirty(super::app::Pane::Cover);
        let (p, now, tx, hwnd) = (self.player.clone(), self.now.clone(), self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| p.download_cover(id, &now))).unwrap_or_else(|_| Err("内部错误".into()));
            let _ = tx.send(AsyncMsg::CoverDone(r));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
            }
        });
    }

    /// 选择本地图片关联到当前歌曲
    pub fn pick_cover_file(&mut self) {
        let Some(id) = self.ui.snap.current else { return };
        let (tx, hwnd) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            let f = rfd::FileDialog::new()
                .set_parent(&crate::platform::window::Parent(hwnd))
                .set_title("关联图片文件")
                .add_filter("图片文件 (*.jpg;*.png;*.bmp;*.gif;*.webp)", &["jpg", "jpeg", "png", "bmp", "gif", "webp"])
                .pick_file();
            if let Some(f) = f {
                let _ = tx.send(AsyncMsg::CoverFile(id, f));
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
                }
            }
        });
    }

    /// 选择本地 .lrc 文件关联到当前歌曲
    pub fn pick_lyric_file(&mut self) {
        let Some(id) = self.ui.snap.current else { return };
        let (tx, hwnd) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            let f = rfd::FileDialog::new()
                .set_parent(&crate::platform::window::Parent(hwnd))
                .set_title("关联歌词文件")
                .add_filter("歌词文件 (*.lrc;*.txt)", &["lrc", "txt"])
                .pick_file();
            if let Some(f) = f {
                let _ = tx.send(AsyncMsg::LyricFile(id, f));
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
                }
            }
        });
    }
}
