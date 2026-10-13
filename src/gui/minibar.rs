//! 迷你条（原版 minibar，2026-10-14 照原版逐项实测）：两条 304×24 的窄条。
//! 上面的控制条：平时只有已播放时间和标语；鼠标移上去（约 0.45 秒淡入）才出现音量、上一首·暂停·下一首、还原·关闭。
//! 下面的歌词条：所有歌词排成一行向左滚动（当前句亮），两头淡出；鼠标移上去出现 □（换成歌词图片面板）和 ✕（关掉歌词条）。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::anim::{self, After, Anim, Dir};
use super::app::App;
use super::canvas::{Canvas, R, Rgba};
use super::desklyric::LineState;
use super::popup::Popup;
use super::text::{self, Font};
use crate::skin::icons;

pub const W: i32 = 304;
pub const H: i32 = 24;
/// 歌词条默认在控制条下方 65 像素（原版位置）
pub const GAP: i32 = 65;
const FONT: Font = Font::px(12.0, false);
const BTN_PREV: R = R::new(123, 6, 16, 12);
const BTN_PLAY: R = R::new(144, 6, 16, 12);
const BTN_NEXT: R = R::new(165, 6, 16, 12);
const BTN_RESTORE: R = R::new(272, 5, 10, 10);
const BTN_CLOSE: R = R::new(284, 5, 10, 10);

pub struct Minibar {
    pub top: Popup,
    pub bottom: Popup,
    pub hover: [bool; 2],
    pub drag: Option<MiniDrag>,
    pub last: LineState,
    ticks: u32,
    /// 正在淡入 / 淡出（打开迷你条、缩到托盘、退出）
    pub fade: Option<Anim>,
    /// 悬停的淡入淡出（0 ~ 1）
    pub hover_t: [f32; 2],
    /// 歌词条显示着（点它的 □ / ✕ 关掉，右键菜单「迷你歌词」再打开）
    pub strip: bool,
    /// 迷你条模式下单独打开的面板（下标与 app.wins 相同，播放条那一格不用）
    pub panes: [bool; 4],
    /// 上次画的标语位置（变了就重画）
    slogan: f32,
    /// 各行歌词的宽度：(歌词的标识, 宽度)
    widths: (u64, Vec<i32>),
}

impl Minibar {
    pub fn new(top: Popup, bottom: Popup, fade: Option<Anim>) -> Minibar {
        Minibar { top, bottom, hover: [false; 2], drag: None, last: LineState::default(), ticks: 0, fade, hover_t: [0.0; 2], strip: true, panes: [false; 4], slogan: -1.0, widths: (0, Vec::new()) }
    }
}

/// 悬停淡入的速度（每 30 毫秒；原版约 0.45 秒）
const HOVER_STEP: f32 = 0.07;
/// 歌词条：两句之间的空隙、当前句的左边在第几个像素、两头淡出的范围（都是照原版量的）
const TICK_GAP: i32 = 24;
const TICK_AT: i32 = 150;
const FADE_FROM: i32 = 8;
const FADE_TO: i32 = 42;

/// 所有任务栏（主屏 Shell_TrayWnd，其它显示器 Shell_SecondaryTrayWnd）的位置
fn taskbars() -> Vec<R> {
    use windows::core::w;
    let mut v = Vec::new();
    unsafe {
        let mut push = |h: HWND| {
            let mut r = RECT::default();
            if GetWindowRect(h, &mut r).is_ok() {
                v.push(R::new(r.left, r.top, r.right - r.left, r.bottom - r.top));
            }
        };
        if let Ok(h) = FindWindowW(w!("Shell_TrayWnd"), None) {
            push(h);
        }
        let mut after = None;
        while let Ok(h) = FindWindowExW(None, after, w!("Shell_SecondaryTrayWnd"), None) {
            push(h);
            after = Some(h);
        }
    }
    v
}

/// 拖动迷你条（原版：拖控制条时歌词条一起移动；拖歌词条只移动歌词条；都会吸附到屏幕边、任务栏和另一条）
#[derive(Clone, Copy)]
pub struct MiniDrag {
    /// 0 = 控制条，1 = 歌词条
    lead: usize,
    cursor: (i32, i32),
    rects: [R; 2],
    taskbar: Option<R>,
}

