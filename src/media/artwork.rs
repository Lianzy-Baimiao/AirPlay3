//! 在线专辑图片。原版的图片服务早已关闭，这里作为替代，两个来源都免费、无需密钥：
//! 1. LrcAPI（与歌词同一个来源，见 `lyrics::online`）：图片多来自 Apple Music，中文歌是简体标签，容易对上；
//! 2. LrcAPI 没有对得上的结果时，用 iTunes Search API。
//! 只在用户点「下载专辑图片」或开启「启用自动下载歌词图片 · 下载专辑图片」时才发送 标题 / 艺人。
//!
//! iTunes 中国区商店没有音乐数据；台湾 / 香港区中文歌最全，但返回的是繁体（周杰倫），与本地简体标签对不上，
//! 所以主要按时长匹配（前几条结果里时长相差 4 秒以内的），时长未知时要求标题、艺人一致。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::NetworkSettings;
use crate::lyrics::query::{clean_artist, clean_title};
use crate::util::text::{contains_cjk, normalize_key};

const API: &str = "https://itunes.apple.com/search";
/// 图片最大 8 MB
const MAX_BYTES: u64 = 8 << 20;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hit {
    #[serde(default)]
    track_name: String,
    #[serde(default)]
    artist_name: String,
    #[serde(default)]
    artwork_url100: String,
    #[serde(default)]
    track_time_millis: Option<f64>,
}

#[derive(Deserialize)]
struct Resp {
    #[serde(default)]
    results: Vec<Hit>,
}

/// 在搜索结果里选一条：时长已知时取时长最接近的（相差 4 秒内）；否则要求标题一致、艺人互相包含。
/// `loose`（用户手动下载）时都不满足就取第一条。
fn pick<'a>(hits: &'a [Hit], title: &str, artist: &str, duration: f64, loose: bool) -> Option<&'a Hit> {
    let hits: Vec<&Hit> = hits.iter().filter(|h| !h.artwork_url100.is_empty()).take(8).collect();
    if duration > 0.0 {
        let near = hits
            .iter()
            .filter_map(|h| h.track_time_millis.map(|ms| ((ms / 1000.0 - duration).abs(), *h)))
            .filter(|(d, _)| *d <= 4.0)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, h)) = near {
            return Some(h);
        }
    }
    let (t, a) = (normalize_key(title), normalize_key(artist));
    let named = hits.iter().find(|h| {
        let (ht, ha) = (normalize_key(&h.track_name), normalize_key(&h.artist_name));
        !t.is_empty() && ht == t && (a.is_empty() || ha.contains(&a) || a.contains(&ha))
    });
    named.copied().or(if loose { hits.first().copied() } else { None })
}

fn countries(title: &str) -> &'static [&'static str] {
    if contains_cjk(title) { &["TW", "HK", "US"] } else { &["US", "TW"] }
}

/// 100x100 的缩略图地址换成 600x600
fn big_url(u: &str) -> String {
    u.replace("100x100bb", "600x600bb")
}

fn download(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let mut img = agent.get(url).call().map_err(|e| e.to_string())?;
    if img.status().as_u16() != 200 {
        return Err(format!("下载图片失败：HTTP {}", img.status().as_u16()));
    }
    img.body_mut().with_config().limit(MAX_BYTES).read_to_vec().map_err(|e| e.to_string())
}

/// LrcAPI 的图片候选，按 iTunes 的规则挑（不放宽：对不上就交给 iTunes）
fn from_lrcapi(net: &NetworkSettings, agent: &ureq::Agent, title: &str, artist: &str, duration: f64) -> Result<Option<Vec<u8>>, String> {
    let hits: Vec<Hit> = crate::lyrics::online::covers(net, title, artist)?
        .into_iter()
        .map(|c| Hit { track_name: c.title, artist_name: c.artist, artwork_url100: c.cover, track_time_millis: (c.duration > 0.0).then_some(c.duration * 1000.0) })
        .collect();
    match pick(&hits, title, artist, duration, false) {
        Some(h) => download(agent, &h.artwork_url100).map(Some),
        None => Ok(None),
    }
}

