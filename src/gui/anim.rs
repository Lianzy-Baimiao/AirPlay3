//! 窗口动画。
//!
//! 打开（照原版实测：orig.py rectpoll 记录窗口矩形 + 每秒 60 帧截屏，2026-10-13 / 14）：
//! - 启动、打开面板、从迷你条回来、打开「配置」：面板的框（标题条和边，主体透明）从主体中心一块 4 像素的小框，
//!   分 8 步（约 0.36 秒，先快后慢）长到原来的大小；框长好后主体出现，里面的内容在约 0.33 秒里淡入
//! - 启动时播放条在 0.17 ~ 0.47 秒淡入
//!
//! 关闭、最小化、隐藏面板、进入迷你条：原版是一下子消失的（这台机器上量过），这里按用户的要求做成打开的倒放
//! （内容 0.1 秒淡出，框再分 8 步缩回中心，约 0.34 秒；播放条同时淡出），动画完了再真正隐藏 / 最小化 / 退出。
//! 从最小化 / 托盘还原时同样播放打开的动画。

use std::time::Instant;

use super::canvas::Canvas;
use super::panels::frame;

/// 框长大的 8 步：宽、高按这个比例从 4 像素长到原来的大小（照原版量的）
const STEPS: [f32; 9] = [0.0, 0.267, 0.483, 0.643, 0.762, 0.853, 0.921, 0.971, 1.0];
/// 打开：每步约 45 毫秒，框长好后内容淡入 0.33 秒
const STEP_IN: f32 = 0.045;
const GROW_IN: f32 = STEP_IN * 8.0;
const FADE_IN: f32 = 0.33;
/// 关闭：内容先淡出 0.1 秒，框再缩回去（每步 30 毫秒）
const FADE_OUT: f32 = 0.10;
const STEP_OUT: f32 = 0.030;
const SHRINK_OUT: f32 = STEP_OUT * 8.0;
pub const PANE_OUT: f32 = FADE_OUT + SHRINK_OUT;
/// 播放条（迷你条）淡入、淡出的时间；启动时淡入前先等 0.17 秒（原版）
pub const BAR_IN: f32 = 0.30;
pub const BAR_OUT: f32 = PANE_OUT;
pub const BAR_DELAY: f32 = 0.17;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    In,
    Out,
}

/// 一个窗口正在播放的动画
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    pub t0: Instant,
    pub dir: Dir,
    /// 开始前先等多久（启动时的播放条），这段时间完全透明
    pub delay: f32,
    /// 关闭动画完了就隐藏这个窗口（单独关掉一个面板）；false = 等整体的操作（最小化、退出……）来处理
    pub hide: bool,
}

impl Anim {
    pub fn open(delay: f32) -> Anim {
        Anim { t0: Instant::now(), dir: Dir::In, delay, hide: false }
    }

    pub fn close(hide: bool) -> Anim {
        Anim { t0: Instant::now(), dir: Dir::Out, delay: 0.0, hide }
    }

    /// 从动画开始（等待之后）算起的秒数，等待期间为负
    pub fn t(&self) -> f32 {
        self.t0.elapsed().as_secs_f32() - self.delay
    }

    /// 关闭动画已经播完（窗口已经完全透明）
    pub fn out_done(&self, bar: bool) -> bool {
        self.dir == Dir::Out && self.t() >= if bar { BAR_OUT } else { PANE_OUT }
    }
}

/// 预乘颜色整体乘 a（淡入淡出）
pub fn fade(c: &mut Canvas, a: f32) {
    if a >= 1.0 {
        return;
    }
    let k = (a.max(0.0) * 256.0) as u32;
    for p in c.px.iter_mut() {
        for v in p.iter_mut() {
            *v = ((*v as u32 * k) >> 8) as u8;
        }
    }
}

/// 播放条、迷你条这种只淡入淡出的窗口在 t 秒时的不透明度
pub fn fade_alpha(a: &Anim, t: f32) -> f32 {
    match a.dir {
        Dir::In => (t / BAR_IN).clamp(0.0, 1.0),
        Dir::Out => 1.0 - (t / BAR_OUT).clamp(0.0, 1.0),
    }
}

/// 把 src 原样复制到 dst 的 (ox, oy)（超出的部分不画）
fn copy_into(dst: &mut Canvas, src: &Canvas, ox: i32, oy: i32) {
    for y in 0..src.h {
        let dy = oy + y;
        if dy < 0 || dy >= dst.h {
            continue;
        }
        for x in 0..src.w {
            let dx = ox + x;
            if dx >= 0 && dx < dst.w {
                dst.px[(dy * dst.w + dx) as usize] = src.px[(y * src.w + x) as usize];
            }
        }
    }
}

