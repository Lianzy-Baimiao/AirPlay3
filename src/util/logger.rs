//! 极简文件日志：写到配置目录下的 `airplay.log`，超过 1 MB 时启动时截断。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use parking_lot::Mutex;

struct FileLogger {
    file: Mutex<Option<File>>,
}

impl log::Log for FileLogger {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        let t = meta.target();
        let noisy = ["egui", "eframe", "winit", "symphonia", "glutin", "lofty", "ureq", "rustls", "native_tls"]
            .iter()
            .any(|p| t.starts_with(p));
        if noisy { meta.level() <= log::Level::Warn } else { meta.level() <= log::Level::Debug }
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("{} {:<5} [{}] {}\n", timestamp(), record.level(), record.target(), record.args());
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        if let Some(f) = self.file.lock().as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Some(f) = self.file.lock().as_mut() {
            let _ = f.flush();
        }
    }
}

fn timestamp() -> String {
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
    )
}

/// 初始化全局日志。可重复调用（只有第一次生效）。
pub fn init(path: &Path) {
    let too_big = std::fs::metadata(path).map(|m| m.len() > 1 << 20).unwrap_or(false);
    let file = OpenOptions::new()
        .create(true)
        .append(!too_big)
        .write(true)
        .truncate(too_big)
        .open(path)
        .ok();
    let logger = Box::leak(Box::new(FileLogger { file: Mutex::new(file) }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
}
