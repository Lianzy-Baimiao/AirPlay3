//! 手动更换歌词经过真实保存、关联、刷新后，各歌词界面仍应显示新内容。
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use super::app::{App, Sizes};
use super::state::UiState;
use crate::config::{DataDirs, Settings};
use crate::library::{Library, Track};
use crate::lyrics::{lrc, online::{self, Candidate, Provider}};
use crate::player::{Player, nowplaying::{Link, NowPlaying}};

fn headless(player: Player, now: crate::player::nowplaying::SharedNow) -> App {
    let ui = UiState::new(&player);
    let sizes = Sizes::from_settings(&player.core.lock().settings.window);
    let (cmds, _) = crossbeam_channel::unbounded();
    let (async_tx, async_rx) = crossbeam_channel::unbounded();
    App { player, now, cmds, wins: Vec::new(), ui, started: Instant::now(), quitting: false, sizes, menu: None, search: None, cfgdir: None, name_prompt: None, after: None, away: None, settings_win: None, hotkey_failed: Vec::new(), desk: None, taskbar: None, minibar: None, smtc: None, captions: Default::default(), async_tx, async_rx }
}

#[test]
fn manual_plain_lyrics_replace_traditional_lyrics_on_screen() {
    let root = std::env::temp_dir().join(format!("airplay3-manual-lyrics-{}", fastrand::u64(..)));
    let mut settings = Settings::default();
    settings.lyrics.embedded_pic = false;
    settings.lyrics.local_pic = false;
    settings.lyrics.auto_download = true;
    settings.lyrics.album_pic = false;
    settings.lyrics.artist_pic = false;
    let mut lib = Library::default();
    let id = lib.upsert(Track { path: root.join("song.wav"), title: "感谢你曾来过".into(), artist: "测试歌手".into(), ..Default::default() });
    let (tx, rx) = mpsc::channel();
    let player = Player::new(DataDirs { root: root.clone() }, settings, lib, Arc::new(move || { let _ = tx.send(()); }));
    player.core.lock().queue.current = Some(id);
    let now = Arc::new(parking_lot::Mutex::new(NowPlaying {
        track: Some(id), lyrics: Some(Arc::new(lrc::parse("[00:01]你最近過得還好嗎\n[00:03]故事裡的那個她"))), ..Default::default()
    }));
    let mut app = headless(player, now);
    let mut candidate = Candidate { provider: Provider::Lrclib, key: String::new(), title: "感谢你曾来过".into(), artist: "测试歌手".into(), album: String::new(), duration: 250.0, lyrics: Some("你最近过得还好吗\n故事里的那个她\n海南的冬天没有雪但不影响它开着花\n自从你走后我回到了那个没有你的家".into()), cover: String::new() };
    let text = online::fetch(&Default::default(), &candidate).unwrap();
    let path = online::save(&text, &candidate, &root, &candidate.title, &candidate.artist).unwrap();
    app.player.set_lyric_link(id, Link::File(path.clone()), &app.now);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !app.now.lock().loaded {
        assert!(rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_ok());
    }
    assert_eq!(app.now.lock().lyrics.as_ref().unwrap().lines[0].text, "你最近过得还好吗", "文件已加载");
    app.ui.snap.current = Some(id);
    app.ui.cover_track = Some(id);
    app.ui.track_started = Instant::now() - Duration::from_secs(10);
    app.tick_cover(Instant::now());
    assert!(app.ui.lyric_mode > 0.0, "手动简体歌词已加载，但歌词面板没有进入显示状态");
    let line = app.line_state();
    assert_eq!(line.text, "你最近过得还好吗", "桌面 / 任务栏歌词应显示新选的纯文本");
    assert_eq!(line.next, "故事里的那个她");
    assert_eq!(line.idx, None, "纯文本不伪造时间轴");
    assert_eq!(line.progress, 0.0);
    assert!(app.scroll_plain_lyrics(-120));
    assert_eq!(app.ui.plain_lyric_scroll, 3);
    app.now.lock().rev += 1; // 图片完成等更新不应打断阅读位置。
    app.tick_cover(Instant::now());
    assert_eq!(app.ui.plain_lyric_scroll, 3);
    app.scroll_plain_lyrics(120);
    assert_eq!(app.ui.plain_lyric_scroll, 0);
    // 可选离屏截图，复用实际 App::paint，检查主面板的纯文本和不同皮肤。
    if let Some(dir) = std::env::var_os("AP3_LYRICS_PREVIEW").map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        let original = super::theme::get();
        app.ui.lyric_mode = 1.0;
        app.ui.info = 0.0;
        for (name, body) in [("dark", [0x20, 0x20, 0x20]), ("blue", [0x10, 0x20, 0x40]), ("light", [0xd0, 0xd0, 0xd0]), ("rose", [0xd0, 0xb0, 0xb0])] {
            super::theme::set(super::theme::Theme { body, ..original });
            let mut canvas = super::canvas::Canvas::with_scale(304, 225, 1.5);
            app.paint(super::app::Pane::Cover, &mut canvas);
            canvas.save_png(&dir.join(format!("plain-{name}.png"))).unwrap();
        }
        super::theme::set(original);
    }
    // 同一路径再替换为同步简体歌词，当前播放立即使用新正文和时间轴。
    candidate.lyrics = Some("[00:01]你最近过得还好吗\n[00:03]故事里的那个她".into());
    online::save(candidate.lyrics.as_ref().unwrap(), &candidate, &root, &candidate.title, &candidate.artist).unwrap();
    app.player.set_lyric_link(id, Link::File(path), &app.now);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !app.now.lock().loaded {
        assert!(rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_ok());
    }
    app.ui.snap.pos = 3.5;
    app.tick_cover(Instant::now());
    let synced = app.line_state();
    assert_eq!(synced.text, "故事里的那个她");
    assert_eq!(synced.idx, Some(1));
    assert!(synced.revision > line.revision);
    assert!(!app.scroll_plain_lyrics(-120), "同步歌词不使用纯文本滚动");
    assert!(root.is_absolute() && root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(&root).unwrap();
}
