//! 歌词图片面板（原版 304×227）：说明文字、大封面 + 歌曲信息浮层，播放一会儿后切换为小封面 + 滚动歌词。

use super::frame;
use crate::gui::theme;
use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::text::{Font, Hint};
use crate::skin::{icons, skin};

pub const W: i32 = 304;
pub const H: i32 = 225;
const FONT: Font = Font::px(12.0, false);
/// 外框尺寸范围：原版主体 280×204 ～ 800×800，加上标题条与边（宽 +4、高 +21）
pub const MIN: (i32, i32) = (284, 225);
pub const MAX: (i32, i32) = (804, 821);

/// 大封面：默认尺寸时与原版位置一致，面板变大时保持居中
fn big_cover(w: i32, h: i32) -> R {
    R::new(88 + (w - W) / 2, (57 + (h - H) / 2).max(30), 127, 127)
}

fn small_cover(h: i32) -> R {
    R::new(20, (85 + (h - H) / 2).max(30), 62, 62)
}
const LINE_H: i32 = 17;

pub struct CoverModel<'a> {
    pub cover: Option<(&'a [u8], u32, u32)>,
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    /// 有歌曲在播放（否则显示 airplay 标志）
    pub active: bool,
    /// 歌词行与当前行
    pub lines: &'a [String],
    pub current: Option<usize>,
    /// 0 = 大封面 + 信息，1 = 小封面 + 歌词（中间为过渡）
    pub lyric_mode: f32,
    /// 信息浮层透明度 0..1
    pub info: f32,
    pub strip_hover: f32,
    /// 临时提示（下载专辑图片的结果），替换顶部的说明文字
    pub status: Option<&'a str>,
}

pub fn paint(c: &mut Canvas, m: &CoverModel) {
    let body = frame::panel(c, m.strip_hover);
    c.push_clip(body);
    // 顶部的说明文字：与原版相同，只在有歌曲时显示（空闲时只有标志）
    if m.active || m.status.is_some() {
        let hint = m.status.unwrap_or("歌曲及歌词相关信息均来自互联网");
        let tw = crate::gui::text::measure(hint, FONT).0 as i32;
        c.text(hint, FONT, (c.w - tw) / 2, body.y + 3, theme::g(150), Hint::ClearType);
    }
    if !m.active {
        let logo = &skin().logo;
        let (x, y) = ((c.w - logo.w as i32) / 2, body.y + 60 + (c.h - H) / 2);
        c.sprite(logo, x, y, theme::g(255));
        c.pop_clip();
        return;
    }
    let (big, small) = (big_cover(c.w, c.h), small_cover(c.h));
    let k = m.lyric_mode.clamp(0.0, 1.0);
    let lerp = |a: i32, b: i32| a + ((b - a) as f32 * k).round() as i32;
    let cr = R::new(lerp(big.x, small.x), lerp(big.y, small.y), lerp(big.w, small.w), lerp(big.h, small.h));
    draw_cover(c, m.cover, cr);
    if k > 0.0 {
        lyric_lines(c, m, small, body, (k * 255.0) as u8);
    }
    if m.info > 0.0 {
        info_overlay(c, m, body, (m.info * 255.0) as u8);
    }
    c.pop_clip();
}

/// 封面 + 下方倒影
fn draw_cover(c: &mut Canvas, cover: Option<(&[u8], u32, u32)>, r: R) {
    match cover {
        Some((px, w, h)) => {
            c.image(px, w, h, r, 1.0);
            // 倒影：翻转后渐隐
            let rh = (r.h / 3).max(1);
            for dy in 0..rh {
                let a = 0.25 * (1.0 - dy as f32 / rh as f32);
                let sy = ((r.h - 1 - dy) as u32 * h / r.h as u32).min(h - 1);
                let row = &px[(sy * w * 4) as usize..((sy + 1) * w * 4) as usize];
                c.image(row, w, 1, R::new(r.x, r.bottom() + 1 + dy, r.w, 1), a);
            }
        }
        None => c.sprite(&skin().cover, r.x + (r.w - 128) / 2, r.y + (r.h - 128) / 2, theme::g(70)),
    }
}

/// 歌词：当前行固定在小封面的垂直中线（原版），之前的行在上、之后的行在下，越远越暗
fn lyric_lines(c: &mut Canvas, m: &CoverModel, small: R, body: R, a: u8) {
    let x = small.right() + 20;
    let anchor = small.y + small.h / 2 - 6;
    let cur = m.current.unwrap_or(0) as i32;
    c.push_clip(R::new(x, body.y + 20, body.right() - x - 4, body.h - 22));
    for (i, line) in m.lines.iter().enumerate() {
        let d = i as i32 - cur;
        let y = anchor + d * LINE_H;
        if y < body.y || y > body.bottom() {
            continue;
        }
        let col = if m.current == Some(i) { theme::focus() } else { theme::g((230 - d.abs().min(6) * 22) as u8) };
        c.text(line, FONT, x, y, col.alpha(a), Hint::ClearType);
    }
    c.pop_clip();
}

fn info_overlay(c: &mut Canvas, m: &CoverModel, body: R, a: u8) {
    let r = R::new(body.x, c.h - 70, body.w, 62);
    c.fill(R::new(r.x, r.y, r.w, 1), Rgba::gray(59).alpha(a));
    c.fill(R::new(r.x, r.y + 1, r.w, r.h - 2), Rgba(10, 10, 10, (a as f32 * 0.85) as u8));
    c.fill(R::new(r.x, r.bottom() - 1, r.w, 1), Rgba::gray(59).alpha(a));
    let col = Rgba::gray(235).alpha(a);
    c.text(m.title, FONT, 14, r.y + 6, col, Hint::ClearType);
    c.icon(icons::MIC, 10, r.y + 24, Rgba::gray(120).alpha(a));
    c.text(m.artist, FONT, 32, r.y + 24, col, Hint::ClearType);
    c.icon(icons::DISC, 10, r.y + 42, Rgba::gray(120).alpha(a));
    c.text(m.album, FONT, 32, r.y + 42, col, Hint::ClearType);
}
