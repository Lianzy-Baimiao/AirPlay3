//! 曲库：曲目表 + 列表（试听列表、默认列表、自建列表、喜爱、不再播放）。

mod lists;
pub mod model;
pub mod playlist_io;
pub mod scan;
#[cfg(test)]
mod tests;

pub use lists::{sort_ids, sort_key};

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use model::*;

pub struct Library {
    pub tracks: HashMap<TrackId, Track>,
    pub lists: Vec<Playlist>,
    next_id: u64,
    by_source: HashMap<(PathBuf, u32), TrackId>,
    favorites: HashSet<TrackId>,
    banned: HashSet<TrackId>,
    /// 任何修改都 +1（UI 缓存失效、自动保存）
    pub rev: u64,
}

#[derive(Serialize)]
struct DiskRef<'a> {
    version: u32,
    next_id: u64,
    tracks: Vec<&'a Track>,
    lists: &'a [Playlist],
}

#[derive(Deserialize)]
struct Disk {
    #[serde(default)]
    next_id: u64,
    #[serde(default)]
    tracks: Vec<Track>,
    #[serde(default)]
    lists: Vec<Playlist>,
}

impl Default for Library {
    fn default() -> Self {
        let mut lib = Library {
            tracks: HashMap::new(),
            lists: Vec::new(),
            next_id: 1,
            by_source: HashMap::new(),
            favorites: HashSet::new(),
            banned: HashSet::new(),
            rev: 1,
        };
        lib.ensure_special_lists();
        lib
    }
}

impl Library {
    pub fn load(path: &Path) -> Library {
        let Some(disk) = crate::config::store::load_json::<Disk>(path) else { return Library::default() };
        let mut lib = Library::default();
        lib.lists.clear();
        lib.next_id = disk.next_id.max(1);
        for t in disk.tracks {
            lib.next_id = lib.next_id.max(t.id + 1);
            lib.by_source.insert(t.source_key(), t.id);
            lib.tracks.insert(t.id, t);
        }
        for mut l in disk.lists {
            l.tracks.retain(|id| lib.tracks.contains_key(id));
            lib.next_id = lib.next_id.max(l.id + 1);
            lib.lists.push(l);
        }
        lib.ensure_special_lists();
        lib.rebuild_sets();
        lib
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut tracks: Vec<&Track> = self.tracks.values().collect();
        tracks.sort_by_key(|t| t.id);
        let d = DiskRef { version: 1, next_id: self.next_id, tracks, lists: &self.lists };
        serde_json::to_vec(&d).unwrap_or_default()
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn ensure_special_lists(&mut self) {
        let specs = [
            (ListKind::Audition, "试听列表"),
            (ListKind::Local, "默认列表"),
            (ListKind::Favorite, "喜爱"),
            (ListKind::Banned, "不再播放"),
        ];
        for (kind, name) in specs {
            if !self.lists.iter().any(|l| l.kind == kind) {
                let id = self.alloc_id();
                self.lists.push(Playlist { id, name: name.into(), kind, tracks: Vec::new() });
            }
        }
        // 显示顺序：试听、默认、自建列表…、喜爱、不再播放
        let rank = |k: ListKind| match k {
            ListKind::Audition => 0,
            ListKind::Local => 1,
            ListKind::User => 2,
            ListKind::Favorite => 3,
            ListKind::Banned => 4,
        };
        self.lists.sort_by_key(|l| rank(l.kind));
    }

    pub(crate) fn rebuild_sets(&mut self) {
        let set = |lib: &Library, k: ListKind| lib.special(k).map(|l| l.tracks.iter().copied().collect()).unwrap_or_default();
        self.favorites = set(self, ListKind::Favorite);
        self.banned = set(self, ListKind::Banned);
    }

    pub fn touch(&mut self) {
        self.rev += 1;
    }

    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.get(&id)
    }

    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.rev += 1;
        self.tracks.get_mut(&id)
    }

    pub fn list(&self, id: ListId) -> Option<&Playlist> {
        self.lists.iter().find(|l| l.id == id)
    }

    pub fn special(&self, kind: ListKind) -> Option<&Playlist> {
        self.lists.iter().find(|l| l.kind == kind)
    }

    pub fn special_id(&self, kind: ListKind) -> ListId {
        self.special(kind).map(|l| l.id).unwrap_or(0)
    }

    pub fn is_favorite(&self, id: TrackId) -> bool {
        self.favorites.contains(&id)
    }

    pub fn is_banned(&self, id: TrackId) -> bool {
        self.banned.contains(&id)
    }

    /// 加入曲库（按 文件+CUE 分轨 去重），返回曲目 id。已存在时更新标签信息但保留用户数据。
    pub fn upsert(&mut self, mut t: Track) -> TrackId {
        self.rev += 1;
        if let Some(&id) = self.by_source.get(&t.source_key()) {
            if let Some(old) = self.tracks.get_mut(&id) {
                t.id = id;
                t.added = old.added;
                t.play_count = old.play_count;
                t.last_played = old.last_played;
                t.lyric_path = old.lyric_path.take();
                t.lyric_unlinked = old.lyric_unlinked;
                t.lyric_offset_ms = old.lyric_offset_ms;
                t.cover_path = old.cover_path.take();
                t.cover_unlinked = old.cover_unlinked;
                *old = t;
            }
            return id;
        }
        t.id = self.alloc_id();
        let id = t.id;
        self.by_source.insert(t.source_key(), id);
        self.tracks.insert(id, t);
        id
    }

    pub fn find_by_path(&self, path: &Path) -> Option<TrackId> {
        self.by_source.get(&(path.to_path_buf(), 0)).copied()
    }
}
