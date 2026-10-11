//! 在本地查找与曲目对应的 .lrc 文件。
//!
//! 查找顺序：同目录同名 → `艺人 - 曲目.lrc`（自定义目录、程序歌词目录、同目录）→ 同目录模糊匹配。
//! 模糊匹配解决 `七月七日晴.lrc` 对应 `许慧欣 - 七月七日晴.flac` 这类文件名不一致的情况。

use std::path::{Path, PathBuf};

use crate::util::sanitize_file_name;
use crate::util::text::{base_title, normalize_key};

pub struct LrcQuery<'a> {
    pub audio: &'a Path,
    /// CUE 分轨共用一个音频文件，不能按音频文件名匹配
    pub is_cue: bool,
    pub title: &'a str,
    pub artist: &'a str,
    pub dirs: &'a [PathBuf],
}

/// 同目录模糊匹配时最多检查的 .lrc 数量（防止超大目录拖慢）
const MAX_SCAN: usize = 4000;

pub fn find_lrc(q: &LrcQuery) -> Option<PathBuf> {
    let dir = q.audio.parent()?;
    if !q.is_cue {
        let same = q.audio.with_extension("lrc");
        if same.is_file() {
            return Some(same);
        }
    }
    let names = candidate_names(q.title, q.artist);
    for d in q.dirs.iter().map(PathBuf::as_path).chain(std::iter::once(dir)) {
        for n in &names {
            let p = d.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    fuzzy_in_dir(dir, q)
}

fn candidate_names(title: &str, artist: &str) -> Vec<String> {
    let mut v = Vec::new();
    if title.is_empty() {
        return v;
    }
    if !artist.is_empty() {
        v.push(format!("{} - {}.lrc", sanitize_file_name(artist), sanitize_file_name(title)));
        v.push(format!("{} - {}.lrc", sanitize_file_name(title), sanitize_file_name(artist)));
    }
    v.push(format!("{}.lrc", sanitize_file_name(title)));
    v
}

fn fuzzy_in_dir(dir: &Path, q: &LrcQuery) -> Option<PathBuf> {
    let title = normalize_key(q.title);
    if title.is_empty() {
        return None;
    }
    let base = normalize_key(base_title(q.title));
    let artist = normalize_key(q.artist);
    let audio_stem = normalize_key(&crate::util::file_stem(q.audio));
    let mut best: Option<(u32, PathBuf)> = None;
    let mut tie = false;
    let entries = std::fs::read_dir(dir).ok()?;
    for e in entries.flatten().take(MAX_SCAN * 4) {
        let p = e.path();
        if !crate::util::ext_lower(&p).eq("lrc") {
            continue;
        }
        let stem = crate::util::file_stem(&p);
        let k = normalize_key(&stem);
        let kb = normalize_key(base_title(&stem));
        let score = if !q.is_cue && k == audio_stem {
            100
        } else if !artist.is_empty() && (k == format!("{artist}{title}") || k == format!("{title}{artist}")) {
            95
        } else if k == title {
            90
        } else if kb == base || k == base || kb == title {
            80
        } else {
            0
        };
        if score == 0 {
            continue;
        }
        match &best {
            Some((s, _)) if *s > score => {}
            Some((s, _)) if *s == score => tie = true,
            _ => {
                best = Some((score, p));
                tie = false;
            }
        }
    }
    if tie { None } else { best.map(|(_, p)| p) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matches_real_world_names() {
        let dir = std::env::temp_dir().join(format!("ap3_lrc_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["七月七日晴.lrc", "暮色回响 (电影《默杀》推广曲).lrc", "其他歌.lrc", "许慧欣 - 七月七日晴.flac"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let dirs = [];
        let find = |audio: &str, title: &str, artist: &str| {
            find_lrc(&LrcQuery { audio: &dir.join(audio), is_cue: false, title, artist, dirs: &dirs })
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        };
        assert_eq!(find("许慧欣 - 七月七日晴.flac", "七月七日晴", "许慧欣").as_deref(), Some("七月七日晴.lrc"));
        assert_eq!(find("张韶涵-暮色回响.flac", "暮色回响", "张韶涵").as_deref(), Some("暮色回响 (电影《默杀》推广曲).lrc"));
        assert_eq!(find("x.flac", "不存在", "谁").as_deref(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
