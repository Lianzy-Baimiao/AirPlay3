//! 音乐列表面板里的「网络电台」页签（配置里 `radio.enabled` 打开后才有）：左边一栏是用户粘贴的
//! 电台清单，右边是清单里的电台。程序里没有任何电台地址，电台都是用户自己粘贴进来的。

use std::time::{Duration, Instant};

use super::app::{App, Pane};
use super::menu::{Act, Item, item};
use super::panels::playlist;
use crate::radio::Station;

/// 底部提示显示多久
const NOTE_TIME: Duration = Duration::from_secs(5);

impl App {
    pub fn radio_enabled(&self) -> bool {
        self.player.core.lock().settings.radio.enabled
    }

    /// 列表面板换到「音乐列表」/「网络电台」页签
    pub fn show_radio_tab(&mut self, radio: bool) {
        let radio = radio && self.radio_enabled();
        let l = &mut self.ui.list;
        if l.radio != radio {
            l.radio = radio;
            (l.scroll, l.hover, l.anchor, l.radio_sel, l.tab_scroll, l.reveal_tab) = (0, None, None, None, 0, radio);
            l.search.clear();
            l.ids_key = None;
            self.set_search_focus(false);
        }
        // 列表面板关着时打开它
        if radio && !self.ui.snap.show[2] {
            self.act(Act::TogglePane(2));
        }
        self.dirty(Pane::List);
    }

    /// 当前清单里（按搜索过滤后）显示的电台
    pub fn refresh_radio_rows(&mut self) {
        let g = self.player.core.lock();
        let lists = &g.settings.radio.lists;
        let li = self.ui.list.radio_list.min(lists.len().saturating_sub(1));
        let q = crate::search::Query::new(&self.ui.list.search);
        let rows: Vec<usize> = lists.get(li).map_or(Vec::new(), |l| {
            (0..l.stations.len()).filter(|&i| q.is_empty() || q.matches(&crate::search::SearchKey::new(&[&l.stations[i].name, l.stations[i].host()]))).collect()
        });
        drop(g);
        let l = &mut self.ui.list;
        l.radio_list = li;
        if l.radio_rows != rows {
            l.radio_rows = rows;
            l.radio_sel = l.radio_sel.filter(|s| l.radio_rows.contains(s));
        }
    }

    /// 底部的临时提示
    fn radio_note(&mut self, s: impl Into<String>) {
        self.ui.list.note = Some((s.into(), Instant::now()));
        self.dirty(Pane::List);
    }

    /// 「粘贴电台清单」：读剪贴板，解析后并进清单
    pub fn radio_paste(&mut self) {
        #[allow(unused_mut)]
        let mut text = super::lyricwin::clipboard_text().unwrap_or_default();
        // 仅调试构建：AIRPLAY3_TEST_PASTE=<文件> 时从文件读（自测不动用户的剪贴板）
        #[cfg(debug_assertions)]
        if let Some(f) = std::env::var_os("AIRPLAY3_TEST_PASTE") {
            text = std::fs::read_to_string(f).unwrap_or_default();
        }
        let found = crate::radio::parse_list(&text);
        if found.is_empty() {
            return self.radio_note("剪贴板里没有电台清单");
        }
        let n = found.len();
        let (added, first) = {
            let mut g = self.player.core.lock();
            let r = crate::radio::import(&mut g.settings.radio.lists, found);
            g.touch_settings();
            r
        };
        self.player.save(false);
        if let Some(i) = first {
            let l = &mut self.ui.list;
            (l.radio_list, l.scroll, l.radio_sel, l.reveal_tab) = (i, 0, None, true);
            l.search.clear();
        }
        self.show_radio_tab(true);
        match added {
            0 => self.radio_note(format!("清单里的 {n} 个电台都已经有了")),
            a => self.radio_note(format!("加了 {a} 个电台")),
        }
    }

