//! 「下载歌词」窗口（照原版截图量出来的）：350 × 600，没有标题条，四周 36 像素的柔和阴影。
//! 上面是标题 / 艺人 / 专辑三个框和「开始搜索」，中间是结果列表、所选歌词的预览，
//! 下面是专辑图片、艺人图片、要下载什么的勾选框，最底下一排按钮。

use super::pill;
use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::text::{self, Font, Hint};
use crate::gui::theme;
use crate::skin::icons;

/// 阴影的宽度；窗口（含阴影）的大小
pub const SHADOW: i32 = 36;
pub const W: i32 = 350 + 2 * SHADOW;
pub const H: i32 = 600 + 2 * SHADOW;
const FONT: Font = Font::px(12.0, false);

/// 窗口里（不含阴影）的坐标
const fn b(x: i32, y: i32, w: i32, h: i32) -> R {
    R::new(SHADOW + x, SHADOW + y, w, h)
}

pub const FIELDS: [R; 3] = [b(12, 10, 216, 19), b(12, 35, 216, 19), b(12, 60, 216, 19)];
pub const BTN_SEARCH: R = b(237, 33, 103, 23);
pub const LIST: R = b(1, 90, 348, 242);
pub const ROW_H: i32 = 21;
const PREVIEW: R = b(1, 333, 348, 118);
/// 预览区放得下的行数
pub const PREVIEW_LINES: usize = 6;
pub const PICS: [R; 2] = [b(10, 465, 82, 82), b(100, 465, 82, 82)];
/// 勾选框（下载歌词、专辑图片、艺人图片、下载后自动修改信息）
pub const CHECKS: [R; 4] = [b(201, 466, 140, 18), b(201, 486, 140, 18), b(201, 506, 140, 18), b(201, 526, 140, 18)];
pub const CHECK_LABELS: [&str; 4] = ["下载歌词", "下载专辑图片", "下载艺人图片", "下载后自动修改信息"];
/// 底下的按钮：补全歌曲信息（没有可补全的时显示「信息没找到」）、下载、关闭
pub const BTN_INFO: R = b(10, 569, 120, 23);
pub const BTN_DOWNLOAD: R = b(180, 569, 80, 23);
pub const BTN_CLOSE: R = b(260, 569, 80, 23);

pub struct ResultRow<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub duration: &'a str,
    pub source: crate::lyrics::online::Provider,
}

pub struct Pic<'a> {
    pub rgba: &'a [u8],
    pub w: u32,
    pub h: u32,
}

pub struct LyricDlModel<'a> {
    pub fields: [&'a str; 3],
    pub focus: Option<usize>,
    pub rows: &'a [ResultRow<'a>],
    pub selected: Option<usize>,
    pub hover: Option<usize>,
    pub scroll: i32,
    /// 列表空着时显示的文字（正在搜索、没找到、出错）
    pub list_note: &'a str,
    /// 预览区的文字（所选歌词的前几行，或下载进度 / 出错）
    pub preview: &'a [String],
    pub pics: [Option<Pic<'a>>; 2],
    pub checks: [bool; 4],
    /// 第 4 个勾选框能不能改（所选结果有可补全的信息时）
    pub info_found: bool,
    pub can_download: bool,
    /// 鼠标下的按钮：0 搜索、1 补全信息、2 下载、3 关闭；鼠标下的勾选框
    pub hot_button: Option<usize>,
    pub hot_check: Option<usize>,
}

/// 原版阴影离窗口 d 像素（d = 1 紧贴窗口）处的不透明度（量出来的）
const FALLOFF: [u8; 22] = [38, 35, 32, 30, 27, 25, 22, 20, 18, 16, 14, 12, 11, 9, 8, 6, 5, 4, 3, 2, 2, 1];

/// 沿着阴影条的方向，离窗口的角 u 像素（负数在角外面）处的强度（量出来的）
fn ramp(u: f32) -> f32 {
    const P: [(f32, f32); 12] =
        [(-12.0, 0.0), (-8.0, 0.08), (-4.0, 0.16), (0.0, 0.21), (4.0, 0.32), (8.0, 0.45), (12.0, 0.58), (16.0, 0.74), (20.0, 0.84), (24.0, 0.92), (28.0, 0.97), (32.0, 1.0)];
    if u >= 32.0 {
        return 1.0;
    }
    P.windows(2).find(|w| u < w[1].0).map_or(0.0, |w| (w[0].1 + (w[1].1 - w[0].1) * (u - w[0].0) / (w[1].0 - w[0].0)).max(0.0))
}

