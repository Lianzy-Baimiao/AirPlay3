//! 在线歌词：LRCLIB (https://lrclib.net)，免费、无需密钥。
//! 原版的 lyric.airplayme.com 早已停止服务，这里作为替代。只在用户点击"下载歌词"
//! 或开启"启用自动下载"时才会发送 艺人/曲目/专辑/时长 到 LRCLIB。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::config::{NetworkSettings, ProxyKind};

const API: &str = "https://lrclib.net/api";

/// 字段缺失或为 null 都取默认值（`#[serde(default)]` 只管缺失；LRCLIB 有的记录 duration 是 null，会让整次搜索解析失败）
fn or_default<'de, D: serde::Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    #[serde(default, deserialize_with = "or_default")]
    pub track_name: String,
    #[serde(default, deserialize_with = "or_default")]
    pub artist_name: String,
    #[serde(default, deserialize_with = "or_default")]
    pub album_name: String,
    #[serde(default, deserialize_with = "or_default")]
    pub duration: f64,
    #[serde(default, deserialize_with = "or_default")]
    pub instrumental: bool,
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
}

impl Record {
    pub fn has_synced(&self) -> bool {
        self.synced_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty())
    }
}

fn proxy_url(n: &NetworkSettings) -> Option<String> {
    if !n.proxy_enabled || n.host.trim().is_empty() {
        return None;
    }
    let scheme = match n.kind {
        ProxyKind::Http => "http",
        ProxyKind::Socks4 => "socks4",
        ProxyKind::Socks5 => "socks5",
    };
    let auth = if n.user.is_empty() { String::new() } else { format!("{}:{}@", pct(&n.user), pct(&n.pass)) };
    Some(format!("{scheme}://{auth}{}:{}", n.host.trim(), n.port))
}

fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// 联网用的 HTTP 客户端（代理、TLS、超时与界面设置一致），专辑图片下载也用它
pub fn agent(net: &NetworkSettings) -> Result<ureq::Agent, String> {
    agent_with(net, Some(Duration::from_secs(15)))
}

/// 同上；`total` = 整个请求的超时（None：只限制连接和等响应头的时间，用于一直不结束的网络电台）
pub fn agent_with(net: &NetworkSettings, total: Option<Duration>) -> Result<ureq::Agent, String> {
    // 未配置自定义代理时沿用环境变量 / Windows 系统代理（与浏览器一致）
    let proxy = match proxy_url(net) {
        Some(u) => Some(ureq::Proxy::new(&u).map_err(|e| format!("代理设置无效: {e}"))?),
        None => ureq::Proxy::try_from_env(),
    };
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    // 带上项目地址：维基百科等接口要求 User-Agent 能联系到作者
    let ua = concat!("AirPlay3-rs/", env!("CARGO_PKG_VERSION"), " (local music player revival; https://github.com/Lianzy-Baimiao/AirPlay3)");
    let cfg = ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(proxy)
        .user_agent(ua)
        .timeout_global(total)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build();
    Ok(cfg.into())
}

/// LRCLIB 过载时会偶发返回 503 ServerOverloaded（与查询内容无关，同一请求重发即可成功），这类状态码自动重试
fn is_busy(status: u16) -> bool {
    matches!(status, 429 | 502 | 503 | 504)
}

/// 两次重试前的等待（毫秒）；服务器给了 Retry-After 时以它为准（最多等 3 秒）
const RETRY_WAITS_MS: [u64; 2] = [800, 1600];

fn retry_wait_ms(retry_after: Option<&str>, attempt: usize) -> u64 {
    retry_after.and_then(|v| v.trim().parse::<u64>().ok()).map_or(RETRY_WAITS_MS[attempt], |s| (s * 1000).min(3000))
}

fn status_error(status: u16) -> String {
    if is_busy(status) || status >= 500 { "LRCLIB 服务器繁忙，请稍后再试".into() } else { format!("LRCLIB 返回 HTTP {status}") }
}

/// GET `API/path?params`，服务器繁忙时重试，返回最后一次的状态码与响应
fn call(net: &NetworkSettings, path: &str, params: &[(&str, String)]) -> Result<(u16, ureq::http::Response<ureq::Body>), String> {
    let agent = agent(net)?;
    let mut attempt = 0;
    loop {
        let mut req = agent.get(format!("{API}/{path}"));
        for (k, v) in params {
            req = req.query(*k, v.as_str());
        }
        let resp = req.call().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        if !is_busy(status) || attempt >= RETRY_WAITS_MS.len() {
            return Ok((status, resp));
        }
        let wait = retry_wait_ms(resp.headers().get("retry-after").and_then(|v| v.to_str().ok()), attempt);
        log::info!("LRCLIB 返回 HTTP {status}，{wait} 毫秒后重试");
        std::thread::sleep(Duration::from_millis(wait));
        attempt += 1;
    }
}

/// 精确查询（需要时长）。没有结果返回 Ok(None)。
pub fn get(net: &NetworkSettings, title: &str, artist: &str, album: &str, duration: f64) -> Result<Option<Record>, String> {
    let mut p = vec![("track_name", title.to_string()), ("artist_name", artist.to_string())];
    if !album.is_empty() {
        p.push(("album_name", album.to_string()));
    }
    p.push(("duration", format!("{}", duration.round() as i64)));
    let (status, mut resp) = call(net, "get", &p)?;
    match status {
        200 => resp.body_mut().read_json::<Record>().map(Some).map_err(|e| e.to_string()),
        404 => Ok(None),
        s => Err(status_error(s)),
    }
}

