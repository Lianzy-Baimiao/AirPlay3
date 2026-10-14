//! 「设置用户配置数据及歌词图片缓存保存位置」窗口（原版的「配置文件存放位置」，从「配置 → 基本设置」
//! 最下面一行打开）：三选一，确认后把配置、曲库、歌词、图片缓存复制到新位置，以后都存在那里。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::actions::AsyncMsg;
use super::app::{App, WM_APP_ASYNC};
use super::panels::cfgdir::{self as cp, CfgDirModel, Hot};
use super::popup::Popup;
use crate::config::DataDirs;

pub struct CfgDirWin {
    pub win: Popup,
    /// 0 操作系统默认位置、1 exe 旁的 AIRPLAY_CONFIG、2 其它位置
    pub choice: usize,
    /// 「其它位置」选的文件夹
    pub custom: Option<std::path::PathBuf>,
    pub hot: Option<Hot>,
    pub path_hot: bool,
    /// 选文件夹的对话框开着（原版这时整个窗口变暗、不响应）
    pub picking: bool,
    /// 「确认」显示着（选文件夹时保持打开对话框前的样子）
    pub can_ok: bool,
    pub drag: Option<(i32, i32)>,
    pub status: String,
}

impl CfgDirWin {
    /// 按所选的位置，数据会存在哪
    fn target(&self) -> Option<std::path::PathBuf> {
        match self.choice {
            0 => Some(DataDirs::system_default()),
            1 => Some(DataDirs::portable_dir()),
            _ => self.custom.clone(),
        }
    }
}

impl App {
    pub fn open_cfgdir(&mut self) {
        if let Some(w) = &self.cfgdir {
            w.win.show(true);
            return;
        }
        let dirs = self.player.core.lock().dirs.clone();
        let choice = dirs.mode();
        // 原版按整个屏幕（含任务栏）居中
        let m = super::popup::monitor_at(super::resize::cursor());
        let (x, y) = (m.x + (m.w - cp::W) / 2, m.y + (m.h - cp::H) / 2);
        let win = Popup::new(x, y, cp::W, cp::H, Some(self.bar_hwnd()), "配置文件存放位置");
        super::window::set_frost(win.hwnd, false);
        let custom = (choice == 2).then(|| dirs.root.clone());
        self.cfgdir = Some(CfgDirWin { win, choice, custom, hot: None, path_hot: false, picking: false, can_ok: true, drag: None, status: String::new() });
        self.paint_cfgdir();
        if let Some(w) = &self.cfgdir {
            w.win.show(true);
        }
        self.dim_settings(true);
    }

    pub fn paint_cfgdir(&mut self) {
        let Some(w) = self.cfgdir.as_mut() else { return };
        let target = w.target();
        if !w.picking {
            w.can_ok = target.is_some();
        }
        // 原版显示的路径末尾带「\」；选文件夹时还没有路径的框是空的
        let path = target.map_or(String::new(), |p| {
            let s = p.display().to_string();
            if s.ends_with('\\') { s } else { s + "\\" }
        });
        let m = CfgDirModel { choice: w.choice, path: &path, status: &w.status, hot: w.hot, path_hot: w.path_hot, can_ok: w.can_ok, dimmed: w.picking };
        cp::paint(&mut w.win.canvas, &m);
        #[cfg(debug_assertions)]
        w.win.canvas.dump("cfgdir");
        w.win.present();
    }

    pub fn close_cfgdir(&mut self) {
        if let Some(mut w) = self.cfgdir.take() {
            w.win.close();
        }
        self.dim_settings(false);
    }

    /// 打开选文件夹的对话框（以这个窗口为父窗口，开着时它不响应）
    fn pick_cfgdir(&mut self) {
        let (tx, bar) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        let Some(w) = self.cfgdir.as_mut() else { return };
        if w.picking {
            return;
        }
        (w.picking, w.win.dirty) = (true, true);
        let owner = w.win.hwnd.0 as isize;
        std::thread::spawn(move || {
            let d = rfd::FileDialog::new().set_parent(&crate::platform::window::Parent(owner)).pick_folder();
            let _ = tx.send(AsyncMsg::DataDir(d));
            unsafe {
                let _ = PostMessageW(Some(HWND(bar as _)), WM_APP_ASYNC, WPARAM(0), LPARAM(0));
            }
        });
    }