/// 窗口四周的阴影：上下、左右四条，每条的强度 = 离窗口的距离的衰减 × 沿着条离角的远近；
/// 紧贴窗口外面还有一道 1 像素的暗线（在角上淡下去）。都照原版量出来的
fn shadow(c: &mut Canvas) {
    let body = b(0, 0, 350, 600);
    let fall = |d: i32| if d <= 2 { 1.0 } else { FALLOFF.get(d as usize - 2).map_or(0.0, |&a| a as f32 / 38.0) };
    for y in 0..c.h {
        for x in 0..c.w {
            if body.contains(x, y) {
                continue;
            }
            let dx = (body.x - x).max(x + 1 - body.right());
            let dy = (body.y - y).max(y + 1 - body.bottom());
            // 在上下两条里时，沿着条的方向是 x；在左右两条里时是 y
            let (d, along) = if dy > 0 { (dy, x.min(body.right() - 1 - x) - body.x) } else { (dx, y.min(body.bottom() - 1 - y) - body.y) };
            let shade = 38.0 * fall(d) * ramp(along as f32);
            let a = if d == 1 && along >= -1 { (73.0 * (0.56 + 0.44 * ((along + 1) as f32 / 30.0).min(1.0))).max(shade) } else { shade };
            if a >= 0.5 {
                c.fill(R::new(x, y, 1, 1), Rgba(0, 0, 0, a.round() as u8));
            }
        }
    }
}

/// 输入框：1 像素的边（四角淡一点），右边一个淡淡的图标
fn field(c: &mut Canvas, r: R, s: &str, icon: crate::skin::Cell, focused: bool) {
    let edge = theme::g(if focused { 150 } else { 105 });
    c.fill(R::new(r.x + 1, r.y, r.w - 2, 1), edge);
    c.fill(R::new(r.x + 1, r.bottom() - 1, r.w - 2, 1), edge);
    c.fill(R::new(r.x, r.y + 1, 1, r.h - 2), edge);
    c.fill(R::new(r.right() - 1, r.y + 1, 1, r.h - 2), edge);
    for (x, y) in [(r.x, r.y), (r.right() - 1, r.y), (r.x, r.bottom() - 1), (r.right() - 1, r.bottom() - 1)] {
        c.fill(R::new(x, y, 1, 1), theme::g(89));
    }
    c.icon(icon, r.right() - 18, r.y + 2, theme::g(92));
    c.push_clip(R::new(r.x + 2, r.y + 1, r.w - 22, r.h - 2));
    let tw = text::measure(s, FONT).0 as i32;
    let x = (r.x + 3).min(r.right() - 22 - tw);
    c.text(s, FONT, x, r.y + 2, Rgba::gray(0xE0), Hint::ClearType);
    if focused {
        c.fill(R::new(x + tw + 1, r.y + 3, 1, r.h - 6), Rgba::gray(0xE0));
    }
    c.pop_clip();
}

/// 按钮：外圈暗、里圈亮的两道边，中间是底色
pub fn button(c: &mut Canvas, r: R, label: &str, enabled: bool, hot: bool) {
    let (outer, inner, fill, ink) = match (enabled, hot) {
        (true, true) => (0x33, 0xA0, 0x6E, 0xF8),
        (true, false) => (0x33, 0x89, 0x5F, 0xE6),
        _ => (0x3B, 0x59, 0x4B, 0x6C),
    };
    c.fill(r, Rgba::gray(outer));
    c.fill(r.shrink(1), Rgba::gray(inner));
    c.fill(r.shrink(2), Rgba::gray(fill));
    let tw = text::measure(label, FONT).0 as i32;
    c.text(label, FONT, r.x + (r.w - tw) / 2, r.y + 3, Rgba::gray(ink), Hint::ClearType);
}

