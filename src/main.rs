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

    // 卸载程序调用：删掉指向本 exe 的文件关联和开机启动后退出（不开窗口、不建数据目录）
    if args.iter().any(|a| a == "--unregister") {
        airplay3::platform::shell::unregister_self();
        return;
    }

    // 单实例：已有实例时把文件交给它（或只激活它的窗口）后退出。
    // 仅调试构建：AIRPLAY3_TEST_ALONE 时不管别的实例（自测用的副本不会把测试文件交给用户正在用的程序）
    #[cfg(debug_assertions)]
    let alone = std::env::var_os("AIRPLAY3_TEST_ALONE").is_some();
    #[cfg(not(debug_assertions))]
    let alone = false;
    if !alone && !airplay3::platform::instance::acquire() && airplay3::platform::instance::forward(&files) {
        return;
    }

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
