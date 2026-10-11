//! 播放列表文件导入/导出：m3u/m3u8/pls/wpl/asx/xspf/ttpl(千千静听)/apl(原版 AIRPLAY 播放列表)。

use std::path::{Path, PathBuf};

use quick_xml::events::Event;

use super::model::Track;

/// 读取播放列表，返回其中的文件路径（相对路径按列表所在目录解析）。
pub fn read_playlist(path: &Path) -> Vec<PathBuf> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    let text = crate::util::text::decode_text(&bytes);
    let base = path.parent().unwrap_or(Path::new("."));
    match crate::util::ext_lower(path).as_str() {
        "m3u" | "m3u8" => parse_m3u(&text, base),
        "pls" => parse_pls(&text, base),
        _ => parse_xml(&text, base),
    }
}

fn resolve(s: &str, base: &Path) -> Option<PathBuf> {
    let s = s.trim().trim_matches('"');
    if s.is_empty() || s.contains("://") && !s.to_ascii_lowercase().starts_with("file:") {
        return None;
    }
    let s = match s.strip_prefix("file:///").or_else(|| s.strip_prefix("file://")).or_else(|| s.strip_prefix("file:")) {
        Some(rest) => percent_decode(rest),
        None => s.to_string(),
    };
    let p = PathBuf::from(s.replace('/', "\\"));
    Some(if p.is_absolute() { p } else { base.join(p) })
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_m3u(text: &str, base: &Path) -> Vec<PathBuf> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).filter_map(|l| resolve(l, base)).collect()
}

fn parse_pls(text: &str, base: &Path) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.trim().split_once('=')?;
            let k = k.trim().to_ascii_lowercase();
            (k.starts_with("file") && k[4..].chars().all(|c| c.is_ascii_digit())).then(|| resolve(v, base)).flatten()
        })
        .collect()
}

/// 通用 XML 列表：属性 src/href/file/path/url，或元素 <location>/<path>/<file>/<url> 的文本。
fn parse_xml(text: &str, base: &Path) -> Vec<PathBuf> {
    let mut r = quick_xml::Reader::from_str(text);
    let mut out = Vec::new();
    let mut text_tag: Option<String> = None;
    let mut acc = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name().as_ref().to_ascii_lowercase();
                for a in e.attributes().flatten() {
                    let key = a.key.as_ref().to_ascii_lowercase();
                    if matches!(key.as_str(), "src" | "href" | "file" | "path" | "url") && name != "playlist" {
                        let v = a.normalized_value(quick_xml::XmlVersion::Implicit1_0);
                        if let Some(p) = v.ok().and_then(|v| resolve(&v, base)) {
                            out.push(p);
                        }
                    }
                }
                if matches!(name.as_str(), "location" | "path" | "file" | "url") {
                    text_tag = Some(name);
                    acc.clear();
                }
            }
            Ok(Event::Text(t)) if text_tag.is_some() => acc.push_str(&t),
            Ok(Event::GeneralRef(g)) if text_tag.is_some() => match g.resolve_char_ref() {
                Ok(Some(c)) => acc.push(c),
                _ => acc.push_str(match &*g {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                }),
            },
            Ok(Event::End(e)) if text_tag.as_deref() == Some(e.name().as_ref().to_ascii_lowercase().as_str()) => {
                if let Some(p) = resolve(&acc, base) {
                    out.push(p);
                }
                text_tag = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// 导出为 m3u8（UTF-8）。CUE 分轨导出其 .cue 文件（每个只写一次）。
pub fn write_m3u8(path: &Path, tracks: &[&Track]) -> std::io::Result<()> {
    let mut s = String::from("#EXTM3U\r\n");
    let mut sheets = std::collections::HashSet::new();
    for t in tracks {
        let target = match &t.cue {
            Some(c) if !sheets.insert(c.sheet.clone()) => continue,
            Some(c) => c.sheet.clone(),
            None => t.path.clone(),
        };
        s.push_str(&format!("#EXTINF:{},{} - {}\r\n{}\r\n", t.duration.round() as i64, t.display_artist(), t.display_title(), target.display()));
    }
    std::fs::write(path, s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let base = Path::new(r"D:\lists");
        let m3u = parse_m3u("#EXTM3U\n#EXTINF:1,a\nsongs\\a.mp3\n\nC:\\x\\b.flac\nhttp://radio/stream\n", base);
        assert_eq!(m3u, vec![PathBuf::from(r"D:\lists\songs\a.mp3"), PathBuf::from(r"C:\x\b.flac")]);
        let pls = parse_pls("[playlist]\nFile1=a.mp3\nTitle1=A\nfile2=E:\\b.ogg\nNumberOfEntries=2\n", base);
        assert_eq!(pls, vec![PathBuf::from(r"D:\lists\a.mp3"), PathBuf::from(r"E:\b.ogg")]);
        let xspf = r#"<playlist><trackList><track><location>file:///E:/Music/%E6%99%B4%E5%A4%A9.mp3</location></track>
            <track><location>file:///C:/a&amp;b.mp3</location></track></trackList></playlist>"#;
        assert_eq!(parse_xml(xspf, base), vec![PathBuf::from(r"E:\Music\晴天.mp3"), PathBuf::from(r"C:\a&b.mp3")]);
        let wpl = r#"<?wpl version="1.0"?><smil><body><seq><media src="..\m\c.wma"/><media src="E:\d.mp3"/></seq></body></smil>"#;
        assert_eq!(parse_xml(wpl, base), vec![PathBuf::from(r"D:\lists\..\m\c.wma"), PathBuf::from(r"E:\d.mp3")]);
    }
}
