//! 网络电台的下载线程：普通 HTTP 流（Icecast / Shoutcast，去掉 ICY 元数据并取出当前节目名），
//! 或 HLS（m3u8：选一路码流，按目标时长轮询播放列表，按顺序下载新的分段）。数据通过通道交给解码线程。

use std::io::Read;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use ureq::ResponseExt;

use crate::config::NetworkSettings;

/// 码流的封装（决定解码前要不要先解 TS）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    /// 直接是 ADTS / MP3 帧
    Raw,
    /// MPEG-TS
    Ts,
}

pub enum Msg {
    Data(Wrap, Vec<u8>),
    /// ICY 元数据里的节目名（StreamTitle）
    Title(String),
    /// 码流说明（如「HLS」「Icecast 128 kbps」）
    Info(String),
    Error(String),
    End,
}

pub fn spawn(url: String, net: NetworkSettings, tx: Sender<Msg>) {
    std::thread::Builder::new()
        .name("radio-fetch".into())
        .spawn(move || {
            let r = run(&url, &net, &tx);
            let _ = tx.send(match r {
                Ok(()) => Msg::End,
                Err(e) => Msg::Error(e),
            });
        })
        .ok();
}

/// 看开头几个字节是不是 TS
fn wrap_of(b: &[u8]) -> Wrap {
    if b.first() == Some(&0x47) && b.get(188).is_none_or(|&x| x == 0x47) { Wrap::Ts } else { Wrap::Raw }
}

fn is_playlist(ctype: &str, url: &str, head: &[u8]) -> bool {
    let c = ctype.to_ascii_lowercase();
    c.contains("mpegurl") || url.to_ascii_lowercase().split('?').next().is_some_and(|p| p.ends_with(".m3u8")) || head.starts_with(b"#EXTM3U")
}

fn run(url: &str, net: &NetworkSettings, tx: &Sender<Msg>) -> Result<(), String> {
    open(url, net, tx, 0)
}

fn open(url: &str, net: &NetworkSettings, tx: &Sender<Msg>, depth: u32) -> Result<(), String> {
    if depth > 3 {
        return Err("播放列表套得太深".into());
    }
    let agent = crate::lyrics::lrclib::agent_with(net, None)?;
    let resp = agent.get(url).header("Icy-MetaData", "1").call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }
    let final_url = resp.get_uri().to_string();
    let ctype = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let metaint: usize = resp.headers().get("icy-metaint").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    let br = resp.headers().get("icy-br").and_then(|v| v.to_str().ok()).map(|s| s.trim().to_string());
    let mut body = resp.into_body().into_reader();
    // 先读一点，判断是播放列表（HLS / m3u / pls）还是音频
    let mut head = vec![0u8; 4096];
    let n = read_some(&mut body, &mut head).map_err(|e| e.to_string())?;
    head.truncate(n);
    if is_playlist(&ctype, &final_url, &head) || ctype.contains("scpls") || head.starts_with(b"[playlist]") {
        let mut text = head;
        body.take(1 << 20).read_to_end(&mut text).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&text).into_owned();
        if text.contains("#EXT-X-") {
            let _ = tx.send(Msg::Info("HLS".into()));
            return hls(&agent, &final_url, Some(text), tx);
        }
        // 普通 m3u / pls：取第一个地址再打开
        let next = text.lines().map(str::trim).find_map(|l| l.strip_prefix("File1=").or((l.starts_with("http") && !l.starts_with('#')).then_some(l)));
        let next = next.ok_or("播放列表里没有电台地址")?;
        return open(&join(&final_url, next), net, tx, depth + 1);
    }
    let info = match br {
        Some(b) if !b.is_empty() => format!("{} kbps", b),
        _ => String::new(),
    };
    let _ = tx.send(Msg::Info(info));
    let wrap = wrap_of(&head);
    let mut icy = Icy::new(metaint);
    let mut on_title = |t: String| {
        let _ = tx.send(Msg::Title(t));
    };
    // 开头已读到的这段也要去掉元数据
    let mut out = Vec::new();
    icy.strip(&head, &mut out, &mut on_title);
    if tx.send(Msg::Data(wrap, out)).is_err() {
        return Ok(());
    }
    let mut buf = vec![0u8; 8192];
    loop {
        let n = body.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        let mut out = Vec::with_capacity(n);
        icy.strip(&buf[..n], &mut out, &mut on_title);
        if tx.send(Msg::Data(wrap, out)).is_err() {
            return Ok(()); // 解码端已关闭（换台 / 停止）
        }
    }
}

fn read_some(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match r.read(&mut buf[got..])? {
            0 => break,
            n => got += n,
        }
    }
    Ok(got)
}

/// ICY 元数据：每 metaint 字节音频后面跟 1 字节长度（×16）和元数据文本；可能跨两次读
struct Icy {
    metaint: usize,
    /// 还有多少字节音频才到元数据
    audio_left: usize,
    /// 正在读的元数据：还差多少字节、已读到的内容
    meta: Option<(usize, Vec<u8>)>,
}

