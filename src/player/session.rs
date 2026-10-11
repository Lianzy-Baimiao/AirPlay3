//! 引擎事件处理、导入、断点续播、保存。

use std::path::PathBuf;
use std::sync::Arc;

use crate::audio::{Event, PlayState};
use crate::library::ListId;
use crate::library::scan::{ImportJob, spawn_import};

use super::{Notice, Player, Source};

impl Player {
    pub fn on_event(&self, ev: Event) {
        match ev {
            Event::Started(token) => {
                let mut g = self.core.lock();
                let core = &mut *g;
                if let Some(id) = core.queue.track_of(token) {
                    core.queue.set_current(&core.lib, id);
                    if self.engine.state() == PlayState::Playing {
                        core.lib.record_play(id);
                    }
                }
                drop(g);
                self.snapshot_last();
            }
            Event::Failed(token, err) => {
                let mut g = self.core.lock();
                let id = g.queue.track_of(token);
                let title = id.and_then(|id| g.lib.track_mut(id)).map(|t| {
                    t.broken = true;
                    t.display_title()
                });
                drop(g);
                self.notify(Notice::Error(format!("无法播放「{}」: {err}", title.unwrap_or_default())));
            }
            Event::Stopped => {}
            Event::DeviceError(msg) => self.notify(Notice::Error(msg)),
        }
        self.repaint();
    }

    /// 记录断点（列表、曲目、位置），用于"从上次中断位置继续"。
    pub fn snapshot_last(&self) {
        let (token, pos, _) = self.engine.position();
        if token == 0 {
            return;
        }
        let mut g = self.core.lock();
        let core = &mut *g;
        let Some(track_id) = core.queue.track_of(token) else { return };
        let list_id = match &core.queue.source {
            Some(Source::List(l)) => *l,
            _ => 0,
        };
        let last = crate::config::LastPlayed { list_id, track_id, position: pos };
        if core.settings.playback.last.as_ref() != Some(&last) {
            core.settings.playback.last = Some(last);
            core.settings_rev += 1;
        }
    }

    /// 启动时恢复上次播放：按设置决定是否续播进度、是否自动开始。
    pub fn restore_session(&self) {
        let g = self.core.lock();
        let pb = g.settings.playback.clone();
        let Some(last) = pb.last.filter(|_| pb.resume_last || pb.autoplay) else { return };
        if g.lib.track(last.track_id).is_none() {
            return;
        }
        let src = match g.lib.list(last.list_id) {
            Some(l) if l.tracks.contains(&last.track_id) => Source::List(l.id),
            _ => Source::Tracks(vec![last.track_id]),
        };
        drop(g);
        let seek = if pb.resume_last { last.position } else { 0.0 };
        self.play_at(src, last.track_id, seek, !pb.autoplay);
    }

    /// 后台导入到列表 `list`。`play_first` 时导入完成后播放第一首导入的曲目。
    pub fn import(&self, paths: Vec<PathBuf>, list: ListId, play_first: bool) -> Arc<ImportJob> {
        let p = self.clone();
        let first = Arc::new(parking_lot::Mutex::new(None));
        let f = first.clone();
        let job = spawn_import(paths, move |tracks| {
            let mut g = p.core.lock();
            let ids: Vec<_> = tracks.into_iter().map(|t| g.lib.upsert(t)).collect();
            g.lib.add_to_list(list, &ids, None);
            drop(g);
            let mut first = f.lock();
            if first.is_none() && !ids.is_empty() {
                *first = Some(ids[0]);
                if play_first {
                    drop(first);
                    p.play_at(Source::List(list), ids[0], 0.0, false);
                }
            }
            p.repaint();
        });
        job
    }

    /// 保存曲库和设置（有修改时）。`sync` 为 false 时在后台线程写盘。
    pub fn save(&self, sync: bool) {
        self.snapshot_last();
        let mut g = self.core.lock();
        let mut jobs: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        if g.lib.rev != g.saved_lib_rev {
            g.lib.gc();
            jobs.push((g.dirs.library_file(), g.lib.to_bytes()));
            g.saved_lib_rev = g.lib.rev;
        }
        if g.settings_rev != g.saved_settings_rev {
            if let Ok(b) = crate::config::store::to_bytes(&g.settings, true) {
                jobs.push((g.dirs.settings_file(), b));
            }
            g.saved_settings_rev = g.settings_rev;
        }
        drop(g);
        if jobs.is_empty() {
            return;
        }
        static WRITE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
        let write = move || {
            let _w = WRITE_LOCK.lock();
            for (path, bytes) in jobs {
                if let Err(e) = crate::config::store::save_bytes(&path, &bytes) {
                    log::error!("保存 {} 失败: {e}", path.display());
                }
            }
        };
        if sync { write() } else { std::thread::spawn(write); }
    }
}
