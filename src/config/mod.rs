//! 配置与数据目录。
//!
//! 原版提供两种配置位置："执行程序所在位置下的 AIRPLAY_CONFIG 目录" 或 "其它位置 (用户自定义)"，
//! 后者通过 exe 旁的 `AIRPLAY\CONFIG.PATH` 指针文件记录。这里保留同样的语义。

pub mod settings;
pub mod store;

use std::path::{Path, PathBuf};

pub use settings::*;

const POINTER_FILE: &str = "AIRPLAY_CONFIG.PATH";
const PORTABLE_DIR: &str = "AIRPLAY_CONFIG";

#[derive(Clone, Debug)]
pub struct DataDirs {
    pub root: PathBuf,
}

impl DataDirs {
    /// 按 环境变量 > 指针文件 > exe 旁目录 > %APPDATA% 的顺序确定数据目录。
    pub fn resolve() -> DataDirs {
        if let Ok(p) = std::env::var("AIRPLAY3_CONFIG_DIR") {
            if let Some(d) = Self::try_root(PathBuf::from(p)) {
                return d;
            }
        }
        let exe_dir = crate::util::exe_dir();
        if let Ok(text) = std::fs::read_to_string(exe_dir.join(POINTER_FILE)) {
            let p = PathBuf::from(text.trim());
            if p.is_absolute() {
                if let Some(d) = Self::try_root(p) {
                    return d;
                }
            }
        }
        if let Some(d) = Self::try_root(exe_dir.join(PORTABLE_DIR)) {
            return d;
        }
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        Self::try_root(appdata.join("AirPlay3")).unwrap_or(DataDirs { root: std::env::temp_dir().join("AirPlay3") })
    }

    fn try_root(root: PathBuf) -> Option<DataDirs> {
        std::fs::create_dir_all(&root).ok()?;
        let probe = root.join(".write_test");
        std::fs::write(&probe, b"ok").ok()?;
        let _ = std::fs::remove_file(&probe);
        let d = DataDirs { root };
        for sub in [d.lyric_dir(), d.cache_dir()] {
            let _ = std::fs::create_dir_all(sub);
        }
        Some(d)
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("config.json")
    }
    pub fn library_file(&self) -> PathBuf {
        self.root.join("library.json")
    }
    pub fn log_file(&self) -> PathBuf {
        self.root.join("airplay.log")
    }
    /// 下载/关联的歌词默认存放位置
    pub fn lyric_dir(&self) -> PathBuf {
        self.root.join("LYRIC")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("CACHE")
    }
    /// 下载的专辑图片存放位置
    pub fn cover_dir(&self) -> PathBuf {
        self.cache_dir().join("COVER")
    }

    /// 艺人图片缓存（按艺人，同一艺人的歌共用）
    pub fn artist_dir(&self) -> PathBuf {
        self.cache_dir().join("ARTIST")
    }

    /// 是否是 exe 旁的便携目录。
    pub fn is_portable(&self) -> bool {
        self.root == crate::util::exe_dir().join(PORTABLE_DIR)
    }

    /// 操作系统默认位置（%APPDATA%\AirPlay3）
    pub fn system_default() -> PathBuf {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("AirPlay3")
    }

    /// exe 旁的 AIRPLAY_CONFIG 目录
    pub fn portable_dir() -> PathBuf {
        crate::util::exe_dir().join(PORTABLE_DIR)
    }

    /// 现在用的是哪种位置（原版「配置文件存放位置」的三选一）：0 操作系统默认位置、1 exe 旁、2 其它位置
    pub fn mode(&self) -> usize {
        if self.is_portable() {
            1
        } else if self.root == Self::system_default() {
            0
        } else {
            2
        }
    }

    /// 切换配置存放位置：复制现有数据到新目录并写/删指针文件。`None` 表示回到 exe 旁目录。
    pub fn relocate(&self, target: Option<&Path>) -> Result<DataDirs, String> {
        self.relocate_from(&crate::util::exe_dir(), target)
    }

    fn relocate_from(&self, exe_dir: &Path, target: Option<&Path>) -> Result<DataDirs, String> {
        let new_root = match target {
            Some(p) => p.to_path_buf(),
            None => exe_dir.join(PORTABLE_DIR),
        };
        if new_root.starts_with(&self.root) && new_root != self.root {
            return Err("新目录不能位于当前配置目录内部".into());
        }
        let new = Self::try_root(new_root.clone()).ok_or_else(|| format!("无法写入目录: {}", new_root.display()))?;
        if new.root != self.root {
            copy_dir(&self.root, &new.root).map_err(|e| e.to_string())?;
        }
        let pointer = exe_dir.join(POINTER_FILE);
        match target {
            Some(p) => std::fs::write(&pointer, p.to_string_lossy().as_bytes()).map_err(|e| format!("无法写入指针文件: {e}"))?,
            None => {
                let _ = std::fs::remove_file(&pointer);
            }
        }
        Ok(new)
    }
}

/// 递归复制目录，已存在的文件不覆盖。
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dst = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dst)?;
        } else if !dst.exists() {
            std::fs::copy(entry.path(), &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relocate_copies_and_points() {
        let base = std::env::temp_dir().join(format!("ap3_relocate_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (exe, old, custom) = (base.join("app"), base.join("old"), base.join("custom"));
        std::fs::create_dir_all(old.join("LYRIC")).unwrap();
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::write(old.join("config.json"), b"old").unwrap();
        std::fs::write(old.join("LYRIC").join("a.lrc"), b"lrc").unwrap();
        let cur = DataDirs { root: old.clone() };
        // 其它位置：复制过去、指针文件记下位置；已有的文件不覆盖
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::write(custom.join("config.json"), b"keep").unwrap();
        let new = cur.relocate_from(&exe, Some(&custom)).unwrap();
        assert_eq!(new.root, custom);
        assert_eq!(std::fs::read(custom.join("LYRIC").join("a.lrc")).unwrap(), b"lrc");
        assert_eq!(std::fs::read(custom.join("config.json")).unwrap(), b"keep");
        assert_eq!(std::fs::read_to_string(exe.join(POINTER_FILE)).unwrap(), custom.to_string_lossy());
        // exe 旁：复制到 AIRPLAY_CONFIG，删掉指针文件
        let back = new.relocate_from(&exe, None).unwrap();
        assert_eq!(back.root, exe.join(PORTABLE_DIR));
        assert!(back.root.join("LYRIC").join("a.lrc").exists());
        assert!(!exe.join(POINTER_FILE).exists());
        // 不能搬到当前目录里面
        assert!(back.relocate_from(&exe, Some(&back.root.join("sub"))).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