/// 只有框和主体底色的画面（内容淡入 / 淡出时的另一端），返回主体区域
fn chrome(w: i32, h: i32, strip_hover: f32) -> (Canvas, super::canvas::R) {
    let mut f = Canvas::new(w, h);
    let body = frame::panel(&mut f, strip_hover);
    (f, body)
}

/// 主体从中心 4 像素长到原来大小的比例为 k 时的框（主体透明），画到 c 上
fn grown(c: &mut Canvas, body: super::canvas::R, k: f32, strip_hover: f32) {
    let (left, top, right, bottom) = (body.x, body.y, c.w - body.right(), c.h - body.bottom());
    let bw = (4.0 + (body.w - 4).max(0) as f32 * k).round() as i32;
    let bh = (4.0 + (body.h - 4).max(0) as f32 * k).round() as i32;
    let mut g = Canvas::new(bw + left + right, bh + top + bottom);
    let inner = frame::panel(&mut g, strip_hover);
    for y in inner.y..inner.bottom() {
        for x in inner.x..inner.right() {
            g.px[(y * g.w + x) as usize] = [0; 4];
        }
    }
    let (cx, cy) = (body.x + body.w / 2, body.y + body.h / 2);
    c.clear();
    copy_into(c, &g, cx - bw / 2 - left, cy - bh / 2 - top);
}

/// 内容在 a（0 = 只有框和底色，1 = 完整画面）之间插值（预乘颜色直接插值）
fn mix(c: &mut Canvas, base: &Canvas, a: f32) {
    let k = (a.clamp(0.0, 1.0) * 256.0) as i32;
    for (d, s) in c.px.iter_mut().zip(base.px.iter()) {
        for i in 0..4 {
            d[i] = (s[i] as i32 + (((d[i] as i32 - s[i] as i32) * k) >> 8)) as u8;
        }
    }
}

/// 带外框的窗口（面板、配置窗口）在动画 t 秒时的画面；c 里已经画好了完整的一帧。
/// 返回 false = 动画已经播完（关闭动画播完时 c 被清成全透明）
pub fn panel_frame(c: &mut Canvas, a: &Anim, strip_hover: f32) -> bool {
    let t = a.t().max(0.0);
    match a.dir {
        Dir::In => {
            if t >= GROW_IN + FADE_IN {
                return false;
            }
            let (base, body) = chrome(c.w, c.h, strip_hover);
            if t < GROW_IN {
                grown(c, body, STEPS[((t / STEP_IN) as usize).min(8)], strip_hover);
            } else {
                mix(c, &base, (t - GROW_IN) / FADE_IN);
            }
        }
        Dir::Out => {
            if t >= PANE_OUT {
                c.clear();
                return false;
            }
            let (base, body) = chrome(c.w, c.h, strip_hover);
            if t < FADE_OUT {
                mix(c, &base, 1.0 - t / FADE_OUT);
            } else {
                let i = (((t - FADE_OUT) / STEP_OUT) as usize).min(8);
                grown(c, body, STEPS[8 - i], strip_hover);
            }
        }
    }
    true
}

/// 播放条在动画 t 秒时的画面（整体淡入 / 淡出）；返回 false = 动画已经播完
pub fn fade_frame(c: &mut Canvas, a: &Anim) -> bool {
    let t = a.t();
    let a2 = fade_alpha(a, t.max(0.0));
    let going = match a.dir {
        Dir::In => t < BAR_IN,
        Dir::Out => t < BAR_OUT,
    };
    if t < 0.0 {
        c.clear();
    } else {
        fade(c, a2);
    }
    going
}

/// 关闭动画播完以后要做的事（对所有主界面窗口）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum After {
    /// 最小化到任务栏
    Minimize,
    /// 缩到托盘（隐藏所有窗口）
    Tray,
    /// 换成迷你条
    Minibar,
    /// 迷你条缩到托盘
    MinibarTray,
    Quit,
}

/// 主界面不在屏幕上的原因
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Away {
    /// 最小化到了任务栏（什么都不画；播放条停在关闭动画最后透明的那一帧，从任务栏还原时淡入）
    Iconic,
    /// 缩到了托盘（什么都不画）
    Tray,
}

use windows::Win32::UI::WindowsAndMessaging::*;

