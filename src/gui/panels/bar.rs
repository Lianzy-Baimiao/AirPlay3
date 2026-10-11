//! 播放条（原版 304×68 分层窗口，可见区域 300×64，四周 2 像素透明边）。
//! 空闲时：封面 + "AIRPLAY" + 标语 + 进度；鼠标悬停时淡入控制按钮（音量、上一首/暂停/下一首、窗口按钮、开关）。

use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::text::Font;
use crate::skin::icons;

pub const W: i32 = 304;
pub const H: i32 = 68;
pub const FRAME: R = R::new(2, 2, 300, 64);
pub const COVER: R = R::new(15, 6, 56, 56);
pub const TRACK: R = R::new(79, 53, 144, 4);
pub const VOLUME: R = R::new(9, 9, 7, 50);
pub const BIG: [R; 3] = [R::new(101, 13, 32, 32), R::new(136, 13, 32, 32), R::new(171, 13, 32, 32)];
pub const WIN_BTNS: [R; 4] = [R::new(238, 4, 15, 15), R::new(252, 4, 15, 15), R::new(267, 4, 15, 15), R::new(282, 4, 15, 15)];
pub const TOGGLES: [R; 3] = [R::new(234, 25, 16, 16), R::new(255, 25, 16, 16), R::new(275, 25, 16, 16)];
pub const MODE_BTN: R = R::new(281, 46, 16, 16);

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

pub fn hit(x: i32, y: i32, hovering: bool) -> Hot {
    if !hovering {
        return if COVER.contains(x, y) { Hot::Cover } else { Hot::None };
    }
    let checks = [
        (VOLUME.shrink(-3), Hot::Volume),
        (BIG[0], Hot::Prev),
        (BIG[1], Hot::PlayPause),
        (BIG[2], Hot::Next),
        (WIN_BTNS[0], Hot::Min),
        (WIN_BTNS[1], Hot::Minibar),
        (WIN_BTNS[2], Hot::Pin),
        (WIN_BTNS[3], Hot::Close),
        (TOGGLES[0], Hot::Lyric),
        (TOGGLES[1], Hot::List),
        (TOGGLES[2], Hot::Eq),
        (MODE_BTN, Hot::Mode),
        (R::new(TRACK.x - 4, TRACK.y - 5, TRACK.w + 8, TRACK.h + 10), Hot::Track),
    ];
    checks.iter().find(|(r, _)| r.contains(x, y)).map_or(Hot::None, |c| c.1)
}

/// 背景：空闲为自上而下的暗色渐变；悬停时叠加中间亮、两侧暗的灰色渐变
fn background(c: &mut Canvas, hover: f32) {
    let f = FRAME.shrink(1);
    c.push_clip(f);
    c.vgrad(f, Rgba::gray(0), Rgba::gray(21));
    if hover > 0.0 {
        let a = (hover * 255.0) as u8;
        for y in f.y..f.bottom() {
            let t = (y - f.y) as f32 / (f.h - 1) as f32;
            let (edge, mid) = (2.0 + 55.0 * t, 66.0 + 80.0 * t);
            for x in f.x..f.right() {
                let d = ((x as f32 - 151.0).abs() / 149.0).min(1.0);
                let v = (mid + (edge - mid) * d) as u8;
                c.blend(x, y, Rgba(v, v, v, a), 1.0);
            }
        }
    }
    c.pop_clip();
    c.round_rect(FRAME, 4.0, None, Some(Rgba::gray(85)));
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
    background(c, m.hover);
    let idle = 1.0 - m.hover;
    // 封面（空闲时半透明，悬停时更暗）
    let cover_alpha = 0.45 * idle + 0.18 * m.hover;
    match m.cover {
        Some((px, w, h)) => c.image(px, w, h, COVER, cover_alpha),
        None => c.fill(COVER, Rgba(38, 69, 73, (cover_alpha * 255.0) as u8)),
    }
    // 标题与标语（悬停时变暗，被控制按钮覆盖）
    let text_a = (255.0 * (0.25 + 0.75 * idle)) as u8;
    c.push_clip(R::new(80, 6, 150, 36));
    c.glow_text("AIRPLAY", FONT, 82, 11, Rgba::gray(235).alpha(text_a), Rgba(255, 255, 255, (55.0 * (0.25 + 0.75 * idle)) as u8), 3);
    let k = m.slogan.clamp(0.0, 1.0);
    let off = (k * 16.0).round() as i32;
    let (a0, a1) = (((1.0 - k) * text_a as f32) as u8, (k * text_a as f32) as u8);
    c.glow_text(SLOGANS[0], FONT_SLOGAN, 82, 22 - off, Rgba::gray(235).alpha(a0), Rgba(255, 255, 255, (a0 as f32 * 0.22) as u8), 3);
    c.glow_text(SLOGANS[1], FONT_SLOGAN, 82, 38 - off, Rgba::gray(235).alpha(a1), Rgba(255, 255, 255, (a1 as f32 * 0.22) as u8), 3);
    c.pop_clip();
    // 进度
    let frac = if m.dur > 0.0 { (m.pos / m.dur).clamp(0.0, 1.0) } else { 0.0 };
    c.fill(TRACK, Rgba::gray(69).mix(Rgba::gray(192), m.hover));
    let kx = TRACK.x + 4 + ((TRACK.w - 8) as f64 * frac) as i32;
    knob(c, kx, TRACK.y + 2, 1.0);
    // 时间：空闲时在进度条右侧显示已播放时间；悬停时左侧已播放、右侧总时长
    let tc = Rgba::gray(117).mix(Rgba::gray(230), m.hover);
    if m.hover < 1.0 {
        c.num6x8(&fmt_time(m.pos), 232, 51, Rgba::gray(117).alpha((idle * 255.0) as u8));
    }
    if m.hover > 0.0 {
        let a = (m.hover * 255.0) as u8;
        let el = fmt_time(m.pos);
        let w = el.chars().count() as i32 * 7;
        c.num6x8(&el, 73 - w, 51, tc.alpha(a));
        c.num6x8(&fmt_time(m.dur), 232, 51, tc.alpha(a));
        controls(c, m, a);
    }
}