    /// 选文件夹的对话框关了：选了文件夹就改成「其它位置」；取消则不变
    pub fn on_cfgdir_picked(&mut self, d: Option<std::path::PathBuf>) {
        let Some(w) = self.cfgdir.as_mut() else { return };
        w.picking = false;
        if let Some(d) = d {
            (w.custom, w.choice) = (Some(d), 2);
            w.status.clear();
        }
        self.paint_cfgdir();
    }

    /// 确认：复制数据到新位置，以后都存在那里
    fn confirm_cfgdir(&mut self) {
        let Some(w) = self.cfgdir.as_ref() else { return };
        let Some(target) = w.target().filter(|_| !w.picking) else { return };
        let choice = w.choice;
        let cur = self.player.core.lock().dirs.root.clone();
        if target == cur {
            self.close_cfgdir();
            return;
        }
        let to = match choice {
            0 => crate::config::Location::System,
            1 => crate::config::Location::Portable,
            _ => crate::config::Location::Custom(target.clone()),
        };
        let r = self.player.core.lock().move_data(&to);
        match r {
            Ok(()) => {
                self.player.save(true);
                log::info!("配置数据改存到 {}", target.display());
                self.close_cfgdir();
            }
            Err(e) => {
                if let Some(w) = self.cfgdir.as_mut() {
                    w.status = e;
                    w.win.dirty = true;
                }
            }
        }
    }

    /// 窗口的消息；不是这个窗口时返回 None
    pub fn cfgdir_msg(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if self.cfgdir.as_ref()?.win.hwnd != hwnd {
            return None;
        }
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        let w = self.cfgdir.as_mut()?;
        let (hit, path_hot) = cp::hit(x, y, w.can_ok);
        match msg {
            WM_MOUSEMOVE => {
                if let Some((dx, dy)) = w.drag {
                    let (cx, cy) = super::resize::cursor();
                    w.win.move_to(cx - dx, cy - dy);
                    return Some(LRESULT(0));
                }
                if (hit, path_hot) != (w.hot, w.path_hot) {
                    (w.hot, w.path_hot, w.win.dirty) = (hit, path_hot, true);
                }
                self.track_leave(hwnd);
            }
            // WM_MOUSELEAVE
            0x02A3 => {
                (w.hot, w.path_hot, w.win.dirty) = (None, false, true);
            }
            WM_LBUTTONDOWN => match hit {
                Some(Hot::Choice(i)) => {
                    (w.choice, w.win.dirty) = (i, true);
                    w.status.clear();
                    // 原版点「其它位置」就弹出选文件夹
                    if i == 2 {
                        self.pick_cfgdir();
                    }
                }
                // 点路径：在资源管理器里打开现在的数据目录
                Some(Hot::Path) => crate::platform::shell::open(&self.player.core.lock().dirs.root.to_string_lossy()),
                Some(Hot::Plus) => self.pick_cfgdir(),
                Some(Hot::Ok) => self.confirm_cfgdir(),
                Some(Hot::Cancel) => self.close_cfgdir(),
                None => {
                    let (cx, cy) = super::resize::cursor();
                    w.drag = Some((cx - w.win.pos.0, cy - w.win.pos.1));
                    unsafe { SetCapture(hwnd) };
                }
            },
            WM_LBUTTONUP => {
                w.drag = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            WM_KEYDOWN if wp.0 == 0x1B => self.close_cfgdir(),
            WM_KEYDOWN if wp.0 == 0x0D => self.confirm_cfgdir(),
            WM_CLOSE => self.close_cfgdir(),
            _ => return None,
        }
        if self.cfgdir.as_ref().is_some_and(|w| w.win.dirty) {
            self.paint_cfgdir();
        }
        Some(LRESULT(0))
    }
}
