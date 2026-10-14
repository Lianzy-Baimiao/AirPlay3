//! 配置与数据目录（配置、曲库、歌词、图片缓存、日志）。
//!
//! 平时存在操作系统默认位置 `%APPDATA%\AirPlay3`：exe 放在哪（比如桌面）旁边都不会多出东西。
//! exe 旁边已经有 `AIRPLAY_CONFIG` 文件夹时（发布的 zip 里带着）就用它，整个文件夹拷走就能用（便携）。
//! 「配置文件存放位置」里选「其它位置」时，选的目录记在 `%APPDATA%\AirPlay3\AIRPLAY_CONFIG.PATH` 里；
//! 旧版本记在 exe 旁的同名文件里，仍然认。

pub mod settings;
pub mod store;

use std::path::{Path, PathBuf};

pub use settings::*;

const POINTER_FILE: &str = "AIRPLAY_CONFIG.PATH";
const PORTABLE_DIR: &str = "AIRPLAY_CONFIG";
/// 离开便携目录时，先把它改成这个名字再删（删到一半失败也不会留下一个缺东西的 AIRPLAY_CONFIG）
const REMOVING_DIR: &str = "AIRPLAY_CONFIG.removing";
const LOG_FILE: &str = "airplay.log";

#[derive(Clone, Debug)]
pub struct DataDirs {
    pub root: PathBuf,
}

/// 「配置文件存放位置」的三种选择
#[derive(Clone, Debug, PartialEq)]
pub enum Location {
    /// 操作系统默认位置（%APPDATA%\AirPlay3）
    System,
    /// exe 旁的 AIRPLAY_CONFIG（便携）
    Portable,
    /// 其它位置（用户选的文件夹）
    Custom(PathBuf),
}

/// 决定数据目录时要看的几个地方（测试时换成临时目录）
struct Env {
    exe_dir: PathBuf,
    appdata: PathBuf,
}

impl Env {
    fn real() -> Env {
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        Env { exe_dir: crate::util::exe_dir(), appdata }
    }
    fn system(&self) -> PathBuf {
        self.appdata.join("AirPlay3")
    }
    fn portable(&self) -> PathBuf {
        self.exe_dir.join(PORTABLE_DIR)
    }
    /// 「其它位置」的指针文件（新）
    fn pointer(&self) -> PathBuf {
        self.system().join(POINTER_FILE)
    }
    /// 旧版本写在 exe 旁的指针文件
    fn legacy_pointer(&self) -> PathBuf {
        self.exe_dir.join(POINTER_FILE)
    }
}

fn read_pointer(f: &Path) -> Option<PathBuf> {
    let p = PathBuf::from(std::fs::read_to_string(f).ok()?.trim());
    p.is_absolute().then_some(p)
}

impl DataDirs {
    /// 按 环境变量 > exe 旁的旧指针文件 > exe 旁已有的 AIRPLAY_CONFIG > 「其它位置」 > %APPDATA%\AirPlay3 的顺序确定数据目录。
    /// exe 旁的 AIRPLAY_CONFIG 只在已经有时才用，不会自己建
    pub fn resolve() -> DataDirs {
        if let Ok(p) = std::env::var("AIRPLAY3_CONFIG_DIR") {
            if let Some(d) = Self::try_root(PathBuf::from(p)) {
                return d;
            }
        }
        Self::resolve_in(&Env::real())
    }

