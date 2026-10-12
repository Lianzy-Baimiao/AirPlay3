//! lofty 不认识的格式（TTA、TAK、DTS）的标签：这些文件开头可能有 ID3v2、末尾可能有 APEv2 / ID3v1。
//! 把这几段原样拼到几帧空白 MP3 前后，交给 lofty 按 MP3 读（所有标签类型、编码、封面都由 lofty 处理）。
//! 时长等音频属性不用 lofty 的（它会把压缩数据误当成 MP3 帧），由自带解码器提供。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// 开头的 ID3v2 标签（原样）
fn head_id3v2(f: &mut File) -> std::io::Result<Vec<u8>> {
    let mut h = [0u8; 10];
    f.seek(SeekFrom::Start(0))?;
    if f.read_exact(&mut h).is_err() || &h[..3] != b"ID3" || h[6..10].iter().any(|b| b & 0x80 != 0) {
        return Ok(Vec::new());
    }
    let size = h[6..10].iter().fold(0u64, |a, &b| (a << 7) | b as u64);
    let total = 10 + size + if h[5] & 0x10 != 0 { 10 } else { 0 };
    let mut v = vec![0u8; total as usize];
    f.seek(SeekFrom::Start(0))?;
    f.read_exact(&mut v)?;
    Ok(v)
}

/// 末尾的 APEv2（含可选的头）和 ID3v1（原样）
fn tail_tags(f: &mut File) -> std::io::Result<Vec<u8>> {
    let len = f.seek(SeekFrom::End(0))?;
    let mut end = len;
    if len >= 128 {
        let mut t = [0u8; 3];
        f.seek(SeekFrom::Start(len - 128))?;
        f.read_exact(&mut t)?;
        if &t == b"TAG" {
            end = len - 128;
        }
    }
    let mut start = end;
    if end >= 32 {
        let mut foot = [0u8; 32];
        f.seek(SeekFrom::Start(end - 32))?;
        f.read_exact(&mut foot)?;
        if &foot[..8] == b"APETAGEX" {
            // size = 各项 + 页脚（不含头）；flags 第 31 位 = 有头
            let size = u32::from_le_bytes(foot[12..16].try_into().unwrap()) as u64;
            let flags = u32::from_le_bytes(foot[20..24].try_into().unwrap());
            let head = if flags & 0x8000_0000 != 0 { 32 } else { 0 };
            if size + head <= end && size < 64 << 20 {
                start = end - size - head;
            }
        }
    }
    let mut v = vec![0u8; (len - start) as usize];
    f.seek(SeekFrom::Start(start))?;
    f.read_exact(&mut v)?;
    Ok(v)
}

/// 一帧 MPEG-1 Layer III（128 kbps、44.1 kHz，417 字节），内容全是 0
fn blank_frame() -> Vec<u8> {
    let mut v = vec![0u8; 417];
    v[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
    v
}

/// 拼好的「MP3」；文件里没有任何标签时返回 None
pub fn build(path: &Path) -> Option<Vec<u8>> {
    let mut f = File::open(path).ok()?;
    let head = head_id3v2(&mut f).ok()?;
    let tail = tail_tags(&mut f).ok()?;
    if head.is_empty() && tail.is_empty() {
        return None;
    }
    let mut v = head;
    for _ in 0..4 {
        v.extend(blank_frame());
    }
    v.extend(tail);
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::prelude::*;

    fn ape_item(key: &str, val: &str) -> Vec<u8> {
        let mut v = (val.len() as u32).to_le_bytes().to_vec();
        v.extend(0u32.to_le_bytes());
        v.extend(key.as_bytes());
        v.push(0);
        v.extend(val.as_bytes());
        v
    }

    fn ape_tag(items: &[(&str, &str)]) -> Vec<u8> {
        let body: Vec<u8> = items.iter().flat_map(|(k, v)| ape_item(k, v)).collect();
        let hdr = |flags: u32| {
            let mut h = b"APETAGEX".to_vec();
            h.extend(2000u32.to_le_bytes());
            h.extend((body.len() as u32 + 32).to_le_bytes());
            h.extend((items.len() as u32).to_le_bytes());
            h.extend(flags.to_le_bytes());
            h.extend([0u8; 8]);
            h
        };
        let mut v = hdr(0xA000_0000);
        v.extend(&body);
        v.extend(hdr(0x8000_0000));
        v
    }

    fn id3v2_title(t: &str) -> Vec<u8> {
        let mut frame = b"TIT2".to_vec();
        frame.extend((t.len() as u32 + 1).to_be_bytes());
        frame.extend([0, 0, 0]);
        frame.extend(t.as_bytes());
        let mut v = b"ID3\x03\x00\x00".to_vec();
        let n = frame.len() as u32;
        v.extend([(n >> 21) as u8 & 0x7F, (n >> 14) as u8 & 0x7F, (n >> 7) as u8 & 0x7F, n as u8 & 0x7F]);
        v.extend(frame);
        v
    }

    #[test]
    fn tags_through_carrier() {
        let dir = std::env::temp_dir().join(format!("ap3_carrier_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.tta");
        let mut file = id3v2_title("Front");
        file.extend(b"TTA1".iter().chain([7u8; 5000].iter()));
        file.extend(ape_tag(&[("Artist", "Somebody"), ("Album", "Record")]));
        std::fs::write(&p, &file).unwrap();
        let data = build(&p).unwrap();
        let opts = lofty::config::ParseOptions::new().read_properties(false);
        let f = lofty::probe::Probe::with_file_type(std::io::Cursor::new(data), lofty::file::FileType::Mpeg).options(opts).read().unwrap();
        let get = |k: ItemKey| f.tags().iter().find_map(|t| t.get_string(k).map(str::to_string));
        assert_eq!(get(ItemKey::TrackTitle).as_deref(), Some("Front"));
        assert_eq!(get(ItemKey::TrackArtist).as_deref(), Some("Somebody"));
        assert_eq!(get(ItemKey::AlbumTitle).as_deref(), Some("Record"));
        // 没有标签
        std::fs::write(&p, b"TTA1 plain audio").unwrap();
        assert!(build(&p).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
