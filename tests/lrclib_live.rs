//! 手动运行的联网测试：cargo test --test lrclib_live -- --ignored --nocapture
use airplay3::lyrics::lrclib;
use airplay3::lyrics::query::{clean_artist, clean_title};

#[test]
#[ignore]
fn search_live() {
    let net = airplay3::config::NetworkSettings::default();
    let t = std::time::Instant::now();
    let r = lrclib::search(&net, "晴天", "周杰伦");
    println!("elapsed {:?} -> {:?}", t.elapsed(), r.as_ref().map(|v| v.len()).map_err(|e| e.clone()));
    assert!(r.is_ok());
}

/// 专辑图片：台湾区返回繁体，靠时长匹配；保存到本地
#[test]
#[ignore]
fn artwork_live() {
    let net = airplay3::config::NetworkSettings::default();
    for (title, artist, dur) in [("晴天", "周杰伦", 269.0), ("体面 (Live)", "于文文", 282.0), ("we don't talk anymore", "charlie puth&selena gomez", 217.0)] {
        let r = airplay3::media::artwork::find(&net, title, artist, dur, false).expect("请求失败");
        println!("{title} / {artist} -> {:?} 字节", r.as_ref().map(|b| b.len()));
        let bytes = r.unwrap_or_else(|| panic!("{title} 没有找到"));
        assert!(bytes.len() > 10_000 && (bytes.starts_with(&[0xFF, 0xD8]) || bytes.starts_with(b"\x89PNG")));
        let dir = std::env::temp_dir().join("ap3_artwork_live");
        let p = airplay3::media::artwork::save(&bytes, &dir, artist, "", title).unwrap();
        assert!(p.is_file());
    }
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("ap3_artwork_live"));
}

/// 曲库里真实的"脏"标签：清理后能搜到；艺人是上传者时放宽为只按标题
#[test]
#[ignore]
fn dirty_tags_live() {
    let net = airplay3::config::NetworkSettings::default();
    for (title, artist, expect_loose) in [
        ("不如回家喝自来水 (Live版)", "陈楚生/王赫野", false),
        ("体面 (Live)", "于文文", false),
        ("漂洋过海来看你【六倍音质】", "起点音乐", true),
    ] {
        let (t, a) = (clean_title(title), clean_artist(artist));
        let r = lrclib::search_loose(&net, &t, &a);
        println!("{title} / {artist} -> 查询 {t} / {a} -> {:?}", r.as_ref().map(|(v, loose)| (v.len(), *loose)));
        let (v, loose) = r.expect("搜索失败");
        assert!(!v.is_empty(), "{t} 没有结果");
        assert_eq!(loose, expect_loose, "{t}");
    }
}
