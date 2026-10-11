//! 封面：内嵌图片 → 同目录 cover.jpg / folder.jpg 等（原版同样查找这几个文件名）。

use std::path::{Path, PathBuf};

const NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart", "albumartsmall", "thumb"];
const EXTS: &[&str] = &["jpg", "jpeg", "png", "bmp", "webp", "gif"];

pub struct CoverRequest<'a> {
    pub audio: &'a Path,
    pub linked: Option<&'a Path>,
    pub unlinked: bool,
    pub embedded: bool,
    pub local: bool,
}

/// 返回原始图片字节。
pub fn find_cover(req: &CoverRequest) -> Option<Vec<u8>> {
    if req.unlinked {
        return None;
    }
    if let Some(p) = req.linked {
        if let Ok(b) = std::fs::read(p) {
            return Some(b);
        }
    }
    if req.embedded {
        if let Some(b) = super::metadata::read_picture(req.audio) {
            return Some(b);
        }
    }
    if req.local {
        if let Some(p) = folder_image(req.audio.parent()?) {
            return std::fs::read(p).ok();
        }
    }
    None
}

/// 目录中的封面图片：优先常见文件名；目录里只有一张图片时也用它。
pub fn folder_image(dir: &Path) -> Option<PathBuf> {
    let mut images = Vec::new();
    for e in std::fs::read_dir(dir).ok()?.flatten().take(2000) {
        let p = e.path();
        let ext = crate::util::ext_lower(&p);
        if !EXTS.contains(&ext.as_str()) {
            continue;
        }
        let stem = crate::util::file_stem(&p).to_lowercase();
        if let Some(rank) = NAMES.iter().position(|n| stem == *n || stem.starts_with(&format!("{n}_"))) {
            images.push((rank, p));
        } else {
            images.push((NAMES.len(), p));
        }
    }
    images.sort();
    let named = images.iter().find(|(r, _)| *r < NAMES.len()).map(|(_, p)| p.clone());
    named.or_else(|| if images.len() == 1 { Some(images[0].1.clone()) } else { None })
}

/// 解码并缩放到不超过 `max` 像素边长，返回 (宽, 高, RGBA)。
pub fn decode_rgba(bytes: &[u8], max: u32) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > max || img.height() > max {
        img.resize(max, max, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}