fn controls(c: &mut Canvas, m: &BarModel, a: u8) {
    let hot = |h: Hot| if m.hot == h { Rgba::gray(255).alpha(a) } else { Rgba::gray(222).alpha(a) };
    // 音量条
    c.round_rect(VOLUME, 3.0, Some(Rgba::gray(40).alpha(a)), None);
    let v = if m.muted { 0.0 } else { m.volume.clamp(0.0, 1.0) };
    let fh = (VOLUME.h as f32 * v) as i32;
    c.push_clip(R::new(VOLUME.x, VOLUME.bottom() - fh, VOLUME.w, fh));
    c.round_rect(VOLUME, 3.0, Some(Rgba::gray(190).alpha(a)), None);
    c.pop_clip();
    c.num6x8(&format!("{}", (v * 100.0).round() as i32), 20, 30, Rgba::gray(230).alpha(a));
    // 大号控制按钮
    c.icon(icons::BIG_PREV, BIG[0].x, BIG[0].y, hot(Hot::Prev));
    c.icon(if m.playing { icons::BIG_PAUSE } else { icons::BIG_PLAY }, BIG[1].x, BIG[1].y, hot(Hot::PlayPause));
    c.icon(icons::BIG_NEXT, BIG[2].x, BIG[2].y, hot(Hot::Next));
    // 窗口按钮
    let pin = if m.topmost { icons::W_PINNED } else { icons::W_PIN };
    for (i, (cell, h)) in [(icons::W_MIN, Hot::Min), (icons::W_MINIBAR, Hot::Minibar), (pin, Hot::Pin), (icons::W_CLOSE, Hot::Close)].iter().enumerate() {
        c.icon(*cell, WIN_BTNS[i].x, WIN_BTNS[i].y, hot(*h));
    }
    // 歌词图片 / 音乐列表 / 均衡器 开关
    let on = |b: bool, h: Hot| if b { hot(h) } else { Rgba::gray(110).alpha(a) };
    c.icon(icons::LYRIC_PIC, TOGGLES[0].x, TOGGLES[0].y, on(m.show_lyric, Hot::Lyric));
    c.icon(icons::LIST, TOGGLES[1].x, TOGGLES[1].y, on(m.show_list, Hot::List));
    c.icon(icons::EQ, TOGGLES[2].x, TOGGLES[2].y, on(m.show_eq, Hot::Eq));
    use crate::config::PlayMode::*;
    let mode = match m.mode {
        Sequential => icons::MODE_SEQ,
        Shuffle => icons::MODE_SHUFFLE,
        RepeatAll => icons::MODE_REPEAT,
        RepeatOne => icons::MODE_ONE,
    };
    c.icon(mode, MODE_BTN.x, MODE_BTN.y, hot(Hot::Mode));
}