use super::app::{App, Pane};

impl App {
    /// 画好一帧后调用：动画还没完时把画面换成动画的这一帧；返回 true = 还要接着重画
    pub fn apply_anim(&mut self, i: usize, c: &mut Canvas) -> bool {
        let (pane, ref w) = self.wins[i];
        let Some(a) = w.anim else { return false };
        let going = match pane {
            Pane::Bar => fade_frame(c, &a),
            _ => panel_frame(c, &a, self.ui.strip_hover[i]),
        };
        // 打开动画播完就去掉；关闭动画留着（画面保持透明），等真正隐藏时再去掉
        if !going && a.dir == Dir::In {
            self.wins[i].1.anim = None;
        }
        going
    }

    /// 这个窗口现在要不要画（缩到托盘、最小化、迷你条时看不见的窗口不画）
    pub fn paints(&self, _p: Pane) -> bool {
        self.away.is_none() && self.minibar.is_none()
    }

    /// 所有关闭动画都播完了（面板、播放条、迷你条）
    fn outs_done(&self) -> bool {
        let wins = self.wins.iter().all(|(p, w)| w.anim.is_none_or(|a| a.dir == Dir::In || a.out_done(*p == Pane::Bar)));
        let mini = self.minibar.as_ref().is_none_or(|m| m.fade.is_none_or(|a| a.dir == Dir::In || a.out_done(true)));
        wins && mini
    }

    /// 每 30 毫秒：关闭动画播完的窗口隐藏掉，所有关闭动画都完了就做 `after`
    pub fn tick_anim(&mut self) {
        for (_, w) in self.wins.iter_mut() {
            if let Some(a) = w.anim
                && a.hide
                && a.out_done(false)
            {
                w.anim = None;
                unsafe {
                    let _ = ShowWindow(w.hwnd, SW_HIDE);
                }
            }
        }
        if self.settings_win.as_ref().is_some_and(|s| s.anim.is_some_and(|a| a.out_done(false))) {
            if let Some(mut s) = self.settings_win.take() {
                s.win.close();
            }
        }
        if let Some(after) = self.after
            && self.outs_done()
        {
            self.after = None;
            self.finish(after);
        }
        // 从任务栏还原了但没收到 WM_SIZE（比如在别的窗口过程里被同步还原）：补上
        if self.away == Some(Away::Iconic) && self.after.is_none() && unsafe { !IsIconic(self.bar_hwnd()).as_bool() } {
            self.on_restored();
        }
    }

    /// 关闭动画播完了：真正最小化 / 缩到托盘 / 换成迷你条 / 退出
    fn finish(&mut self, after: After) {
        match after {
            After::Minimize => {
                self.hide_main(false);
                unsafe {
                    let _ = ShowWindow(self.bar_hwnd(), SW_MINIMIZE);
                }
                // 播放条留在透明的最后一帧（关闭动画不去掉），还原时从透明淡入
                self.away = Some(Away::Iconic);
            }
            After::Tray => {
                self.hide_main(true);
                self.away = Some(Away::Tray);
            }
            After::Minibar => {
                self.hide_main(true);
                self.open_minibar();
            }
            After::MinibarTray => {
                if let Some(m) = self.minibar.as_mut() {
                    unsafe {
                        let _ = ShowWindow(m.top.hwnd, SW_HIDE);
                        let _ = ShowWindow(m.bottom.hwnd, SW_HIDE);
                    }
                    m.fade = None;
                }
                self.away = Some(Away::Tray);
            }
            After::Quit => self.quit_now(),
        }
    }

    /// 隐藏主界面的窗口（bar = 连播放条一起），去掉它们的关闭动画（下次显示时画完整的画面）；
    /// 不隐藏播放条时（最小化）它的关闭动画留着，停在透明的一帧
    fn hide_main(&mut self, bar: bool) {
        for (p, w) in self.wins.iter_mut() {
            if bar || *p != Pane::Bar {
                w.anim = None;
                w.dirty = true;
                unsafe {
                    let _ = ShowWindow(w.hwnd, SW_HIDE);
                }
            }
        }
    }

