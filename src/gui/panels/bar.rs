//! 播放条（原版 304×68 分层窗口，可见区域 300×64，四周 2 像素透明边）。
//! 空闲时：封面 + "AIRPLAY" + 标语 + 进度；鼠标悬停时淡入控制按钮（音量、上一首/暂停/下一首、窗口按钮、开关）。

use super::frame;
use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::text::Font;
use crate::gui::theme;
use crate::skin::icons;

/// 默认宽度；原版可左右拖动调整宽度（WM_GETMINMAXINFO：284 ~ 804），高度固定
pub const W: i32 = 304;
pub const H: i32 = 68;
pub const MIN_W: i32 = 284;
pub const MAX_W: i32 = 804;
pub const COVER: R = R::new(15, 6, 56, 56);
pub const VOLUME: R = R::new(9, 9, 7, 50);

/// 随宽度变化的位置（与原版相同：音量、封面、标题靠左；大按钮居中；窗口按钮、开关、模式、总时长靠右；进度条拉伸）
pub struct Layout {
    pub frame: R,
    pub track: R,
    pub big: [R; 3],
    pub win_btns: [R; 4],
    pub toggles: [R; 3],
    pub mode: R,
    /// 右侧时间的 x
    pub time_x: i32,
}

impl Layout {
    pub fn of(w: i32) -> Layout {
        let d = w - W;
        let c = d / 2;
        let rx = |x: i32, y: i32, s: i32| R::new(x + d, y, s, s);
        Layout {
            frame: R::new(2, 2, w - 4, 64),
            track: R::new(79, 53, 144 + d, 4),
            big: [R::new(101 + c, 13, 32, 32), R::new(136 + c, 13, 32, 32), R::new(171 + c, 13, 32, 32)],
            win_btns: [rx(238, 4, 15), rx(252, 4, 15), rx(267, 4, 15), rx(282, 4, 15)],
            toggles: [rx(234, 25, 16), rx(255, 25, 16), rx(275, 25, 16)],
            mode: rx(281, 46, 16),
            time_x: 232 + d,
        }
    }
}

pub const FONT: Font = Font::px(12.0, false);
pub const FONT_SLOGAN: Font = Font::px(12.0, false);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hot {
    None,
    Cover,
    Volume,
    Prev,
    PlayPause,
    Next,
    Min,
    Minibar,
    Pin,
    Close,
    Lyric,
    List,
    Eq,
    Mode,
    Track,
}

pub struct BarModel<'a> {
    pub cover: Option<(&'a [u8], u32, u32)>,
    pub title: &'a str,
    pub artist: &'a str,
    pub playing: bool,
    pub pos: f64,
    pub dur: f64,
    pub volume: f32,
    pub muted: bool,
    pub mode: crate::config::PlayMode,
    pub topmost: bool,
    pub show_lyric: bool,
    pub show_list: bool,
    pub show_eq: bool,
    /// 悬停淡入程度 0..1
    pub hover: f32,
    pub hot: Hot,
    /// 标语切换：0 = 第一句，1 = 第二句，中间值为上下滑动过渡
    pub slogan: f32,
}

pub const SLOGANS: [&str; 2] = ["乐声飞扬 聆听彼此", "link between souls"];

pub fn hit(w: i32, x: i32, y: i32, hovering: bool) -> Hot {
    if !hovering {
        return if COVER.contains(x, y) { Hot::Cover } else { Hot::None };
    }
    let l = Layout::of(w);
    let t = l.track;
    let checks = [
        (VOLUME.shrink(-3), Hot::Volume),
        (l.big[0], Hot::Prev),
        (l.big[1], Hot::PlayPause),
        (l.big[2], Hot::Next),
        (l.win_btns[0], Hot::Min),
        (l.win_btns[1], Hot::Minibar),
        (l.win_btns[2], Hot::Pin),
        (l.win_btns[3], Hot::Close),
        (l.toggles[0], Hot::Lyric),
        (l.toggles[1], Hot::List),
        (l.toggles[2], Hot::Eq),
        (l.mode, Hot::Mode),
        (R::new(t.x - 4, t.y - 5, t.w + 8, t.h + 10), Hot::Track),
    ];
    checks.iter().find(|(r, _)| r.contains(x, y)).map_or(Hot::None, |c| c.1)
}

/// 背景：整条都是半透明的玻璃（与原版实测的效果相同）。空闲时几乎完全透明（不透明度自上而下 0 → 32），
/// 悬停时变成中间更不透明的浅灰玻璃；「边框透明」在此基础上加一层不透明度。四周是亮线和柔和阴影
fn background(c: &mut Canvas, frame: R, hover: f32) {
    glass(c, frame, 4.0, hover);
}

