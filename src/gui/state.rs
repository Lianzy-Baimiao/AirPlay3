//! 界面状态与每帧更新（悬停淡入、标语切换、歌词/封面过渡、列表数据）。

use std::time::{Duration, Instant};

use super::app::{App, Pane};
use crate::config::PlayMode;
use crate::library::{ListId, ListKind, TrackId};
use crate::player::Player;

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Snap {
    pub token: u64,
    pub playing: bool,
    pub pos_sec: u64,
    pub pos: f64,
    pub dur: f64,
    pub volume: f32,
    pub muted: bool,
    pub mode: PlayMode,
    pub current: Option<TrackId>,
    pub lib_rev: u64,
    pub topmost: bool,
    pub show: [bool; 3],
    pub eq_enabled: bool,
    pub eq_preamp: f32,
    pub eq_bands: [f32; 10],
    pub eq_preset: String,
    pub desk_visible: bool,
    pub desk_locked: bool,
    pub taskbar_visible: bool,
}

#[derive(Default)]
pub struct ListUi {
    pub list: Option<ListId>,
    pub ids: Vec<TrackId>,
    pub ids_key: Option<(u64, ListId, String)>,
    pub search: String,
    pub search_focus: bool,
    pub scroll: i32,
    pub hover: Option<usize>,
    pub hot_tool: Option<usize>,
    pub selected: std::collections::HashSet<TrackId>,
    pub anchor: Option<usize>,
    pub thumb_hot: bool,
}

pub struct UiState {
    pub snap: Snap,
    pub hover: [bool; 4],
    pub strip_hover: [f32; 4],
    pub bar_hover: f32,
    pub bar_hot: super::panels::bar::Hot,
    pub slogan: f32,
    pub cover_track: Option<TrackId>,
    pub now_rev: u64,
    pub track_started: Instant,
    pub lyric_mode: f32,
    pub info: f32,
    pub lyric_idx: Option<usize>,
    pub list: ListUi,
    pub eq_hot: Option<usize>,
    pub drag: Drag,
    pub last_frame: Instant,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Drag {
    None,
    /// 拖动整组窗口：按下时鼠标相对播放条左上角的偏移
    Move(i32, i32),
    Seek,
    Volume,
    EqSlider(usize),
    /// 拖动滚动条滑块：按下时的 y 与滚动量
    Scroll(i32, i32),
    /// 调整面板大小：面板、边、按下时鼠标相对窗口宽高的偏移
    Resize(Pane, super::resize::Edge, i32, i32),
}

impl UiState {
    pub fn new(p: &Player) -> UiState {
        let list = p.core.lock().lib.special_id(ListKind::Audition);
        UiState {
            snap: Snap::default(),
            hover: [false; 4],
            strip_hover: [0.0; 4],
            bar_hover: 0.0,
            bar_hot: super::panels::bar::Hot::None,
            slogan: 0.0,
            cover_track: None,
            now_rev: u64::MAX,
            track_started: Instant::now(),
            lyric_mode: 0.0,
            info: 1.0,
            lyric_idx: None,
            list: ListUi { list: Some(list), ..Default::default() },
            eq_hot: None,
            drag: Drag::None,
            last_frame: Instant::now(),
        }
    }
}

/// 启动位置：上次保存的位置（在屏幕内时），否则居中
pub fn initial_origin(p: &Player) -> (i32, i32) {
    let saved = p.core.lock().settings.window.main;
    if let Some(r) = saved {
        if crate::platform::window::point_on_screen(r.x + 20.0, r.y + 20.0) {
            return (r.x as i32, r.y as i32);
        }
    }
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    ((sw - 625) / 2, (sh - 431) / 2)
}

pub fn approach(v: &mut f32, target: f32, step: f32) -> bool {
    if (*v - target).abs() < 1e-3 {
        return false;
    }
    *v = if *v < target { (*v + step).min(target) } else { (*v - step).max(target) };
    true
}
impl App {
    fn take_snap(&self) -> Snap {
        let (token, pos, dur) = self.player.engine.position();
        let playing = self.player.engine.state() == crate::audio::PlayState::Playing;
        let g = self.player.core.lock();
        let current = self.player.current(&g);
        let s = &g.settings;
        Snap {
            token,
            playing,
            pos_sec: pos as u64,
            pos,
            dur: if dur > 0.0 { dur } else { current.and_then(|id| g.lib.track(id)).map_or(0.0, |t| t.duration) },
            volume: s.playback.volume,
            muted: s.playback.muted,
            mode: s.playback.mode,
            current,
            lib_rev: g.lib.rev,
            topmost: s.general.topmost,
            show: [s.window.show_cover, s.window.show_eq, s.window.show_list],
            eq_enabled: s.eq.enabled,
            eq_preamp: s.eq.preamp_db,
            eq_bands: s.eq.bands,
            eq_preset: s.eq.preset.clone(),
            desk_visible: s.desk_lyric.visible,
            desk_locked: s.desk_lyric.locked,
            taskbar_visible: s.taskbar_lyric.visible,
        }
    }

