//! 播放队列：来源（列表或曲目快照）、播放顺序（顺序/随机/全部循环/单曲循环）、token → 曲目映射。

use std::collections::{HashMap, HashSet, VecDeque};

use crate::audio::PlayRequest;
use crate::config::PlayMode;
use crate::library::{Library, ListId, ListKind, TrackId};

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// 实时跟随列表内容
    List(ListId),
    /// 视图快照（全部曲目、艺人、专辑、搜索结果）
    Tracks(Vec<TrackId>),
}

pub fn order_of<'a>(src: &'a Source, lib: &'a Library) -> &'a [TrackId] {
    match src {
        Source::List(id) => lib.list(*id).map(|l| l.tracks.as_slice()).unwrap_or(&[]),
        Source::Tracks(v) => v,
    }
}

#[derive(Default)]
pub struct Queue {
    pub source: Option<Source>,
    pub current: Option<TrackId>,
    /// 当前曲目在列表中的位置；曲目被删除后从这里继续
    index: usize,
    shuffle: Vec<TrackId>,
    tokens: HashMap<u64, TrackId>,
    token_order: VecDeque<u64>,
    next_token: u64,
}

impl Queue {
    pub fn order<'a>(&'a self, lib: &'a Library) -> &'a [TrackId] {
        self.source.as_ref().map_or(&[], |s| order_of(s, lib))
    }

    pub fn set_source(&mut self, src: Source) {
        if self.source.as_ref() != Some(&src) {
            self.shuffle.clear();
        }
        self.source = Some(src);
    }

    pub fn set_current(&mut self, lib: &Library, id: TrackId) {
        self.current = Some(id);
        if let Some(i) = self.order(lib).iter().position(|t| *t == id) {
            self.index = i;
        }
    }

    pub fn track_of(&self, token: u64) -> Option<TrackId> {
        self.tokens.get(&token).copied()
    }

    fn issue(&mut self, id: TrackId) -> u64 {
        self.next_token += 1;
        let t = self.next_token;
        self.tokens.insert(t, id);
        self.token_order.push_back(t);
        while self.token_order.len() > 64 {
            if let Some(old) = self.token_order.pop_front() {
                self.tokens.remove(&old);
            }
        }
        t
    }

    /// 为曲目生成播放请求（分配新 token）。
    pub fn request(&mut self, lib: &Library, id: TrackId, seek: f64) -> Option<PlayRequest> {
        let t = lib.track(id)?;
        let (start, end) = t.clip();
        let path = t.path.clone();
        Some(PlayRequest { token: self.issue(id), path, start, end, seek })
    }

    fn playable(&self, lib: &Library, id: TrackId) -> bool {
        // 从"不再播放"列表本身播放时不跳过
        let in_banned_list = matches!(&self.source, Some(Source::List(l)) if lib.list(*l).is_some_and(|l| l.kind == ListKind::Banned));
        lib.track(id).is_some_and(|t| !t.broken) && (in_banned_list || !lib.is_banned(id))
    }

    pub fn first_playable(&self, lib: &Library) -> Option<TrackId> {
        self.order(lib).iter().copied().find(|id| self.playable(lib, *id))
    }

    /// `after` 之后的下一首。`auto` = 自动续播：单曲循环重复当前、顺序播放到末尾停止；手动"下一首"会循环。
    pub fn next_after(&mut self, lib: &Library, after: TrackId, mode: PlayMode, auto: bool) -> Option<TrackId> {
        if mode == PlayMode::RepeatOne && auto {
            return self.playable(lib, after).then_some(after);
        }
        if mode == PlayMode::Shuffle {
            return self.shuffle_step(lib, after, 1);
        }
        let ord = self.order(lib);
        let n = ord.len() as isize;
        let start = match ord.iter().position(|t| *t == after) {
            Some(p) => p as isize,
            None => self.index.min(ord.len()) as isize - 1,
        };
        for k in 1..=n {
            let mut i = start + k;
            if i >= n {
                if mode == PlayMode::Sequential && auto {
                    return None;
                }
                i %= n;
            }
            let id = ord[i as usize];
            if self.playable(lib, id) {
                return Some(id);
            }
        }
        None
    }

    pub fn prev_before(&mut self, lib: &Library, cur: TrackId, mode: PlayMode) -> Option<TrackId> {
        if mode == PlayMode::Shuffle {
            return self.shuffle_step(lib, cur, -1);
        }
        let ord = self.order(lib);
        let n = ord.len() as isize;
        let start = ord.iter().position(|t| *t == cur).map_or(self.index.min(ord.len()) as isize, |p| p as isize);
        (1..=n).map(|k| ord[(start - k).rem_euclid(n) as usize]).find(|id| self.playable(lib, *id))
    }

    /// 让随机序列与列表内容一致：删掉已移除的，新加入的随机插到当前位置之后。
    fn sync_shuffle(&mut self, lib: &Library, anchor: TrackId) {
        let ord: Vec<TrackId> = self.order(lib).to_vec();
        let set: HashSet<TrackId> = ord.iter().copied().collect();
        self.shuffle.retain(|id| set.contains(id));
        let have: HashSet<TrackId> = self.shuffle.iter().copied().collect();
        let mut missing: Vec<TrackId> = ord.into_iter().filter(|id| !have.contains(id)).collect();
        fastrand::shuffle(&mut missing);
        match self.shuffle.iter().position(|t| *t == anchor) {
            Some(p) => {
                for id in missing {
                    let at = fastrand::usize(p + 1..=self.shuffle.len());
                    self.shuffle.insert(at, id);
                }
            }
            None if !missing.is_empty() => {
                self.shuffle.extend(missing);
                if let Some(i) = self.shuffle.iter().position(|t| *t == anchor) {
                    self.shuffle.swap(0, i);
                }
            }
            None => {}
        }
    }

    fn shuffle_step(&mut self, lib: &Library, cur: TrackId, dir: isize) -> Option<TrackId> {
        self.sync_shuffle(lib, cur);
        let n = self.shuffle.len() as isize;
        let mut pos = self.shuffle.iter().position(|t| *t == cur).map_or(-1, |p| p as isize);
        for _ in 0..n {
            pos += dir;
            if pos >= n {
                // 一轮结束：重新洗牌。刚播放的曲目放到新一轮末尾：
                // 既不会紧接着重复，"上一首"从新一轮第一首往回也能回到它。
                fastrand::shuffle(&mut self.shuffle);
                if let Some(i) = self.shuffle.iter().position(|t| *t == cur) {
                    self.shuffle.swap(i, n as usize - 1);
                }
                pos = 0;
            } else if pos < 0 {
                pos = n - 1;
            }
            let id = self.shuffle[pos as usize];
            if self.playable(lib, id) {
                return Some(id);
            }
        }
        None
    }
}
