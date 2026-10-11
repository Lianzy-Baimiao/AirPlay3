//! 列表操作：增删、移动、喜爱/不再播放、永久排序、"全部曲目"、清理。

use std::collections::HashSet;

use super::*;

impl Library {
    pub fn list_mut(&mut self, id: ListId) -> Option<&mut Playlist> {
        self.rev += 1;
        self.lists.iter_mut().find(|l| l.id == id)
    }

    /// 插入到列表（列表内去重）。`at = None` 追加到末尾。返回新增数量。
    pub fn add_to_list(&mut self, list: ListId, ids: &[TrackId], at: Option<usize>) -> usize {
        let Some(l) = self.lists.iter_mut().find(|l| l.id == list) else { return 0 };
        let existing: HashSet<TrackId> = l.tracks.iter().copied().collect();
        let mut seen = HashSet::new();
        let new: Vec<TrackId> = ids.iter().copied().filter(|id| !existing.contains(id) && seen.insert(*id)).collect();
        let n = new.len();
        let pos = at.unwrap_or(l.tracks.len()).min(l.tracks.len());
        l.tracks.splice(pos..pos, new);
        if n > 0 {
            self.rev += 1;
            self.rebuild_sets();
        }
        n
    }

    pub fn remove_from_list(&mut self, list: ListId, ids: &HashSet<TrackId>) {
        if let Some(l) = self.lists.iter_mut().find(|l| l.id == list) {
            l.tracks.retain(|id| !ids.contains(id));
        }
        self.rev += 1;
        self.rebuild_sets();
    }

    pub fn clear_list(&mut self, list: ListId) {
        if let Some(l) = self.lists.iter_mut().find(|l| l.id == list) {
            l.tracks.clear();
        }
        self.rev += 1;
        self.rebuild_sets();
    }

    /// 把选中的曲目移动到 `to` 位置（拖拽排序）。
    pub fn move_in_list(&mut self, list: ListId, ids: &HashSet<TrackId>, to: usize) {
        let Some(l) = self.lists.iter_mut().find(|l| l.id == list) else { return };
        let to = to.min(l.tracks.len());
        let moving: Vec<TrackId> = l.tracks.iter().copied().filter(|id| ids.contains(id)).collect();
        let before = l.tracks[..to].iter().filter(|id| ids.contains(id)).count();
        l.tracks.retain(|id| !ids.contains(id));
        let at = (to - before).min(l.tracks.len());
        l.tracks.splice(at..at, moving);
        self.rev += 1;
    }

    pub fn create_list(&mut self, name: &str) -> ListId {
        let id = self.alloc_id();
        let pos = self.lists.iter().position(|l| matches!(l.kind, ListKind::Favorite | ListKind::Banned)).unwrap_or(self.lists.len());
        let name = if name.trim().is_empty() { "未命名".to_string() } else { name.trim().to_string() };
        self.lists.insert(pos, Playlist { id, name, kind: ListKind::User, tracks: Vec::new() });
        self.rev += 1;
        id
    }

    /// 只有自建列表和默认列表可以改名
    pub fn rename_list(&mut self, id: ListId, name: &str) -> bool {
        let name = name.trim();
        match self.lists.iter_mut().find(|l| l.id == id && matches!(l.kind, ListKind::User | ListKind::Local)) {
            Some(l) if !name.is_empty() => {
                l.name = name.to_string();
                self.rev += 1;
                true
            }
            _ => false,
        }
    }

    pub fn delete_list(&mut self, id: ListId) -> bool {
        let before = self.lists.len();
        self.lists.retain(|l| !(l.id == id && l.kind == ListKind::User));
        self.rev += 1;
        self.lists.len() != before
    }

    fn set_special(&mut self, kind: ListKind, id: TrackId, on: bool) {
        let lid = self.special_id(kind);
        if on {
            self.add_to_list(lid, &[id], None);
        } else {
            self.remove_from_list(lid, &HashSet::from([id]));
        }
    }