impl Icy {
    fn new(metaint: usize) -> Self {
        Icy { metaint, audio_left: metaint, meta: None }
    }

    /// 去掉元数据，音频追加到 `out`，读到的节目名发给 `on_title`
    fn strip(&mut self, mut data: &[u8], out: &mut Vec<u8>, on_title: &mut dyn FnMut(String)) {
        if self.metaint == 0 {
            out.extend_from_slice(data);
            return;
        }
        while !data.is_empty() {
            if let Some((left, buf)) = self.meta.as_mut() {
                let n = (*left).min(data.len());
                buf.extend_from_slice(&data[..n]);
                *left -= n;
                data = &data[n..];
                if *left == 0 {
                    if let Some(t) = title(buf) {
                        on_title(t);
                    }
                    self.meta = None;
                    self.audio_left = self.metaint;
                }
            } else if self.audio_left > 0 {
                let n = self.audio_left.min(data.len());
                out.extend_from_slice(&data[..n]);
                self.audio_left -= n;
                data = &data[n..];
            } else {
                let len = data[0] as usize * 16;
                data = &data[1..];
                if len == 0 {
                    self.audio_left = self.metaint;
                } else {
                    self.meta = Some((len, Vec::with_capacity(len)));
                }
            }
        }
    }
}

/// StreamTitle='...';
fn title(meta: &[u8]) -> Option<String> {
    let s = crate::util::text::decode_text(meta.split(|&b| b == 0).next().unwrap_or(meta));
    let start = s.find("StreamTitle='")? + 13;
    let end = s[start..].find("';").map_or(s.len(), |e| start + e);
    let t = s[start..end].trim().to_string();
    (!t.is_empty()).then_some(t)
}

