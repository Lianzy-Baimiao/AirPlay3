//! 媒体文件相关：支持的格式、标签读取、封面、CUE。

pub mod cover;
pub mod cue;
pub mod metadata;

/// 能解码的音频扩展名。
/// symphonia: mp3/mp2/mp1/flac/ogg/oga/wav/aif/aiff/aifc/m4a/mp4/aac/caf/mka/webm/alac；
/// Media Foundation 兜底: wma/asf/ac3/m4b 等。
pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "mp2", "mp1", "mpa", "flac", "ogg", "oga", "wav", "aif", "aiff", "aifc", "m4a", "m4b", "mp4", "aac", "alac",
    "caf", "mka", "webm", "wma", "asf", "ac3",
];

/// 能导入的播放列表扩展名（原版"列表文件"）。
pub const PLAYLIST_EXTS: &[&str] = &["m3u", "m3u8", "pls", "wpl", "asx", "xspf", "ttpl", "apl"];

pub fn is_audio_ext(ext: &str) -> bool {
    AUDIO_EXTS.contains(&ext)
}

pub fn is_playlist_ext(ext: &str) -> bool {
    PLAYLIST_EXTS.contains(&ext)
}

/// 文件对话框用的过滤器：(名称, 扩展名列表)。
pub fn dialog_filters() -> Vec<(&'static str, Vec<&'static str>)> {
    let mut all: Vec<&str> = AUDIO_EXTS.to_vec();
    all.push("cue");
    all.extend_from_slice(PLAYLIST_EXTS);
    vec![
        ("全部支持格式", all),
        ("有损压缩音频格式", vec!["mp3", "mp2", "mp1", "mpa", "ogg", "oga", "m4a", "aac", "wma", "asf", "ac3"]),
        ("无损压缩音频格式", vec!["flac", "alac", "m4a", "wav", "aif", "aiff", "aifc", "caf"]),
        ("CUE 音频索引文件", vec!["cue"]),
        ("列表文件", PLAYLIST_EXTS.to_vec()),
        ("所有文件", vec!["*"]),
    ]
}
