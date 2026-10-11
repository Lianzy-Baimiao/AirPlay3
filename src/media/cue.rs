//! CUE 索引文件解析（原版"CUE 音频索引文件"）。整轨 APE/FLAC + CUE 是中文无损资源的常见形式。

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct CueTrack {
    pub number: u32,
    pub title: String,
    pub performer: String,
    pub file: PathBuf,
    /// 秒（INDEX 01）
    pub start: f64,
    /// 同文件下一轨的 INDEX 01；最后一轨为 None（到文件结尾）
    pub end: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct CueSheet {
    pub title: String,
    pub performer: String,
    pub genre: String,
    pub date: String,
    pub tracks: Vec<CueTrack>,
}

/// 把一行拆成参数，支持双引号。
fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut has) = (false, false);
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            c if c.is_whitespace() && !quoted => {
                if has {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        out.push(cur);
    }
    out
}

/// `mm:ss:ff`，75 帧/秒。
fn parse_msf(s: &str) -> Option<f64> {
    let mut it = s.split(':').map(|p| p.trim().parse::<u32>().ok());
    let (m, sec, f) = (it.next()??, it.next()??, it.next().flatten().unwrap_or(0));
    Some(m as f64 * 60.0 + sec as f64 + f as f64 / 75.0)
}

/// CUE 里写的文件名常与实际不符（写 .wav 实际是 .ape/.flac），按主干名在同目录找音频文件。
fn resolve_file(base: &Path, name: &str) -> PathBuf {
    let p = base.join(name);
    if p.is_file() {
        return p;
    }
    let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    if let Ok(rd) = std::fs::read_dir(base) {
        for e in rd.flatten() {
            let ep = e.path();
            if crate::util::file_stem(&ep).to_lowercase() == stem && super::is_audio_ext(&crate::util::ext_lower(&ep)) {
                return ep;
            }
        }
    }
    p
}

pub fn parse(text: &str, base_dir: &Path) -> CueSheet {
    let mut sheet = CueSheet::default();
    let mut file: Option<PathBuf> = None;
    let mut cur: Option<CueTrack> = None;
    let mut tracks = Vec::new();
    for line in text.lines() {
        let args = split_args(line.trim());
        let Some(cmd) = args.first() else { continue };
        let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
        match cmd.to_ascii_uppercase().as_str() {
            "FILE" => {
                let f = resolve_file(base_dir, &arg(1));
                if let Some(t) = cur.as_mut().filter(|t| t.start < 0.0) {
                    t.file = f.clone();
                }
                file = Some(f);
            }
            "TRACK" => {
                tracks.extend(cur.take().filter(|t| t.start >= 0.0));
                if let Some(f) = &file {
                    let number = arg(1).parse().unwrap_or(0);
                    cur = Some(CueTrack { number, title: String::new(), performer: String::new(), file: f.clone(), start: -1.0, end: None });
                }
            }
            "TITLE" => match cur.as_mut() {
                Some(t) => t.title = arg(1),
                None => sheet.title = arg(1),
            },
            "PERFORMER" => match cur.as_mut() {
                Some(t) => t.performer = arg(1),
                None => sheet.performer = arg(1),
            },
            "INDEX" => {
                if let (Some(t), Ok(1)) = (cur.as_mut(), arg(1).parse::<u32>()) {
                    if let Some(s) = parse_msf(&arg(2)) {
                        t.start = s;
                    }
                }
            }
            "REM" => match arg(1).to_ascii_uppercase().as_str() {
                "GENRE" => sheet.genre = arg(2),
                "DATE" => sheet.date = arg(2),
                _ => {}
            },
            _ => {}
        }
    }
    tracks.extend(cur.take().filter(|t| t.start >= 0.0));
    for i in 0..tracks.len() {
        let next = tracks.get(i + 1).filter(|n| n.file == tracks[i].file).map(|n| n.start);
        tracks[i].end = next.filter(|&e| e > tracks[i].start);
        if tracks[i].performer.is_empty() {
            tracks[i].performer = sheet.performer.clone();
        }
    }
    sheet.tracks = tracks;
    sheet
}

pub fn read_cue_file(path: &Path) -> Option<CueSheet> {
    let bytes = std::fs::read(path).ok()?;
    Some(parse(&crate::util::text::decode_text(&bytes), path.parent()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_multi_file_cue() {
        let dir = std::env::temp_dir().join(format!("ap3_cue_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // CUE 里写 .wav，实际文件是 .flac
        std::fs::write(dir.join("album.flac"), b"").unwrap();
        let src = "REM GENRE Pop\nREM DATE 2003\nPERFORMER \"周杰伦\"\nTITLE \"叶惠美\"\n\
                   FILE \"album.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"以父之名\"\n    INDEX 01 00:00:00\n\
                   TRACK 02 AUDIO\n    TITLE \"懦夫\"\n    PERFORMER \"Jay\"\n    INDEX 00 05:30:00\n    INDEX 01 05:32:33\n\
                   FILE \"b.wav\" WAVE\n  TRACK 03 AUDIO\n    TITLE \"晴天\"\n    INDEX 01 00:00:00\n";
        let s = parse(src, &dir);
        assert_eq!((s.title.as_str(), s.genre.as_str(), s.date.as_str()), ("叶惠美", "Pop", "2003"));
        assert_eq!(s.tracks.len(), 3);
        let t = &s.tracks;
        assert_eq!(t[0].file, dir.join("album.flac"));
        assert_eq!(t[0].performer, "周杰伦");
        assert_eq!(t[1].performer, "Jay");
        assert_eq!(t[0].end, Some(5.0 * 60.0 + 32.0 + 33.0 / 75.0));
        assert_eq!(t[1].end, None);
        assert_eq!(t[2].file, dir.join("b.wav"));
        assert_eq!(t[2].number, 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