    /// 播放当前清单里的第 i 个电台（上一个 / 下一个在这个清单里切换）
    pub fn play_radio(&mut self, i: usize) {
        let li = self.ui.list.radio_list;
        let list = self.player.core.lock().settings.radio.lists.get(li).map(|l| l.stations.clone()).unwrap_or_default();
        if let Some(st) = list.get(i).cloned() {
            self.ui.list.radio_sel = Some(i);
            self.player.play_station(st, list);
            self.dirty(Pane::List);
        }
    }

    /// 从当前清单里删掉第 i 个电台（清单空了就删掉清单）
    pub fn radio_remove(&mut self, i: usize) {
        let li = self.ui.list.radio_list;
        {
            let mut g = self.player.core.lock();
            let lists = &mut g.settings.radio.lists;
            if let Some(l) = lists.get_mut(li).filter(|l| i < l.stations.len()) {
                l.stations.remove(i);
                if l.stations.is_empty() {
                    lists.remove(li);
                }
            }
            g.touch_settings();
        }
        self.player.save(false);
        self.ui.list.radio_sel = None;
        self.dirty(Pane::List);
    }

    /// 删掉第 li 个清单
    pub fn radio_remove_list(&mut self, li: usize) {
        let name = {
            let mut g = self.player.core.lock();
            let lists = &mut g.settings.radio.lists;
            let name = (li < lists.len()).then(|| lists.remove(li).name);
            g.touch_settings();
            name
        };
        self.player.save(false);
        let l = &mut self.ui.list;
        (l.radio_list, l.scroll, l.radio_sel, l.reveal_tab) = (li.saturating_sub(1), 0, None, true);
        if let Some(name) = name {
            self.radio_note(format!("已删除清单「{name}」"));
        }
    }

    /// 电台页签的右键菜单
    pub fn radio_menu(&self) -> Vec<Item> {
        let mut v = Vec::new();
        if let Some(i) = self.ui.list.radio_sel {
            v.extend([item("播放", Act::RadioPlay(i)), item("复制地址", Act::RadioCopy(i)), item("删除这个电台", Act::RadioRemove(i)), Item::Sep]);
        }
        v.push(item("粘贴电台清单", Act::RadioPaste));
        let li = self.ui.list.radio_list;
        if let Some(l) = self.player.core.lock().settings.radio.lists.get(li) {
            v.push(item(&format!("删除清单「{}」", l.name), Act::RadioRemoveList(li)));
        }
        v
    }

    pub fn radio_copy(&mut self, i: usize) {
        let url = self.player.core.lock().settings.radio.lists.get(self.ui.list.radio_list).and_then(|l| l.stations.get(i)).map(|s| s.url.clone());
        if let Some(url) = url {
            super::lyricwin::set_clipboard_text(&url);
            self.radio_note("已复制电台地址");
        }
    }

    /// 电台页签里按下鼠标（行、左边的清单、工具栏）；返回 true = 处理了
    pub fn radio_list_down(&mut self, x: i32, y: i32, dbl: bool) -> bool {
        if playlist::PASTE.contains(x, y) {
            self.radio_paste();
            return true;
        }
        let lay = self.list_layout();
        if lay.tabs.contains(x, y) {
            let n = self.player.core.lock().settings.radio.lists.len();
            if let Some(i) = lay.tab_at(y, self.ui.list.tab_scroll, n).filter(|&i| i != self.ui.list.radio_list) {
                let l = &mut self.ui.list;
                (l.radio_list, l.scroll, l.radio_sel, l.hover, l.reveal_tab) = (i, 0, None, None, true);
                self.dirty(Pane::List);
            }
            return true;
        }
        if !lay.rows.contains(x, y) {
            return false;
        }
        let l = &self.ui.list;
        let row = lay.row_at(y, l.scroll);
        let Some(&i) = l.radio_rows.get(row) else { return true };
        if dbl || (x < lay.rows.x + 18 && l.hover == Some(row)) {
            self.play_radio(i);
        } else if x > lay.rows.right() - 20 && l.hover == Some(row) {
            self.radio_remove(i);
        } else {
            self.ui.list.radio_sel = Some(i);
            self.dirty(Pane::List);
        }
        true
    }

