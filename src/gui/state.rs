//! 界面状态与每帧更新（悬停淡入、标语切换、歌词/封面过渡、列表数据）。

use std::time::{Duration, Instant};

use super::app::{App, Pane};
use super::canvas::R;
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
    /// 正在听的电台（播放地址）
    pub radio: Option<String>,
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
    /// 列表面板显示的是「网络电台」（标题条上的页签；配置里打开电台后才有）
    pub radio: bool,
    /// 电台：选中的清单（左边一栏）、当前清单里（按搜索过滤后）显示的电台序号、选中的电台
    pub radio_list: usize,
    pub radio_rows: Vec<usize>,
    pub radio_sel: Option<usize>,
    /// 电台页签的工具栏「粘贴电台清单」在鼠标下
    pub radio_paste_hot: bool,
    /// 底部临时显示的提示（粘贴的结果），到时间后换回计数
    pub note: Option<(String, Instant)>,
    /// 左边一栏的滚动量（电台清单多时）；电台页签左边一栏的宽度（按最长的清单名，画的时候更新）
    pub tab_scroll: i32,
    pub radio_tab_w: i32,
    /// 下次画的时候把选中的清单滚到看得见（粘贴、删除清单后）
    pub reveal_tab: bool,
    /// 鼠标在搜索框上；光标闪烁从什么时候算起（获得输入、打字时重新算，让光标马上亮）
    pub search_hover: bool,
    pub caret_t0: Option<Instant>,
    /// 上次画的时候光标是亮的
    pub caret_drawn: bool,
}

impl ListUi {
    /// 现在显示的行数（歌曲或电台）
    pub fn rows_len(&self) -> usize {
        if self.radio { self.radio_rows.len() } else { self.ids.len() }
    }
}

pub struct UiState {
    pub snap: Snap,
    pub hover: [bool; 4],
    pub strip_hover: [f32; 4],
    pub bar_hover: f32,
    pub bar_hot: super::panels::bar::Hot,
    pub slogan: f32,
    pub cover_track: Option<TrackId>,
    pub cover_radio: Option<String>,
    pub now_rev: u64,
    pub track_started: Instant,
    pub lyric_mode: f32,
    pub info: f32,
    pub lyric_idx: Option<usize>,
    pub list: ListUi,
    pub eq_hot: Option<usize>,
    pub drag: Drag,
    pub last_frame: Instant,
    /// 歌词图片面板顶部临时显示的提示（下载专辑图片的结果），显示几秒后恢复原来的说明文字
    pub cover_status: Option<(String, Instant)>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Drag {
    None,
    /// 移动：按住的面板、按下时的鼠标位置、各面板按下时的外框与是否一起移动（按 `resize::SNAP_ORDER`）
    Move { lead: Pane, cursor: (i32, i32), rects: [R; 4], moving: [bool; 4], taskbar: Option<R> },
    Seek,
    Volume,
    EqSlider(usize),
    /// 拖动滚动条滑块：按下时的 y 与滚动量
    Scroll(i32, i32),
    /// 调整面板大小：面板、拖动的边（`snap::LEFT` 等）、按下时的鼠标位置与各面板外框
    Resize { pane: Pane, edges: u32, cursor: (i32, i32), rects: [R; 4], taskbar: Option<R> },
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
            cover_radio: None,
            now_rev: u64::MAX,
            track_started: Instant::now(),
            lyric_mode: 0.0,
            info: 1.0,
            lyric_idx: None,
            list: ListUi { list: Some(list), ..Default::default() },
            eq_hot: None,
            drag: Drag::None,
            last_frame: Instant::now(),
            cover_status: None,
        }
    }
}

/// 上次保存的播放条位置（实际像素；还在某个显示器上时）
pub fn saved_origin_px(p: &Player) -> Option<(i32, i32)> {
    let r = p.core.lock().settings.window.main?;
    // 播放条左上部能被拖到即可（吸附到屏幕边时外框会在 -1）
    let (x, y) = (r.x as i32, r.y as i32);
    crate::platform::window::point_on_screen(x + 40, y + 20).then_some((x, y))
}