/// 一条迷你条的玻璃（原版实测）：平时几乎完全透明（往下渐渐到约 4%），悬停时中间最不透明、往两边变淡，
/// 越往下越不透明；与播放条的玻璃同一种样子，但系数不同
fn strip(c: &mut Canvas, hover: f32) {
    use super::panels::frame;
    c.clear();
    let o = R::new(2, 2, W - 4, H - 4);
    let ba = super::theme::border_alpha() as f32;
    let h = hover.clamp(0.0, 1.0);
    let fill = |x: i32, y: i32| {
        if y < o.y + 1 || y >= o.bottom() - 1 {
            return frame::HIGHLIGHT;
        }
        let t = (y - (o.y + 1)) as f32;
        let idle = t * 10.0 / 17.0;
        let hov = (130.4 + 4.0 * t - 0.882 * (x as f32 + 0.5 - 152.5).abs()).max(idle);
        let a = (ba + idle + (hov - idle) * h).clamp(0.0, 255.0);
        let (r, g, b) = super::theme::tint((141.0 + 32.0 * t / 17.0 + (159.0 - 141.0 + 0.81 * t - 32.0 * t / 17.0) * h).round() as u8);
        Rgba(r, g, b, a.round() as u8)
    };
    frame::shape(c, o, 3.0, 3.0, |_| frame::HIGHLIGHT, fill);
}

/// 把 src（预乘颜色）盖到 dst 上
fn over(dst: &mut Canvas, src: &Canvas) {
    for (d, s) in dst.px.iter_mut().zip(src.px.iter()) {
        let k = 255 - s[3] as u32;
        for i in 0..4 {
            d[i] = (s[i] as u32 + (d[i] as u32 * k + 127) / 255).min(255) as u8;
        }
    }
}

/// 歌词条的滚动位置：当前句（第 i 句）的左边在 TICK_AT，在第 i 句和第 i+1 句的时间之间匀速移到第 i+1 句的位置；
/// 返回 (当前句, 第 0 句左边离 TICK_AT 多远)。starts[i] = 第 i 句左边离第 0 句左边多远
fn ticker_scroll(times: &[f64], starts: &[i32], t: f64) -> (Option<usize>, f64) {
    let n = times.partition_point(|&s| s <= t);
    let Some(i) = n.checked_sub(1) else { return (None, 0.0) };
    let a = starts[i] as f64;
    let Some((&t1, &b)) = times.get(i + 1).zip(starts.get(i + 1)) else { return (Some(i), a) };
    let k = if t1 > times[i] { ((t - times[i]) / (t1 - times[i])).clamp(0.0, 1.0) } else { 1.0 };
    (Some(i), a + (b as f64 - a) * k)
}

/// 文字后面的暗色光晕：玻璃几乎透明，放在什么背景上都看得清
const HALO: Rgba = Rgba(16, 16, 16, 190);

impl App {
    /// 主界面 ⇄ 迷你条：主界面先播关闭动画再换成迷你条（淡入）；回来时迷你条直接消失，面板从中心长出来（原版）
    pub fn toggle_minibar(&mut self) {
        if self.quitting || self.after == Some(After::Minibar) {
            return;
        }
        if let Some(mut m) = self.minibar.take() {
            m.top.close();
            m.bottom.close();
            self.show_main(false);
            return;
        }
        if self.away.is_some() || self.after.is_some() {
            return;
        }
        self.close_all(After::Minibar);
    }

    pub fn tick_minibar(&mut self) {
        // 缩到托盘时不画
        if self.minibar.is_none() || self.away.is_some() {
            return;
        }
        let st = self.line_state();
        let (playing, slogan) = (self.ui.snap.playing, self.ui.slogan);
        let m = self.minibar.as_mut().unwrap();
        m.ticks = m.ticks.wrapping_add(1);
        let check_z = m.ticks % 15 == 0;
        // 淡入淡出期间每一帧都画；淡入完了去掉（淡出的留着，等 tick_anim 隐藏）
        let fading = m.fade.is_some_and(|a| a.dir == Dir::In || !a.out_done(true));
        if m.fade.is_some_and(|a| a.dir == Dir::In && a.t() >= anim::BAR_IN) {
            m.fade = None;
        }
        // 悬停淡入淡出、标语切换；播放时时间和歌词条一直在动
        let mut hov = false;
        for i in 0..2 {
            hov |= super::state::approach(&mut m.hover_t[i], if m.hover[i] { 1.0 } else { 0.0 }, HOVER_STEP);
        }
        let sl = (m.slogan - slogan).abs() > 1e-3;
        m.slogan = slogan;
        if st != m.last || fading || hov || sl || playing {
            m.last = st;
            self.paint_minibar();
        }
        if check_z {
            self.keep_minibar_on_top();
        }
    }

