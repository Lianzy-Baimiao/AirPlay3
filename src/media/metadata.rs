//! 读取音频标签（lofty）。处理 GBK 乱码、ID3v2/ID3v1/APEv2 多标签共存、无标签时从文件名推断。

use std::path::Path;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::picture::PictureType;
use lofty::prelude::*;
use lofty::tag::Tag;

use crate::util::text::fix_mojibake;

#[derive(Clone, Debug, Default)]
pub struct TagInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: String,
    pub year: Option<u16>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub duration: f64,
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate: u32,
}

/// lofty 不认识、标签另外读的格式（见 `carrier`）；音频属性由自带解码器提供
fn via_carrier(path: &Path) -> bool {
    matches!(crate::util::ext_lower(path).as_str(), "tta" | "tak")
}

fn open(path: &Path, cover: bool) -> Option<lofty::file::TaggedFile> {
    let opts = ParseOptions::new().read_cover_art(cover).parsing_mode(ParsingMode::Relaxed);
    if !via_carrier(path) {
        let read = || lofty::probe::Probe::open(path).ok()?.options(opts).guess_file_type().ok()?.read().ok();
        if let Some(f) = read() {
            return Some(f);
        }
        // lofty 认不出的变体（如浮点的 APE「MACF」）：有自带解码器的格式改用下面的办法读标签
        if !crate::audio::codec::EXTS.contains(&crate::util::ext_lower(path).as_str()) {
            return None;
        }
    }
    let data = super::carrier::build(path)?;
    let opts = ParseOptions::new().read_cover_art(cover).parsing_mode(ParsingMode::Relaxed).read_properties(false);
    let p = lofty::probe::Probe::with_file_type(std::io::Cursor::new(data), lofty::file::FileType::Mpeg);
    p.options(opts).read().ok()
}

/// 主标签优先，其它标签补缺。
fn ordered_tags(f: &lofty::file::TaggedFile) -> Vec<&Tag> {
    let mut v: Vec<&Tag> = Vec::new();
    v.extend(f.primary_tag());
    v.extend(f.tags().iter().filter(|t| Some(t.tag_type()) != f.primary_tag().map(|p| p.tag_type())));
    v
}

fn pick(tags: &[&Tag], get: impl Fn(&Tag) -> Option<String>) -> String {
    tags.iter()
        .filter_map(|t| get(t))
        .map(|s| fix_mojibake(s.trim()).into_owned())
        .find(|s| !s.is_empty())
        .unwrap_or_default()
}

pub fn read_tags(path: &Path) -> Option<TagInfo> {
    if !via_carrier(path) {
        if let Some(mut t) = read_lofty(path) {
            // lofty 有时读不全属性（如多声道 WavPack 的声道数是 0、经标签载体读的没有属性）、或时长不准
            // （SV7 Musepack 没算无缝播放信息）：有自带解码器的格式由它补上
            let mpc = matches!(crate::util::ext_lower(path).as_str(), "mpc" | "mp+" | "mpp");
            if mpc || t.channels == 0 || t.sample_rate == 0 || t.duration <= 0.0 {
                fill_from_decoder(path, &mut t);
            }
            return Some(t);
        }
    }
    // 没有标签也要有时长、采样率等
    let mut t = read_lofty(path).unwrap_or_default();
    fill_from_decoder(path, &mut t)?;
    Some(t)
}

/// 用自带解码器给出采样率、声道数、时长、码率；不是自带解码器的格式时返回 None
fn fill_from_decoder(path: &Path, t: &mut TagInfo) -> Option<()> {
    let dec = crate::audio::codec::open_decoder(path)?.ok()?;
    let s = dec.spec();
    t.sample_rate = s.rate;
    t.channels = s.channels.min(255) as u8;
    t.duration = dec.total_frames().map_or(0.0, |n| n as f64 / s.rate.max(1) as f64);
    if t.duration > 0.0 {
        let bytes = std::fs::metadata(path).map_or(0, |m| m.len());
        t.bitrate = (bytes as f64 * 8.0 / t.duration / 1000.0).round() as u32;
    }
    Some(())
}

