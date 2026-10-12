//! 在线歌词来源：两个专门的歌词 API，都免费、无需密钥。原版的 lyric.airplayme.com 早已停止服务。
//! - LrcAPI（<https://docs.lrc.cx>，开源项目 HisAtri/LrcApi 的公开实例 api.lrc.cx）：中文歌多，同时提供歌词和专辑图片。
//!   它自己说明公开实例的数据来自酷狗等接口（实测结果里有 Apple Music、网易云的歌词），可能较慢、不完全准确。
//! - LRCLIB（<https://lrclib.net>）：社区维护的开放歌词库，欧美歌多，中文歌少。
//!
//! 在本机曲库里随机抽 24 首，用 `best_match` 实测（2026-10-12）：LrcAPI 18 首、LRCLIB 7 首找到时长对得上的同步歌词；
//! 没找到的 6 首都是上传者翻唱 / 伴奏版，两个来源里没有时长相差 5 秒内的同步歌词（宁可不下载，也不配错版本）。
//! 「自动」同时搜索两个来源；标题一致时，时长对得上的优先，再按 LrcAPI → LRCLIB 的顺序。
//! 只在用户下载歌词 / 图片或开启自动下载时，才把 标题 / 艺人 / 时长 发给所选来源。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::NetworkSettings;
use crate::util::text::normalize_key;

/// 歌词来源（设置里可选；Auto = 全部来源，按可用性排序）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
pub enum Provider {
    #[default]
    Auto,
    LrcApi,
    Lrclib,
}

/// 配置里是不认识的值（例如以后改名 / 删掉的来源）时按「自动」处理，不让整个配置文件解析失败
impl<'de> Deserialize<'de> for Provider {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::ALL.into_iter().find(|p| p.key() == s).unwrap_or_default())
    }
}

impl Provider {
    /// 「自动」时的顺序（实测命中率从高到低）
    pub const ORDER: [Provider; 2] = [Provider::LrcApi, Provider::Lrclib];
    pub const ALL: [Provider; 3] = [Provider::Auto, Provider::LrcApi, Provider::Lrclib];

    /// 写进配置文件的名字（与 Serialize 一致）
    fn key(self) -> &'static str {
        match self {
            Provider::Auto => "Auto",
            Provider::LrcApi => "LrcApi",
            Provider::Lrclib => "Lrclib",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Auto => "自动（LrcAPI + LRCLIB，推荐）",
            Provider::LrcApi => "LrcAPI",
            Provider::Lrclib => "LRCLIB",
        }
    }

    /// 搜索结果列表里的短名
    pub fn tag(self) -> &'static str {
        match self {
            Provider::Auto => "",
            Provider::LrcApi => "LrcAPI",
            Provider::Lrclib => "LRCLIB",
        }
    }

    fn rank(self) -> usize {
        Self::ORDER.iter().position(|p| *p == self).unwrap_or(9)
    }

    /// 实际要搜索的来源
    pub fn targets(self) -> Vec<Provider> {
        if self == Provider::Auto { Self::ORDER.to_vec() } else { vec![self] }
    }
}

/// 一条搜索结果；两个来源都在搜索时就带回歌词
#[derive(Clone, Debug)]
pub struct Candidate {
    pub provider: Provider,
    /// 来源内部的 id
    pub key: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// 秒；0 = 未知
    pub duration: f64,
    pub lyrics: Option<String>,
    /// 专辑图片地址（600x600 左右；只有 LrcAPI 提供）
    pub cover: String,
}

const LRCAPI: &str = "https://api.lrc.cx/jsonapi";
const MAX_BODY: u64 = 4 << 20;

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// LrcAPI 搜索（/jsonapi）。一条结果同时带歌词和专辑图片；搜不到时返回 404。
fn search_lrcapi(net: &NetworkSettings, title: &str, artist: &str) -> Result<Vec<Candidate>, String> {
    let agent = super::lrclib::agent(net)?;
    let mut resp = agent
        .get(LRCAPI)
        .query("title", title)
        .query("artist", artist)
        .query("album", "")
        .query("path", "None")
        .query("limit", "10")
        .call()
        .map_err(|e| e.to_string())?;
    let v: Value = match resp.status().as_u16() {
        200 => serde_json::from_str(&resp.body_mut().with_config().limit(MAX_BODY).read_to_string().map_err(|e| e.to_string())?).map_err(|e| format!("解析失败: {e}"))?,
        404 => return Ok(Vec::new()),
        st => return Err(format!("HTTP {st}")),
    };
    Ok(v.as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|x| Candidate {
            provider: Provider::LrcApi,
            key: s(&x["id"]),
            title: s(&x["title"]),
            artist: s(&x["artist"]),
            album: s(&x["album"]),
            duration: seconds(&x["duration"]),
            lyrics: Some(strip_sections(&s(&x["lrc"]))).filter(|t| !t.trim().is_empty()),
            cover: cover_url(&s(&x["cover"])),
        })
        .filter(|c| !c.title.is_empty())
        .collect())
}

