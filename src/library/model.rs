//! 曲库数据模型。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub type TrackId = u64;
pub type ListId = u64;

/// CUE 分轨信息（多个分轨共用一个音频文件）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CueRef {
    pub sheet: PathBuf,
    pub index: u32,
    pub start: f64,
    pub end: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: TrackId,
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cue: Option<CueRef>,
    pub title: String,
    pub artist: String,
    pub album: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub album_artist: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub genre: String,
    pub year: Option<u16>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub duration: f64,
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate: u32,
    pub added: u64,
    pub play_count: u32,
    pub last_played: u64,
    /// 用户手动关联的歌词 / 图片
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyric_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub lyric_unlinked: bool,
    /// 歌词时间微调（毫秒，正值 = 歌词提前）
    #[serde(skip_serializing_if = "is_zero")]
    pub lyric_offset_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cover_unlinked: bool,
    /// 文件不存在或无法解码（运行时检测，用于灰显/删除不能播放的内容）
    #[serde(skip)]
    pub broken: bool,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

impl Track {
    pub fn display_title(&self) -> String {
        if self.title.trim().is_empty() { crate::util::file_stem(&self.path) } else { self.title.clone() }
    }

    pub fn display_artist(&self) -> &str {
        if self.artist.trim().is_empty() { "未知艺人" } else { &self.artist }
    }

    pub fn display_album(&self) -> &str {
        if self.album.trim().is_empty() { "未知专辑" } else { &self.album }
    }

    /// 专辑分组键：专辑名 + 专辑艺人（没有专辑艺人时只按专辑名，便于合辑归到一起）
    pub fn album_key(&self) -> (String, String) {
        let name = crate::util::text::normalize_key(self.display_album());
        (name, crate::util::text::normalize_key(&self.album_artist))
    }

    /// 去重键：同一文件（及同一 CUE 分轨）只入库一次
    pub fn source_key(&self) -> (PathBuf, u32) {
        (self.path.clone(), self.cue.as_ref().map_or(0, |c| c.index))
    }

    pub fn is_cue(&self) -> bool {
        self.cue.is_some()
    }

    /// 播放区间 (start, end)
    pub fn clip(&self) -> (f64, Option<f64>) {
        self.cue.as_ref().map_or((0.0, None), |c| (c.start, c.end))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListKind {
    /// 试听列表：命令行/双击打开的文件
    Audition,
    /// 默认的本地列表
    Local,
    User,
    Favorite,
    /// 不再播放
    Banned,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub id: ListId,
    pub name: String,
    pub kind: ListKind,
    pub tracks: Vec<TrackId>,
}

impl Playlist {
    pub fn is_special(&self) -> bool {
        !matches!(self.kind, ListKind::User)
    }
}

/// 永久排序方式（原版"编辑"菜单：按名称/艺人/专辑/时间长度排序）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    Title,
    Artist,
    Album,
    Duration,
    Path,
}
