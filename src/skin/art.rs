//! 所有图标、像素数字、标志的画法（原创，按原版界面的样子设计：白色圆润的图形，外面一圈淡淡的暗边）。

use super::sdf::{S, Shape, draw};
use super::{Skin, Sprite};

pub const LOGO_W: u32 = 120;
pub const LOGO_H: u32 = 66;

fn rect(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> S {
    S::Rect(x0, y0, x1, y1, r)
}

fn line(ax: f32, ay: f32, bx: f32, by: f32, w: f32) -> S {
    S::Line(ax, ay, bx, by, w)
}

fn tri(a: (f32, f32), b: (f32, f32), c: (f32, f32), r: f32) -> S {
    S::Poly(vec![a, b, c], r)
}

/// 画一个格子（尺寸 n 的图集里第 col 列、第 row 行）
fn cell(sp: &mut Sprite, n: u32, col: u32, row: u32, shape: Shape) {
    draw(sp, col * n, row * n, n, n, &shape, true);
}

pub fn build() -> Skin {
    let mut player = Sprite::new(128, 32);
    big_controls(&mut player);
    let mut mini = Sprite::new(64, 48);
    small_icons(&mut mini);
    let mut gui10 = Sprite::new(30, 10);
    // 迷你条：还原（顶边粗的方框）、关闭
    cell(&mut gui10, 10, 1, 0, Shape::of(vec![rect(1.0, 2.0, 9.0, 8.6, 0.4)]).minus(vec![rect(2.4, 4.2, 7.6, 7.2, 0.0)]));
    cell(&mut gui10, 10, 2, 0, Shape::of(vec![line(2.4, 2.4, 7.6, 7.6, 1.8), line(7.6, 2.4, 2.4, 7.6, 1.8)]));
    let mut gui15 = Sprite::new(90, 60);
    window_icons(&mut gui15);
    let mut icon15 = Sprite::new(165, 45);
    list_icons(&mut icon15);
    Skin { player, mini, gui10, gui15, icon15, num6x8: digits(), logo: logo(), logo_arc: logo_arc(), cover: cover() }
}

/// 播放条上的大号按钮（32×32）
fn big_controls(sp: &mut Sprite) {
    cell(sp, 32, 0, 0, Shape::of(vec![rect(5.5, 7.5, 10.0, 24.5, 2.0), tri((25.5, 8.5), (25.5, 23.5), (12.0, 16.0), 1.6)]));
    cell(sp, 32, 1, 0, Shape::of(vec![tri((8.5, 6.0), (8.5, 26.0), (26.0, 16.0), 1.9)]));
    cell(sp, 32, 2, 0, Shape::of(vec![rect(8.0, 6.0, 14.5, 26.0, 2.4), rect(17.5, 6.0, 24.0, 26.0, 2.4)]));
    cell(sp, 32, 3, 0, Shape::of(vec![rect(22.0, 7.5, 26.5, 24.5, 2.0), tri((6.5, 8.5), (6.5, 23.5), (20.0, 16.0), 1.6)]));
}

/// 16×16：迷你条的播放控制、播放条右侧的开关、播放模式
fn small_icons(sp: &mut Sprite) {
    cell(sp, 16, 0, 0, Shape::of(vec![rect(2.5, 3.5, 4.8, 12.5, 0.8), tri((12.8, 4.0), (12.8, 12.0), (5.6, 8.0), 0.6)]));
    cell(sp, 16, 1, 0, Shape::of(vec![tri((4.5, 2.8), (4.5, 13.2), (13.2, 8.0), 0.7)]));
    cell(sp, 16, 2, 0, Shape::of(vec![rect(3.5, 3.0, 6.8, 13.0, 0.8), rect(9.2, 3.0, 12.5, 13.0, 0.8)]));
    cell(sp, 16, 3, 0, Shape::of(vec![rect(11.2, 3.5, 13.5, 12.5, 0.8), tri((3.2, 4.0), (3.2, 12.0), (10.4, 8.0), 0.6)]));
    // 歌词图片：方框 + 音符
    let note = vec![
        rect(0.8, 3.0, 8.8, 13.0, 0.5),
        S::Circle(11.4, 12.0, 2.0),
        line(13.1, 3.0, 13.1, 11.8, 1.5),
        line(13.1, 3.2, 15.2, 5.2, 1.4),
    ];
    cell(sp, 16, 1, 1, Shape::of(note).minus(vec![rect(2.3, 4.5, 7.3, 11.5, 0.0)]));
    // 音乐列表：三行
    let mut list = Vec::new();
    for y in [4.0, 8.0, 12.0] {
        list.push(rect(1.5, y - 1.2, 4.0, y + 1.2, 0.4));
        list.push(rect(5.6, y - 1.1, 14.5, y + 1.1, 0.4));
    }
    cell(sp, 16, 2, 1, Shape::of(list));
    // 均衡器：「EQ」
    let eq = vec![
        rect(0.8, 3.0, 2.8, 13.0, 0.2),
        rect(0.8, 3.0, 5.6, 5.0, 0.2),
        rect(0.8, 7.0, 4.9, 9.0, 0.2),
        rect(0.8, 11.0, 5.6, 13.0, 0.2),
        S::Ring(10.6, 7.9, 3.1, 1.9),
        line(11.8, 10.3, 14.2, 12.9, 1.7),
    ];
    cell(sp, 16, 3, 1, Shape::of(eq));
    // 顺序播放：三条横线
    cell(sp, 16, 0, 2, Shape::of([4.0, 8.0, 12.0].iter().map(|&y| rect(1.5, y - 1.1, 14.5, y + 1.1, 0.5)).collect()));
    // 随机播放：交叉的两条线，右端箭头
    let shuffle = vec![
        line(1.5, 4.5, 5.2, 4.5, 1.8),
        line(5.2, 4.5, 10.6, 11.5, 1.8),
        line(10.6, 11.5, 12.4, 11.5, 1.8),
        tri((11.8, 8.9), (15.4, 11.5), (11.8, 14.1), 0.2),
        line(1.5, 11.5, 5.2, 11.5, 1.8),
        line(5.2, 11.5, 10.6, 4.5, 1.8),
        line(10.6, 4.5, 12.4, 4.5, 1.8),
        tri((11.8, 1.9), (15.4, 4.5), (11.8, 7.1), 0.2),
    ];
    cell(sp, 16, 1, 2, Shape::of(shuffle));
    // 列表循环：方形的循环箭头
    cell(sp, 16, 2, 2, Shape::of(repeat_loop(0.0)));
    // 单曲循环：循环箭头 + 中间的「1」
    let mut one = repeat_loop(0.0);
    one.push(line(8.2, 6.4, 8.2, 9.8, 1.4));
    cell(sp, 16, 3, 2, Shape::of(one));
}

fn repeat_loop(dx: f32) -> Vec<S> {
    vec![
        line(2.8 + dx, 9.6, 2.8 + dx, 4.5, 1.8),
        line(2.8 + dx, 4.5, 10.8 + dx, 4.5, 1.8),
        tri((10.4 + dx, 1.9), (14.3 + dx, 4.5), (10.4 + dx, 7.1), 0.3),
        line(13.2 + dx, 6.4, 13.2 + dx, 11.5, 1.8),
        line(13.2 + dx, 11.5, 5.2 + dx, 11.5, 1.8),
        tri((5.6 + dx, 8.9), (1.7 + dx, 11.5), (5.6 + dx, 14.1), 0.3),
    ]
}
/// 15×15：窗口按钮、箭头、搜索、勾选、删除
fn window_icons(sp: &mut Sprite) {
    cell(sp, 15, 0, 0, Shape::of(vec![rect(3.0, 6.5, 12.0, 8.5, 0.8)]));
    cell(sp, 15, 2, 0, Shape::of(vec![line(4.0, 4.0, 11.0, 11.0, 2.2), line(11.0, 4.0, 4.0, 11.0, 2.2)]));
    // 置顶：图钉（已置顶时是实心的）
    let pin = |filled: bool| {
        let s = Shape::of(vec![rect(4.0, 2.0, 11.0, 7.4, 0.8), rect(6.6, 6.0, 8.4, 10.0, 0.0), rect(3.0, 9.6, 12.0, 11.6, 0.6)]);
        if filled { s } else { s.minus(vec![rect(5.6, 3.6, 9.4, 6.0, 0.0)]) }
    };
    cell(sp, 15, 3, 0, pin(true));
    cell(sp, 15, 4, 0, pin(false));
    // 迷你条：三角 + 横条
    cell(sp, 15, 5, 0, Shape::of(vec![tri((4.0, 3.0), (11.0, 3.0), (7.5, 6.6), 0.3), rect(3.0, 8.6, 12.0, 10.8, 0.6)]));
    cell(sp, 15, 2, 1, Shape::of(vec![tri((5.0, 3.5), (5.0, 11.5), (10.8, 7.5), 0.5)]));
    cell(sp, 15, 5, 1, Shape::of(vec![tri((3.5, 5.0), (11.5, 5.0), (7.5, 10.8), 0.5)]));
    // 音乐列表搜索框：放大镜（镜片在右上、柄朝左下）；有字时换成「清除」：实心圆里挖掉一个 ×（照原版，都没有暗边）
    let flat = |sp: &mut Sprite, col: u32, row: u32, shape: Shape| draw(sp, col * 15, row * 15, 15, 15, &shape, false);
    // （参数是对着原版屏幕上的样子逐像素拟合的）
    flat(sp, 1, 2, Shape::of(vec![S::Ring(8.6, 6.9, 4.45, 1.9), line(5.25, 10.4, 2.95, 12.55, 2.15)]));
    flat(sp, 2, 3, Shape::of(vec![S::Circle(7.5, 7.5, 6.2)]).minus(vec![line(5.0, 5.0, 10.0, 10.0, 2.2), line(10.0, 5.0, 5.0, 10.0, 2.2)]));
    cell(sp, 15, 2, 2, Shape::of(vec![line(3.2, 7.6, 6.0, 10.4, 2.2), line(6.0, 10.4, 11.8, 4.6, 2.2)]));
    cell(sp, 15, 1, 3, Shape::of(vec![line(4.0, 4.0, 11.0, 11.0, 2.4), line(11.0, 4.0, 4.0, 11.0, 2.4)]));
}

/// 15×15：音乐列表的工具栏（添加、编辑、排序）、分栏（耳机）、歌曲信息（话筒、唱片）、行首的播放三角
fn list_icons(sp: &mut Sprite) {
    cell(sp, 15, 0, 0, Shape::of(vec![rect(6.3, 2.5, 8.7, 12.5, 0.6), rect(2.5, 6.3, 12.5, 8.7, 0.6)]));
    let sort = vec![
        tri((1.8, 5.8), (7.0, 5.8), (4.4, 2.0), 0.3),
        rect(3.4, 5.0, 5.4, 12.5, 0.3),
        tri((8.0, 9.2), (13.2, 9.2), (10.6, 13.0), 0.3),
        rect(9.6, 2.5, 11.6, 10.0, 0.3),
    ];
    cell(sp, 15, 2, 0, Shape::of(sort));
    cell(sp, 15, 3, 0, Shape::of(vec![line(5.0, 10.0, 11.0, 4.0, 3.2), tri((2.2, 12.8), (3.0, 8.8), (6.2, 12.0), 0.2)]));
    // 音符：符头、符干、符尾（「下载歌词」窗口的标题框）
    cell(sp, 15, 0, 1, Shape::of(vec![S::Circle(5.2, 11.0, 2.6), rect(6.6, 2.0, 8.2, 11.2, 0.3), line(7.6, 2.6, 11.6, 6.4, 1.8)]));
    cell(sp, 15, 1, 1, Shape::of(vec![S::Circle(9.6, 5.4, 3.4), line(7.6, 7.4, 2.8, 12.2, 2.2)]));
    cell(sp, 15, 2, 1, Shape::of(vec![S::Circle(7.5, 7.5, 6.0)]).minus(vec![S::Circle(7.5, 7.5, 1.6), S::Ring(7.5, 7.5, 4.0, 0.9)]));
    cell(sp, 15, 10, 1, Shape::of(vec![S::Arc(7.5, 8.5, 5.0, 180.0, 0.0, 1.8), rect(1.6, 8.5, 4.4, 13.0, 0.8), rect(10.6, 8.5, 13.4, 13.0, 0.8)]));
    cell(sp, 15, 1, 2, Shape::of(vec![tri((5.0, 3.5), (5.0, 11.5), (11.0, 7.5), 0.4)]));
}

/// 像素数字（每格 6×8）：0-9 : . （空格） + -。数字是原版那样的方块笔画（2 像素粗，像数码管），照原版截图量的
#[rustfmt::skip]
const DIGITS: [[&str; 8]; 15] = [
    ["######", "######", "##..##", "##..##", "##..##", "##..##", "######", "######"],
    ["....##", "....##", "....##", "....##", "....##", "....##", "....##", "....##"],
    ["######", "######", "....##", "######", "######", "##....", "######", "######"],
    ["######", "######", "....##", "######", "######", "....##", "######", "######"],
    ["##..##", "##..##", "##..##", "######", "######", "....##", "....##", "....##"],
    ["######", "######", "##....", "######", "######", "....##", "######", "######"],
    ["##....", "##....", "##....", "######", "######", "##..##", "######", "######"],
    ["######", "######", "....##", "....##", "....##", "....##", "....##", "....##"],
    ["######", "######", "##..##", "######", "######", "##..##", "######", "######"],
    ["######", "######", "##..##", "######", "######", "....##", "....##", "....##"],
    ["......", "..##..", "..##..", "......", "......", "..##..", "..##..", "......"],
    [".....", ".....", ".....", ".....", ".....", ".....", ".##..", ".##.."],
    [".....", ".....", ".....", ".....", ".....", ".....", ".....", "....."],
    [".....", ".....", "..#..", "..#..", "#####", "..#..", "..#..", "....."],
    [".....", ".....", ".....", ".....", "####.", ".....", ".....", "....."],
];

fn digits() -> Sprite {
    let mut sp = Sprite::new(96, 8);
    for (i, g) in DIGITS.iter().enumerate() {
        for (y, row) in g.iter().enumerate() {
            for (x, ch) in row.bytes().enumerate() {
                if ch == b'#' {
                    sp.px[y * 96 + i * 6 + x] = [255; 4];
                }
            }
        }
    }
    sp
}
/// 白色图形改成自上而下的灰度渐变（top → bottom，在 y0..y1 之间）
fn gradient(sp: &mut Sprite, y0: f32, y1: f32, top: f32, bottom: f32) {
    for y in 0..sp.h {
        let t = ((y as f32 + 0.5 - y0) / (y1 - y0)).clamp(0.0, 1.0);
        let g = (top + (bottom - top) * t) / 255.0;
        for x in 0..sp.w {
            let p = &mut sp.px[(y * sp.w + x) as usize];
            for c in &mut p[..3] {
                *c = (*c as f32 * g).round() as u8;
            }
        }
    }
}

/// 「airplay」字样：圆润的几何字形（单层的 a），基线在第 54 行，灰色渐变
fn logo() -> Sprite {
    let (sw, base, xh) = (4.6f32, 54.0f32, 38.0f32);
    let (r, cy) = (5.7f32, (base + xh) / 2.0);
    let stem = |x: f32, y0: f32, y1: f32| rect(x - sw / 2.0, y0, x + sw / 2.0, y1, sw / 2.0);
    let glyphs = vec![
        // a：圆 + 右侧竖笔
        S::Ring(9.0, cy, r, sw),
        stem(15.4, xh, base),
        // i：竖笔 + 点
        stem(26.0, xh, base),
        S::Circle(26.0, 31.2, 2.9),
        // r：竖笔 + 右上的弯
        stem(35.2, xh, base),
        S::Arc(41.6, 44.4, 6.4, 180.0, 300.0, sw),
        // p：竖笔（下伸）+ 圆
        stem(54.4, xh, 64.0),
        S::Ring(61.2, cy, r, sw),
        // l：竖笔（上伸）
        stem(75.6, 26.0, base),
        // a
        S::Ring(87.4, cy, r, sw),
        stem(93.8, xh, base),
        // y：左边 U 形、右边竖笔下伸后向左弯回
        stem(103.0, xh, 47.5),
        S::Arc(109.2, 47.5, 6.2, 0.0, 180.0, sw),
        stem(115.4, xh, 56.0),
        S::Arc(109.2, 56.0, 6.2, 0.0, 120.0, sw),
    ];
    let mut sp = Sprite::new(LOGO_W, LOGO_H);
    draw(&mut sp, 0, 0, LOGO_W, LOGO_H, &Shape::of(glyphs), false);
    gradient(&mut sp, xh - 12.0, 64.0, 210.0, 128.0);
    sp
}

/// 字样上方的弧线：两头细、渐隐（白色，按焦点颜色着色）
fn logo_arc() -> Sprite {
    let mut sp = Sprite::new(LOGO_W, LOGO_H);
    let (cx, cy, r, a0, a1) = (69.2f32, 57.97f32, 55.28f32, 216.4f32, 320.6f32);
    for y in 0..LOGO_H {
        for x in 0..LOGO_W {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let ang = (py - cy).atan2(px - cx).to_degrees().rem_euclid(360.0);
            let t = (ang - a0) / (a1 - a0);
            if !(0.0..=1.0).contains(&t) {
                continue;
            }
            // 中间约 2.2 像素宽，两头收细；左边淡、右边亮
            let taper = (t.min(1.0 - t) * 6.0).min(1.0);
            let w = 0.5 + 1.7 * taper;
            let cov = (w / 2.0 + 0.5 - ((px - cx).hypot(py - cy) - r).abs()).clamp(0.0, 1.0);
            let a = cov * taper * (0.55 + 0.45 * t);
            if a > 0.0 {
                sp.px[(y * LOGO_W + x) as usize] = [255, 255, 255, (a * 255.0).round() as u8];
            }
        }
    }
    sp
}

/// 没有封面时的默认图案：粗笔画的单层 a（与标志的字形一致）
fn cover() -> Sprite {
    let mut sp = Sprite::new(128, 128);
    // 圆环 + 右侧比圆环更靠下的竖笔（两者之间在右下角留出一个缺口）
    let shape = Shape::of(vec![S::Ring(54.0, 62.0, 31.0, 22.0), rect(82.0, 30.0, 106.0, 114.0, 12.0)]);
    draw(&mut sp, 0, 0, 128, 128, &shape, false);
    sp
}