#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! AirPlay3 —— 程序入口。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};

use airplay3::config::DataDirs;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let files: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    let start_in_tray = args.iter().any(|a| a == "--tray");

    let dirs = DataDirs::resolve();
    airplay3::util::logger::init(&dirs.log_file());
    log::info!("AirPlay3 {} 启动，配置目录 {}", env!("CARGO_PKG_VERSION"), dirs.root.display());

    // 后台线程（播放、导入、歌词下载）状态变化时，向播放条窗口投递重绘消息
    static BAR: AtomicIsize = AtomicIsize::new(0);
    let repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {
        let h = BAR.load(Ordering::Acquire);
        if h != 0 {
            airplay3::gui::app::post_repaint(h);
        }
    });
    let ctl = airplay3::controller::start(dirs, repaint);
    if files.is_empty() {
        ctl.player.restore_session();
    } else {
        airplay3::controller::open_files(&ctl.player, files);
    }
    airplay3::gui::run(ctl.player.clone(), ctl.now.clone(), ctl.cmds.clone(), start_in_tray, &BAR);
    ctl.player.save(true);
    log::info!("退出");
}