    /// 显示主界面：面板从中心长出来，播放条淡入（bar_fade = false 时直接出现，如从迷你条回来）
    pub fn show_main(&mut self, bar_fade: bool) {
        self.away = None;
        self.after = None;
        for (p, w) in self.wins.iter_mut() {
            w.dirty = true;
            w.anim = match p {
                Pane::Bar if bar_fade => Some(Anim::open(0.0)),
                Pane::Bar => None,
                _ if w.visible => Some(Anim::open(0.0)),
                _ => None,
            };
        }
        // 先画好第一帧再显示（不闪出上次的画面）
        self.flush();
        let bar = self.bar_hwnd();
        unsafe {
            for (p, w) in &self.wins {
                if *p == Pane::Bar {
                    let _ = ShowWindow(w.hwnd, SW_RESTORE);
                } else {
                    let _ = ShowWindow(w.hwnd, if w.visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
                }
            }
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(bar);
        }
    }

    /// 从任务栏还原了（点任务栏按钮等，不经过我们的 restore）：没显示出来的面板从中心长出来
    pub fn on_restored(&mut self) {
        self.away = None;
        self.after = None;
        // 迷你条模式下播放条本来是隐藏的（被别的程序最小化 / 还原了）：藏回去
        if self.minibar.is_some() {
            unsafe {
                let _ = ShowWindow(self.bar_hwnd(), SW_HIDE);
            }
            return;
        }
        let shown: Vec<bool> = self.wins.iter().map(|(_, w)| unsafe { IsWindowVisible(w.hwnd).as_bool() }).collect();
        for (i, (p, w)) in self.wins.iter_mut().enumerate() {
            w.dirty = true;
            // 关闭动画播到一半被别的程序最小化了（Win+M）：不能停在透明的画面上
            let closing = w.anim.is_some_and(|a| a.dir == Dir::Out);
            if *p == Pane::Bar {
                // 是我们最小化的（停在透明的一帧）：淡入；被别的程序最小化的：画面是完整的，直接显示
                if closing {
                    w.anim = Some(Anim::open(0.0));
                }
            } else if w.visible && (!shown[i] || closing) {
                w.anim = Some(Anim::open(0.0));
            } else if closing {
                w.anim = None;
            }
        }
        self.flush();
        unsafe {
            for (i, (p, w)) in self.wins.iter().enumerate() {
                if *p != Pane::Bar && w.visible && !shown[i] {
                    let _ = ShowWindow(w.hwnd, SW_SHOWNOACTIVATE);
                }
            }
        }
    }

    /// 主界面（或迷你条）现在在屏幕上（托盘图标点一下是隐藏还是显示）
    pub fn is_shown(&self) -> bool {
        self.away.is_none() && !matches!(self.after, Some(After::Minimize | After::Tray | After::MinibarTray))
    }

    /// 开始关闭这些窗口：主界面（播放条 + 显示着的面板）、迷你条一起淡出 / 缩回去，然后做 `after`
    pub fn close_all(&mut self, after: After) {
        if let Some(m) = self.minibar.as_mut() {
            // 已经在淡出的（如缩到托盘时又点了退出）接着淡出，不从头再来
            if !m.fade.is_some_and(|a| a.dir == Dir::Out) {
                m.fade = Some(Anim::close(false));
            }
        } else {
            for (p, w) in self.wins.iter_mut() {
                // 已经在单独关闭的面板接着播自己的动画
                if w.anim.is_some_and(|a| a.dir == Dir::Out) {
                    continue;
                }
                if *p == Pane::Bar || w.visible {
                    w.anim = Some(Anim::close(false));
                    w.dirty = true;
                }
            }
        }
        self.after = Some(after);
    }

    /// 最小化（按设置到任务栏或托盘）：先播关闭动画
    pub fn minimize(&mut self) {
        let to_tray = self.player.core.lock().settings.general.minimize_to_tray;
        self.minimize_to(to_tray);
    }

    /// 关闭按钮、Alt+F4：开了「点关闭按钮缩到托盘」时缩到托盘，否则退出（主菜单的「退出」总是退出）
    pub fn request_close(&mut self) {
        if self.player.core.lock().settings.general.close_to_tray && !self.quitting {
            self.minimize_to(true);
        } else {
            self.quit();
        }
    }

    fn minimize_to(&mut self, to_tray: bool) {
        if self.quitting || !self.is_shown() || self.after.is_some() {
            return;
        }
        // 迷你条没有任务栏按钮，总是缩到托盘
        let after = if self.minibar.is_some() {
            After::MinibarTray
        } else if to_tray {
            After::Tray
        } else {
            After::Minimize
        };
        self.close_all(after);
    }

    /// 从最小化、托盘还原（迷你条模式下还原迷你条）；已经在屏幕上时只是放到前面
    pub fn restore(&mut self) {
        if self.quitting {
            return;
        }
        if self.is_shown() && self.after.is_none() {
            let h = self.minibar.as_ref().map_or(self.bar_hwnd(), |m| m.top.hwnd);
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(h);
            }
            return;
        }
        if self.minibar.is_some() && self.after != Some(After::Minibar) {
            self.away = None;
            self.after = None;
            let Some(m) = self.minibar.as_mut() else { return };
            m.fade = Some(Anim::open(0.0));
            self.paint_minibar();
            if let Some(m) = &self.minibar {
                m.top.show(true);
                m.bottom.show(false);
            }
            return;
        }
        // 正要换成迷你条时还原：取消，留在主界面
        self.show_main(true);
    }

