//! 艺人图片（原版「下载艺人图片」）：歌曲没有专辑图片时，歌词图片显示艺人照片。原版的图片服务早已关闭，
//! 这里用两个免费、无需密钥的公开接口：
//! 1. Deezer 公开 API（api.deezer.com 的艺人搜索，官方公开文档，不需要登录）：艺人照片全，正方形；
//! 2. Deezer 没有时用中文维基百科的条目配图；要求条目简介里提到歌手 / 乐队等，避免把同名的别的条目当成艺人。
//! 在本机曲库出现最多的 20 位艺人上实测（2026-10-12）：Deezer 18 位、维基百科 13 位有图，合起来 19 位
//! （剩下 1 位是上传者「起点音乐」，不是艺人）。
//! 这两个网站在中国大陆通常需要代理（程序会用系统代理或「配置 → 网络连接」里的代理）；连不上时只是没有艺人图片。
//! 照片按艺人缓存，同一艺人的歌共用；只在开启「下载艺人图片」或手动下载图片时才发送艺人名。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::config::NetworkSettings;
use crate::lyrics::query::clean_artist;
use crate::util::text::{normalize_key, to_simplified};

const DEEZER: &str = "https://api.deezer.com/search/artist";
const WIKI: &str = "https://zh.wikipedia.org/w/api.php";
/// 两个来源都没有的艺人记下来，这段时间内不再联网
const MISS_DAYS: u64 = 7;
/// 连不上（如没有代理）后这段时间内自动下载不再尝试，免得每换一首歌都等超时
const BACKOFF_SECS: u64 = 600;
static LAST_FAIL: AtomicU64 = AtomicU64::new(0);

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// 比较用的名字：转成简体再规范化（本地标签和网上的繁简常常不一样：苏打绿 / 蘇打綠）
fn key(s: &str) -> String {
    normalize_key(&to_simplified(s))
}

/// 搜索结果的名字是不是这位艺人
fn same_name(local: &str, remote: &str) -> bool {
    let (a, b) = (key(local), key(remote));
    !a.is_empty() && a == b
}

/// Deezer：取同名（繁简视为相同）的第一条；同名的艺人没有照片时不拿别人的
fn pick_deezer(v: &Value, artist: &str) -> Option<String> {
    let list = v["data"].as_array()?;
    let hit = list.iter().find(|x| same_name(artist, x["name"].as_str().unwrap_or_default()))?;
    let url = hit["picture_xl"].as_str().or(hit["picture_big"].as_str())?;
    // 没有照片时地址里的图片编号是空的（…/images/artist//1000x1000-…）
    (url.starts_with("https://") && !url.contains("/artist//")).then(|| url.to_string())
}

/// 维基百科：条目要有配图，简介里要像是音乐人（避免同名的神仙、地名、公司）
fn pick_wiki(v: &Value) -> Option<String> {
    const WORDS: [&str; 18] =
        ["歌手", "乐队", "樂隊", "乐团", "樂團", "音乐人", "音樂人", "歌唱", "唱作", "组合", "組合", "演唱", "说唱", "說唱", "饶舌", "饒舌", "作曲", "摇滚"];
    let pg = v["query"]["pages"].as_object()?.values().next()?;
    let intro = pg["extract"].as_str().unwrap_or_default();
    if !WORDS.iter().any(|w| intro.contains(w)) {
        return None;
    }
    pg["thumbnail"]["source"].as_str().filter(|u| u.starts_with("https://")).map(String::from)
}

