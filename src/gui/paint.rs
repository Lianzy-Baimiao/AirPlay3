//! 由界面状态构造各面板的绘制模型。

use super::app::{App, Pane};
use super::canvas::Canvas;
use super::panels::{bar, cover, eq, playlist};
use crate::library::ListKind;

pub fn fmt_total(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

impl App {
    pub fn paint(&mut self, pane: Pane, c: &mut Canvas) {
        let (title, artist, album, cover_img, lines) = {
            let n = self.now.lock();
            let lines: Vec<String> = n.lyrics.as_ref().map_or(Vec::new(), |l| l.lines.iter().map(|x| x.text.clone()).collect());
            (n.title.clone(), n.artist.clone(), n.album.clone(), n.cover.as_ref().map(|c| (c.rgba.clone(), c.size)), lines)
        };
        let cov = cover_img.as_ref().map(|(px, s)| (px.as_slice(), s[0] as u32, s[1] as u32));
        let ui = &self.ui;
        let s = &ui.snap;
        match pane {
            Pane::Bar => {
                let m = bar::BarModel {
                    cover: cov,
                    title: &title,
                    artist: &artist,
                    playing: s.playing,
                    pos: s.pos,
                    dur: s.dur,
                    volume: s.volume,
                    muted: s.muted,
                    mode: s.mode,
                    topmost: s.topmost,
                    show_lyric: s.show[0],
                    show_list: s.show[2],
                    show_eq: s.show[1],
                    hover: ui.bar_hover,
                    hot: ui.bar_hot,
                    slogan: ui.slogan,
                };
                bar::paint(c, &m);
            }
            Pane::Cover => {
                let m = cover::CoverModel {
                    cover: cov,
                    title: &title,
                    artist: &artist,
                    album: &album,
                    active: s.current.is_some() || s.radio.is_some(),
                    lines: &lines,
                    current: ui.lyric_idx,
                    lyric_mode: ui.lyric_mode,
                    info: ui.info,
                    strip_hover: ui.strip_hover[1],
                    status: ui.cover_status.as_ref().map(|(s, _)| s.as_str()),
                };
                cover::paint(c, &m);
            }
            Pane::Eq => {
                let m = eq::EqModel {
                    enabled: s.eq_enabled,
                    preamp: s.eq_preamp,
                    bands: s.eq_bands,
                    preset: &s.eq_preset,
                    hot_button: ui.eq_hot,
                    strip_hover: ui.strip_hover[2],
                };
                eq::paint(c, &m);
            }
            Pane::List => self.paint_list(c),
        }
    }

    fn paint_list(&mut self, c: &mut Canvas) {
        if self.ui.list.radio && self.radio_enabled() {
            return self.paint_radio_list(c);
        }
        self.ui.list.radio = false;
        let radio_tabs = self.radio_enabled().then_some(false);
        self.refresh_list();
        let caret = self.caret_on();
        self.ui.list.caret_drawn = caret;
        self.ui.list.tab_scroll = 0;
        let ui = &self.ui;
        let l = &ui.list;
        let first = (l.scroll / playlist::ROW_H).max(0) as usize;
        let n_vis = (playlist::Layout::of(self.sizes.list).rows.h / playlist::ROW_H + 2) as usize;
        let g = self.player.core.lock();
        let mut total = 0.0;
        for id in &l.ids {
            total += g.lib.track(*id).map_or(0.0, |t| t.duration);
        }
        let data: Vec<(String, String, String, bool, bool)> = l.ids[first.min(l.ids.len())..(first + n_vis).min(l.ids.len())]
            .iter()
            .map(|id| {
                let t = g.lib.track(*id);
                (
                    t.map_or(String::new(), |t| t.display_title()),
                    t.map_or(String::new(), |t| t.display_artist().to_string()),
                    t.map_or(String::new(), |t| bar::fmt_time(t.duration)),
                    ui.snap.current == Some(*id),
                    l.selected.contains(id),
                )
            })
            .collect();
        let tabs: Vec<(String, bool)> = [ListKind::Audition, ListKind::Favorite]
            .iter()
            .filter_map(|k| g.lib.special(*k))
            .filter(|p| p.kind == ListKind::Audition || !p.tracks.is_empty())
            .map(|p| (if p.kind == ListKind::Audition { "默认".to_string() } else { p.name.clone() }, Some(p.id) == l.list))
            .collect();
        drop(g);
        let rows: Vec<playlist::Row> =
            data.iter().map(|(t, a, d, p, s)| playlist::Row { title: t, artist: a, duration: d, playing: *p, selected: *s }).collect();
        let tab_refs: Vec<(&str, bool)> = tabs.iter().map(|(n, a)| (n.as_str(), *a)).collect();
        let footer = format!("{} / {}", l.ids.len(), fmt_total(total));
        let m = playlist::PlaylistModel {
            tabs: &tab_refs,
            rows: &rows,
            first,
            scroll: l.scroll,
            total_rows: l.ids.len(),
            hover_row: l.hover,
            hot_tool: l.hot_tool,
            footer: &footer,
            search: &l.search,
            search_focus: l.search_focus,
            strip_hover: ui.strip_hover[3],
            thumb_hot: l.thumb_hot,
            strip_tabs: radio_tabs,
            paste_hot: false,
            radio_empty: false,
            tab_w: playlist::TAB_W,
            tab_scroll: 0,
            search_hover: l.search_hover,
            caret_on: caret,
        };
        playlist::paint(c, &m);
        self.place_caret();
    }
}