    fn resolve_in(env: &Env) -> DataDirs {
        // 上次离开便携目录时没删干净的
        let _ = std::fs::remove_dir_all(env.exe_dir.join(REMOVING_DIR));
        let custom = |f: PathBuf| read_pointer(&f).and_then(Self::try_root);
        if let Some(d) = custom(env.legacy_pointer()) {
            return d;
        }
        if env.portable().is_dir() {
            if let Some(d) = Self::try_root(env.portable()) {
                return d;
            }
        }
        if let Some(d) = custom(env.pointer()) {
            return d;
        }
        Self::try_root(env.system()).unwrap_or(DataDirs { root: std::env::temp_dir().join("AirPlay3") })
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

    /// 操作系统默认位置（%APPDATA%\AirPlay3）
    pub fn system_default() -> PathBuf {
        Env::real().system()
    }

    /// exe 旁的 AIRPLAY_CONFIG 目录
    pub fn portable_dir() -> PathBuf {
        Env::real().portable()
    }

    /// 现在用的是哪种位置（原版「配置文件存放位置」的三选一）：0 操作系统默认位置、1 exe 旁、2 其它位置
    pub fn mode(&self) -> usize {
        Self::mode_in(&self.root, &Env::real())
    }

    fn mode_in(root: &Path, env: &Env) -> usize {
        if root == env.portable() {
            1
        } else if root == env.system() {
            0
        } else {
            2
        }
    }

    /// 切换配置存放位置：把现有数据复制到新位置，记下「其它位置」。离开 exe 旁的 AIRPLAY_CONFIG 时把它删掉
    /// （不然下次启动又会用它），这时以它为准覆盖新位置里的同名文件
    pub fn relocate(&self, to: &Location) -> Result<DataDirs, String> {
        self.relocate_in(&Env::real(), to)
    }

    fn relocate_in(&self, env: &Env, to: &Location) -> Result<DataDirs, String> {
        let new_root = match to {
            Location::System => env.system(),
            Location::Portable => env.portable(),
            Location::Custom(p) => p.clone(),
        };
        if new_root.starts_with(&self.root) && new_root != self.root {
            return Err("新目录不能位于当前配置目录内部".into());
        }
        let leaving_portable = self.root == env.portable() && new_root != self.root;
        let new = Self::try_root(new_root.clone()).ok_or_else(|| format!("无法写入目录: {}", new_root.display()))?;
        if new.root != self.root {
            copy_dir(&self.root, &new.root, leaving_portable).map_err(|e| e.to_string())?;
        }
        // 「其它位置」记在 %APPDATA%\AirPlay3 里；exe 旁的旧指针文件一律去掉
        let legacy = env.legacy_pointer();
        remove_file_if_any(&legacy).map_err(|e| format!("无法删除 {}: {e}", legacy.display()))?;
        match to {
            Location::Custom(p) => {
                std::fs::create_dir_all(env.system()).map_err(|e| e.to_string())?;
                std::fs::write(env.pointer(), p.to_string_lossy().as_bytes()).map_err(|e| format!("无法写入指针文件: {e}"))?;
            }
            Location::System => remove_file_if_any(&env.pointer()).map_err(|e| e.to_string())?,
            Location::Portable => {}
        }
        // 日志也换到新位置（关掉旧目录里的日志文件，旧目录才能删掉）
        crate::util::logger::retarget(&new.log_file());
        if leaving_portable {
            // 先改名再删：改名不成功时什么都没动，删到一半失败也不会留下一个缺东西的 AIRPLAY_CONFIG
            let gone = env.exe_dir.join(REMOVING_DIR);
            let _ = std::fs::remove_dir_all(&gone);
            if let Err(e) = std::fs::rename(&self.root, &gone) {
                crate::util::logger::retarget(&self.log_file());
                return Err(format!("数据已复制到新位置，但无法移走 {}: {e}", self.root.display()));
            }
            let _ = std::fs::remove_dir_all(&gone);
        }
        Ok(new)
    }
}

fn remove_file_if_any(f: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(f) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// 递归复制目录；overwrite 为 false 时已存在的文件不覆盖。指针文件和日志（正开着）不复制
fn copy_dir(from: &Path, to: &Path, overwrite: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == POINTER_FILE || name == LOG_FILE {
            continue;
        }
        let dst = to.join(&name);
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dst, overwrite)?;
        } else if overwrite || !dst.exists() {
            std::fs::copy(entry.path(), &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 临时的「exe 所在目录」和「%APPDATA%」
    fn sandbox(name: &str) -> (PathBuf, Env) {
        let base = std::env::temp_dir().join(format!("ap3_dirs_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("Desktop")).unwrap();
        let env = Env { exe_dir: base.join("Desktop"), appdata: base.join("Roaming") };
        (base, env)
    }

    fn names(d: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d).map_or(Vec::new(), |r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect());
        v.sort();
        v
    }

    /// exe 放在桌面上直接运行：数据在 %APPDATA%\AirPlay3，桌面上什么都不多
    #[test]
    fn fresh_start_leaves_exe_dir_alone() {
        let (base, env) = sandbox("fresh");
        let d = DataDirs::resolve_in(&env);
        assert_eq!(d.root, env.system());
        assert!(d.lyric_dir().is_dir() && d.cache_dir().is_dir());
        assert!(names(&env.exe_dir).is_empty(), "{:?}", names(&env.exe_dir));
        assert_eq!(DataDirs::mode_in(&d.root, &env), 0);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// exe 旁已经有 AIRPLAY_CONFIG（zip 里带着）：便携，优先于「其它位置」；旧版本 exe 旁的指针文件仍然认
    #[test]
    fn portable_and_pointers() {
        let (base, env) = sandbox("portable");
        std::fs::create_dir_all(env.portable()).unwrap();
        assert_eq!(DataDirs::resolve_in(&env).root, env.portable());
        assert_eq!(DataDirs::mode_in(&env.portable(), &env), 1);
        let custom = base.join("custom");
        std::fs::create_dir_all(env.system()).unwrap();
        std::fs::write(env.pointer(), custom.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(DataDirs::resolve_in(&env).root, env.portable());
        std::fs::remove_dir_all(env.portable()).unwrap();
        assert_eq!(DataDirs::resolve_in(&env).root, custom);
        assert_eq!(DataDirs::mode_in(&custom, &env), 2);
        let old = base.join("old");
        std::fs::write(env.legacy_pointer(), old.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(DataDirs::resolve_in(&env).root, old);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// 换位置：数据跟过去；离开 exe 旁的 AIRPLAY_CONFIG 时它被删掉、以它为准；其它位置记在 %APPDATA% 里
    #[test]
    fn relocate_moves_data() {
        let (base, env) = sandbox("relocate");
        let p = env.portable();
        std::fs::create_dir_all(p.join("LYRIC")).unwrap();
        std::fs::write(p.join("config.json"), b"new").unwrap();
        std::fs::write(p.join("LYRIC").join("a.lrc"), b"lrc").unwrap();
        std::fs::write(p.join(LOG_FILE), b"log").unwrap();
        std::fs::write(env.legacy_pointer(), b"C:\\nowhere").unwrap();
        std::fs::create_dir_all(env.system()).unwrap();
        std::fs::write(env.system().join("config.json"), b"stale").unwrap();
        let cur = DataDirs { root: p.clone() };
        // 便携 → 操作系统默认位置：覆盖旧的同名文件，exe 旁什么都不剩
        let sys = cur.relocate_in(&env, &Location::System).unwrap();
        assert_eq!(sys.root, env.system());
        assert_eq!(std::fs::read(sys.root.join("config.json")).unwrap(), b"new");
        assert_eq!(std::fs::read(sys.root.join("LYRIC").join("a.lrc")).unwrap(), b"lrc");
        assert!(!sys.root.join(LOG_FILE).exists());
        assert!(names(&env.exe_dir).is_empty(), "{:?}", names(&env.exe_dir));
        assert_eq!(DataDirs::resolve_in(&env).root, env.system());
        // → 其它位置：复制过去（已有的文件不覆盖），指针记在 %APPDATA%\AirPlay3
        let custom = base.join("custom");
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::write(custom.join("config.json"), b"keep").unwrap();
        let c = sys.relocate_in(&env, &Location::Custom(custom.clone())).unwrap();
        assert_eq!(std::fs::read(custom.join("config.json")).unwrap(), b"keep");
        assert!(custom.join("LYRIC").join("a.lrc").exists() && !custom.join(POINTER_FILE).exists());
        assert_eq!(DataDirs::resolve_in(&env).root, custom);
        // → 便携：exe 旁有了 AIRPLAY_CONFIG，就用它
        let back = c.relocate_in(&env, &Location::Portable).unwrap();
        assert_eq!(back.root, p);
        assert_eq!(DataDirs::resolve_in(&env).root, p);
        // → 操作系统默认位置：「其它位置」的指针也去掉
        back.relocate_in(&env, &Location::System).unwrap();
        assert!(!env.pointer().exists() && names(&env.exe_dir).is_empty());
        assert_eq!(DataDirs::resolve_in(&env).root, env.system());
        // 不能搬到当前目录里面
        assert!(sys.relocate_in(&env, &Location::Custom(sys.root.join("sub"))).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
