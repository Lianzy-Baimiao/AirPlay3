//! 界面缩放的切换：用户在菜单里改缩放，或「跟随系统」时显示缩放变了（播放条拖到另一个显示器、系统设置里改了缩放）。
//! 播放条留在屏幕上原来的地方，各面板相对播放条的位置（逻辑像素）不变，所以吸附在一起的面板还是吸附在一起。

use windows::Win32::UI::WindowsAndMessaging::*;

use super::app::App;
use super::popup::Popup;
use super::scale;

/// 拖动窗口时显示缩放变了：等松手再换（拖到一半换缩放，拖动的位置就乱了）
static PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 弹出窗口留在屏幕上原来的地方（实际像素），换成新缩放下的逻辑坐标；画布下次重画时按新缩放重建
fn rescale_popup(p: &mut Popup, old: f32) {
    let (px, py) = (scale::rnd(p.pos.0 as f32 * old), scale::rnd(p.pos.1 as f32 * old));
    p.move_to(scale::to_logical(px), scale::to_logical(py));
    let (w, h) = (p.canvas.w, p.canvas.h);
    p.resize(w, h);
}

impl App {
    /// 现在应该用的缩放（设置 + 播放条所在显示器的缩放）
    fn wanted_zoom(&self) -> u32 {
        let z = self.player.core.lock().settings.window.zoom;
        scale::effective(z)
    }

    /// 用户改缩放（None = 跟随系统）：存进设置并立即生效
    pub fn set_zoom(&mut self, zoom: Option<u32>) {
        // 选的正好是系统的缩放：记成「跟随系统」，以后系统缩放变了也跟着变
        let zoom = zoom.map(|z| z.clamp(scale::MIN, scale::MAX)).filter(|&z| z != scale::system());
        {
            let mut g = self.player.core.lock();
            g.settings.window.zoom = zoom;
            g.touch_settings();
        }
        self.apply_zoom(self.wanted_zoom());
        self.player.save(false);
    }

    /// 播放条收到 WM_DPICHANGED：记下新的系统缩放；跟随系统时换过去
    pub fn on_dpi_changed(&mut self, dpi: u32) {
        scale::set_system((dpi * 100 + 48) / 96);
        if self.ui.drag != super::state::Drag::None || self.minibar.as_ref().is_some_and(|m| m.drag.is_some()) {
            PENDING.store(true, std::sync::atomic::Ordering::Relaxed);
            return;
        }
        self.apply_zoom(self.wanted_zoom());
    }

    /// 拖动结束：拖动时显示缩放变过的话现在换
    pub fn apply_pending_zoom(&mut self) {
        if PENDING.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.apply_zoom(self.wanted_zoom());
        }
    }

    /// 播放条和看得见的面板超出了屏幕（放大了，或上次用小一点的缩放存的位置）：整组挪回播放条所在显示器的工作区，
    /// 放不下时保证左上角在屏幕里
    pub fn keep_layout_on_screen(&mut self) {
        if self.minibar.is_some() {
            return;
        }
        let rects: Vec<super::canvas::R> = self.wins.iter().filter(|(p, w)| *p == super::app::Pane::Bar || w.visible).map(|(p, _)| self.rect_of(*p)).collect();
        let (bx, by) = self.bar_pos();
        let (dx, dy) = super::snap::keep_inside(&rects, (0, 0), super::popup::work_area_at((bx + 40, by + 20)));
        if (dx, dy) == (0, 0) {
            return;
        }
        for (_, w) in self.wins.iter_mut() {
            w.pos = (w.pos.0 + dx, w.pos.1 + dy);
            unsafe {
                let _ = SetWindowPos(w.hwnd, None, scale::to_px(w.pos.0), scale::to_px(w.pos.1), 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
    }

    /// 换成 percent% 的缩放：所有窗口按新缩放重画，播放条不动，面板跟着播放条
    pub fn apply_zoom(&mut self, percent: u32) {
        let old = scale::get();
        if percent == scale::percent() {
            // 菜单上的「界面缩放」可能要更新（例如选了系统的那一档，变成跟随系统）
            if let Some(m) = self.menu.as_mut() {
                m.present();
            }
            return;
        }
        // 播放条左上角在屏幕上的实际像素位置不变
        let (bx, by) = self.bar_pos();
        let (px, py) = (scale::rnd(bx as f32 * old), scale::rnd(by as f32 * old));
        let offsets: Vec<(i32, i32)> = self.wins.iter().map(|(_, w)| (w.pos.0 - bx, w.pos.1 - by)).collect();
        scale::set_percent(percent);
        let (nx, ny) = (scale::to_logical(px), scale::to_logical(py));
        for (i, (_, w)) in self.wins.iter_mut().enumerate() {
            w.pos = (nx + offsets[i].0, ny + offsets[i].1);
            unsafe {
                let _ = SetWindowPos(w.hwnd, None, scale::to_px(w.pos.0), scale::to_px(w.pos.1), 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
        self.keep_layout_on_screen();
        if let Some(m) = self.minibar.as_mut() {
            rescale_popup(&mut m.top, old);
            rescale_popup(&mut m.bottom, old);
        }
        if let Some(s) = self.settings_win.as_mut() {
            rescale_popup(&mut s.win, old);
        }
        if let Some(s) = self.search.as_mut() {
            rescale_popup(&mut s.win, old);
        }
        if let Some(c) = self.cfgdir.as_mut() {
            rescale_popup(&mut c.win, old);
        }
        // 桌面歌词、任务栏歌词不跟随界面缩放（字号单独调），不用动
        // 开着的菜单（在菜单里点的 − / +）留在原来的地方，按新缩放重画
        if let Some(m) = self.menu.as_mut() {
            m.rescale(old);
        }
        log::info!("界面缩放 {}%", percent);
        self.save_layout();
        self.dirty_all();
        for (on, f) in [
            (self.search.is_some(), App::paint_search as fn(&mut App)),
            (self.cfgdir.is_some(), App::paint_cfgdir),
            (self.minibar.is_some(), App::paint_minibar),
        ] {
            if on {
                f(self);
            }
        }
        self.flush();
    }
}