/// 时长：数字或数字字符串（秒），缺失为 0
fn seconds(v: &Value) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())).unwrap_or(0.0)
}

/// 图片地址换成 600x600：Apple 的 …/4000x4000bb.jpg 换最后一段，网易云加 ?param=600y600（并改用 https）
fn cover_url(u: &str) -> String {
    if u.contains("mzstatic.com/image/thumb/") {
        if let Some((base, last)) = u.rsplit_once('/') {
            if last.contains("bb.") {
                return format!("{base}/600x600bb.jpg");
            }
        }
    } else if u.contains(".music.126.net/") && !u.contains('?') {
        return format!("{}?param=600y600", u.replacen("http://", "https://", 1));
    }
    u.to_string()
}

/// 去掉 Apple Music 歌词里单独一行的段落标记（`[Verse]`、`[Chorus]`），保留时间标签和 `[ti:…]` 这类元数据
fn strip_sections(t: &str) -> String {
    t.lines()
        .filter(|l| {
            let l = l.trim();
            !(l.starts_with('[') && l.ends_with(']') && !l.contains(':') && l.len() > 2)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn search_lrclib(net: &NetworkSettings, title: &str, artist: &str) -> Result<Vec<Candidate>, String> {
    Ok(super::lrclib::search(net, title, artist)?
        .into_iter()
        .filter(|r| !r.instrumental)
        .map(|r| {
            let lyrics = r.synced_lyrics.clone().filter(|t| !t.trim().is_empty()).or(r.plain_lyrics.clone());
            Candidate { provider: Provider::Lrclib, key: String::new(), title: r.track_name, artist: r.artist_name, album: r.album_name, duration: r.duration, lyrics, cover: String::new() }
        })
        .collect())
}

fn search_one(net: &NetworkSettings, p: Provider, title: &str, artist: &str) -> Result<Vec<Candidate>, String> {
    match p {
        Provider::LrcApi => search_lrcapi(net, title, artist),
        Provider::Lrclib => search_lrclib(net, title, artist),
        Provider::Auto => Ok(Vec::new()),
    }
}

/// 一条结果的歌词文本（LRC）。两个来源都在搜索时带回歌词；没有歌词的结果（纯音乐、只有图片）返回错误
pub fn fetch(_net: &NetworkSettings, c: &Candidate) -> Result<String, String> {
    match &c.lyrics {
        Some(t) if !t.trim().is_empty() => Ok(t.clone()),
        _ => Err("这首歌没有歌词（可能是纯音乐）".into()),
    }
}

/// 专辑图片候选（LrcAPI）：标题 / 艺人 / 时长 / 图片地址
pub fn covers(net: &NetworkSettings, title: &str, artist: &str) -> Result<Vec<Candidate>, String> {
    Ok(search_lrcapi(net, title, artist)?.into_iter().filter(|c| !c.cover.is_empty()).collect())
}

/// 同时搜索多个来源，合并结果。全部来源都失败时返回第一个错误。
pub fn search(net: &NetworkSettings, providers: &[Provider], title: &str, artist: &str) -> Result<Vec<Candidate>, String> {
    let results: Vec<(Provider, Result<Vec<Candidate>, String>)> = std::thread::scope(|sc| {
        let hs: Vec<_> = providers.iter().map(|&p| (p, sc.spawn(move || search_one(net, p, title, artist)))).collect();
        hs.into_iter().map(|(p, h)| (p, h.join().unwrap_or_else(|_| Err("内部错误".into())))).collect()
    });
    let mut out = Vec::new();
    let mut first_err = None;
    let mut any_ok = false;
    for (p, r) in results {
        match r {
            Ok(v) => {
                any_ok = true;
                // 歌词搜索只列出有歌词的结果（LrcAPI 偶尔有只带图片的条目）
                out.extend(v.into_iter().filter(|c| c.lyrics.is_some()));
            }
            Err(e) => {
                log::info!("{} 搜索失败: {e}", p.label());
                first_err.get_or_insert(format!("{}：{e}", p.label()));
            }
        }
    }
    if !any_ok {
        return Err(first_err.unwrap_or_else(|| "没有可用的歌词来源".into()));
    }
    Ok(out)
}

/// 有时间轴：至少 5 行以 `[数字…:` 开头
pub fn is_synced(t: &str) -> bool {
    t.lines()
        .filter(|l| {
            let l = l.trim_start();
            l.strip_prefix('[').is_some_and(|r| {
                let d = r.chars().take_while(char::is_ascii_digit).count();
                d > 0 && r[d..].starts_with(':')
            })
        })
        .count()
        >= 5
}

/// 时长相差 5 秒内；时长未知时标题一致或互相包含
fn plausible(c: &Candidate, title: &str, duration: f64) -> bool {
    if duration > 0.0 && c.duration > 0.0 {
        return (c.duration - duration).abs() <= 5.0;
    }
    let (a, b) = (normalize_key(&c.title), normalize_key(title));
    !b.is_empty() && (a == b || a.contains(&b) || b.contains(&a))
}

/// 标题接近程度：0 = 完全一致，1 = 去掉括号说明后一致（原版优先于 Live / remix），2 = 其它
fn title_rank(c: &Candidate, title: &str) -> u8 {
    let t = normalize_key(title);
    if normalize_key(&c.title) == t {
        0
    } else if normalize_key(&super::query::clean_title(&c.title)) == t {
        1
    } else {
        2
    }
}

/// 搜索窗口里的顺序：标题一致（含去掉 (Live) 等说明后一致）的在前，再看艺人是否对得上，
/// 然后按时长接近程度（3 秒 / 10 秒 / 更远分档）、来源顺序。
/// LrcAPI 是模糊搜索，会混进标题相近的别的歌；只按时长排会把它们排到真正的结果前面。
pub fn sort_for_list(v: &mut [Candidate], title: &str, artist: &str, duration: f64) {
    let a = normalize_key(artist);
    let key = |c: &Candidate| {
        let ca = normalize_key(&c.artist);
        let artist_ok = a.is_empty() || (!ca.is_empty() && (ca.contains(&a) || a.contains(&ca)));
        let d = if duration > 0.0 && c.duration > 0.0 { (c.duration - duration).abs() } else { 999.0 };
        let band = if d <= 3.0 { 0 } else if d <= 10.0 { 1 } else { 2 };
        // 同名歌里，艺人对得上的（哪怕是 Live 版）排在别人翻唱的前面
        let tr = title_rank(c, title);
        (tr == 2, !artist_ok, tr, band, c.provider.rank(), (d * 10.0) as i64)
    };
    v.sort_by_key(key);
}

/// 自动下载：在所选来源里找时长 / 标题对得上的结果，按来源顺序下载，返回第一份带时间轴的歌词。
/// 标题 / 艺人先清理（去掉 (Live)、【六倍音质】、多位艺人等）。
pub fn best_match(net: &NetworkSettings, source: Provider, title: &str, artist: &str, duration: f64) -> Result<Option<(Candidate, String)>, String> {
    use super::query::{clean_artist, clean_title};
    let (title, artist) = (clean_title(title), clean_artist(artist));
    let targets = source.targets();
    let usable = |c: &Candidate| plausible(c, &title, duration) && c.lyrics.as_deref().is_some_and(is_synced);
    let mut v: Vec<Candidate> = search(net, &targets, &title, &artist)?.into_iter().filter(usable).collect();
    // 艺人常是上传者（如「起点音乐」）：带艺人搜不到时只按标题再搜一次（靠时长 / 标题把关）
    if v.is_empty() && !artist.is_empty() {
        v = search(net, &targets, &title, "")?.into_iter().filter(usable).collect();
    }
    // 标题一致的优先；同样一致时，时长对得上的优先于时长未知的，再按来源顺序
    v.sort_by_key(|c| {
        let gap = dur_gap(c, duration);
        (title_rank(c, &title), gap == 9_999, c.provider.rank(), gap)
    });
    Ok(v.into_iter().next().and_then(|c| {
        let t = fetch(net, &c).ok()?;
        Some((c, t))
    }))
}

/// 时长差（0.1 秒为单位）；任一方时长未知时排在已知的后面
fn dur_gap(c: &Candidate, duration: f64) -> i64 {
    if duration > 0.0 && c.duration > 0.0 { ((c.duration - duration).abs() * 10.0) as i64 } else { 9_999 }
}

/// 保存为 `艺人 - 曲目.lrc`（本地标签，与本地查找规则一致），返回文件路径
pub fn save(text: &str, c: &Candidate, dir: &std::path::Path, title: &str, artist: &str) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let name = if artist.is_empty() {
        format!("{}.lrc", crate::util::sanitize_file_name(title))
    } else {
        format!("{} - {}.lrc", crate::util::sanitize_file_name(artist), crate::util::sanitize_file_name(title))
    };
    let path = dir.join(name);
    let head = if text.contains("[ti:") { String::new() } else { format!("[ti:{}]\n[ar:{}]\n[al:{}]\n", c.title, c.artist, c.album) };
    std::fs::write(&path, format!("{head}[re:{}]\n{}\n", c.provider.label(), text.trim_end()).as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(p: Provider, t: &str, d: f64) -> Candidate {
        Candidate { provider: p, key: "k".into(), title: t.into(), artist: "a".into(), album: String::new(), duration: d, lyrics: None, cover: String::new() }
    }

    #[test]
    fn helpers() {
        let lrc = "[ti:x]\n[00:01.00]a\n[00:02.00]b\n[00:03.00]c\n[00:04.00]d\n[00:05.00]e\n";
        assert!(is_synced(lrc));
        assert!(!is_synced("[ti:x]\n纯文本歌词\n第二行"));
        assert_eq!(strip_sections("[Verse]\n[00:21.168]简单点\n[ti:演员]\n[Chorus]\n[]"), "[00:21.168]简单点\n[ti:演员]\n[]");
        assert_eq!(seconds(&serde_json::json!(250.491)), 250.491);
        assert_eq!(seconds(&serde_json::json!("261.25")), 261.25);
        assert_eq!(seconds(&Value::Null), 0.0);
        assert_eq!(
            cover_url("https://is1-ssl.mzstatic.com/image/thumb/Music211/v4/0d/e3/x.jpg/4000x4000bb.jpg"),
            "https://is1-ssl.mzstatic.com/image/thumb/Music211/v4/0d/e3/x.jpg/600x600bb.jpg"
        );
        assert_eq!(cover_url("http://p2.music.126.net/abc==/109.jpg"), "https://p2.music.126.net/abc==/109.jpg?param=600y600");
        assert_eq!(cover_url("https://example.com/a.jpg"), "https://example.com/a.jpg");
    }

    #[test]
    fn config_value() {
        let p: Provider = serde_json::from_str("\"LrcApi\"").unwrap();
        assert_eq!(p, Provider::LrcApi);
        // 以前的 / 不认识的值按「自动」处理
        let p: Provider = serde_json::from_str("\"Qq\"").unwrap();
        assert_eq!(p, Provider::Auto);
        for p in Provider::ALL {
            assert_eq!(serde_json::from_str::<Provider>(&serde_json::to_string(&p).unwrap()).unwrap(), p);
        }
    }

    #[test]
    fn matching_and_order() {
        assert!(plausible(&c(Provider::LrcApi, "怪咖", 250.0), "怪咖", 252.0));
        assert!(!plausible(&c(Provider::LrcApi, "怪咖", 30.0), "怪咖", 250.0));
        assert!(plausible(&c(Provider::LrcApi, "怪咖 (Live)", 0.0), "怪咖", 0.0));
        let mut v = vec![c(Provider::Lrclib, "怪咖", 250.0), c(Provider::LrcApi, "别的歌", 250.0), c(Provider::LrcApi, "怪咖", 251.0), c(Provider::LrcApi, "怪咖", 300.0)];
        sort_for_list(&mut v, "怪咖", "a", 250.0);
        let order: Vec<_> = v.iter().map(|x| (x.provider, x.duration as i64)).collect();
        assert_eq!(order, vec![(Provider::LrcApi, 251), (Provider::Lrclib, 250), (Provider::LrcApi, 300), (Provider::LrcApi, 250)]);
        // LrcAPI 模糊搜索混进来的别的歌（时长更接近）、别人翻唱的同名歌，都排在本人的 Live 版后面
        let a = |p, t: &str, ar: &str, d| Candidate { artist: ar.into(), ..c(p, t, d) };
        let mut v = vec![
            a(Provider::LrcApi, "亲爱的你在不在", "王羿", 328.0),
            a(Provider::LrcApi, "亲爱的你的棉裤真的不在我家", "翻唱", 200.0),
            a(Provider::LrcApi, "亲爱的你的棉裤真的不在我家 (Live 深圳, 2018)", "齐一", 319.0),
        ];
        sort_for_list(&mut v, "亲爱的你的棉裤真的不在我家", "齐一", 328.0);
        let order: Vec<_> = v.iter().map(|x| x.artist.as_str()).collect();
        assert_eq!(order, vec!["齐一", "翻唱", "王羿"]);
        assert_eq!(Provider::Auto.targets(), Provider::ORDER.to_vec());
        assert_eq!(Provider::Lrclib.targets(), vec![Provider::Lrclib]);
        assert_eq!(dur_gap(&c(Provider::LrcApi, "x", 0.0), 250.0), 9_999);
    }
}
