//! 歌词：LRC 解析、本地查找、在线下载（LRCLIB，替代已停止服务的 lyric.airplayme.com）。

pub mod find;
pub mod lrc;
pub mod lrclib;
pub mod online;
pub mod query;

use std::path::{Path, PathBuf};

pub use lrc::{Line, Lyrics};

#[derive(Clone, Debug, PartialEq)]
pub enum LyricSource {
    File(PathBuf),
    Embedded,
}

pub struct LyricRequest<'a> {
    pub audio: &'a Path,
    pub is_cue: bool,
    pub title: &'a str,
    pub artist: &'a str,
    /// 用户手动关联的歌词文件
    pub linked: Option<&'a Path>,
    /// 用户选择了"取消歌词关联"
    pub unlinked: bool,
    pub search_dirs: &'a [PathBuf],
}

pub fn read_lrc_file(path: &Path) -> Option<Lyrics> {
    let bytes = std::fs::read(path).ok()?;
    Some(lrc::parse(&crate::util::text::decode_text(&bytes)))
}

/// 按优先级加载：手动关联 > 内嵌(带时间轴) > 本地 .lrc > 内嵌纯文本。
pub fn load(req: &LyricRequest) -> Option<(Lyrics, LyricSource)> {
    if req.unlinked {
        return None;
    }
    if let Some(p) = req.linked {
        if let Some(l) = read_lrc_file(p) {
            return Some((l, LyricSource::File(p.to_path_buf())));
        }
    }
    let embedded = if req.is_cue {
        None
    } else {
        crate::media::metadata::read_embedded_lyrics(req.audio).map(|t| lrc::parse(&t))
    };
    if embedded.as_ref().is_some_and(|l| l.synced && !l.lines.is_empty()) {
        return embedded.map(|l| (l, LyricSource::Embedded));
    }
    let q = find::LrcQuery { audio: req.audio, is_cue: req.is_cue, title: req.title, artist: req.artist, dirs: req.search_dirs };
    if let Some(p) = find::find_lrc(&q) {
        if let Some(l) = read_lrc_file(&p).filter(|l| !l.lines.is_empty()) {
            return Some((l, LyricSource::File(p)));
        }
    }
    embedded.filter(|l| !l.lines.is_empty()).map(|l| (l, LyricSource::Embedded))
}