    /// 迷你条放在任务栏上时，点任务栏（开始菜单、切换窗口）会让任务栏压到迷你条上面，迷你条就点不到了：
    /// 盖着任务栏时每半秒把迷你条重新放到最上层（关了「始终置顶」也一样，离开任务栏后再取消置顶）
    fn keep_minibar_on_top(&mut self) {
        let on_top = self.player.core.lock().settings.general.always_on_top;
        let Some(m) = self.minibar.as_ref() else { return };
        if m.drag.is_some() {
            return;
        }
        let strips = [(m.top.hwnd, m.top.pos), (m.bottom.hwnd, m.bottom.pos)];
        let bars = taskbars();
        for (h, (x, y)) in strips {
            let r = R::new(x, y, W, H);
            let over_bar = bars.iter().any(|b| b.inter(&r).w > 0 && b.inter(&r).h > 0);
            let now_top = unsafe { GetWindowLongW(h, GWL_EXSTYLE) } & WS_EX_TOPMOST.0 as i32 != 0;
            let want = on_top || over_bar;
            if over_bar || want != now_top {
                unsafe {
                    let _ = SetWindowPos(h, Some(if want { HWND_TOPMOST } else { HWND_NOTOPMOST }), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
            }
        }
    }

    /// 「恢复默认位置」：迷你条回到播放条的位置（控制条在上，歌词条在下方 65 像素）
    pub fn reset_minibar(&mut self) {
        let (x, y) = self.bar_pos();
        if let Some(m) = self.minibar.as_mut() {
            m.top.move_to(x, y);
            m.bottom.move_to(x, y + GAP);
        }
        self.keep_minibar_on_top();
    }

    pub fn paint_minibar(&mut self) {
        use super::panels::bar::{FONT_SLOGAN, SLOGANS, fmt_time};
        let (playing, vol, muted, pos, dur, slogan) = (self.ui.snap.playing, self.ui.snap.volume, self.ui.snap.muted, self.ui.snap.pos, self.ui.snap.dur, self.ui.slogan);
        // 歌词条要画的几句：(左边的 x, 文字, 是不是当前句)
        let lines = self.ticker_lines(pos);
        let Some(m) = self.minibar.as_mut() else { return };
        let alpha = m.fade.map_or(1.0, |a| anim::fade_alpha(&a, a.t().max(0.0)));
        let [h0, h1] = m.hover_t;
        // 上面的控制条
        let c = &mut m.top.canvas;
        strip(c, h0);
        // 平时：已播放时间（有歌曲时）+ 标语；悬停时时间淡出、标语变淡，音量和按钮淡入
        if dur > 0.0 && h0 < 1.0 {
            c.num6x8(&fmt_time(pos), 10, 8, Rgba(207, 207, 207, 128).alpha(((1.0 - h0) * 255.0) as u8));
        }
        let text_a = 255.0 * (0.25 + 0.75 * (1.0 - h0));
        let halo = |a: f32| Rgba(16, 16, 16, (a * 0.8) as u8);
        c.push_clip(R::new(3, 3, W - 6, H - 6));
        let k = slogan.clamp(0.0, 1.0);
        let off = (k * 18.0).round() as i32;
        let (a0, a1) = ((1.0 - k) * text_a, k * text_a);
        c.glow_text(SLOGANS[0], FONT_SLOGAN, 56, 5 - off, Rgba::gray(235).alpha(a0 as u8), halo(a0), 3);
        c.glow_text(SLOGANS[1], FONT_SLOGAN, 56, 23 - off, Rgba::gray(235).alpha(a1 as u8), halo(a1), 3);
        c.pop_clip();
        if h0 > 0.0 {
            let a = (h0 * 255.0) as u8;
            let pill = R::new(9, 7, 7, 11);
            let v = if muted { 0.0 } else { vol };
            c.round_rect(pill, 3.0, Some(Rgba(0x9F, 0x9F, 0x9F, 107).alpha(a)), None);
            let fh = (pill.h as f32 * v) as i32;
            c.push_clip(R::new(pill.x, pill.bottom() - fh, pill.w, fh));
            c.round_rect(pill, 3.0, Some(Rgba(0xE3, 0xE3, 0xE3, 208).alpha(a)), None);
            c.pop_clip();
            c.icon(icons::PREV, BTN_PREV.x, BTN_PREV.y - 2, Rgba::gray(230).alpha(a));
            c.icon(if playing { icons::PAUSE } else { icons::PLAY }, BTN_PLAY.x, BTN_PLAY.y - 2, Rgba::gray(230).alpha(a));
            c.icon(icons::NEXT, BTN_NEXT.x, BTN_NEXT.y - 2, Rgba::gray(230).alpha(a));
            c.icon(icons::S_RESTORE, BTN_RESTORE.x, BTN_RESTORE.y, Rgba::gray(220).alpha(a));
            c.icon(icons::S_CLOSE, BTN_CLOSE.x, BTN_CLOSE.y, Rgba::gray(220).alpha(a));
        }
        anim::fade(c, alpha);
        #[cfg(debug_assertions)]
        {
            c.dump("minitop");
            if m.fade.is_some() {
                c.dump_seq("MiniTop", self.started.elapsed().as_millis());
            }
        }
        m.top.present();
        // 下面的歌词条：所有歌词排成一行向左滚动，当前句亮，两头淡出；没有带时间轴的歌词时是空的（与原版相同）
        let c = &mut m.bottom.canvas;
        strip(c, h1);
        if !lines.is_empty() {
            let mut t = Canvas::new(W, H);
            t.push_clip(R::new(3, 3, W - 6, H - 6));
            for (x, s, cur) in &lines {
                t.glow_text(s, FONT, *x, 3, Rgba::gray(if *cur { 253 } else { 190 }), HALO, 2);
            }
            t.pop_clip();
            let fade = |x: i32| ((x.min(W - 1 - x) - FADE_FROM) as f32 / (FADE_TO - FADE_FROM) as f32).clamp(0.0, 1.0);
            for y in 0..H {
                for x in 0..W {
                    let f = fade(x);
                    if f < 1.0 {
                        let p = &mut t.px[(y * W + x) as usize];
                        for v in p.iter_mut() {
                            *v = (*v as f32 * f) as u8;
                        }
                    }
                }
            }
            over(c, &t);
        }
        if h1 > 0.0 {
            let a = (h1 * 255.0) as u8;
            c.icon(icons::S_RESTORE, BTN_RESTORE.x, BTN_RESTORE.y, Rgba::gray(220).alpha(a));
            c.icon(icons::S_CLOSE, BTN_CLOSE.x, BTN_CLOSE.y, Rgba::gray(220).alpha(a));
        }
        anim::fade(c, alpha);
        #[cfg(debug_assertions)]
        c.dump("minibottom");
        m.bottom.present();
    }

    /// 歌词条上看得见的几句：(左边的 x, 文字, 是不是当前句)
    fn ticker_lines(&mut self, pos: f64) -> Vec<(i32, String, bool)> {
        let n = self.now.lock();
        let Some(m) = self.minibar.as_mut() else { return Vec::new() };
        let Some(l) = n.lyrics.as_ref().filter(|l| l.synced && !l.lines.is_empty() && m.strip) else { return Vec::new() };
        let t = pos + n.lyric_offset_ms as f64 / 1000.0 + l.offset_ms as f64 / 1000.0;
        // 各句的宽度只在换了歌词时量一次
        let key = (l.lines.len() as u64) ^ l.lines[0].time.to_bits().rotate_left(17) ^ l.lines[l.lines.len() - 1].time.to_bits().rotate_left(31);
        if m.widths.0 != key || m.widths.1.len() != l.lines.len() {
            m.widths = (key, l.lines.iter().map(|x| text::measure(&x.text, FONT).0.round() as i32).collect());
        }
        let mut starts = Vec::with_capacity(l.lines.len());
        let mut acc = 0;
        for w in &m.widths.1 {
            starts.push(acc);
            acc += w + TICK_GAP;
        }
        let times: Vec<f64> = l.lines.iter().map(|x| x.time).collect();
        let (cur, scroll) = ticker_scroll(&times, &starts, t);
        let base = TICK_AT as f64 - scroll;
        let mut out = Vec::new();
        for (i, x) in l.lines.iter().enumerate() {
            let left = (base + starts[i] as f64).round() as i32;
            if left > W || left + m.widths.1[i] < 0 || x.text.trim().is_empty() {
                continue;
            }
            out.push((left, x.text.clone(), cur == Some(i)));
        }
        out
    }

    /// 按鼠标位置移动迷你条（吸附算法与主界面面板相同，见 snap.rs）
    fn minibar_drag(&mut self, d: MiniDrag) {
        self.minibar_drag_at(d, super::resize::cursor());
    }

    fn minibar_drag_at(&mut self, d: MiniDrag, (cx, cy): (i32, i32)) {
        let s = d.rects[d.lead];
        let p = R::new(s.x + cx - d.cursor.0, s.y + cy - d.cursor.1, s.w, s.h);
        let moving = if d.lead == 0 { [true, true] } else { [false, true] };
        let env = super::snap::Env { taskbar: d.taskbar, work_area: &super::resize::work_area };
        let q = super::snap::group_move(&d.rects, &[true, true], &moving, d.lead, p, d.lead == 0, &env);
        // 可以盖住任务栏，但不能拖出屏幕（限制在光标所在显示器的整个屏幕范围里）
        let group: Vec<R> = (0..2).filter(|&i| moving[i]).map(|i| d.rects[i]).collect();
        let (dx, dy) = super::snap::keep_inside(&group, (q.x - s.x, q.y - s.y), super::popup::monitor_at((cx, cy)));
        if let Some(m) = self.minibar.as_mut() {
            if moving[0] {
                m.top.move_to(d.rects[0].x + dx, d.rects[0].y + dy);
            }
            m.bottom.move_to(d.rects[1].x + dx, d.rects[1].y + dy);
        }
        // 拖控制条时，隐藏的主界面跟着移动（还原时回到迷你条的位置）
        if moving[0] {
            self.move_to(d.rects[0].x + dx, d.rects[0].y + dy);
        }
    }

    /// 仅调试构建：模拟拖动迷你条（0 = 控制条，1 = 歌词条），返回两条的位置
    #[cfg(debug_assertions)]
    pub fn test_minibar_drag(&mut self, lead: usize, dx: i32, dy: i32) -> Option<[(i32, i32); 2]> {
        if self.minibar.is_none() {
            self.toggle_minibar();
        }
        let m = self.minibar.as_ref()?;
        let rect = |p: &Popup| R::new(p.pos.0, p.pos.1, W, H);
        let c0 = (10_000, 10_000);
        let d = MiniDrag { lead, cursor: c0, rects: [rect(&m.top), rect(&m.bottom)], taskbar: super::resize::taskbar_rect() };
        self.minibar_drag_at(d, (c0.0 + dx, c0.1 + dy));
        let m = self.minibar.as_ref()?;
        Some([m.top.pos, m.bottom.pos])
    }

    pub fn minibar_msg(&mut self, hwnd: HWND, msg: u32, _wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let m = self.minibar.as_mut()?;
        let which = if m.top.hwnd == hwnd { 0 } else if m.bottom.hwnd == hwnd { 1 } else { return None };
        let (x, y) = (super::wnd::lo(lp), super::wnd::hi(lp));
        match msg {
            WM_MOUSEMOVE => {
                if let Some(d) = m.drag {
                    self.minibar_drag(d);
                } else if !m.hover[which] {
                    m.hover[which] = true;
                    self.track_leave(hwnd);
                    self.paint_minibar();
                }
            }
            0x02A3 => {
                m.hover[which] = false;
                self.paint_minibar();
            }
            WM_LBUTTONDOWN => {
                if which == 0 && BTN_PREV.contains(x, y) {
                    self.player.prev();
                } else if which == 0 && BTN_PLAY.contains(x, y) {
                    self.player.toggle_pause();
                } else if which == 0 && BTN_NEXT.contains(x, y) {
                    self.player.next();
                } else if which == 0 && BTN_RESTORE.shrink(-2).contains(x, y) {
                    self.toggle_minibar();
                } else if which == 0 && BTN_CLOSE.shrink(-2).contains(x, y) {
                    self.request_close();
                } else if which == 1 && BTN_RESTORE.shrink(-2).contains(x, y) {
                    // 歌词条的 □：换成歌词图片面板（原版）
                    self.mini_strip(Some(false));
                    if self.minibar.as_ref().is_some_and(|m| !m.panes[1]) {
                        self.mini_pane(1);
                    }
                    return Some(LRESULT(0));
                } else if which == 1 && BTN_CLOSE.shrink(-2).contains(x, y) {
                    // 歌词条的 ✕：关掉歌词条（右键菜单「迷你歌词」再打开）
                    self.mini_strip(Some(false));
                    return Some(LRESULT(0));
                } else {
                    let rect = |p: &Popup| R::new(p.pos.0, p.pos.1, W, H);
                    m.drag = Some(MiniDrag { lead: which, cursor: super::resize::cursor(), rects: [rect(&m.top), rect(&m.bottom)], taskbar: super::resize::taskbar_rect() });
                    unsafe { SetCapture(hwnd) };
                }
                self.paint_minibar();
            }
            WM_LBUTTONUP => {
                m.drag = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            WM_LBUTTONDBLCLK => self.toggle_minibar(),
            // Alt+F4 等：与主界面一样（完全退出，或开了「点关闭按钮缩到托盘」时缩到托盘）
            WM_CLOSE => self.request_close(),
            WM_RBUTTONUP => self.act(super::menu::Act::MainMenu),
            WM_MOUSEWHEEL => {
                let d = ((_wp.0 >> 16) & 0xFFFF) as i16 as i32;
                self.player.volume_step(if d > 0 { 0.05 } else { -0.05 });
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }
}