/// 播放条式的玻璃（播放条、迷你条共用）
pub fn glass(c: &mut Canvas, frame: R, radius: f32, hover: f32) {
    let ba = theme::border_alpha() as f32;
    let hover = hover.clamp(0.0, 1.0);
    let (top, bottom) = (frame.y + 1, frame.bottom() - 1);
    let cx = frame.x as f32 + frame.w as f32 / 2.0;
    let fill = |x: i32, y: i32| {
        if y < top || y >= bottom {
            return frame::HIGHLIGHT;
        }
        let t = (y - top) as f32;
        let idle = t * 32.0 / 61.0;
        let hov = (24.0 + 1.53 * t - 0.867 * (x as f32 + 0.5 - cx).abs()).max(idle);
        let a = (ba + idle + (hov - idle) * hover).clamp(0.0, 255.0);
        let (r, g, b) = theme::tint((141.0 + 32.0 * t / 61.0).round() as u8);
        Rgba(r, g, b, a.round() as u8)
    };
    frame::shape(c, frame, radius, radius, |_| frame::HIGHLIGHT, fill);
}

fn knob(c: &mut Canvas, cx: i32, cy: i32, bright: f32) {
    // 原版：直径约 6 像素的亮点（≈200）+ 约 6 像素的柔和光晕
    for y in cy - 7..=cy + 7 {
        for x in cx - 7..=cx + 7 {
            let d = (((x - cx) as f32 + 0.5).powi(2) + ((y - cy) as f32 + 0.5).powi(2)).sqrt();
            let core = (3.4 - d).clamp(0.0, 1.0);
            let halo = (1.0 - d / 7.5).clamp(0.0, 1.0) * 0.33;
            c.blend(x, y, Rgba::gray(205), (core.max(halo)) * bright);
        }
    }
}