    /// 退出：先保存（动画期间被结束进程也不丢东西），再播关闭动画，完了结束消息循环；
    /// 动画期间再要求退出（再按一次 Alt+F4）、窗口不在屏幕上时直接退出
    pub fn quit(&mut self) {
        if self.quitting {
            if self.after == Some(After::Quit) {
                self.after = None;
                self.quit_now();
            }
            return;
        }
        self.quitting = true;
        self.save_layout();
        self.player.snapshot_last();
        self.player.stop();
        self.player.save(true);
        let bar = self.bar_hwnd();
        crate::platform::hotkeys::unregister_all(bar.0 as isize);
        super::tray::remove(bar);
        self.close_menu();
        let shown = self.away.is_none() && !matches!(self.after, Some(After::Minimize | After::Tray | After::MinibarTray));
        if shown {
            if let Some(s) = self.settings_win.as_mut()
                && s.anim.is_none_or(|a| a.dir == Dir::In)
            {
                s.anim = Some(Anim::close(false));
                s.win.dirty = true;
            }
            self.close_all(After::Quit);
        } else {
            self.after = None;
            self.quit_now();
        }
    }

    /// 结束消息循环（此时仍在窗口过程中，App 已借用，DestroyWindow 同步发送的 WM_DESTROY 会走默认处理，
    /// 所以退出消息必须在这里直接投递，否则消息循环不结束、进程留在后台继续播放）
    pub fn quit_now(&mut self) {
        unsafe {
            for (_, w) in &self.wins {
                let _ = ShowWindow(w.hwnd, SW_HIDE);
            }
            if let Some(m) = &self.minibar {
                let _ = ShowWindow(m.top.hwnd, SW_HIDE);
                let _ = ShowWindow(m.bottom.hwnd, SW_HIDE);
            }
            windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
        }
    }

    /// 打开 / 关闭一个面板：打开时框从中心长出来（原版），关闭时缩回去
    pub fn set_visible(&mut self, p: Pane, on: bool) {
        let i = self.wins.iter().position(|(q, _)| *q == p).expect("pane");
        let w = &mut self.wins[i].1;
        w.visible = on;
        w.dirty = true;
        // 主界面不在屏幕上（迷你条、托盘、最小化）或正在整体关闭：只记下来，回到主界面时按它显示
        if self.away.is_some() || self.minibar.is_some() || self.after.is_some() {
            if !on {
                w.anim = None;
                unsafe {
                    let _ = ShowWindow(w.hwnd, SW_HIDE);
                }
            }
            return;
        }
        if on {
            w.anim = Some(Anim::open(0.0));
            // 先画好动画的第一帧再显示（不闪出上次的画面）
            self.paint_pane(i);
            unsafe {
                let _ = ShowWindow(self.wins[i].1.hwnd, SW_SHOWNOACTIVATE);
            }
        } else {
            w.anim = Some(Anim::close(true));
        }
    }

