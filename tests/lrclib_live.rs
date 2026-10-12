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

/// 多来源：用户曲库里 LRCLIB 搜不到的歌，「自动」能找到带时间轴的歌词；LrcAPI 同时给出专辑图片
#[test]
#[ignore]
fn online_sources_live() {
    use airplay3::lyrics::online::{Provider, best_match, covers, search};
    let net = airplay3::config::NetworkSettings::default();
    for p in Provider::ORDER {
        let r = search(&net, &[p], "怪咖", "薛之谦");
        println!("{} 搜索 怪咖 -> {:?}", p.label(), r.as_ref().map(|v| v.len()));
    }
    let c = covers(&net, "怪咖", "薛之谦").expect("LrcAPI 图片搜索失败");
    println!("LrcAPI 图片 {} 条，第一条 {}", c.len(), c.first().map(|x| x.cover.as_str()).unwrap_or(""));
    assert!(!c.is_empty());
    // LrcAPI 是模糊搜索：乱写的歌名也会返回相近的结果；有时返回 404（按「没有结果」处理），都不应报错
    assert!(search(&net, &[Provider::LrcApi], "这首歌肯定不存在qwxz", "没有这个人qwxz").is_ok());
    let cases = [("亲爱的你的棉裤真的不在我家[music.migu.cn]", "齐一[music.migu.cn]", 0.0), ("站在高岗上", "起点音乐", 0.0), ("冬眠", "严浩翔", 0.0), ("一剪梅", "费玉清", 0.0)];
    let mut found = 0;
    for (t, a, d) in cases {
        let r = best_match(&net, Provider::Auto, t, a, d);
        let got = r.as_ref().ok().and_then(|x| x.as_ref()).map(|(c, txt)| format!("{} 「{}」 {} 行", c.provider.label(), c.title, txt.lines().count()));
        println!("{t} / {a} -> {got:?} {}", r.as_ref().err().cloned().unwrap_or_default());
        found += got.is_some() as usize;
    }
    assert!(found >= 3, "只找到 {found} 首");
}

/// 艺人图片：Deezer / 维基百科（中国大陆需要代理），按艺人缓存
#[test]
#[ignore]
fn artist_pic_live() {
    let net = airplay3::config::NetworkSettings::default();
    let dir = std::env::temp_dir().join("ap3_artist_live");
    let _ = std::fs::remove_dir_all(&dir);
    let mut found = 0;
    for a in ["谢春花", "周杰伦", "告五人", "Coldplay", "起点音乐"] {
        let t = std::time::Instant::now();
        let r = airplay3::media::artist::find(&net, &dir, a, true);
        println!("{a} -> {:?} {:?}", r.as_ref().map(|b| b.as_ref().map(|b| b.len())), t.elapsed());
        if let Ok(Some(b)) = &r {
            assert!(b.starts_with(&[0xFF, 0xD8]) || b.starts_with(b"\x89PNG"), "{a} 不是图片");
            found += 1;
        }
    }
    // 第二次从缓存取
    assert!(matches!(airplay3::media::artist::cached(&dir, "谢春花"), Some(Some(_))));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(found >= 3, "只找到 {found} 位");
}
