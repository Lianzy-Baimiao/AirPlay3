//! 自动渐隐（「配置 → 基本设置」，默认关着）：鼠标离开主界面一段时间后，歌词图片、均衡器、音乐列表
//! 渐渐隐去，只留播放条（迷你条模式下只留迷你条）；鼠标移回播放条或面板原来的位置时重新显示。
//! 三个面板在设置里分别开关：没勾的那个一直显示。
//!
//! 隐去的面板是全透明的：分层窗口全透明的地方点不到，所以每 30 毫秒看一次鼠标在屏幕上的位置，
//! 不靠鼠标消息。拖动、开着菜单、在搜索框里打字、正在播放开关动画时不隐去。

use std::time::{Duration, Instant};

use super::app::{App, Pane};
use super::canvas::R;
use super::state::Drag;

/// 隐去用 0.5 秒，重新显示用 0.2 秒
const OUT_SECS: f32 = 0.5;
const IN_SECS: f32 = 0.2;

pub struct AutoFade {
    /// 跟着隐去的面板现在的不透明度（1 = 完全显示，0 = 完全隐去）
    pub level: f32,
    /// 哪些面板跟着隐去：歌词图片、均衡器、音乐列表（设置里分别开关）
    panes: [bool; 3],
    /// 上一次鼠标在主界面上（或界面正忙）的时刻
    seen: Instant,
    last: Instant,
    /// 最近一帧不透明度变了（面板要重画）
    moved: bool,
    /// 仅调试构建：自测时代替真实的鼠标位置（Some(true) = 当作在主界面上）
    #[cfg(debug_assertions)]
    pub test_cursor: Option<bool>,
}

impl AutoFade {
    pub fn new(now: Instant) -> AutoFade {
        AutoFade {
            level: 1.0,
            panes: [true; 3],
            seen: now,
            last: now,
            moved: false,
            #[cfg(debug_assertions)]
            test_cursor: None,
        }
    }

    /// 推进一帧。wait = 鼠标离开多久后隐去（None = 功能关着）；active = 鼠标在主界面上，或界面正忙。
    /// 返回 true = 不透明度变了
    pub fn step(&mut self, now: Instant, active: bool, wait: Option<Duration>) -> bool {
        let dt = now.saturating_duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        let show = match wait {
            Some(w) if !active => now.saturating_duration_since(self.seen) < w,
            // 关着、或鼠标在上面：从现在重新算
            _ => {
                self.seen = now;
                true
            }
        };
        let (target, secs) = if show { (1.0, IN_SECS) } else { (0.0, OUT_SECS) };
        self.moved = super::state::approach(&mut self.level, target, dt / secs);
        self.moved
    }

    /// 换成这几个面板跟着隐去（歌词图片、均衡器、音乐列表）。返回 true = 和原来不一样（面板要重画）
    pub fn pick(&mut self, panes: [bool; 3]) -> bool {
        let changed = self.panes != panes;
        self.panes = panes;
        self.moved |= changed;
        changed
    }

    /// 这个面板跟不跟着隐去（播放条从不隐去）
    fn fades(&self, p: Pane) -> bool {
        match p {
            Pane::Bar => false,
            Pane::Cover => self.panes[0],
            Pane::Eq => self.panes[1],
            Pane::List => self.panes[2],
        }
    }

    /// 这个面板现在的不透明度
    pub fn level_of(&self, p: Pane) -> f32 {
        if self.fades(p) { self.level } else { 1.0 }
    }

    /// 这个面板已经完全隐去、画面也已经提交过了（不用再画）
    pub fn hidden(&self, p: Pane) -> bool {
        self.fades(p) && self.level <= 0.0 && !self.moved
    }
}

impl App {
    /// 每 30 毫秒：按鼠标的位置推进渐隐 / 重新显示
    pub fn tick_auto_fade(&mut self, now: Instant) {
        let (wait, panes) = {
            let g = self.player.core.lock();
            let s = &g.settings.general;
            let panes = [s.auto_fade_cover, s.auto_fade_eq, s.auto_fade_list];
            let on = s.auto_fade && panes.contains(&true);
            (on.then(|| Duration::from_secs(s.auto_fade_secs.clamp(1, 60) as u64)), panes)
        };
        let active = wait.is_none() || self.fade_blocked() || self.cursor_on_main();
        let was = self.ui.fade.level;
        let moved = self.ui.fade.step(now, active, wait);
        // 设置里改了哪些面板跟着隐去：马上按新的来（刚取消的那个立即显示回来）
        let picked = self.ui.fade.pick(panes);
        if !moved && !picked {
            return;
        }
        // 全透明的地方「透明雾化」照样把桌面弄模糊：开始隐去时先关掉这些面板的雾化，完全显示后再打开
        if super::theme::get().frost && (picked || was >= 1.0 || self.ui.fade.level >= 1.0) {
            self.sync_pane_frost();
        }
        for p in [Pane::Cover, Pane::Eq, Pane::List] {
            self.dirty(p);
        }
    }

    /// 面板（不含播放条）的透明雾化：按设置来，正在隐去 / 已经隐去的面板关着
    pub(super) fn sync_pane_frost(&self) {
        let frost = super::theme::get().frost;
        for (p, w) in &self.wins {
            if *p != Pane::Bar {
                super::window::set_frost(w.hwnd, frost && self.ui.fade.level_of(*p) >= 1.0);
            }
        }
    }