    /// 换成迷你条（主界面的关闭动画播完以后）：两条在播放条的位置淡入
    fn open_minibar(&mut self) {
        use super::minibar::{GAP, H, Minibar, W};
        use super::popup::Popup;
        let (x, y) = self.bar_pos();
        let top = Popup::new(x, y, W, H, None, "AirPlay3 迷你条");
        let bottom = Popup::new(x, y + GAP, W, H, Some(top.hwnd), "AirPlay3 迷你歌词");
        let on_top = self.player.core.lock().settings.general.always_on_top;
        unsafe {
            for h in [top.hwnd, bottom.hwnd] {
                let _ = SetWindowPos(h, Some(if on_top { HWND_TOPMOST } else { HWND_NOTOPMOST }), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
        self.minibar = Some(Minibar::new(top, bottom, Some(Anim::open(0.0))));
        self.paint_minibar();
        if let Some(m) = &self.minibar {
            m.top.show(false);
            m.bottom.show(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::canvas::{R, Rgba};
    use std::time::Duration;

    fn full(c: &mut Canvas) {
        frame::panel(c, 0.0);
        c.fill(R::new(100, 100, 20, 20), Rgba(255, 255, 255, 255));
    }

    /// 已经播了 t 秒的动画
    fn at(dir: Dir, t: f32) -> Anim {
        Anim { t0: Instant::now() - Duration::from_secs_f32(t), dir, delay: 0.0, hide: false }
    }

    #[test]
    fn open_grows_then_fades_in() {
        // 刚开始：只有中心附近一小块框，四周全透明
        let mut c = Canvas::new(304, 225);
        full(&mut c);
        assert!(panel_frame(&mut c, &at(Dir::In, 0.0), 0.0));
        assert_eq!(c.px[(10 * c.w + 10) as usize][3], 0);
        let opaque = c.px.iter().filter(|p| p[3] > 0).count();
        assert!(opaque > 0 && opaque < 304 * 30, "{opaque}");
        // 长到一半：中心（主体）透明
        let mut c = Canvas::new(304, 225);
        full(&mut c);
        panel_frame(&mut c, &at(Dir::In, 0.2), 0.0);
        assert_eq!(c.px[(112 * c.w + 152) as usize][3], 0);
        // 淡入一半：内容是一半的亮度
        let mut c = Canvas::new(304, 225);
        full(&mut c);
        panel_frame(&mut c, &at(Dir::In, GROW_IN + FADE_IN / 2.0), 0.0);
        let v = c.px[(110 * c.w + 110) as usize][0];
        assert!((100..200).contains(&v), "{v}");
        // 结束：画面不变
        let mut c = Canvas::new(304, 225);
        full(&mut c);
        assert!(!panel_frame(&mut c, &at(Dir::In, GROW_IN + FADE_IN + 0.01), 0.0));
        assert_eq!(c.px[(110 * c.w + 110) as usize], [255, 255, 255, 255]);
    }

    #[test]
    fn close_fades_then_shrinks() {
        // 刚开始关闭：还是完整的画面
        let mut c = Canvas::new(304, 225);
        full(&mut c);
        assert!(panel_frame(&mut c, &at(Dir::Out, 0.0), 0.0));
        assert!(c.px[(110 * c.w + 110) as usize][0] > 240);
        // 内容淡出一半
        let mut c = Canvas::new(304, 225);
        full(&mut c);
        panel_frame(&mut c, &at(Dir::Out, FADE_OUT / 2.0), 0.0);
        let v = c.px[(110 * c.w + 110) as usize][0];
        assert!((100..200).contains(&v), "{v}");
        // 缩到一半：主体透明，四角已经空了
        let mut c = Canvas::new(304, 225);
        full(&mut c);
        assert!(panel_frame(&mut c, &at(Dir::Out, FADE_OUT + SHRINK_OUT / 2.0), 0.0));
        assert_eq!(c.px[(112 * c.w + 152) as usize][3], 0);
        assert_eq!(c.px[(5 * c.w + 5) as usize][3], 0);
        // 播完：全透明
        let mut c = Canvas::new(304, 225);
        full(&mut c);
        assert!(!panel_frame(&mut c, &at(Dir::Out, PANE_OUT + 0.01), 0.0));
        assert!(c.px.iter().all(|p| *p == [0; 4]));
    }

    #[test]
    fn bar_fades() {
        let mut c = Canvas::new(10, 10);
        c.fill(R::new(0, 0, 10, 10), Rgba(200, 200, 200, 255));
        let before = c.px.clone();
        // 等待期间全透明
        let mut a = at(Dir::In, 0.05);
        a.delay = BAR_DELAY;
        assert!(fade_frame(&mut c, &a));
        assert!(c.px.iter().all(|p| *p == [0; 4]));
        // 淡出到一半
        c.px = before.clone();
        assert!(fade_frame(&mut c, &at(Dir::Out, BAR_OUT / 2.0)));
        assert!((90..140).contains(&c.px[0][3]), "{}", c.px[0][3]);
        // 淡出完：全透明，返回 false
        c.px = before;
        assert!(!fade_frame(&mut c, &at(Dir::Out, BAR_OUT + 0.01)));
        assert_eq!(c.px[0][3], 0);
        assert!(at(Dir::Out, BAR_OUT + 0.01).out_done(true));
        assert!(!at(Dir::Out, BAR_OUT - 0.05).out_done(true));
    }
}
