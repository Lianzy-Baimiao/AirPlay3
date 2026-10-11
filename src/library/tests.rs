use std::collections::HashSet;
use std::path::PathBuf;

use super::scan::natural_cmp;
use super::*;

fn track(path: &str, title: &str, artist: &str, dur: f64) -> Track {
    Track { path: PathBuf::from(path), title: title.into(), artist: artist.into(), duration: dur, ..Default::default() }
}

#[test]
fn natural_sort() {
    let mut v = vec!["10.mp3", "2.mp3", "1.mp3", "a10", "A2", "01.x"];
    v.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(v, vec!["1.mp3", "01.x", "2.mp3", "10.mp3", "A2", "a10"]);
}

#[test]
fn lists_and_special() {
    let mut lib = Library::default();
    assert_eq!(lib.lists.len(), 4);
    let a = lib.upsert(track(r"C:\a.mp3", "晴天", "周杰伦", 269.0));
    let b = lib.upsert(track(r"C:\b.mp3", "Apple", "Zed", 100.0));
    let c = lib.upsert(track(r"C:\c.mp3", "传奇", "王菲", 300.0));
    // 去重：同一路径再次导入得到同一个 id，并保留播放次数
    lib.track_mut(a).unwrap().play_count = 5;
    assert_eq!(lib.upsert(track(r"C:\a.mp3", "晴天(新标签)", "周杰伦", 269.0)), a);
    assert_eq!(lib.track(a).unwrap().play_count, 5);

    let user = lib.create_list("我的");
    assert_eq!(lib.lists.iter().position(|l| l.id == user), Some(2), "自建列表在喜爱之前");
    assert_eq!(lib.add_to_list(user, &[a, b, c, a], None), 3);
    lib.move_in_list(user, &HashSet::from([c]), 0);
    assert_eq!(lib.list(user).unwrap().tracks, vec![c, a, b]);
    lib.sort_list(user, SortBy::Title);
    // Apple < chuan qi < qing tian
    assert_eq!(lib.list(user).unwrap().tracks, vec![b, c, a]);
    lib.sort_list(user, SortBy::Duration);
    assert_eq!(lib.list(user).unwrap().tracks, vec![b, a, c]);

    assert!(lib.toggle_favorite(a));
    assert!(lib.is_favorite(a));
    lib.set_banned(b, true);
    assert!(lib.is_banned(b));
    let all = lib.all_track_ids();
    assert!(all.contains(&a) && all.contains(&b), "被禁曲目仍在自建列表中");
    lib.remove_from_list(user, &HashSet::from([b]));
    assert!(!lib.all_track_ids().contains(&b), "只在不再播放列表中的曲目不算全部曲目");

    assert!(!lib.delete_list(lib.special_id(ListKind::Favorite)), "特殊列表不能删除");
    assert!(lib.rename_list(user, "新名字"));

    // 保存/读取往返
    let dir = std::env::temp_dir().join(format!("ap3_lib_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("library.json");
    crate::config::store::save_bytes(&f, &lib.to_bytes()).unwrap();
    let mut back = Library::load(&f);
    assert_eq!(back.lists.len(), lib.lists.len());
    assert!(back.is_favorite(a) && back.is_banned(b));
    assert_eq!(back.list(user).unwrap().name, "新名字");
    let d = back.upsert(track(r"C:\d.mp3", "", "", 1.0));
    assert!(d > c, "新 id 不与旧 id 冲突");
    std::fs::remove_dir_all(&dir).unwrap();
}
