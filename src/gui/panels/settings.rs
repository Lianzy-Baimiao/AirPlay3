//! 「配置」窗口（原版的样子）：左边一列分页（选中的一格是暗色的，与右边的内容连成一片），
//! 右边是各分页的控件（布局见 `settings_layout`，控件的画法见 `pill`）。

use super::frame;
use super::pill::{self, FONT};
use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::settings_layout::{Glyph, Item, Toggle, Widget};
use crate::gui::settings_model::{Swatch, TABS};
use crate::gui::text::{self, Hint};
use crate::gui::theme;
use crate::skin::sdf::{S, Shape};

pub const W: i32 = 504;
pub const H: i32 = 386;
/// 分页栏、分隔线的 x；分页的第一格的 y、每格的间距
const TAB_W: i32 = 98;
const SEP_X: i32 = 101;
const TAB_Y: i32 = 50;
const TAB_PITCH: i32 = 22;

pub fn tab_rect(i: usize) -> R {
    R::new(2, TAB_Y + i as i32 * TAB_PITCH, TAB_W + 1, 20)
}

/// 各控件现在的状态（由窗口算好传进来）
#[derive(Clone, Default, Debug)]
pub struct State {
    /// 勾着
    pub on: bool,
    /// 能改（不能改的画暗）
    pub enabled: bool,
    /// 画成暗的（灰掉、或关着的热键）
    pub dim: bool,
    /// 下拉框、输入框、热键的文字
    pub value: String,
    pub focused: bool,
}

pub struct SettingsModel<'a> {
    pub tab: usize,
    pub items: &'a [(Item, State)],
    pub hover: Option<usize>,
    pub hover_tab: Option<usize>,
    /// 鼠标下的色块
    pub hover_cell: Option<usize>,
    /// 标题条上的光（鼠标在窗口里时为 1）
    pub strip: f32,
}

/// 色块里与当前颜色相同的一格（与原版一样只在完全相同时打勾：默认的焦点颜色不在色块里）
pub fn swatch_cur(k: Swatch) -> Option<usize> {
    let t = theme::get();
    let want = if k == Swatch::Body { t.body } else { t.focus };
    k.presets().iter().position(|c| *c == want)
}

/// 一段竖线的渐变（y0 到 y1 从 a 变到 b）
fn vline(c: &mut Canvas, x: i32, y0: i32, y1: i32, a: u8, b: u8) {
    for y in y0..y1 {
        let t = if y1 - y0 > 1 { (y - y0) as f32 / (y1 - y0 - 1) as f32 } else { 0.0 };
        c.fill(R::new(x, y, 1, 1), Rgba::gray((a as f32 + (b as f32 - a as f32) * t).round() as u8));
    }
}

fn tabs(c: &mut Canvas, sel: usize, hover: Option<usize>) {
    let (col, body) = (theme::g(64), theme::body());
    let light = |v: Rgba| v.mix(Rgba::gray(255), 65.0 / 255.0);
    // 标题条下面的暗线；分页栏、内容的底色；左边、底边的亮边
    c.fill(R::new(3, 19, TAB_W, 1), theme::g(37));
    c.fill(R::new(SEP_X, 19, 1, 1), theme::g(62));
    c.fill(R::new(SEP_X + 1, 19, W - SEP_X - 4, 1), theme::g(18));
    c.fill(R::new(3, 20, TAB_W, 363), col);
    c.fill(R::new(SEP_X + 1, 20, W - SEP_X - 4, 363), body);
    c.fill(R::new(2, 19, 1, 364), light(col));
    c.fill(R::new(3, 383, TAB_W, 1), light(col));
    c.fill(R::new(SEP_X, 383, 1, 1), light(col));
    c.fill(R::new(SEP_X + 1, 383, W - SEP_X - 4, 1), light(body));
    // 选中的一格：暗色，上下各一条线，与右边连成一片
    let bar = tab_rect(sel);
    c.fill(R::new(bar.x + 1, bar.y - 1, TAB_W, 1), theme::g(86));
    c.fill(R::new(bar.x + 1, bar.y, TAB_W + 1, 20), body);
    c.fill(R::new(bar.x + 1, bar.y + 20, TAB_W, 1), theme::g(86));
    // 分页栏右边的分隔线：越靠近选中的一格越亮
    vline(c, SEP_X, 20, bar.y - 1, 95, 124);
    c.fill(R::new(SEP_X, bar.y - 1, 1, 1), theme::g(104));
    c.fill(R::new(SEP_X, bar.y + 20, 1, 1), theme::g(104));
    vline(c, SEP_X, bar.y + 21, 383, 125, 97);
    for (i, name) in TABS.iter().enumerate() {
        let r = tab_rect(i);
        let tw = text::measure(name, FONT).0 as i32;
        let v = if i == sel { 255 } else if hover == Some(i) { 0xE8 } else { 0xBF };
        c.text(name, FONT, r.x + (TAB_W - tw) / 2 + 1, r.y + 2, Rgba::gray(v), Hint::ClearType);
    }
}