/// 查找并下载专辑图片，返回图片字节。没有找到返回 Ok(None)。
pub fn find(net: &NetworkSettings, title: &str, artist: &str, duration: f64, loose: bool) -> Result<Option<Vec<u8>>, String> {
    let (title, artist) = (clean_title(title), clean_artist(artist));
    if title.is_empty() {
        return Ok(None);
    }
    let agent = crate::lyrics::lrclib::agent(net)?;
    match from_lrcapi(net, &agent, &title, &artist, duration) {
        Ok(Some(bytes)) => return Ok(Some(bytes)),
        Ok(None) => {}
        Err(e) => log::info!("LrcAPI 图片搜索失败，改用 iTunes: {e}"),
    }
    let term = if artist.is_empty() { title.clone() } else { format!("{title} {artist}") };
    for c in countries(&title) {
        let mut resp = agent
            .get(API)
            .query("term", term.as_str())
            .query("media", "music")
            .query("entity", "song")
            .query("limit", "10")
            .query("country", *c)
            .call()
            .map_err(|e| e.to_string())?;
        if resp.status().as_u16() != 200 {
            return Err(format!("iTunes 返回 HTTP {}", resp.status().as_u16()));
        }
        let r: Resp = resp.body_mut().read_json().map_err(|e| e.to_string())?;
        if let Some(h) = pick(&r.results, &title, &artist, duration, loose) {
            return download(&agent, &big_url(&h.artwork_url100)).map(Some);
        }
    }
    Ok(None)
}

/// 保存到 `dir/艺人 - 专辑.jpg`（没有专辑时用曲目名），返回文件路径
pub fn save(bytes: &[u8], dir: &Path, artist: &str, album: &str, title: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let name = if album.trim().is_empty() { title } else { album };
    let stem = if artist.trim().is_empty() {
        crate::util::sanitize_file_name(name)
    } else {
        format!("{} - {}", crate::util::sanitize_file_name(artist), crate::util::sanitize_file_name(name))
    };
    let ext = if bytes.starts_with(b"\x89PNG") { "png" } else { "jpg" };
    let path = dir.join(format!("{stem}.{ext}"));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(t: &str, a: &str, secs: f64) -> Hit {
        Hit { track_name: t.into(), artist_name: a.into(), artwork_url100: "https://x/100x100bb.jpg".into(), track_time_millis: Some(secs * 1000.0) }
    }

    #[test]
    fn picks_by_duration_even_in_traditional() {
        let hits = [hit("晴天 (Live)", "周杰倫", 249.7), hit("晴天", "周杰倫", 269.7)];
        assert_eq!(pick(&hits, "晴天", "周杰伦", 269.0, false).map(|h| h.track_name.as_str()), Some("晴天"));
        // 时长都对不上、标题是繁体：自动下载不取，手动下载取第一条
        assert!(pick(&hits, "体面", "于文文", 100.0, false).is_none());
        assert_eq!(pick(&hits, "体面", "于文文", 100.0, true).map(|h| h.track_name.as_str()), Some("晴天 (Live)"));
    }

    #[test]
    fn picks_by_name_without_duration() {
        let hits = [hit("Someone Like You", "Adele", 285.0), hit("We Don't Talk Anymore (feat. Selena Gomez)", "Charlie Puth", 217.7)];
        assert_eq!(pick(&hits, "someone like you", "adele", 0.0, false).map(|h| h.artist_name.as_str()), Some("Adele"));
        assert!(pick(&hits, "Hello", "Adele", 0.0, false).is_none());
        assert_eq!(big_url("https://a/b/100x100bb.jpg"), "https://a/b/600x600bb.jpg");
        assert_eq!(countries("晴天"), &["TW", "HK", "US"]);
    }
}