/// 关键字搜索。
pub fn search(net: &NetworkSettings, title: &str, artist: &str) -> Result<Vec<Record>, String> {
    let mut p = vec![("track_name", title.to_string())];
    if !artist.is_empty() {
        p.push(("artist_name", artist.to_string()));
    }
    let (status, mut resp) = call(net, "search", &p)?;
    match status {
        200 => resp.body_mut().read_json::<Vec<Record>>().map_err(|e| e.to_string()),
        s => Err(status_error(s)),
    }
}

/// 搜索窗口用：先按 标题 + 艺人 搜，没有结果再只按标题搜（艺人标签常常是上传者，例如「起点音乐」）。
/// 返回 (结果, 是否放宽为只按标题)。
pub fn search_loose(net: &NetworkSettings, title: &str, artist: &str) -> Result<(Vec<Record>, bool), String> {
    let v = search(net, title, artist)?;
    if !v.is_empty() || artist.trim().is_empty() {
        return Ok((v, false));
    }
    Ok((search(net, title, "")?, true))
}

/// 自动下载：先精确查询，再搜索取最匹配（有时间轴、时长接近）的一条。
/// 标题 / 艺人先清理（去掉 `(Live)`、`【六倍音质】`、多位艺人等），不放宽艺人，避免自动下错歌词。
pub fn best_match(net: &NetworkSettings, title: &str, artist: &str, album: &str, duration: f64) -> Result<Option<Record>, String> {
    use super::query::{clean_artist, clean_title};
    let (title, artist, album) = (clean_title(title), clean_artist(artist), clean_title(album));
    if duration > 0.0 {
        // 精确查询失败（含服务器繁忙）不影响后面的搜索
        match get(net, &title, &artist, &album, duration) {
            Ok(Some(r)) if r.has_synced() || r.plain_lyrics.is_some() => return Ok(Some(r)),
            Ok(_) => {}
            Err(e) => log::info!("LRCLIB 精确查询失败: {e}"),
        }
    }
    let mut list = search(net, &title, &artist)?;
    list.retain(|r| !r.instrumental && (r.has_synced() || r.plain_lyrics.is_some()));
    let score = |r: &Record| {
        let dd = if duration > 0.0 { (r.duration - duration).abs() } else { 0.0 };
        (if r.has_synced() { 0.0 } else { 100.0 }) + dd
    };
    list.sort_by(|a, b| score(a).total_cmp(&score(b)));
    Ok(list.into_iter().find(|r| duration <= 0.0 || (r.duration - duration).abs() < 10.0))
}

/// 保存为 `艺人 - 曲目.lrc`，返回文件路径。
pub fn save(rec: &Record, dir: &Path, title: &str, artist: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let name = if artist.is_empty() {
        format!("{}.lrc", crate::util::sanitize_file_name(title))
    } else {
        format!("{} - {}.lrc", crate::util::sanitize_file_name(artist), crate::util::sanitize_file_name(title))
    };
    let path = dir.join(name);
    let body = rec.synced_lyrics.as_deref().filter(|s| !s.trim().is_empty()).or(rec.plain_lyrics.as_deref()).unwrap_or("");
    let text = format!("[ti:{}]\n[ar:{}]\n[al:{}]\n[re:LRCLIB]\n{}\n", rec.track_name, rec.artist_name, rec.album_name, body);
    std::fs::write(&path, text.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_fields() {
        let v: Vec<Record> = serde_json::from_str(r#"[{"trackName":"冬眠","artistName":null,"duration":null,"instrumental":null,"syncedLyrics":"[00:01.00]a"},{"trackName":"x","duration":196.2}]"#).unwrap();
        assert_eq!((v[0].duration, v[0].artist_name.as_str(), v[0].instrumental), (0.0, "", false));
        assert_eq!(v[1].duration, 196.2);
    }

    #[test]
    fn busy_retry_policy() {
        assert!(is_busy(503) && is_busy(429) && is_busy(502) && is_busy(504));
        assert!(!is_busy(200) && !is_busy(404) && !is_busy(400));
        assert_eq!(retry_wait_ms(None, 0), 800);
        assert_eq!(retry_wait_ms(None, 1), 1600);
        assert_eq!(retry_wait_ms(Some("2"), 0), 2000);
        assert_eq!(retry_wait_ms(Some("120"), 1), 3000);
        assert_eq!(retry_wait_ms(Some("abc"), 1), 1600);
        assert_eq!(status_error(503), "LRCLIB 服务器繁忙，请稍后再试");
        assert_eq!(status_error(403), "LRCLIB 返回 HTTP 403");
    }

    #[test]
    fn proxy_urls() {
        let mut n = NetworkSettings { proxy_enabled: true, host: "127.0.0.1".into(), port: 7890, ..Default::default() };
        assert_eq!(proxy_url(&n).as_deref(), Some("http://127.0.0.1:7890"));
        n.kind = ProxyKind::Socks5;
        n.user = "u@x".into();
        n.pass = "p:w".into();
        assert_eq!(proxy_url(&n).as_deref(), Some("socks5://u%40x:p%3Aw@127.0.0.1:7890"));
        n.proxy_enabled = false;
        assert_eq!(proxy_url(&n), None);
    }
}
