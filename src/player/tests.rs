use std::collections::HashSet;
use std::path::PathBuf;

use super::queue::{Queue, Source};
use crate::config::PlayMode::*;
use crate::library::{Library, ListId, Track, TrackId};

fn setup(n: usize) -> (Library, ListId, Vec<TrackId>) {
    let mut lib = Library::default();
    let list = lib.create_list("t");
    let ids: Vec<TrackId> = (0..n)
        .map(|i| lib.upsert(Track { path: PathBuf::from(format!("C:\\{i}.mp3")), duration: 1.0, ..Default::default() }))
        .collect();
    lib.add_to_list(list, &ids, None);
    (lib, list, ids)
}

#[test]
fn sequential_and_repeat() {
    let (mut lib, list, ids) = setup(3);
    let mut q = Queue::default();
    q.set_source(Source::List(list));
    q.set_current(&lib, ids[0]);
    assert_eq!(q.next_after(&lib, ids[0], Sequential, true), Some(ids[1]));
    assert_eq!(q.next_after(&lib, ids[2], Sequential, true), None, "顺序播放到末尾停止");
    assert_eq!(q.next_after(&lib, ids[2], Sequential, false), Some(ids[0]), "手动下一首循环");
    assert_eq!(q.next_after(&lib, ids[2], RepeatAll, true), Some(ids[0]));
    assert_eq!(q.next_after(&lib, ids[1], RepeatOne, true), Some(ids[1]));
    assert_eq!(q.next_after(&lib, ids[1], RepeatOne, false), Some(ids[2]));
    assert_eq!(q.prev_before(&lib, ids[0], Sequential), Some(ids[2]));
    // 不再播放、无法播放的曲目被跳过
    lib.set_banned(ids[1], true);
    assert_eq!(q.next_after(&lib, ids[0], Sequential, true), Some(ids[2]));
    lib.track_mut(ids[2]).unwrap().broken = true;
    assert_eq!(q.next_after(&lib, ids[0], RepeatAll, true), Some(ids[0]));
    // 从"不再播放"列表本身播放时不跳过
    let banned = lib.special_id(crate::library::ListKind::Banned);
    q.set_source(Source::List(banned));
    assert_eq!(q.first_playable(&lib), Some(ids[1]));
}

#[test]
fn removed_current_continues_from_position() {
    let (mut lib, list, ids) = setup(4);
    let mut q = Queue::default();
    q.set_source(Source::List(list));
    q.set_current(&lib, ids[1]);
    lib.remove_from_list(list, &HashSet::from([ids[1]]));
    assert_eq!(q.next_after(&lib, ids[1], Sequential, true), Some(ids[2]));
}

#[test]
fn shuffle_covers_all_and_handles_additions() {
    let (mut lib, list, ids) = setup(10);
    let mut q = Queue::default();
    q.set_source(Source::List(list));
    let mut cur = ids[0];
    let mut seen = HashSet::from([cur]);
    for _ in 0..9 {
        cur = q.next_after(&lib, cur, Shuffle, true).unwrap();
        assert!(seen.insert(cur), "一轮内不重复");
    }
    assert_eq!(seen.len(), 10);
    // 上一首回到刚才那首
    let next = q.next_after(&lib, cur, Shuffle, true).unwrap();
    assert_eq!(q.prev_before(&lib, next, Shuffle), Some(cur));
    // 新加入的曲目会在本轮内播到
    let extra = lib.upsert(Track { path: PathBuf::from("C:\\extra.mp3"), ..Default::default() });
    lib.add_to_list(list, &[extra], None);
    let mut c = next;
    let mut hit = false;
    for _ in 0..11 {
        c = q.next_after(&lib, c, Shuffle, true).unwrap();
        hit |= c == extra;
    }
    assert!(hit);
}

#[test]
fn tokens_map_back() {
    let (lib, list, ids) = setup(2);
    let mut q = Queue::default();
    q.set_source(Source::List(list));
    let r = q.request(&lib, ids[1], 3.0).unwrap();
    assert_eq!(q.track_of(r.token), Some(ids[1]));
    assert_eq!(r.seek, 3.0);
    for _ in 0..100 {
        q.request(&lib, ids[0], 0.0);
    }
    assert_eq!(q.track_of(r.token), None, "旧 token 被淘汰");
}