pub fn paint(c: &mut Canvas, m: &SettingsModel) {
    frame::panel(c, m.strip);
    tabs(c, m.tab, m.hover_tab);
    for (i, (it, st)) in m.items.iter().enumerate() {
        let hot = m.hover == Some(i);
        let r = it.r;
        match &it.w {
            Widget::Pill { tone, label, glyph, act } => {
                pill::pill(c, r, *tone, st.dim);
                if hot && !st.dim {
                    c.fill(r.shrink(3), Rgba(255, 255, 255, 22));
                }
                if let Some(g) = glyph {
                    draw_glyph(c, *g, r.x + 20, r.y + 10, if st.dim { pill::dim_ink(*tone) } else { pill::PILL_TEXT });
                }
                let text = if label.is_empty() { st.value.as_str() } else { label.as_str() };
                if !text.is_empty() {
                    pill::pill_text(c, r, text, *tone, st.dim);
                }
                if !matches!(act, Toggle::Act(_)) {
                    pill::pill_box(c, r, *tone, st.on, st.dim);
                }
            }
            Widget::Check(_, label) => pill::plain_check(c, r, label, st.on, hot),
            Widget::Label(s) => {
                c.text(s, FONT, r.x, r.y + (r.h - 16) / 2 + 1, Rgba::gray(0xA0), Hint::ClearType);
            }
            Widget::Title(s) => {
                let tw = text::measure(s, FONT).0 as i32;
                c.text(s, FONT, r.x + (r.w - tw) / 2, r.y, Rgba::gray(0xBB), Hint::ClearType);
            }
            Widget::Drop(_) => pill::dropdown(c, r, &st.value, st.enabled, hot),
            Widget::Edit(_, hint) => pill::edit(c, r, &st.value, hint, st.enabled, st.focused),
            Widget::Path(_, hint) => pill::path_box(c, r, &st.value, hint, st.enabled, hot),
            Widget::Swatches(k) => {
                let hover = if hot { m.hover_cell } else { None };
                pill::swatches(c, r, k.presets(), swatch_cur(*k), hover);
            }
            Widget::Slider => pill::slider(c, r, theme::border_alpha(), hot),
            Widget::Groove(peak) => pill::groove(c, r, *peak),
        }
    }
}

/// 全局热键页药丸上的小图标（中心在 cx, cy）
fn draw_glyph(c: &mut Canvas, g: Glyph, cx: i32, cy: i32, col: Rgba) {
    let tri = |x0: f32, dir: f32| S::Poly(vec![(x0, -0.5), (x0, 8.5), (x0 + 7.0 * dir, 4.0)], 0.0);
    let speaker = S::Poly(vec![(0.0, 2.5), (2.5, 2.5), (5.5, 0.0), (5.5, 8.0), (2.5, 5.5), (0.0, 5.5)], 0.0);
    let s = match g {
        Glyph::PlayPause => Shape::of(vec![tri(-9.0, 1.0), S::Rect(0.5, -0.5, 3.0, 8.5, 0.0), S::Rect(5.0, -0.5, 7.5, 8.5, 0.0)]),
        Glyph::Prev => Shape::of(vec![S::Rect(-4.0, 0.0, -2.0, 8.0, 0.0), tri(4.0, -1.0)]),
        Glyph::Next => Shape::of(vec![tri(-4.0, 1.0), S::Rect(2.0, 0.0, 4.0, 8.0, 0.0)]),
        Glyph::Mute => Shape::of(vec![speaker.clone(), S::Line(7.5, 5.0, 10.5, 8.0, 1.2), S::Line(10.5, 5.0, 7.5, 8.0, 1.2)]),
        Glyph::VolDown => Shape::of(vec![speaker.clone(), S::Line(7.0, 7.5, 11.0, 7.5, 1.4)]),
        Glyph::VolUp => Shape::of(vec![speaker, S::Line(7.0, 7.0, 11.0, 7.0, 1.4), S::Line(9.0, 5.0, 9.0, 9.0, 1.4)]),
        Glyph::Lyric => Shape::of(vec![S::Rect(-5.0, 0.0, 5.0, 6.5, 3.0), S::Poly(vec![(-3.0, 5.0), (-1.0, 5.0), (-4.0, 9.0)], 0.0)]),
        Glyph::Heart => Shape::of(vec![S::Circle(-2.3, 2.5, 2.6), S::Circle(2.3, 2.5, 2.6), S::Poly(vec![(-4.6, 3.5), (4.6, 3.5), (0.0, 8.5)], 0.3)]),
        Glyph::Ban => Shape::of(vec![S::Ring(0.0, 4.0, 4.0, 1.5), S::Line(-2.8, 1.2, 2.8, 6.8, 1.5)]),
        Glyph::Window => Shape::of(vec![S::Rect(-5.0, 0.0, 5.0, 8.0, 0.5)]).minus(vec![S::Rect(-3.6, 2.6, 3.6, 6.6, 0.0)]),
    };
    let ox = if matches!(g, Glyph::Mute | Glyph::VolDown | Glyph::VolUp) { cx - 5 } else { cx };
    let (fx, fy, k) = (ox as f32, (cy - 4) as f32, c.scale);
    c.shade(R::new(ox - 8, cy - 6, 21, 14), false, |x, y| (col, (0.5 - s.d(x - fx, y - fy) * k).clamp(0.0, 1.0)));
}
