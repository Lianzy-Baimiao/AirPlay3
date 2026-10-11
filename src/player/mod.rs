//! 播放控制：曲库 + 设置 + 队列 + 播放引擎。UI、托盘、热键、系统媒体控制都通过 `Player` 操作。
//!
//! 锁的约定：`core` 只做短时间持有；持有 `core` 时只能调用不阻塞的引擎接口（发命令/读原子量），
//! 不能弹出对话框或做 IO。引擎的解码线程会在 `next` 回调里锁 `core`。

mod control;
pub mod nowplaying;
pub mod queue;
mod session;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use parking_lot::Mutex;

use crate::audio::{Engine, NextProvider};
use crate::config::{DataDirs, Settings};
use crate::library::{Library, ListId, TrackId};
pub use queue::{Queue, Source};

pub struct Core {
    pub lib: Library,
    pub settings: Settings,
    /// 设置修改计数（自动保存用）
    pub settings_rev: u64,
    pub queue: Queue,
    pub dirs: DataDirs,
    /// 请求界面切换到该列表（命令行/第二个实例打开文件时）
    pub reveal: Option<ListId>,
    saved_lib_rev: u64,
    saved_settings_rev: u64,
}

impl Core {
    pub fn touch_settings(&mut self) {
        self.settings_rev += 1;
    }

    /// 歌词查找目录：自定义目录 + 程序歌词目录
    pub fn lyric_dirs(&self) -> Vec<std::path::PathBuf> {
        let mut v = Vec::new();
        if let Some(d) = self.settings.lyrics.custom_dir.as_ref().filter(|d| !d.trim().is_empty()) {
            v.push(std::path::PathBuf::from(d));
        }
        v.push(self.dirs.lyric_dir());
        v
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    Info(String),
    Error(String),
}

#[derive(Clone)]
pub struct Player {
    pub core: Arc<Mutex<Core>>,
    pub engine: Arc<Engine>,
    pub notices: Arc<Mutex<Vec<Notice>>>,
    repaint: Arc<dyn Fn() + Send + Sync>,
}

impl Player {
    pub fn new(dirs: DataDirs, settings: Settings, lib: Library, repaint: Arc<dyn Fn() + Send + Sync>) -> Player {
        let pb = settings.playback.clone();
        let (eq, audio) = (settings.eq.clone(), settings.audio.clone());
        let saved_lib_rev = lib.rev;
        let core = Arc::new(Mutex::new(Core {
            lib,
            settings,
            settings_rev: 0,
            queue: Queue::default(),
            dirs,
            reveal: None,
            saved_lib_rev,
            saved_settings_rev: 0,
        }));
        let c = core.clone();
        // 解码线程在一首快结束时调用：按播放模式取下一首（无缝衔接）
        let next: NextProvider = Box::new(move |token| {
            let mut g = c.lock();
            let core = &mut *g;
            let cur = core.queue.track_of(token)?;
            let mode = core.settings.playback.mode;
            let id = core.queue.next_after(&core.lib, cur, mode, true)?;
            core.queue.request(&core.lib, id, 0.0)
        });
        let engine = Arc::new(Engine::new(pb.volume, eq, audio, next));
        engine.set_muted(pb.muted);
        Player { core, engine, notices: Default::default(), repaint }
    }

    pub fn repaint(&self) {
        (self.repaint)()
    }

    pub fn notify(&self, n: Notice) {
        let mut v = self.notices.lock();
        v.push(n);
        let len = v.len();
        if len > 5 {
            v.drain(..len - 5);
        }
        drop(v);
        self.repaint();
    }

    /// 当前曲目：正在听到的，否则是队列记录的最后一首。
    pub fn current(&self, core: &Core) -> Option<TrackId> {
        let (token, _, _) = self.engine.position();
        core.queue.track_of(token).or(core.queue.current)
    }
}