fn get_json(agent: &ureq::Agent, url: &str, q: &[(&str, &str)]) -> Result<Value, String> {
    let mut req = agent.get(url);
    for (k, v) in q {
        req = req.query(*k, *v);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    match resp.status().as_u16() {
        200 => serde_json::from_str(&resp.body_mut().with_config().limit(2 << 20).read_to_string().map_err(|e| e.to_string())?).map_err(|e| format!("解析失败: {e}")),
        s => Err(format!("HTTP {s}")),
    }
}

fn deezer(agent: &ureq::Agent, artist: &str) -> Result<Option<String>, String> {
    Ok(pick_deezer(&get_json(agent, DEEZER, &[("q", artist), ("limit", "5")])?, artist))
}

fn wiki(agent: &ureq::Agent, artist: &str) -> Result<Option<String>, String> {
    let q = [
        ("action", "query"),
        ("titles", artist),
        ("prop", "pageimages|extracts"),
        ("pithumbsize", "640"),
        ("exintro", "1"),
        ("explaintext", "1"),
        ("exsentences", "2"),
        ("redirects", "1"),
        ("converttitles", "1"),
        ("format", "json"),
    ];
    Ok(pick_wiki(&get_json(agent, WIKI, &q)?))
}

/// 缓存文件：`dir/艺人.jpg`；`艺人.none` = 两个来源都没有（MISS_DAYS 天内不再联网）
fn file(dir: &Path, artist: &str, ext: &str) -> PathBuf {
    dir.join(format!("{}.{ext}", crate::util::sanitize_file_name(&key(artist))))
}

/// 本地缓存：Some(Some(图片)) = 有，Some(None) = 最近查过、没有，None = 没查过
pub fn cached(dir: &Path, artist: &str) -> Option<Option<Vec<u8>>> {
    let artist = clean_artist(artist);
    if artist.is_empty() {
        return Some(None);
    }
    if let Ok(b) = std::fs::read(file(dir, &artist, "jpg")) {
        return Some(Some(b));
    }
    let miss = std::fs::metadata(file(dir, &artist, "none")).and_then(|m| m.modified()).ok()?;
    let age = SystemTime::now().duration_since(miss).unwrap_or_default();
    (age < Duration::from_secs(MISS_DAYS * 86_400)).then_some(None)
}

fn save(dir: &Path, artist: &str, bytes: Option<&[u8]>) {
    let _ = std::fs::create_dir_all(dir);
    let (img, miss) = (file(dir, artist, "jpg"), file(dir, artist, "none"));
    let r = match bytes {
        Some(b) => std::fs::write(&img, b).and_then(|_| std::fs::remove_file(&miss).or(Ok(()))),
        None => std::fs::write(&miss, b""),
    };
    if let Err(e) = r {
        log::warn!("保存艺人图片缓存失败: {e}");
    }
}

/// 找艺人照片（先查本地缓存），找到的存进 `dir`。Ok(None) = 两个来源都确定没有；连不上返回 Err（下次再试）。
/// `manual`（用户手动下载）时不受「最近连不上」的限制
pub fn find(net: &NetworkSettings, dir: &Path, artist: &str, manual: bool) -> Result<Option<Vec<u8>>, String> {
    let artist = clean_artist(artist);
    if artist.is_empty() || artist == "未知艺人" {
        return Ok(None);
    }
    if let Some(c) = cached(dir, &artist) {
        return Ok(c);
    }
    if !manual && now_secs().saturating_sub(LAST_FAIL.load(Ordering::Relaxed)) < BACKOFF_SECS {
        return Err("最近连不上艺人图片服务器，稍后再试".into());
    }
    let agent = crate::lyrics::lrclib::agent(net)?;
    let mut err = None;
    for src in [deezer, wiki] {
        match src(&agent, &artist) {
            Ok(Some(url)) => match super::artwork::download(&agent, &url) {
                Ok(b) if b.len() > 1000 => {
                    save(dir, &artist, Some(&b));
                    return Ok(Some(b));
                }
                Ok(_) => {}
                Err(e) => err = Some(e),
            },
            Ok(None) => {}
            Err(e) => err = Some(e),
        }
    }
    match err {
        // 有一个来源没连上：不能确定没有，下次再试
        Some(e) => {
            LAST_FAIL.store(now_secs(), Ordering::Relaxed);
            Err(e)
        }
        None => {
            save(dir, &artist, None);
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names() {
        assert!(same_name("苏打绿", "蘇打綠"));
        assert!(same_name("One t", "One-T"));
        assert!(same_name("陈珊妮", "陳珊妮"));
        assert!(same_name("黄耀明", "黃耀明"));
        assert!(!same_name("周杰伦", "周华健"));
        assert!(!same_name("卫兰", "周华健"));
        assert!(!same_name("Piffz", "Piff"));
        assert!(!same_name("七八点", "这首歌"));
        assert!(!same_name("", "x"));
    }

    #[test]
    fn deezer_pick() {
        let pic = |id: &str| format!("https://cdn-images.dzcdn.net/images/artist/{id}/1000x1000-000000-80-0-0.jpg");
        let v = json!({"data": [{"name": "Piff", "picture_xl": pic("a")}, {"name": "PiffZ", "picture_xl": pic("b")}]});
        assert_eq!(pick_deezer(&v, "Piffz"), Some(pic("b")));
        // 同名但没有照片：不拿别人的
        let v = json!({"data": [{"name": "周杰伦", "picture_xl": pic("")}, {"name": "周杰伦乐队", "picture_xl": pic("c")}]});
        assert_eq!(pick_deezer(&v, "周杰伦"), None);
        // 繁简不同的第一条
        let v = json!({"data": [{"name": "蘇打綠", "picture_xl": pic("d")}]});
        assert_eq!(pick_deezer(&v, "苏打绿"), Some(pic("d")));
        let v = json!({"data": [{"name": "Piff", "picture_xl": pic("e")}]});
        assert_eq!(pick_deezer(&v, "Piffz"), None);
        // 第一条不是这个人时找后面同名的
        let v = json!({"data": [{"name": "周华健", "picture_xl": pic("f")}, {"name": "周杰倫", "picture_xl": pic("g")}]});
        assert_eq!(pick_deezer(&v, "周杰伦"), Some(pic("g")));
        assert_eq!(pick_deezer(&json!({"data": []}), "x"), None);
    }

    #[test]
    fn wiki_pick() {
        let page = |extract: &str| json!({"query": {"pages": {"1": {"extract": extract, "thumbnail": {"source": "https://upload.wikimedia.org/x.jpg"}}}}});
        assert_eq!(pick_wiki(&page("告五人是台湾的独立乐团。")).as_deref(), Some("https://upload.wikimedia.org/x.jpg"));
        assert_eq!(pick_wiki(&page("福禄寿是中国民间信仰中的三位神仙。")), None);
        assert_eq!(pick_wiki(&json!({"query": {"pages": {"-1": {"missing": ""}}}})), None);
    }

    #[test]
    fn cache_files() {
        let dir = std::env::temp_dir().join(format!("ap3_artist_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(cached(&dir, "谢春花"), None);
        save(&dir, "谢春花", None);
        assert_eq!(cached(&dir, "谢春花"), Some(None));
        // 之后找到了：图片优先，并删掉「没有」的记录
        save(&dir, "谢春花", Some(b"jpeg"));
        assert_eq!(cached(&dir, "谢春花"), Some(Some(b"jpeg".to_vec())));
        assert!(!file(&dir, "谢春花", "none").exists());
        // 繁体标签和简体标签共用缓存；多位艺人只按第一位找；未知艺人不联网
        assert_eq!(cached(&dir, "謝春花"), Some(Some(b"jpeg".to_vec())));
        assert_eq!(cached(&dir, "谢春花/某人"), Some(Some(b"jpeg".to_vec())));
        assert_eq!(find(&NetworkSettings::default(), &dir, "未知艺人", true), Ok(None));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