/// 启动位置（逻辑像素）：上次保存的位置（在屏幕内时），否则居中
pub fn initial_origin(p: &Player) -> (i32, i32) {
    match saved_origin_px(p) {
        Some((x, y)) => (super::scale::to_logical(x), super::scale::to_logical(y)),
        None => default_origin(),
    }
}

/// 默认位置：主显示器中央（按默认布局 625×431 居中；逻辑像素）
pub fn default_origin() -> (i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    ((super::scale::to_logical(sw) - 625) / 2, (super::scale::to_logical(sh) - 431) / 2)
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
            radio: g.radio.as_ref().map(|r| r.station.url.clone()),
            lib_rev: g.lib.rev,
            topmost: s.general.always_on_top,
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
        let mut cover = self.tick_cover(now);
        // 下载提示显示 4 秒（"正在下载"一直显示到有结果）
        if self.ui.cover_status.as_ref().is_some_and(|(s, t)| !s.starts_with("正在") && now - *t > Duration::from_secs(4)) {
            self.ui.cover_status = None;
            cover = true;
        }
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
        // 搜索框的光标闪烁：亮 / 灭变了才重画
        if self.ui.list.search_focus && self.caret_on() != self.ui.list.caret_drawn {
            self.dirty(Pane::List);
        }
        self.sync_radio();
        self.tick_desk();
        self.tick_taskbar();
        self.tick_minibar();
        self.tick_anim();
        self.tick_smtc();
        self.ui.last_frame = now;
    }

    /// 系统媒体控制：按设置创建 / 关闭，同步播放状态与歌曲信息
    fn tick_smtc(&mut self) {
        let want = self.player.core.lock().settings.hotkeys.media_keys;
        if want != self.smtc.is_some() {
            self.smtc = if want {
                let (cmds, bar) = (self.cmds.clone(), self.bar_hwnd());
                let on = move |b: crate::platform::smtc::Button| {
                    use crate::controller::Command;
                    use crate::platform::smtc::Button;
                    let c = match b {
                        Button::PlayPause => Command::PlayPause,
                        Button::Play => Command::Play,
                        Button::Pause => Command::Pause,
                        Button::Next => Command::Next,
                        Button::Prev => Command::Prev,
                        Button::Stop => Command::Stop,
                    };
                    let _ = cmds.send(c);
                };
                match crate::platform::smtc::Smtc::new(bar, on) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        log::warn!("系统媒体控制不可用: {e}");
                        // 不再重试：关掉设置里的开关，免得每帧都尝试
                        self.player.core.lock().settings.hotkeys.media_keys = false;
                        None
                    }
                }
            } else {
                None
            };
        }
        // 多媒体键盘的六个开关：前三个经系统媒体控制，静音 / 音量键用键盘钩子
        let (on, m) = {
            let hk = &self.player.core.lock().settings.hotkeys;
            (hk.media_keys, hk.media)
        };
        use crate::platform::mediakeys::{MUTE, VOL_DOWN, VOL_UP};
        let mask = if on { [(m.mute, MUTE), (m.vol_down, VOL_DOWN), (m.vol_up, VOL_UP)].iter().filter(|b| b.0).map(|b| b.1).sum() } else { 0 };
        crate::platform::mediakeys::sync(mask, self.bar_hwnd());
        let Some(s) = self.smtc.as_mut() else { return };
        s.set_buttons(m.play, m.prev, m.next);
        let snap = &self.ui.snap;
        let n = self.now.lock();
        let cover = n.cover.as_ref().map(|c| c.raw.clone());
        let (title, artist, album) = (n.title.clone(), n.artist.clone(), n.album.clone());
        drop(n);
        s.update(snap.playing, snap.current.is_none() && snap.radio.is_none(), &title, &artist, &album, cover.as_deref().map(|v| v.as_slice()));
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
        if ui.cover_track != ui.snap.current || ui.cover_radio != ui.snap.radio {
            ui.cover_track = ui.snap.current;
            ui.cover_radio = ui.snap.radio.clone();
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
        // 电台没有歌词：电台名 / 当前节目一直显示
        let info_target = if age > Duration::from_secs(8) && ui.snap.radio.is_none() { 0.0 } else { 1.0 };
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