fn get_text(agent: &ureq::Agent, url: &str) -> Result<(String, String), String> {
    let resp = agent.get(url).call().map_err(|e| e.to_string())?;
    if resp.status().as_u16() != 200 {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let real = resp.get_uri().to_string();
    let mut b = Vec::new();
    resp.into_body().into_reader().take(4 << 20).read_to_end(&mut b).map_err(|e| e.to_string())?;
    Ok((real, String::from_utf8_lossy(&b).into_owned()))
}

fn get_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let resp = agent.get(url).call().map_err(|e| e.to_string())?;
    if resp.status().as_u16() != 200 {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let mut b = Vec::new();
    resp.into_body().into_reader().take(32 << 20).read_to_end(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

/// 媒体播放列表：(第一个分段的序号, 目标时长秒, 各分段地址, 是否已结束)
fn media_playlist(text: &str) -> (u64, f64, Vec<String>, bool) {
    let mut seq = 0;
    let mut target = 6.0;
    let mut segs = Vec::new();
    for l in text.lines().map(str::trim) {
        if let Some(v) = l.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            seq = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = l.strip_prefix("#EXT-X-TARGETDURATION:") {
            target = v.trim().parse().unwrap_or(6.0);
        } else if !l.is_empty() && !l.starts_with('#') {
            segs.push(l.to_string());
        }
    }
    (seq, target, segs, text.contains("#EXT-X-ENDLIST"))
}

/// 主播放列表里的第一路码流（电台通常只有一路，或第一路是默认的）
fn first_variant(text: &str) -> Option<&str> {
    let mut want = false;
    for l in text.lines().map(str::trim) {
        if l.starts_with("#EXT-X-STREAM-INF") {
            want = true;
        } else if want && !l.is_empty() && !l.starts_with('#') {
            return Some(l);
        }
    }
    None
}

/// HLS：按顺序下载新分段；第一次只从最后 3 段开始（直播的最新位置）
fn hls(agent: &ureq::Agent, url: &str, first: Option<String>, tx: &Sender<Msg>) -> Result<(), String> {
    let (mut url, mut text) = match first {
        Some(t) => (url.to_string(), t),
        None => get_text(agent, url)?,
    };
    // 主播放列表：换成第一路码流
    for _ in 0..3 {
        let Some(v) = first_variant(&text) else { break };
        let next = join(&url, v);
        (url, text) = get_text(agent, &next)?;
    }
    if text.contains("#EXT-X-KEY") && !text.contains("METHOD=NONE") {
        return Err("加密的 HLS 电台，不支持".into());
    }
    let mut next_seq: Option<u64> = None;
    let mut errors = 0;
    loop {
        let t0 = Instant::now();
        let (seq, target, segs, ended) = media_playlist(&text);
        let start = match next_seq {
            // 序号倒退或跳得很远（服务器重启）：从最新的几段重新开始
            Some(n) if n >= seq && n <= seq + segs.len() as u64 => n,
            _ => seq + segs.len().saturating_sub(3) as u64,
        };
        for (i, s) in segs.iter().enumerate() {
            let n = seq + i as u64;
            if n < start {
                continue;
            }
            match get_bytes(agent, &join(&url, s)) {
                Ok(b) => {
                    errors = 0;
                    if tx.send(Msg::Data(wrap_of(&b), b)).is_err() {
                        return Ok(());
                    }
                }
                Err(e) => {
                    errors += 1;
                    log::info!("电台分段下载失败: {e}");
                    if errors > 5 {
                        return Err(e);
                    }
                }
            }
            next_seq = Some(n + 1);
        }
        if ended {
            return Ok(());
        }
        // 半个目标时长刷新一次播放列表
        let wait = Duration::from_secs_f64((target / 2.0).clamp(1.0, 10.0));
        if let Some(w) = wait.checked_sub(t0.elapsed()) {
            std::thread::sleep(w);
        }
        // 解码端关了就不再刷新
        if tx.send(Msg::Info(String::new())).is_err() {
            return Ok(());
        }
        text = match get_text(agent, &url) {
            Ok((_, t)) => t,
            Err(e) => {
                errors += 1;
                if errors > 5 {
                    return Err(e);
                }
                continue;
            }
        };
    }
}

/// 相对地址 → 绝对地址
pub fn join(base: &str, rel: &str) -> String {
    let rel = rel.trim();
    if rel.contains("://") {
        return rel.to_string();
    }
    let scheme_end = base.find("://").map_or(0, |i| i + 3);
    if let Some(r) = rel.strip_prefix("//") {
        return format!("{}{}", &base[..scheme_end], r);
    }
    let host_end = base[scheme_end..].find('/').map_or(base.len(), |i| scheme_end + i);
    if rel.starts_with('/') {
        return format!("{}{}", &base[..host_end], rel);
    }
    let path = base.split(['?', '#']).next().unwrap_or(base);
    let dir = path.rfind('/').filter(|&i| i >= host_end).map_or(path.len(), |i| i + 1);
    let mut d = path[..dir].to_string();
    if !d.ends_with('/') {
        d.push('/');
    }
    format!("{d}{rel}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let b = "https://a.com/live/x/index.m3u8?k=1";
        assert_eq!(join(b, "seg1.ts"), "https://a.com/live/x/seg1.ts");
        assert_eq!(join(b, "/other/s.aac"), "https://a.com/other/s.aac");
        assert_eq!(join(b, "//cdn.b.com/s.ts"), "https://cdn.b.com/s.ts");
        assert_eq!(join(b, "http://c.com/s.ts"), "http://c.com/s.ts");
        assert_eq!(join("http://h.com", "a.ts"), "http://h.com/a.ts");
        assert_eq!(join("http://h.com:8000/live", "a.ts"), "http://h.com:8000/a.ts");
    }

    #[test]
    fn icy_metadata() {
        // metaint = 4：4 字节音频 + 元数据；元数据跨两次读
        let meta = b"StreamTitle='\xe6\x99\xb4\xe5\xa4\xa9 - x';StreamUrl='';";
        let blocks = meta.len().div_ceil(16);
        let mut m = meta.to_vec();
        m.resize(blocks * 16, 0);
        let mut data = b"AAAA".to_vec();
        data.push(blocks as u8);
        data.extend(&m);
        data.extend(b"BBBB\x00CCCC");
        let mut icy = Icy::new(4);
        let (mut out, mut titles) = (Vec::new(), Vec::new());
        let mut on = |t: String| titles.push(t);
        let cut = 10;
        icy.strip(&data[..cut], &mut out, &mut on);
        icy.strip(&data[cut..], &mut out, &mut on);
        assert_eq!(out, b"AAAABBBBCCCC");
        assert_eq!(titles, vec!["晴天 - x".to_string()]);
        // 没有 metaint：原样
        let mut plain = Icy::new(0);
        let mut o = Vec::new();
        plain.strip(b"xyz", &mut o, &mut |_| {});
        assert_eq!(o, b"xyz");
    }

    #[test]
    fn playlists() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=64000\nchunklist_w1.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=128000\nhi.m3u8\n";
        assert_eq!(first_variant(master), Some("chunklist_w1.m3u8"));
        assert_eq!(first_variant("#EXTM3U\n#EXTINF:5,\na.ts\n"), None);
        let media = "#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:4711\n#EXTINF:10,\na.aac\n#EXTINF:10,\nb.aac\n";
        assert_eq!(media_playlist(media), (4711, 10.0, vec!["a.aac".into(), "b.aac".into()], false));
        assert!(media_playlist("#EXTM3U\n#EXT-X-ENDLIST\n").3);
        assert!(is_playlist("application/vnd.apple.mpegurl", "x", b""));
        assert!(is_playlist("audio/mpeg", "http://a/b.m3u8?x=1", b""));
        assert!(!is_playlist("audio/mpeg", "http://a/live.mp3", b"\xff\xfb"));
        assert_eq!(wrap_of(&[0x47; 400]), Wrap::Ts);
        assert_eq!(wrap_of(b"ID3\x04"), Wrap::Raw);
    }
}