fn paint_rows(c: &mut Canvas, m: &LyricDlModel) {
    use crate::lyrics::online::Provider;
    c.push_clip(LIST);
    for (i, r) in m.rows.iter().enumerate() {
        let y = LIST.y + i as i32 * ROW_H - m.scroll;
        if y + ROW_H < LIST.y || y > LIST.bottom() {
            continue;
        }
        let row = R::new(LIST.x, y, LIST.w, ROW_H);
        if m.selected == Some(i) {
            c.fill(row, theme::g(75));
        } else if m.hover == Some(i) {
            c.fill(row, theme::g(48));
        } else if i % 2 == 1 {
            c.fill(row, theme::g(40));
        }
        let tag = if r.source == Provider::LrcApi { Rgba(60, 150, 240, 255) } else { theme::g(150) };
        c.text(r.source.tag(), FONT, row.x + 6, y + 3, tag, Hint::ClearType);
        let dw = text::measure(r.duration, FONT).0 as i32;
        c.push_clip(R::new(row.x + 50, y, row.w - 60 - dw, ROW_H));
        let col = if m.selected == Some(i) { theme::focus() } else { theme::g(224) };
        let w = c.text(r.title, FONT, row.x + 50, y + 3, col, Hint::ClearType);
        c.text(r.artist, FONT, row.x + 50 + w + 10, y + 3, theme::g(150), Hint::ClearType);
        c.pop_clip();
        c.text(r.duration, FONT, row.right() - dw - 8, y + 3, theme::g(170), Hint::ClearType);
    }
    c.pop_clip();
    if m.rows.is_empty() && !m.list_note.is_empty() {
        let tw = text::measure(m.list_note, FONT).0 as i32;
        c.text(m.list_note, FONT, LIST.x + (LIST.w - tw) / 2, LIST.y + LIST.h / 2 - 8, theme::g(150), Hint::ClearType);
    }
}

pub fn paint(c: &mut Canvas, m: &LyricDlModel) {
    c.clear();
    shadow(c);
    let (grey, dark, line) = (theme::g(64), theme::g(32), theme::g(95));
    // 外边、上面一块灰、结果列表、预览、图片和勾选框、按钮一排
    c.fill(b(0, 0, 350, 600), theme::g(101));
    c.fill(b(1, 1, 348, 88), grey);
    c.fill(b(1, 89, 348, 1), line);
    c.fill(LIST, dark);
    c.fill(b(1, 332, 348, 1), line);
    c.fill(PREVIEW, grey);
    c.fill(b(1, 451, 348, 1), line);
    c.fill(b(1, 452, 348, 108), dark);
    c.fill(b(1, 560, 348, 1), line);
    c.fill(b(1, 561, 348, 38), grey);

    let icons = [icons::NOTE, icons::MIC, icons::DISC];
    for (i, r) in FIELDS.iter().enumerate() {
        field(c, *r, m.fields[i], icons[i], m.focus == Some(i));
    }
    button(c, BTN_SEARCH, "开始搜索", true, m.hot_button == Some(0));
    paint_rows(c, m);

    // 预览：所选歌词的前几行（居中）
    c.push_clip(PREVIEW.shrink(2));
    for (i, l) in m.preview.iter().take(PREVIEW_LINES).enumerate() {
        let tw = text::measure(l, FONT).0 as i32;
        c.text(l, FONT, PREVIEW.x + (PREVIEW.w - tw).max(8) / 2, PREVIEW.y + 9 + i as i32 * 17, theme::g(176), Hint::ClearType);
    }
    c.pop_clip();

    // 专辑图片、艺人图片
    for (i, r) in PICS.iter().enumerate() {
        c.fill(*r, Rgba::gray(0x1B));
        c.fill(r.shrink(1), Rgba::gray(0x3D));
        c.fill(r.shrink(2), dark);
        if let Some(p) = &m.pics[i] {
            c.image(p.rgba, p.w, p.h, r.shrink(2), 1.0);
        }
    }
    for (i, r) in CHECKS.iter().enumerate() {
        let enabled = i < 3 || m.info_found;
        pill::plain_check_ex(c, *r, CHECK_LABELS[i], m.checks[i] && enabled, m.hot_check == Some(i) && enabled, enabled);
    }

    let info = if m.info_found { "补全歌曲信息" } else { "信息没找到" };
    button(c, BTN_INFO, info, m.info_found, m.hot_button == Some(1) && m.info_found);
    button(c, BTN_DOWNLOAD, "下载", m.can_download, m.hot_button == Some(2) && m.can_download);
    button(c, BTN_CLOSE, "关闭", true, m.hot_button == Some(3));
}

/// 歌词文字的前几行（去掉时间标签和空行）
pub fn preview_lines(text: &str, n: usize) -> Vec<String> {
    text.lines()
        .map(|l| {
            let mut s = l.trim();
            while s.starts_with('[') {
                match s.find(']') {
                    Some(e) => s = s[e + 1..].trim_start(),
                    None => break,
                }
            }
            s.trim().to_string()
        })
        .filter(|l| !l.is_empty())
        .take(n)
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn preview_strips_tags() {
        let t = "[ti:x]\n[00:01.00]第一句\n\n[00:02.00][00:30.00]第二句\n";
        assert_eq!(super::preview_lines(t, 5), vec!["第一句".to_string(), "第二句".to_string()]);
    }
}