//! 从界面动作到曲库的回归测试，不创建可见窗口，不读取用户配置。

use super::*;
use crate::config::{DataDirs, Settings};
use crate::gui::app::{PANES, Sizes, Win};
use crate::gui::canvas::Canvas;
use crate::gui::panels::playlist;
use crate::gui::state::UiState;
use crate::gui::window::Surface;
use crate::library::Library;
use crate::player::Player;
use std::time::Instant;

fn test_app(name: &str, lib: Library) -> App {
    let root = std::env::temp_dir().join(format!("ap3_playlist_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let settings = Settings::default();
    let sizes = Sizes::from_settings(&settings.window);
    let player = Player::new(DataDirs { root }, settings, lib, std::sync::Arc::new(|| {}));
    let ui = UiState::new(&player);
    let wins = PANES.into_iter().map(|pane| (pane, Win {
        hwnd: HWND::default(), canvas: Canvas::new(1, 1), surface: Surface::new(1, 1),
        dirty: false, visible: false, pos: (0, 0), anim: None,
    })).collect();
    let (async_tx, async_rx) = crossbeam_channel::unbounded();
    App {
        player, ui, sizes, wins, now: Default::default(), cmds: crossbeam_channel::unbounded().0,
        started: Instant::now(), quitting: false, menu: None, search: None, cfgdir: None, name_prompt: None,
        after: None, away: None, settings_win: None, desk: None, taskbar: None, minibar: None,
        smtc: None, captions: Default::default(), hotkey_failed: Vec::new(), async_tx, async_rx,
    }
}

#[test]
fn playlist_add_menu_offers_creation() {
    assert!(add_menu().iter().any(|i| matches!(i, Item::Do { label, act: Act::NewList, .. } if label == "新建播放列表…")),
        "点击 + 必须能新建播放列表，不能只有添加文件/文件夹");
}

#[test]
fn playlist_sidebar_click_selects_user_list() {
    let mut lib = Library::default();
    let user = lib.create_list("通勤");
    let mut app = test_app("select", lib);
    let lay = app.list_layout();
    app.mouse_down(Pane::List, lay.tabs.x + 25, lay.tabs.y + playlist::TAB_H + 10, false, false, false);
    assert_eq!(app.ui.list.list, Some(user), "点击左侧第二个列表必须切换导入目标");
    assert_eq!(app.ui.drag, crate::gui::state::Drag::None, "点列表不能变成拖动窗口");
}

fn send(app: &mut App, msg: AsyncMsg) {
    app.async_tx.send(msg).unwrap();
    app.drain_async();
}

fn wait_for(mut check: impl FnMut() -> bool) {
    let until = Instant::now() + std::time::Duration::from_secs(5);
    while !check() {
        assert!(Instant::now() < until, "后台导入或保存未完成");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn wav_folder(app: &App) -> std::path::PathBuf {
    let folder = app.player.core.lock().dirs.root.join("测试专辑");
    std::fs::create_dir_all(&folder).unwrap();
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend(38u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes()); // PCM
    wav.extend(1u16.to_le_bytes()); // mono
    wav.extend(44100u32.to_le_bytes());
    wav.extend(88200u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(2u32.to_le_bytes());
    wav.extend(0i16.to_le_bytes());
    std::fs::write(folder.join("一首歌.wav"), wav).unwrap();
    folder
}

#[test]
fn playlist_creation_rename_and_reload() {
    let mut app = test_app("create", Library::default());
    app.finish_list_name(None, "  通勤歌单  ");
    let id = app.ui.list.list.unwrap();
    assert!(app.music_lists().contains(&(id, "通勤歌单".into())), "新建的空列表必须立即显示");
    assert_eq!(app.player.core.lock().lib.list(id).unwrap().kind, ListKind::User);
    let file = app.player.core.lock().dirs.library_file();
    wait_for(|| Library::load(&file).list(id).is_some());
    let count = app.player.core.lock().lib.lists.len();
    app.finish_list_name(None, "  ");
    assert_eq!(app.player.core.lock().lib.lists.len(), count, "空名称不能创建列表");
    app.finish_list_name(Some(id), "夜间音乐");
    wait_for(|| Library::load(&file).list(id).is_some_and(|l| l.name == "夜间音乐"));
    let restored = test_app("reload", Library::load(&file));
    assert!(restored.music_lists().contains(&(id, "夜间音乐".into())), "重新加载后保留名称和空列表");
}

#[test]
fn playlist_folder_import_keeps_captured_target_after_switch() {
    let mut app = test_app("import", Library::default());
    app.finish_list_name(None, "导入目标");
    let target = app.ui.list.list.unwrap();
    let default = app.player.core.lock().lib.special_id(ListKind::Audition);
    let folder = wav_folder(&app);
    app.ui.list.search = "找不到".into();
    app.ui.list.selected.insert(999);
    app.ui.list.anchor = Some(99);
    app.ui.list.scroll = 200;
    let lay = app.list_layout();
    app.mouse_down(Pane::List, lay.tabs.x + 25, lay.tabs.y + 10, false, false, false);
    assert_eq!(app.ui.list.list, Some(default));
    assert!(app.ui.list.search.is_empty() && app.ui.list.selected.is_empty());
    assert_eq!((app.ui.list.anchor, app.ui.list.scroll), (None, 0));
    // 选文件夹期间切换了列表，返回结果仍使用打开对话框时捕获的目标。
    send(&mut app, AsyncMsg::Files(vec![folder], target));
    wait_for(|| app.player.core.lock().lib.list(target).unwrap().tracks.len() == 1);
    assert!(app.player.core.lock().lib.list(default).unwrap().tracks.is_empty());
    app.select_music_list(target);
    assert_eq!(app.ui.list.ids.len(), 1);
    app.select_music_list(default);
    assert!(app.ui.list.ids.is_empty(), "切换后不能残留另一个列表的歌曲");
    app.player.save(true);
    let lib = Library::load(&app.player.core.lock().dirs.library_file());
    assert_eq!(lib.list(target).unwrap().tracks.len(), 1);
    assert!(lib.list(default).unwrap().tracks.is_empty());
}

#[test]
fn playlist_folder_creates_named_list() {
    let mut app = test_app("folder_list", Library::default());
    let default = app.ui.list.list.unwrap();
    let folder = wav_folder(&app);
    send(&mut app, AsyncMsg::FolderList(folder));
    let id = app.ui.list.list.unwrap();
    assert_ne!(id, default);
    assert!(app.music_lists().contains(&(id, "测试专辑".into())));
    wait_for(|| app.player.core.lock().lib.list(id).unwrap().tracks.len() == 1);
    assert!(app.player.core.lock().lib.list(default).unwrap().tracks.is_empty());
}

#[test]
fn playlist_sidebar_reveals_and_selects_scrolled_lists() {
    let mut lib = Library::default();
    let last = (0..25).map(|n| lib.create_list(&format!("我的播放列表 {n}"))).last().unwrap();
    let mut app = test_app("scroll", lib);
    app.select_music_list(last);
    let mut canvas = Canvas::new(app.sizes.list.0, app.sizes.list.1);
    app.paint(Pane::List, &mut canvas);
    let scroll = app.ui.list.tab_scroll;
    assert!(scroll > 0 && app.list_layout().tabs.w > playlist::TAB_W);
    app.paint(Pane::List, &mut canvas);
    assert_eq!(app.ui.list.tab_scroll, scroll, "重绘不能把左栏滚动重置到顶部");
    let lay = app.list_layout();
    let y = lay.tabs.y + 25 * playlist::TAB_H + 10 - scroll;
    assert_eq!(app.music_list_at(lay.tabs.x + 25, y), Some(last));
    app.mouse_down(Pane::List, lay.tabs.x + 25, y, false, false, false);
    assert_eq!(app.ui.list.list, Some(last));
}

#[test]
fn playlist_name_dialog_confirms_renames_and_cancels() {
    use crate::gui::nameprompt::NamePrompt;
    let mut app = test_app("name_messages", Library::default());
    let h = HWND::default();
    app.name_prompt = Some(NamePrompt::offscreen(None, "新建列表"));
    for unit in "夜间音乐🎵".encode_utf16() {
        app.list_name_msg(h, WM_CHAR, WPARAM(unit as usize), LPARAM(0));
    }
    app.list_name_msg(h, WM_KEYDOWN, WPARAM(13), LPARAM(0));
    assert!(app.name_prompt.is_none());
    let id = app.ui.list.list.unwrap();
    assert_eq!(app.player.core.lock().lib.list(id).unwrap().name, "夜间音乐🎵");
    app.name_prompt = Some(NamePrompt::offscreen(Some(id), "通勤"));
    app.list_name_msg(h, WM_KEYDOWN, WPARAM(13), LPARAM(0));
    assert_eq!(app.player.core.lock().lib.list(id).unwrap().name, "通勤");
    let count = app.player.core.lock().lib.lists.len();
    app.name_prompt = Some(NamePrompt::offscreen(None, "取消的列表"));
    app.list_name_msg(h, WM_KEYDOWN, WPARAM(27), LPARAM(0));
    assert!(app.name_prompt.is_none());
    assert_eq!(app.player.core.lock().lib.lists.len(), count);
}

#[test]
fn playlist_name_dialog_ignores_empty_and_ime_confirmation() {
    use crate::gui::nameprompt::NamePrompt;
    let mut app = test_app("name_ime", Library::default());
    let h = HWND::default();
    let count = app.player.core.lock().lib.lists.len();
    app.name_prompt = Some(NamePrompt::offscreen(None, "  "));
    app.list_name_msg(h, WM_KEYDOWN, WPARAM(13), LPARAM(0));
    assert!(app.name_prompt.is_some());
    app.name_prompt = Some(NamePrompt::offscreen(None, "中文名称"));
    app.list_name_msg(h, WM_IME_STARTCOMPOSITION, WPARAM(0), LPARAM(0));
    app.list_name_msg(h, WM_KEYDOWN, WPARAM(13), LPARAM(0));
    app.list_name_msg(h, WM_KEYDOWN, WPARAM(27), LPARAM(0));
    assert!(app.name_prompt.is_some(), "输入法选择候选词时不能关闭弹框");
    assert_eq!(app.player.core.lock().lib.lists.len(), count);
    app.list_name_msg(h, WM_IME_ENDCOMPOSITION, WPARAM(0), LPARAM(0));
    app.list_name_msg(h, WM_KEYDOWN, WPARAM(13), LPARAM(0));
    assert!(app.name_prompt.is_none());
    assert_eq!(app.player.core.lock().lib.lists.len(), count + 1);
}