pub fn fmt_time(t: f64) -> String {
    let s = t.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn paint(c: &mut Canvas, m: &BarModel) {
    c.clear();
    let lay = Layout::of(c.w);
    background(c, lay.frame, m.hover);
    let idle = 1.0 - m.hover;
    // 封面（半透明：空闲时约 56%，悬停时更淡，被控制按钮盖住）
    let cover_alpha = 0.56 * idle + 0.18 * m.hover;
    match m.cover {
        Some((px, w, h)) => c.image(px, w, h, COVER, cover_alpha),
        None => {
            // 没有封面：很淡的灰色方块和一个圆环（与原版相同的位置和不透明度）
            let a = (32.0 * (idle + 0.5 * m.hover)) as u8;
            c.fill(COVER, Rgba(0x50, 0x50, 0x50, a));
            let (cx, cy) = (COVER.x as f32 + COVER.w as f32 / 2.0, COVER.y as f32 + COVER.h as f32 / 2.0);
            for y in COVER.y..COVER.bottom() {
                for x in COVER.x..COVER.right() {
                    let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                    let ring = (2.8 - (d - 13.0).abs()).clamp(0.0, 1.0);
                    c.blend(x, y, Rgba(0x9A, 0x9A, 0x9A, a), ring * 0.45);
                }
            }
        }
    }
    // 标题与标语：浅色字 + 一圈暗色的光晕（玻璃几乎透明，放在什么背景上都看得清；悬停时变淡，被控制按钮覆盖）
    let text_a = (255.0 * (0.25 + 0.75 * idle)) as u8;
    let halo = |a: u8| Rgba(16, 16, 16, (a as f32 * 0.8) as u8);
    c.push_clip(R::new(80, 6, 150, 20));
    c.glow_text("AIRPLAY", FONT, 82, 11, Rgba::gray(235).alpha(text_a), halo(text_a), 3);
    c.pop_clip();
    // 标语在自己的一行里上下切换（与原版一致：标题第 15~23 行，标语第 29~40 行），滑出的标语不会压到标题上
    c.push_clip(R::new(80, 25, 152, 19));
    let k = m.slogan.clamp(0.0, 1.0);
    let off = (k * 16.0).round() as i32;
    let (a0, a1) = (((1.0 - k) * text_a as f32) as u8, (k * text_a as f32) as u8);
    c.glow_text(SLOGANS[0], FONT_SLOGAN, 82, 27 - off, Rgba::gray(235).alpha(a0), halo(a0), 3);
    c.glow_text(SLOGANS[1], FONT_SLOGAN, 82, 43 - off, Rgba::gray(235).alpha(a1), halo(a1), 3);
    c.pop_clip();
    // 进度（空闲时是一条淡淡的浅色线）
    let frac = if m.dur > 0.0 { (m.pos / m.dur).clamp(0.0, 1.0) } else { 0.0 };
    let track = lay.track;
    c.fill(track, Rgba::gray(0xAE).alpha(60).mix(Rgba::gray(192), m.hover));
    let kx = track.x + 4 + ((track.w - 8) as f64 * frac) as i32;
    // 没有在播放的歌曲时（与原版相同）空闲状态不显示进度点和时间
    let loaded = m.dur > 0.0;
    if loaded || m.hover > 0.0 {
        knob(c, kx, track.y + 2, if loaded { 1.0 } else { m.hover });
    }
    // 时间：空闲时在进度条右侧显示已播放时间；悬停时左侧已播放、右侧总时长
    let tc = Rgba::gray(117).mix(Rgba::gray(230), m.hover);
    if m.hover < 1.0 && loaded {
        c.num6x8(&fmt_time(m.pos), lay.time_x, 51, Rgba::gray(117).alpha((idle * 255.0) as u8));
    }
    if m.hover > 0.0 {
        let a = (m.hover * 255.0) as u8;
        let el = fmt_time(m.pos);
        let w = el.chars().count() as i32 * 7;
        c.num6x8(&el, 73 - w, 51, tc.alpha(a));
        c.num6x8(&fmt_time(m.dur), lay.time_x, 51, tc.alpha(a));
        controls(c, m, &lay, a);
    }
}

fn controls(c: &mut Canvas, m: &BarModel, l: &Layout, a: u8) {
    let (big, win_btns, toggles) = (&l.big, &l.win_btns, &l.toggles);
    let hot = |h: Hot| if m.hot == h { Rgba::gray(255).alpha(a) } else { Rgba::gray(222).alpha(a) };
    // 音量条（浅色玻璃：空的部分约 42% 不透明，音量部分约 82%）
    c.round_rect(VOLUME, 3.0, Some(Rgba(0x9F, 0x9F, 0x9F, 107).alpha(a)), None);
    let v = if m.muted { 0.0 } else { m.volume.clamp(0.0, 1.0) };
    let fh = (VOLUME.h as f32 * v) as i32;
    c.push_clip(R::new(VOLUME.x, VOLUME.bottom() - fh, VOLUME.w, fh));
    c.round_rect(VOLUME, 3.0, Some(Rgba(0xE3, 0xE3, 0xE3, 208).alpha(a)), None);
    c.pop_clip();
    c.num6x8(&format!("{}", (v * 100.0).round() as i32), 20, 30, Rgba::gray(230).alpha(a));
    // 大号控制按钮
    c.icon(icons::BIG_PREV, big[0].x, big[0].y, hot(Hot::Prev));
    c.icon(if m.playing { icons::BIG_PAUSE } else { icons::BIG_PLAY }, big[1].x, big[1].y, hot(Hot::PlayPause));
    c.icon(icons::BIG_NEXT, big[2].x, big[2].y, hot(Hot::Next));
    // 窗口按钮
    let pin = if m.topmost { icons::W_PINNED } else { icons::W_PIN };
    for (i, (cell, h)) in [(icons::W_MIN, Hot::Min), (icons::W_MINIBAR, Hot::Minibar), (pin, Hot::Pin), (icons::W_CLOSE, Hot::Close)].iter().enumerate() {
        c.icon(*cell, win_btns[i].x, win_btns[i].y, hot(*h));
    }
    // 歌词图片 / 音乐列表 / 均衡器 开关
    let on = |b: bool, h: Hot| if b { hot(h) } else { Rgba::gray(110).alpha(a) };
    c.icon(icons::LYRIC_PIC, toggles[0].x, toggles[0].y, on(m.show_lyric, Hot::Lyric));
    c.icon(icons::LIST, toggles[1].x, toggles[1].y, on(m.show_list, Hot::List));
    c.icon(icons::EQ, toggles[2].x, toggles[2].y, on(m.show_eq, Hot::Eq));
    use crate::config::PlayMode::*;
    let mode = match m.mode {
        Sequential => icons::MODE_SEQ,
        Shuffle => icons::MODE_SHUFFLE,
        RepeatAll => icons::MODE_REPEAT,
        RepeatOne => icons::MODE_ONE,
    };
    c.icon(mode, l.mode.x, l.mode.y, hot(Hot::Mode));
}
