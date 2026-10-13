//! 「下载歌词」窗口（原版的样子，画法见 `panels::lyricdl`）：按标题 / 艺人 / 专辑搜索，
//! 列出结果、预览所选的歌词，按勾选下载歌词、专辑图片、艺人图片，可以补全歌曲信息。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::actions::AsyncMsg;
use super::app::{App, WM_APP_ASYNC};
use super::lyricwin::clipboard_text;
use super::panels::lyricdl::{self as sp, LyricDlModel, Pic, ResultRow};
use super::popup::Popup;
use crate::library::TrackId;
use crate::lyrics::online::{self, Candidate};
use crate::player::nowplaying::Link;

pub struct LyricSearch {
    pub win: Popup,
    pub track: TrackId,
    /// 标题、艺人、专辑
    pub fields: [String; 3],
    pub focus: Option<usize>,
    pub results: Vec<Candidate>,
    pub selected: Option<usize>,
    pub hover: Option<usize>,
    pub hot_button: Option<usize>,
    pub hot_check: Option<usize>,
    pub scroll: i32,
    /// 列表空着时的提示（正在搜索、没找到、出错）
    pub list_note: String,
    /// 预览区的文字：所选歌词的前几行，或下载的进度 / 出的错
    pub preview: Vec<String>,
    /// 下载歌词、专辑图片、艺人图片、下载后自动修改信息
    pub checks: [bool; 4],
    /// 现在的专辑图片、艺人图片（RGBA, 宽, 高）
    pub pics: [Option<(Vec<u8>, u32, u32)>; 2],
    pub busy: bool,
    pub seq: u64,
    pub drag: Option<(i32, i32)>,
    /// 当前歌曲时长（秒），用于把时长接近的结果排在前面
    pub duration: f64,
}

impl App {
    /// 打开「下载歌词」窗口（预填清理过的标题、艺人和专辑，可再修改）并立即搜索
    pub fn open_lyric_search(&mut self) {
        let Some(id) = self.ui.snap.current else { return };
        let (track, ls, artist_dir) = {
            let g = self.player.core.lock();
            (g.lib.track(id).cloned(), g.settings.lyrics.clone(), g.dirs.artist_dir())
        };
        let Some(t) = track else { return };
        let fields = [crate::lyrics::query::clean_title(&t.display_title()), crate::lyrics::query::clean_artist(&t.artist), t.album.trim().to_string()];
        if let Some(mut old) = self.search.take() {
            old.win.close();
        }
        // 两个图片框：现在的图片、已下载的艺人图片
        let cover = self.now.lock().cover.as_ref().map(|c| (c.rgba.to_vec(), c.size[0] as u32, c.size[1] as u32));
        let artist = crate::media::artist::cached(&artist_dir, &t.artist).flatten().and_then(|b| crate::media::cover::decode_rgba(&b, 160));
        let (x, y) = Popup::centered(sp::W, sp::H);
        let win = Popup::new(x, y, sp::W, sp::H, Some(self.bar_hwnd()), "下载歌词");
        // 窗口不透明，四周是宽阴影：不雾化（否则阴影里是一大片模糊）
        super::window::set_frost(win.hwnd, false);
        self.search = Some(LyricSearch {
            win,
            track: id,
            fields,
            focus: Some(0),
            results: Vec::new(),
            selected: None,
            hover: None,
            hot_button: None,
            hot_check: None,
            scroll: 0,
            list_note: String::new(),
            preview: Vec::new(),
            checks: [ls.download_lyric, ls.album_pic, ls.artist_pic, ls.auto_tag],
            pics: [cover, artist.map(|(w, h, px)| (px, w, h))],
            busy: false,
            seq: 0,
            drag: None,
            duration: t.duration,
        });
        self.paint_search();
        if let Some(s) = &self.search {
            s.win.show(true);
        }
        self.run_search();
    }