    /// 画电台页签
    pub fn paint_radio_list(&mut self, c: &mut crate::gui::canvas::Canvas) {
        self.refresh_radio_rows();
        // 左边一栏按最长的清单名字加宽；清单多时可以滚动，选中的清单（刚粘贴的、删除后选中的）滚到看得见
        let names: Vec<String> = self.player.core.lock().settings.radio.lists.iter().map(|x| x.name.clone()).collect();
        let tab_w = playlist::tab_width(names.iter().map(|s| s.as_str()), self.sizes.list.0);
        let lay = playlist::Layout::with_tabs(self.sizes.list, tab_w);
        let caret = self.caret_on();
        let l = &mut self.ui.list;
        l.radio_tab_w = tab_w;
        if std::mem::take(&mut l.reveal_tab) {
            l.tab_scroll = lay.reveal_tab(l.radio_list, l.tab_scroll, names.len());
        }
        l.tab_scroll = l.tab_scroll.clamp(0, lay.max_tab_scroll(names.len()));
        l.caret_drawn = caret;
        let l = &self.ui.list;
        let first = (l.scroll / playlist::ROW_H).max(0) as usize;
        let n_vis = (lay.rows.h / playlist::ROW_H + 2) as usize;
        let g = self.player.core.lock();
        let playing = g.radio.as_ref().map(|r| r.station.url.clone());
        let lists = &g.settings.radio.lists;
        let cur = lists.get(l.radio_list);
        let shown: Vec<(String, String, bool, bool)> = l.radio_rows[first.min(l.radio_rows.len())..(first + n_vis).min(l.radio_rows.len())]
            .iter()
            .filter_map(|&i| cur.and_then(|c| c.stations.get(i)).map(|s| (i, s)))
            .map(|(i, s): (usize, &Station)| (s.name.clone(), s.host().to_string(), playing.as_deref() == Some(s.url.as_str()), l.radio_sel == Some(i)))
            .collect();
        let tabs: Vec<(String, bool)> = lists.iter().enumerate().map(|(i, x)| (x.name.clone(), i == l.radio_list)).collect();
        let empty = lists.iter().all(|x| x.stations.is_empty());
        let footer = match &l.note {
            Some((s, t)) if t.elapsed() < NOTE_TIME => s.clone(),
            _ => cur.map_or(String::new(), |c| format!("{} · {} 个电台", c.name, c.stations.len())),
        };
        drop(g);
        let rows: Vec<playlist::Row> = shown.iter().map(|(n, h, p, s)| playlist::Row { title: n, artist: h, duration: "", playing: *p, selected: *s }).collect();
        let tab_refs: Vec<(&str, bool)> = tabs.iter().map(|(n, a)| (n.as_str(), *a)).collect();
        let m = playlist::PlaylistModel {
            tabs: &tab_refs,
            rows: &rows,
            first,
            scroll: l.scroll,
            total_rows: l.radio_rows.len(),
            hover_row: l.hover,
            hot_tool: None,
            footer: &footer,
            search: &l.search,
            search_focus: l.search_focus,
            strip_hover: self.ui.strip_hover[3],
            thumb_hot: l.thumb_hot,
            strip_tabs: Some(true),
            paste_hot: l.radio_paste_hot,
            radio_empty: empty,
            tab_w,
            tab_scroll: l.tab_scroll,
            search_hover: l.search_hover,
            caret_on: caret,
        };
        playlist::paint(c, &m);
        self.place_caret();
    }

    /// 每帧：听电台时把电台名、当前节目、连接状态写进「正在播放」；换台时加载台标；底部提示到时间后去掉
    pub fn sync_radio(&mut self) {
        if self.ui.list.note.as_ref().is_some_and(|(_, t)| t.elapsed() >= NOTE_TIME) {
            self.ui.list.note = None;
            self.dirty(Pane::List);
        }
        let radio = self.player.core.lock().radio.clone();
        let Some(r) = radio else { return };
        let st = crate::audio::stream::status();
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
        let album = [r.station.host().to_string(), st.info.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
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