fn read_lofty(path: &Path) -> Option<TagInfo> {
    let f = open(path, false)?;
    let p = f.properties();
    let tags = ordered_tags(&f);
    Some(TagInfo {
        title: pick(&tags, |t| t.title().map(|s| s.into_owned())),
        artist: pick(&tags, |t| t.artist().map(|s| s.into_owned())),
        album: pick(&tags, |t| t.album().map(|s| s.into_owned())),
        album_artist: pick(&tags, |t| t.get_string(ItemKey::AlbumArtist).map(str::to_string)),
        genre: pick(&tags, |t| t.genre().map(|s| s.into_owned())),
        year: tags.iter().find_map(|t| t.date()).map(|d| d.year),
        track_no: tags.iter().find_map(|t| t.track()),
        disc_no: tags.iter().find_map(|t| t.disk()),
        duration: p.duration().as_secs_f64(),
        sample_rate: p.sample_rate().unwrap_or(0),
        channels: p.channels().unwrap_or(0),
        bitrate: p.audio_bitrate().or(p.overall_bitrate()).unwrap_or(0),
    })
}

/// 内嵌封面（优先 Front Cover）。
pub fn read_picture(path: &Path) -> Option<Vec<u8>> {
    let f = open(path, true)?;
    let tags = ordered_tags(&f);
    let pics = || tags.iter().flat_map(|t| t.pictures().iter());
    pics()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pics().next())
        .map(|p| p.data().to_vec())
}

/// 内嵌歌词（USLT / LYRICS / ©lyr）。
pub fn read_embedded_lyrics(path: &Path) -> Option<String> {
    let f = open(path, false)?;
    let tags = ordered_tags(&f);
    let s = pick(&tags, |t| t.get_string(ItemKey::Lyrics).or_else(|| t.get_string(ItemKey::UnsyncLyrics)).map(str::to_string));
    (!s.is_empty()).then_some(s)
}

/// 无标签时从文件名推断 (艺人, 标题)：`01. 晴天`、`周杰伦 - 晴天`、`张韶涵-暮色回响`。
pub fn guess_from_filename(path: &Path) -> (String, String) {
    let stem = crate::util::file_stem(path);
    let s = strip_track_prefix(&stem);
    if let Some((a, t)) = s.split_once(" - ") {
        return (a.trim().to_string(), t.trim().to_string());
    }
    // 只有一个短横线且含中日韩文字时才按 `艺人-标题` 拆分，避免拆坏 `Spider-Man`
    if s.matches('-').count() == 1 && crate::util::text::contains_cjk(s) {
        let (a, t) = s.split_once('-').unwrap();
        if !a.trim().is_empty() && !t.trim().is_empty() {
            return (a.trim().to_string(), t.trim().to_string());
        }
    }
    (String::new(), s.trim().to_string())
}

/// 去掉 `01. `、`01 - `、`1.`、`02_` 这类音轨号前缀。
fn strip_track_prefix(s: &str) -> &str {
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 3 {
        return s;
    }
    let rest = &s[digits..];
    let trimmed = rest.trim_start_matches(['.', '_', ')', '、']).trim_start_matches(" - ").trim_start();
    let had_sep = rest.len() != trimmed.len() && (rest.starts_with(['.', '_', ')', '、']) || rest.starts_with(" - "));
    let space_sep = digits >= 2 && rest.starts_with(' ');
    if (had_sep || space_sep) && !trimmed.is_empty() { trimmed } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_guess() {
        let g = |s: &str| guess_from_filename(Path::new(&format!("{s}.flac")));
        assert_eq!(g("1. Crush on You"), ("".into(), "Crush on You".into()));
        assert_eq!(g("01.想和你去吹吹风"), ("".into(), "想和你去吹吹风".into()));
        assert_eq!(g("许慧欣 - 七月七日晴"), ("许慧欣".into(), "七月七日晴".into()));
        assert_eq!(g("张韶涵-暮色回响"), ("张韶涵".into(), "暮色回响".into()));
        assert_eq!(g("Spider-Man"), ("".into(), "Spider-Man".into()));
        assert_eq!(g("7 Rings"), ("".into(), "7 Rings".into()));
        assert_eq!(g("1989"), ("".into(), "1989".into()));
        assert_eq!(g("02 Love Story"), ("".into(), "Love Story".into()));
    }

    #[test]
    fn reads_real_files_if_present() {
        // 本机曲库里的真实文件（不存在就跳过）
        let p = Path::new(r"E:\Music\2025-05-27\DbPyy3S\许慧欣 - 七月七日晴.flac");
        if p.exists() {
            let t = read_tags(p).expect("tags");
            assert!(t.duration > 60.0, "duration {}", t.duration);
            assert!(!t.title.is_empty());
        }
    }
}