    /// 切换"喜爱"，返回新状态
    pub fn toggle_favorite(&mut self, id: TrackId) -> bool {
        let on = !self.is_favorite(id);
        self.set_special(ListKind::Favorite, id, on);
        on
    }

    pub fn set_banned(&mut self, id: TrackId, on: bool) {
        self.set_special(ListKind::Banned, id, on);
    }

    /// 全部曲目：除"不再播放"外所有列表中的曲目（按首次出现顺序）。
    pub fn all_track_ids(&self) -> Vec<TrackId> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for l in self.lists.iter().filter(|l| l.kind != ListKind::Banned) {
            out.extend(l.tracks.iter().copied().filter(|id| seen.insert(*id)));
        }
        out
    }

    /// 删除不在任何列表中的曲目（保存前调用）。
    pub fn gc(&mut self) {
        let used: HashSet<TrackId> = self.lists.iter().flat_map(|l| l.tracks.iter().copied()).collect();
        let before = self.tracks.len();
        self.tracks.retain(|id, _| used.contains(id));
        if self.tracks.len() != before {
            self.by_source.retain(|_, id| used.contains(id));
        }
    }

    /// 永久排序（原版"按名称 / 艺人 / 专辑 / 时间长度排序"）。
    pub fn sort_list(&mut self, list: ListId, by: SortBy) {
        let Some(mut ids) = self.list(list).map(|l| l.tracks.clone()) else { return };
        sort_ids(self, &mut ids, by);
        if let Some(l) = self.lists.iter_mut().find(|l| l.id == list) {
            l.tracks = ids;
        }
        self.rev += 1;
    }

    /// 删除不能播放的内容，返回删除数量。
    pub fn remove_broken(&mut self, list: ListId) -> usize {
        let broken: HashSet<TrackId> = self.tracks.values().filter(|t| t.broken).map(|t| t.id).collect();
        let n = self.list(list).map_or(0, |l| l.tracks.iter().filter(|id| broken.contains(id)).count());
        self.remove_from_list(list, &broken);
        n
    }

    pub fn record_play(&mut self, id: TrackId) {
        if let Some(t) = self.tracks.get_mut(&id) {
            t.play_count += 1;
            t.last_played = crate::util::now_secs();
            self.rev += 1;
        }
    }
}

/// 按字段排序一组曲目（稳定排序；中文按拼音）。
pub fn sort_ids(lib: &Library, ids: &mut Vec<TrackId>, by: SortBy) {
    let mut keyed: Vec<(String, f64, TrackId)> = ids
        .iter()
        .filter_map(|id| lib.tracks.get(id))
        .map(|t| {
            let k = match by {
                SortBy::Title => sort_key(&t.display_title()),
                SortBy::Artist => format!("{}\u{1}{}", sort_key(t.display_artist()), sort_key(&t.display_title())),
                SortBy::Album => format!("{}\u{1}{:04}{:04}", sort_key(t.display_album()), t.disc_no.unwrap_or(0), t.track_no.unwrap_or(0)),
                SortBy::Duration => String::new(),
                SortBy::Path => t.path.to_string_lossy().to_lowercase(),
            };
            (k, t.duration, t.id)
        })
        .collect();
    if by == SortBy::Duration { keyed.sort_by(|a, b| a.1.total_cmp(&b.1)) } else { keyed.sort_by(|a, b| a.0.cmp(&b.0)) }
    *ids = keyed.into_iter().map(|k| k.2).collect();
}

/// 排序键：汉字转拼音，其余小写。
pub fn sort_key(s: &str) -> String {
    use pinyin::ToPinyin;
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c.to_pinyin() {
            Some(p) => {
                out.push_str(p.plain());
                out.push(' ');
            }
            None => out.extend(c.to_lowercase()),
        }
    }
    out
}