    pub fn run_search(&mut self) {
        let (net, source) = {
            let g = self.player.core.lock();
            (g.settings.network.clone(), g.settings.lyrics.source)
        };
        let Some(s) = self.search.as_mut() else { return };
        s.seq += 1;
        s.busy = true;
        (s.results, s.selected, s.scroll) = (Vec::new(), None, 0);
        s.list_note = "正在搜索…".into();
        s.preview.clear();
        s.win.dirty = true;
        let (seq, title, artist) = (s.seq, s.fields[0].clone(), s.fields[1].clone());
        let (tx, hwnd) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            // 后台线程出错也要让界面得到结果，不能一直显示“正在搜索”
            let (title, artist) = (title.trim().to_string(), artist.trim().to_string());
            let targets = source.targets();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let v = online::search(&net, &targets, &title, &artist)?;
                if !v.is_empty() || artist.is_empty() {
                    return Ok((v, false));
                }
                Ok((online::search(&net, &targets, &title, "")?, true))
            }))
            .unwrap_or_else(|_| Err("内部错误".into()));
            let _ = tx.send(AsyncMsg::LyricResults(seq, r));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
            }
        });
    }

    pub fn on_lyric_results(&mut self, seq: u64, r: Result<(Vec<Candidate>, bool), String>) {
        let Some(s) = self.search.as_mut() else { return };
        if s.seq != seq {
            return;
        }
        s.busy = false;
        match r {
            Ok((mut v, loose)) => {
                // 标题、艺人对得上的在前，再按时长接近程度、专辑、来源顺序（LrcAPI → LRCLIB）
                online::sort_for_list(&mut v, &s.fields[0], &s.fields[1], &s.fields[2], s.duration);
                s.list_note = if v.is_empty() { "没有找到，试试修改标题或艺人".into() } else { String::new() };
                s.results = v;
                s.scroll = 0;
                let first = (!s.results.is_empty()).then_some(0);
                self.select_result(first);
                if loose {
                    if let Some(s) = self.search.as_mut() {
                        s.preview.insert(0, "（按标题找到的，艺人可能不同）".into());
                    }
                }
            }
            Err(e) => s.list_note = format!("搜索失败：{e}"),
        }
        if let Some(s) = self.search.as_mut() {
            s.win.dirty = true;
        }
    }

    /// 选中第 i 个结果：预览它的歌词
    fn select_result(&mut self, i: Option<usize>) {
        let Some(s) = self.search.as_mut() else { return };
        s.selected = i;
        s.preview = match i.and_then(|i| s.results.get(i)) {
            Some(c) => match &c.lyrics {
                Some(text) => sp::preview_lines(text, sp::PREVIEW_LINES),
                None => vec!["（歌词在下载时取得）".into()],
            },
            None => Vec::new(),
        };
        s.win.dirty = true;
    }

    /// 所选结果能补全这首歌的信息（艺人、专辑空着，歌名、时长对得上）
    fn info_found(&self) -> bool {
        let Some(s) = self.search.as_ref() else { return false };
        let Some(c) = s.selected.and_then(|i| s.results.get(i)) else { return false };
        let g = self.player.core.lock();
        g.lib.track(s.track).is_some_and(|t| crate::player::nowplaying::fill_info(t, c).is_some())
    }

    /// 「补全歌曲信息」：用所选结果补全空着的艺人、专辑
    fn fill_info_now(&mut self) {
        let Some(s) = self.search.as_ref() else { return };
        let Some(c) = s.selected.and_then(|i| s.results.get(i)).cloned() else { return };
        let id = s.track;
        let filled = self.player.fill_track_info(id, &c, &self.now);
        if let Some(s) = self.search.as_mut() {
            s.preview = vec![if filled.is_some() { "已补全歌曲信息".into() } else { "没有可补全的信息".into() }];
            s.win.dirty = true;
        }
        self.player.save(false);
    }

    /// 「下载」：按勾选下载所选结果的歌词、专辑图片、艺人图片，并补全歌曲信息（后台联网）
    pub fn download_selected(&mut self) {
        let info = self.info_found();
        let Some(s) = self.search.as_mut() else { return };
        let Some(cand) = s.selected.and_then(|i| s.results.get(i)).cloned() else { return };
        let [lyric, album, artist, tag] = s.checks;
        if s.busy || !(lyric || album || artist) {
            return;
        }
        s.busy = true;
        s.preview = vec![format!("正在从{}下载…", cand.provider.label())];
        s.win.dirty = true;
        let id = s.track;
        let (net, dir, title, artist_name) = {
            let g = self.player.core.lock();
            let t = g.lib.track(id);
            (g.settings.network.clone(), crate::player::nowplaying::save_lyric_dir(&g), t.map(|t| t.display_title()).unwrap_or_default(), t.map(|t| t.artist.clone()).unwrap_or_default())
        };
        let (tx, hwnd) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        let (p, now) = (self.player.clone(), self.now.clone());
        std::thread::spawn(move || {
            let (path, errors) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut errors = Vec::new();
                let mut path = None;
                if lyric {
                    match online::fetch(&net, &cand).and_then(|text| online::save(&text, &cand, &dir, &title, &artist_name).map_err(|e| format!("保存失败：{e}"))) {
                        Ok(p) => path = Some(p),
                        Err(e) => errors.push(format!("歌词：{e}")),
                    }
                }
                errors.extend(p.download_pics_for(id, &cand, album, artist, &now));
                // 「下载后自动修改信息」：补全空着的艺人、专辑
                if tag && info {
                    p.fill_track_info(id, &cand, &now);
                }
                (path, errors)
            }))
            .unwrap_or_else(|_| (None, vec!["内部错误".into()]));
            let _ = tx.send(AsyncMsg::LyricSaved(id, path, errors));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
            }
        });
    }

    /// 下载完成：关联歌词；都成功时关闭窗口，有出错的在预览区列出来
    pub fn on_lyric_saved(&mut self, id: TrackId, path: Option<std::path::PathBuf>, errors: Vec<String>) {
        if let Some(p) = path {
            self.player.set_lyric_link(id, Link::File(p), &self.now);
        }
        self.dirty(super::app::Pane::Cover);
        self.player.save(false);
        if errors.is_empty() {
            self.close_search();
        } else if let Some(s) = self.search.as_mut() {
            s.busy = false;
            s.preview = errors;
            s.win.dirty = true;
        }
    }

    pub fn close_search(&mut self) {
        if let Some(mut s) = self.search.take() {
            s.win.close();
        }
    }

    pub fn paint_search(&mut self) {
        let info = self.info_found();
        let Some(s) = self.search.as_mut() else { return };
        let durs: Vec<String> = s.results.iter().map(|r| super::panels::bar::fmt_time(r.duration)).collect();
        let rows: Vec<ResultRow> = s.results.iter().zip(&durs).map(|(r, d)| ResultRow { title: &r.title, artist: &r.artist, duration: d, source: r.provider }).collect();
        fn pic(p: &Option<(Vec<u8>, u32, u32)>) -> Option<Pic<'_>> {
            p.as_ref().map(|(px, w, h)| Pic { rgba: px, w: *w, h: *h })
        }
        let m = LyricDlModel {
            fields: [&s.fields[0], &s.fields[1], &s.fields[2]],
            focus: s.focus,
            rows: &rows,
            selected: s.selected,
            hover: s.hover,
            scroll: s.scroll,
            list_note: &s.list_note,
            preview: &s.preview,
            pics: [pic(&s.pics[0]), pic(&s.pics[1])],
            checks: s.checks,
            info_found: info,
            can_download: s.selected.is_some() && !s.busy && s.checks[..3].iter().any(|&c| c),
            hot_button: s.hot_button,
            hot_check: s.hot_check,
        };
        sp::paint(&mut s.win.canvas, &m);
        #[cfg(debug_assertions)]
        s.win.canvas.dump("lyricdl");
        s.win.present();
    }

    /// 「下载歌词」窗口的消息；不是该窗口时返回 None
    pub fn search_msg(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if self.search.as_ref()?.win.hwnd != hwnd {
            return None;
        }
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        let info = self.info_found();
        let s = self.search.as_mut()?;
        let row_at = |s: &LyricSearch, y: i32| {
            let i = ((y - sp::LIST.y + s.scroll) / sp::ROW_H) as usize;
            (sp::LIST.contains(sp::LIST.x + 1, y) && i < s.results.len()).then_some(i)
        };
        let buttons = [sp::BTN_SEARCH, sp::BTN_INFO, sp::BTN_DOWNLOAD, sp::BTN_CLOSE];
        match msg {
            WM_MOUSEMOVE => {
                if let Some((dx, dy)) = s.drag {
                    let (cx, cy) = super::resize::cursor();
                    s.win.move_to(cx - dx, cy - dy);
                    return Some(LRESULT(0));
                }
                let hover = if sp::LIST.contains(x, y) { row_at(s, y) } else { None };
                let hot = buttons.iter().position(|r| r.contains(x, y));
                let check = sp::CHECKS.iter().position(|r| r.contains(x, y));
                if (hover, hot, check) != (s.hover, s.hot_button, s.hot_check) {
                    (s.hover, s.hot_button, s.hot_check) = (hover, hot, check);
                    s.win.dirty = true;
                }
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let dbl = msg == WM_LBUTTONDBLCLK;
                if let Some(f) = sp::FIELDS.iter().position(|r| r.contains(x, y)) {
                    s.focus = Some(f);
                } else if let Some(b) = buttons.iter().position(|r| r.contains(x, y)) {
                    match b {
                        0 => self.run_search(),
                        1 if info => self.fill_info_now(),
                        2 => self.download_selected(),
                        3 => self.close_search(),
                        _ => {}
                    }
                } else if let Some(k) = sp::CHECKS.iter().position(|r| r.contains(x, y)) {
                    // 第 4 项只在有可补全的信息时能改
                    if k < 3 || info {
                        s.checks[k] = !s.checks[k];
                    }
                } else if sp::LIST.contains(x, y) {
                    if let Some(i) = row_at(s, y) {
                        self.select_result(Some(i));
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
                    9 => s.focus = Some((f + 1) % 3),
                    13 => self.run_search(),
                    27 => self.close_search(),
                    22 => {
                        if let Some(t) = clipboard_text() {
                            s.fields[f].push_str(t.lines().next().unwrap_or("").trim());
                        }
                    }
                    c if c >= 32 => s.fields[f].extend(char::from_u32(c)),
                    _ => {}
                }
                if let Some(s) = self.search.as_mut() {
                    s.win.dirty = true;
                }
            }
            // 没有输入框在输入时按 Esc 也关闭
            WM_KEYDOWN if wp.0 == 0x1B && s.focus.is_none() => self.close_search(),
            WM_CLOSE => self.close_search(),
            _ => return None,
        }
        if self.search.as_ref().is_some_and(|s| s.win.dirty) {
            self.paint_search();
        }
        Some(LRESULT(0))
    }
}