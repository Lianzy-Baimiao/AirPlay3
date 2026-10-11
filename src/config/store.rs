//! JSON 文件的原子读写：先写 `.tmp` 再替换，旧文件留作 `.bak`；读取失败时回退到 `.bak`。

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

fn with_ext(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

/// 读取 JSON。文件不存在返回 `None`；主文件损坏时尝试 `.bak`，并把损坏文件改名为 `.broken` 以免被覆盖。
pub fn load_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(v) => return Some(v),
            Err(e) => {
                log::error!("{} 解析失败: {e}", path.display());
                let _ = std::fs::rename(path, with_ext(path, ".broken"));
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::error!("{} 读取失败: {e}", path.display()),
    }
    let bak = with_ext(path, ".bak");
    let bytes = std::fs::read(&bak).ok()?;
    let v = serde_json::from_slice(&bytes).ok();
    if v.is_some() {
        log::warn!("已从备份恢复 {}", bak.display());
    }
    v
}

pub fn to_bytes<T: Serialize>(value: &T, pretty: bool) -> serde_json::Result<Vec<u8>> {
    if pretty { serde_json::to_vec_pretty(value) } else { serde_json::to_vec(value) }
}

/// 原子写入已序列化好的字节。
pub fn save_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = with_ext(path, ".tmp");
    std::fs::write(&tmp, bytes)?;
    if path.exists() {
        let _ = std::fs::copy(path, with_ext(path, ".bak"));
    }
    std::fs::rename(&tmp, path)
}

pub fn save_json<T: Serialize>(path: &Path, value: &T, pretty: bool) -> std::io::Result<()> {
    let bytes = to_bytes(value, pretty).map_err(std::io::Error::other)?;
    save_bytes(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_roundtrip_and_recovery() {
        let dir = std::env::temp_dir().join(format!("ap3_store_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.json");
        save_json(&f, &vec![1, 2, 3], false).unwrap();
        save_json(&f, &vec![4, 5], false).unwrap();
        assert_eq!(load_json::<Vec<i32>>(&f), Some(vec![4, 5]));
        std::fs::write(&f, b"{garbage").unwrap();
        // 主文件损坏 → 用 .bak（上一次的内容）
        assert_eq!(load_json::<Vec<i32>>(&f), Some(vec![1, 2, 3]));
        assert!(dir.join("x.json.broken").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
