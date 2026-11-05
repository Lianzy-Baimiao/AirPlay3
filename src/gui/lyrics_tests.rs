//! 歌词下载、替换及封面等待时间的回归；不创建桌面窗口。
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use super::app::{App, Sizes};
use super::state::UiState;
use crate::config::{DataDirs, Settings};
use crate::library::{Library, Track};
use crate::lyrics::{lrc, online::{self, Candidate, Provider}};
use crate::player::{Player, nowplaying::{Link, NowPlaying}};

pub(super) fn headless() -> (App, mpsc::Receiver<()>) {
    let root = std::env::temp_dir().join(format!("airplay3-lyrics-ui-{}", fastrand::u64(..)));
    let mut settings = Settings::default();
    settings.lyrics.embedded_pic = false;
    settings.lyrics.local_pic = false;
    let mut lib = Library::default();
    let id = lib.upsert(Track { path: root.join("song.wav"), title: "测试歌曲".into(), ..Default::default() });
    let (tx, rx) = mpsc::channel();
    let player = Player::new(DataDirs { root }, settings, lib, Arc::new(move || { let _ = tx.send(()); }));
    player.core.lock().queue.current = Some(id);
    let now = Arc::new(parking_lot::Mutex::new(NowPlaying {
        track: Some(id), lyrics: Some(Arc::new(lrc::parse("[00:01]繁體第一句\n[00:03]繁體第二句"))), ..Default::default()
    }));
    let mut ui = UiState::new(&player);
    ui.snap.current = Some(id);
    let sizes = Sizes::from_settings(&player.core.lock().settings.window);
    let (cmds, _) = crossbeam_channel::unbounded();
    let (async_tx, async_rx) = crossbeam_channel::unbounded();
    (App { player, now, cmds, wins: Vec::new(), ui, started: Instant::now(), quitting: false, sizes, menu: None, search: None, cfgdir: None, name_prompt: None, after: None, away: None, settings_win: None, hotkey_failed: Vec::new(), desk: None, taskbar: None, minibar: None, smtc: None, captions: Default::default(), async_tx, async_rx }, rx)
}

#[test]
fn manual_synced_replacement_reloads_same_path() {
    let (mut app, rx) = headless();
    let root = app.player.core.lock().dirs.root.clone();
    let id = app.now.lock().track.unwrap();
    let mut candidate = Candidate { provider: Provider::LrcApi, key: String::new(), title: "测试歌曲".into(), artist: String::new(), album: String::new(), duration: 250.0, lyrics: None, cover: String::new() };
    let mut previous_path = None;
    for (text, expected) in [("[00:01]繁體第一句\n[00:03]繁體第二句", "繁體第二句"), ("[00:01]简体第一句\n[00:03]简体第二句", "简体第二句")] {
        candidate.lyrics = Some(text.into());
        let text = online::fetch(&Default::default(), &candidate).unwrap();
        let path = online::save(&text, &candidate, &root, &candidate.title, &candidate.artist).unwrap();
        if let Some(previous) = &previous_path { assert_eq!(previous, &path); }
        previous_path = Some(path.clone());
        let rev = app.now.lock().rev;
        app.player.set_lyric_link(id, Link::File(path), &app.now);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !app.now.lock().loaded {
            assert!(rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_ok());
        }
        app.ui.snap.pos = 3.5;
        let line = app.line_state();
        assert_eq!(line.text, expected);
        assert_eq!(line.idx, Some(1));
        assert!(line.revision > rev);
    }
    assert!(root.is_absolute() && root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cover_intro_duration_defaults_shortens_and_disables() {
    let (mut app, _) = headless();
    let started = Instant::now();
    app.tick_cover(started);
    app.tick_cover(started + Duration::from_secs(7));
    assert_eq!(app.ui.lyric_mode, 0.0, "默认八秒内展示封面");
    app.tick_cover(started + Duration::from_secs(8));
    assert!(app.ui.lyric_mode > 0.0);
    app.player.core.lock().settings.lyrics.cover_intro_secs = 2;
    app.ui.lyric_mode = 0.0;
    app.tick_cover(started + Duration::from_secs(1));
    assert_eq!(app.ui.lyric_mode, 0.0);
    app.tick_cover(started + Duration::from_secs(2));
    assert!(app.ui.lyric_mode > 0.0, "缩短时长即时生效");
    app.player.core.lock().settings.lyrics.cover_intro_secs = 0;
    app.ui.cover_track = None; // 新一首歌，播放刚开始。
    app.tick_cover(started);
    assert_eq!((app.ui.lyric_mode, app.ui.info), (1.0, 0.0), "关闭时直接展示歌词，不闪现信息遮罩");
    if let Some(dir) = std::env::var_os("AP3_LYRICS_PREVIEW").map(std::path::PathBuf::from) {
        use super::panels::settings::{self, State, SettingsModel};
        use super::settings_layout::{self as lay, Toggle, Widget};
        use super::settings_model::{self as model, TAB_LYRICS};
        std::fs::create_dir_all(&dir).unwrap();
        let s = app.player.core.lock().settings.clone();
        let items: Vec<_> = lay::layout(TAB_LYRICS).into_iter().map(|it| {
            let mut st = State { enabled: true, ..Default::default() };
            match &it.w {
                Widget::Pill { act: Toggle::Key(k), .. } | Widget::Check(k, _) => { st.on = model::get(&s, *k); st.enabled = model::enabled(&s, *k); st.dim = !st.enabled; }
                Widget::Edit(f, _) | Widget::Path(f, _) => st.value = model::text(&s, *f),
                Widget::Drop(_) => st.value = s.lyrics.source.label().into(),
                _ => {}
            }
            (it, st)
        }).collect();
        let original = super::theme::get();
        for (name, body) in [("dark", [0x20, 0x20, 0x20]), ("blue", [0x10, 0x20, 0x40]), ("light", [0xd0, 0xd0, 0xd0]), ("rose", [0xd0, 0xb0, 0xb0])] {
            super::theme::set(super::theme::Theme { body, ..original });
            let mut canvas = super::canvas::Canvas::with_scale(settings::W, settings::H, 1.25);
            settings::paint(&mut canvas, &SettingsModel { tab: TAB_LYRICS, items: &items, hover: None, hover_tab: None, hover_cell: None, strip: 1.0 });
            canvas.save_png(&dir.join(format!("lyrics-settings-{name}.png"))).unwrap();
        }
        super::theme::set(original);
    }
    app.now.lock().lyrics = None;
    app.tick_cover(started);
    assert_eq!(app.ui.lyric_mode, 0.0, "没有歌词时仍可展示封面");
}
