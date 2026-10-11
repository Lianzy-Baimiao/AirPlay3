//! 在线歌词：LRCLIB (https://lrclib.net)，免费、无需密钥。
//! 原版的 lyric.airplayme.com 早已停止服务，这里作为替代。只在用户点击"下载歌词"
//! 或开启"启用自动下载"时才会发送 艺人/曲目/专辑/时长 到 LRCLIB。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::config::{NetworkSettings, ProxyKind};

const API: &str = "https://lrclib.net/api";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    #[serde(default)]
    pub track_name: String,
    #[serde(default)]
    pub artist_name: String,
    #[serde(default)]
    pub album_name: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
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

fn agent(net: &NetworkSettings) -> Result<ureq::Agent, String> {
    // 未配置自定义代理时沿用环境变量 / Windows 系统代理（与浏览器一致）
    let proxy = match proxy_url(net) {
        Some(u) => Some(ureq::Proxy::new(&u).map_err(|e| format!("代理设置无效: {e}"))?),
        None => ureq::Proxy::try_from_env(),
    };
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let ua = concat!("AirPlay3-rs/", env!("CARGO_PKG_VERSION"), " (local music player revival)");
    let cfg = ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(proxy)
        .user_agent(ua)
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build();
    Ok(cfg.into())
}

/// 精确查询（需要时长）。没有结果返回 Ok(None)。
pub fn get(net: &NetworkSettings, title: &str, artist: &str, album: &str, duration: f64) -> Result<Option<Record>, String> {
    let mut req = agent(net)?.get(format!("{API}/get")).query("track_name", title).query("artist_name", artist);
    if !album.is_empty() {
        req = req.query("album_name", album);
    }
    let mut resp = req.query("duration", format!("{}", duration.round() as i64)).call().map_err(|e| e.to_string())?;
    match resp.status().as_u16() {
        200 => resp.body_mut().read_json::<Record>().map(Some).map_err(|e| e.to_string()),
        404 => Ok(None),
        s => Err(format!("LRCLIB 返回 HTTP {s}")),
    }
}

/// 关键字搜索。
pub fn search(net: &NetworkSettings, title: &str, artist: &str) -> Result<Vec<Record>, String> {
    let mut req = agent(net)?.get(format!("{API}/search")).query("track_name", title);
    if !artist.is_empty() {
        req = req.query("artist_name", artist);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    match resp.status().as_u16() {
        200 => resp.body_mut().read_json::<Vec<Record>>().map_err(|e| e.to_string()),
        s => Err(format!("LRCLIB 返回 HTTP {s}")),
    }
}

/// 自动下载：先精确查询，再搜索取最匹配（有时间轴、时长接近）的一条。
pub fn best_match(net: &NetworkSettings, title: &str, artist: &str, album: &str, duration: f64) -> Result<Option<Record>, String> {
    if duration > 0.0 {
        if let Some(r) = get(net, title, artist, album, duration)? {
            if r.has_synced() || r.plain_lyrics.is_some() {
                return Ok(Some(r));
            }
        }
    }
    let mut list = search(net, title, artist)?;
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