    /// 每 30 毫秒：同步播放状态、推进动画，标记需要重绘的窗口
    pub fn tick(&mut self) {
        self.poll_menu();
        let now = Instant::now();
        let snap = self.take_snap();
        let old = std::mem::replace(&mut self.ui.snap, snap.clone());
        if old.pos_sec != snap.pos_sec || old.playing != snap.playing || old.volume != snap.volume || old.muted != snap.muted
            || old.mode != snap.mode || old.topmost != snap.topmost || old.show != snap.show || old.dur != snap.dur
        {
            self.dirty(Pane::Bar);
        }
        if old.current != snap.current || old.lib_rev != snap.lib_rev {
            self.dirty(Pane::List);
        }
        if old.eq_enabled != snap.eq_enabled || old.eq_preamp != snap.eq_preamp || old.eq_bands != snap.eq_bands || old.eq_preset != snap.eq_preset {
            self.dirty(Pane::Eq);
        }
        // 热键 / 设置窗口修改了桌面歌词、任务栏歌词开关时，同步窗口
        if old.desk_visible != snap.desk_visible || old.desk_locked != snap.desk_locked {
            self.sync_desk_lyric();
        }
        if old.taskbar_visible != snap.taskbar_visible {
            self.sync_taskbar_lyric();
        }
        if old.show != snap.show {
            for (i, p) in [Pane::Cover, Pane::Eq, Pane::List].into_iter().enumerate() {
                if self.win(p).visible != snap.show[i] {
                    self.set_visible(p, snap.show[i]);
                }
            }
        }
        let reveal = self.player.core.lock().reveal.take();
        if let Some(id) = reveal {
            self.ui.list.list = Some(id);
            self.ui.list.search.clear();
            self.ui.list.scroll = 0;
            self.dirty(Pane::List);
        }
        let ui = &mut self.ui;
        let mut bar = approach(&mut ui.bar_hover, if ui.hover[0] { 1.0 } else { 0.0 }, 0.16);
        // 标语每 6 秒上下切换一次（0.5 秒滑动）
        let t = (now - self.started).as_secs_f32() % 12.0;
        let target = if t < 5.5 { 0.0 } else if t < 6.0 { (t - 5.5) * 2.0 } else if t < 11.5 { 1.0 } else { 1.0 - (t - 11.5) * 2.0 };
        if (ui.slogan - target).abs() > 1e-3 {
            ui.slogan = target;
            bar = true;
        }
        let mut strips = [false; 4];
        for i in 0..4 {
            strips[i] = approach(&mut ui.strip_hover[i], if ui.hover[i] { 1.0 } else { 0.0 }, 0.2);
        }
        let cover = self.tick_cover(now);
        if bar || strips[0] {
            self.dirty(Pane::Bar);
        }
        if cover || strips[1] {
            self.dirty(Pane::Cover);
        }
        if strips[2] {
            self.dirty(Pane::Eq);
        }
        if strips[3] {
            self.dirty(Pane::List);
        }
        self.tick_desk();
        self.tick_taskbar();
        self.tick_minibar();
        self.ui.last_frame = now;
    }

    /// 封面/歌词：新歌开始时显示大封面 + 信息，约 8 秒后有歌词则切换为小封面 + 歌词
    fn tick_cover(&mut self, now: Instant) -> bool {
        let (rev, has_lyrics, idx) = {
            let n = self.now.lock();
            let has = n.lyrics.as_ref().is_some_and(|l| l.synced && !l.lines.is_empty());
            (n.rev, has, if has { n.lyric_index(self.ui.snap.pos) } else { None })
        };
        let ui = &mut self.ui;
        let mut dirty = false;
        if ui.cover_track != ui.snap.current {
            ui.cover_track = ui.snap.current;
            ui.track_started = now;
            ui.info = 1.0;
            ui.lyric_mode = 0.0;
            dirty = true;
        }
        if ui.now_rev != rev {
            ui.now_rev = rev;
            dirty = true;
        }
        let age = now - ui.track_started;
        let lyric_target = if has_lyrics && age > Duration::from_secs(8) { 1.0 } else { 0.0 };
        let info_target = if age > Duration::from_secs(8) { 0.0 } else { 1.0 };
        dirty |= approach(&mut ui.lyric_mode, lyric_target, 0.06);
        dirty |= approach(&mut ui.info, info_target, 0.08);
        if ui.lyric_idx != idx {
            ui.lyric_idx = idx;
            dirty = true;
        }
        dirty
    }

    /// 当前列表（含搜索过滤）的歌曲
    pub fn refresh_list(&mut self) {
        let g = self.player.core.lock();
        let list = self.ui.list.list.unwrap_or_else(|| g.lib.special_id(ListKind::Audition));
        let key = (g.lib.rev, list, self.ui.list.search.clone());
        if self.ui.list.ids_key.as_ref() == Some(&key) {
            return;
        }
        let q = crate::search::Query::new(&self.ui.list.search);
        let ids: Vec<TrackId> = g.lib.list(list).map_or(Vec::new(), |l| {
            l.tracks
                .iter()
                .copied()
                .filter(|id| {
                    q.is_empty()
                        || g.lib.track(*id).is_some_and(|t| q.matches(&crate::search::SearchKey::new(&[&t.display_title(), t.display_artist(), t.display_album()])))
                })
                .collect()
        });
        drop(g);
        self.ui.list.ids = ids;
        self.ui.list.ids_key = Some(key);
        self.ui.list.list = Some(list);
        self.dirty(Pane::List);
    }
}