    /// 界面正忙，不能隐去：不在屏幕上、正在整体关闭、拖动、开着菜单或名称弹框、在搜索框里打字、正在播放开关动画
    fn fade_blocked(&self) -> bool {
        self.away.is_some()
            || self.after.is_some()
            || self.ui.drag != Drag::None
            || self.menu.is_some()
            || self.name_prompt.is_some()
            || self.ui.list.search_focus
            || self.minibar.as_ref().is_some_and(|m| m.drag.is_some() || m.fade.is_some())
            || self.wins.iter().any(|(_, w)| w.anim.is_some())
    }

    /// 鼠标在播放条（迷你条）或某个显示着的面板的范围里（面板隐去了也算：移回原来的位置就重新显示）
    fn cursor_on_main(&self) -> bool {
        #[cfg(debug_assertions)]
        if let Some(v) = self.ui.fade.test_cursor {
            return v;
        }
        let (x, y) = super::scale::cursor();
        let pane = |i: usize| self.rect_of(self.wins[i].0).contains(x, y);
        match &self.minibar {
            Some(m) => {
                let strip = |p: (i32, i32)| R::new(p.0, p.1, super::minibar::W, super::minibar::H).contains(x, y);
                strip(m.top.pos) || (m.strip && strip(m.bottom.pos)) || (1..self.wins.len()).any(|i| m.panes[i] && pane(i))
            }
            None => (0..self.wins.len()).any(|i| (i == 0 || self.wins[i].1.visible) && pane(i)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一帧 30 毫秒地走 secs 秒
    fn run(f: &mut AutoFade, t: &mut Instant, secs: f32, active: bool, wait: Option<Duration>) {
        for _ in 0..(secs / 0.03).round() as u32 {
            *t += Duration::from_millis(30);
            f.step(*t, active, wait);
        }
    }

    #[test]
    fn fades_after_the_wait_and_comes_back_on_hover() {
        let mut t = Instant::now();
        let mut f = AutoFade::new(t);
        let wait = Some(Duration::from_secs(3));
        // 关着：一直显示
        run(&mut f, &mut t, 10.0, false, None);
        assert_eq!(f.level, 1.0);
        // 打开后从这一刻算：3 秒以内不动
        run(&mut f, &mut t, 2.9, false, wait);
        assert_eq!(f.level, 1.0);
        // 过了 3 秒开始隐去，约 0.5 秒隐完；隐完后不再要求重画
        run(&mut f, &mut t, 0.3, false, wait);
        assert!(f.level > 0.0 && f.level < 1.0, "{}", f.level);
        assert!(!f.hidden(Pane::List));
        run(&mut f, &mut t, 0.6, false, wait);
        assert_eq!(f.level, 0.0);
        assert!(f.hidden(Pane::List));
        // 鼠标移回来：约 0.2 秒重新显示
        t += Duration::from_millis(30);
        assert!(f.step(t, true, wait));
        assert!(!f.hidden(Pane::List));
        run(&mut f, &mut t, 0.3, true, wait);
        assert_eq!(f.level, 1.0);
        // 鼠标一直在上面就一直显示；离开后重新等 3 秒
        run(&mut f, &mut t, 10.0, true, wait);
        run(&mut f, &mut t, 2.9, false, wait);
        assert_eq!(f.level, 1.0);
        run(&mut f, &mut t, 1.0, false, wait);
        assert_eq!(f.level, 0.0);
        // 隐去以后关掉功能：马上显示回来
        run(&mut f, &mut t, 0.3, false, None);
        assert_eq!(f.level, 1.0);
    }

    /// 三个面板分别开关：没勾的一直显示，隐去以后改了开关马上按新的来
    #[test]
    fn each_pane_fades_only_when_picked() {
        let mut t = Instant::now();
        let mut f = AutoFade::new(t);
        let wait = Some(Duration::from_secs(3));
        assert!(f.pick([true, false, true]));
        assert!(!f.pick([true, false, true]));
        run(&mut f, &mut t, 4.0, false, wait);
        let levels = |f: &AutoFade| [Pane::Bar, Pane::Cover, Pane::Eq, Pane::List].map(|p| f.level_of(p));
        let hidden = |f: &AutoFade| [Pane::Bar, Pane::Cover, Pane::Eq, Pane::List].map(|p| f.hidden(p));
        assert_eq!(levels(&f), [1.0, 0.0, 1.0, 0.0]);
        assert_eq!(hidden(&f), [false, true, false, true]);
        // 隐去以后把音乐列表取消、均衡器勾上：这一帧两个都要重画，之后均衡器算隐去了
        assert!(f.pick([true, true, false]));
        assert_eq!(levels(&f), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(hidden(&f), [false; 4]);
        t += Duration::from_millis(30);
        assert!(!f.step(t, false, wait));
        assert_eq!(hidden(&f), [false, true, true, false]);
    }

    /// 隐去的面板整个画布全透明（点不到，也看不见调整大小用的边）
    #[test]
    fn faded_canvas_is_fully_transparent() {
        use crate::gui::canvas::{Canvas, Rgba};
        let mut c = Canvas::with_scale(20, 10, 1.5);
        c.fill(R::new(0, 0, 20, 10), Rgba(200, 100, 50, 255));
        crate::gui::anim::fade(&mut c, 0.5);
        assert!(c.px.iter().all(|p| (126..=128).contains(&p[3])));
        crate::gui::anim::fade(&mut c, 0.0);
        assert!(c.px.iter().all(|p| *p == [0; 4]));
    }
}
