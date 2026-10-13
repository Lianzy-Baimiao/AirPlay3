//! 启动流程与控制线程。控制线程处理引擎事件、刷新正在播放、定时保存，
//! 并执行来自托盘/热键/系统媒体控制/第二个实例的命令（窗口隐藏时也能工作）。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, select, unbounded};

use crate::config::{DataDirs, Settings, store};
use crate::library::{Library, ListKind};
use crate::player::Player;
use crate::player::nowplaying::SharedNow;

#[derive(Clone, Debug)]
pub enum Command {
    PlayPause,
    Play,
    Pause,
    Next,
    Prev,
    Stop,
    VolumeUp,
    VolumeDown,
    Mute,
    Favorite,
    Ban,
    DeskLyric,
    Seek(f64),
    OpenFiles(Vec<PathBuf>),
}

pub struct Controller {
    pub player: Player,
    pub now: SharedNow,
    pub cmds: Sender<Command>,
}

fn handle(p: &Player, c: Command) {
    match c {
        Command::PlayPause => p.toggle_pause(),
        Command::Play => p.play(),
        Command::Pause => p.pause(),
        Command::Next => p.next(),
        Command::Prev => p.prev(),
        Command::Stop => p.stop(),
        Command::VolumeUp => p.volume_step(0.05),
        Command::VolumeDown => p.volume_step(-0.05),
        Command::Mute => p.toggle_mute(),
        Command::Favorite => {
            p.toggle_favorite_current();
        }
        Command::Ban => p.ban_current(),
        Command::DeskLyric => {
            let mut g = p.core.lock();
            g.settings.desk_lyric.visible = !g.settings.desk_lyric.visible;
            g.touch_settings();
            drop(g);
            // 界面线程在下一帧根据设置创建 / 关闭桌面歌词
            p.repaint();
        }
        Command::Seek(s) => p.seek(s),
        Command::OpenFiles(files) => open_files(p, files),
    }
    p.repaint();
}

/// 命令行/资源管理器打开的文件：加入"试听列表"并播放第一首。
pub fn open_files(p: &Player, files: Vec<PathBuf>) {
    let list = {
        let mut g = p.core.lock();
        let id = g.lib.special_id(ListKind::Audition);
        g.reveal = Some(id);
        id
    };
    p.import(files, list, true);
    p.repaint();
}

fn run_loop(p: Player, now: SharedNow, cmds: Receiver<Command>) {
    let events = p.engine.events.clone();
    let mut last_save = Instant::now();
    let mut last_snap = Instant::now();
    loop {
        select! {
            recv(events) -> ev => match ev {
                Ok(e) => p.on_event(e),
                Err(_) => return,
            },
            recv(cmds) -> c => match c {
                Ok(c) => handle(&p, c),
                Err(_) => return,
            },
            default(Duration::from_millis(250)) => {}
        }
        p.refresh_now_playing(&now, false);
        if last_snap.elapsed() > Duration::from_secs(10) {
            last_snap = Instant::now();
            p.snapshot_last();
        }
        if last_save.elapsed() > Duration::from_secs(4) {
            last_save = Instant::now();
            p.save(false);
        }
    }
}

/// 加载配置与曲库，创建播放器并启动控制线程。`ctx_slot` 在窗口创建后填入，用于请求重绘。
/// `repaint`：后台状态变化时通知界面重绘（任意线程调用）
pub fn start(dirs: DataDirs, repaint: Arc<dyn Fn() + Send + Sync>) -> Controller {
    let mut settings: Settings = store::load_json(&dirs.settings_file()).unwrap_or_default();
    settings.radio.migrate();
    let lib = Library::load(&dirs.library_file());
    log::info!("曲库: {} 首, {} 个列表", lib.tracks.len(), lib.lists.len());
    let player = Player::new(dirs, settings, lib, repaint);
    let now = SharedNow::default();
    let (tx, rx) = unbounded();
    {
        let (p, n) = (player.clone(), now.clone());
        std::thread::Builder::new().name("controller".into()).spawn(move || run_loop(p, n, rx)).expect("controller thread");
    }
    Controller { player, now, cmds: tx }
}
