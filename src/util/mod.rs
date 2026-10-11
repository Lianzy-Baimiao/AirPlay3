//! 通用小工具：时间格式化、路径、文本编码等。

pub mod logger;
pub mod text;

use std::path::{Path, PathBuf};
use std::time::Duration;

/// 格式化为 `m:ss` 或 `h:mm:ss`（与原版 `%d:%02d` / `%d:%02d:%02d` 一致）。
pub fn fmt_time(secs: f64) -> String {
    let s = if secs.is_finite() && secs > 0.0 { secs as u64 } else { 0 };
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

pub fn fmt_duration(d: Duration) -> String {
    fmt_time(d.as_secs_f64())
}

/// 文件扩展名（小写，不含点）。
pub fn ext_lower(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// 文件名主干（不含扩展名）。
pub fn file_stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// 可执行文件所在目录。
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 去掉文件名里 Windows 不允许的字符。
pub fn sanitize_file_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '_' } else { c })
        .collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "_".into() } else { s }
}

/// 当前 Unix 时间（秒）。
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_format() {
        assert_eq!(fmt_time(0.0), "0:00");
        assert_eq!(fmt_time(65.9), "1:05");
        assert_eq!(fmt_time(3725.0), "1:02:05");
        assert_eq!(fmt_time(f64::NAN), "0:00");
    }

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_file_name("AC/DC: Back?"), "AC_DC_ Back_");
        assert_eq!(sanitize_file_name("  ..."), "_");
    }
}
