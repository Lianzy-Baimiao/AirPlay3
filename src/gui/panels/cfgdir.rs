//! 「设置用户配置数据及歌词图片缓存保存位置」窗口（照原版逐像素量出来的）：360 × 180，四周宽阴影，
//! 三选一的位置、显示路径的框（右边「+」选文件夹）、确认 / 取消。

use super::lyricdl::{SHADOW, button_ex, shadow};
use crate::gui::canvas::{Canvas, R, Rgba};
use crate::gui::text::{self, Font, Hint};
use crate::gui::theme;

pub const W: i32 = 360 + 2 * SHADOW;
pub const H: i32 = 180 + 2 * SHADOW;
const FONT: Font = Font::px(12.0, false);

const fn b(x: i32, y: i32, w: i32, h: i32) -> R {
    R::new(SHADOW + x, SHADOW + y, w, h)
}

const BODY: R = b(0, 0, 360, 180);
const TITLE: &str = "设置用户配置数据及歌词图片缓存保存位置";
const LABELS: [&str; 3] = ["操作系统默认位置", "执行程序所在位置下的 AIRPLAY_CONFIG 目录", "其它位置 (用户自定义)"];
/// 路径框（鼠标在框里任何地方，框和底色都亮一点）
const PATH: R = b(15, 105, 330, 21);
/// 路径文字的感应区和显示范围（到「+」左边为止）
const PATH_TEXT: R = b(18, 106, 309, 19);
const PATH_CLIP: R = b(17, 106, 311, 19);
/// 「+」的感应区，加号画在它正中
const PLUS: R = b(327, 108, 15, 15);
const BTN_OK: R = b(190, 149, 80, 23);
const BTN_CANCEL: R = b(270, 149, 80, 23);
/// 没有路径时框里的灰字
const PLACEHOLDER: &str = "配置文件存放位置";
/// 按钮：(外圈, 里圈, 底色, 文字) 的灰度
const BTN: (u8, u8, u8, u8) = (0x19, 0x75, 0x45, 0xC0);
const BTN_HOT: (u8, u8, u8, u8) = (0x18, 0x8F, 0x57, 0xFF);

/// 鼠标下的东西
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hot {
    Choice(usize),
    Path,
    Plus,
    Ok,
    Cancel,
}

pub struct CfgDirModel<'a> {
    /// 选中的是第几个位置
    pub choice: usize,
    /// 路径框里的路径（空：显示灰色的「配置文件存放位置」）
    pub path: &'a str,
    pub status: &'a str,
    pub hot: Option<Hot>,
    /// 鼠标在路径框（含「+」）上
    pub path_hot: bool,
    /// 有路径才显示「确认」
    pub can_ok: bool,
    /// 选文件夹的对话框开着：整个窗口变暗
    pub dimmed: bool,
}
/// 第 i 个位置的感应区：方块到文字末尾，高 19
fn choice_rect(i: usize) -> R {
    let tw = text::measure(LABELS[i], FONT).0 as i32;
    b(15, 40 + 20 * i as i32, 22 + tw, 19)
}

/// 鼠标在哪：(东西, 是否在路径框上)
pub fn hit(x: i32, y: i32, can_ok: bool) -> (Option<Hot>, bool) {
    let path_hot = PATH.contains(x, y);
    let h = if let Some(i) = (0..3).find(|&i| choice_rect(i).contains(x, y)) {
        Some(Hot::Choice(i))
    } else if PLUS.contains(x, y) {
        Some(Hot::Plus)
    } else if PATH_TEXT.contains(x, y) {
        Some(Hot::Path)
    } else if can_ok && R::new(BTN_OK.x + 1, BTN_OK.y + 1, BTN_OK.w - 2, BTN_OK.h - 1).contains(x, y) {
        Some(Hot::Ok)
    } else if R::new(BTN_CANCEL.x + 1, BTN_CANCEL.y + 1, BTN_CANCEL.w - 2, BTN_CANCEL.h - 1).contains(x, y) {
        Some(Hot::Cancel)
    } else {
        None
    };
    (h, path_hot)
}

pub fn paint(c: &mut Canvas, m: &CfgDirModel) {
    c.clear();
    shadow(c, BODY);
    c.fill(BODY, theme::g(75));
    c.fill(BODY.shrink(1), theme::g(32));
    let tw = text::measure(TITLE, FONT).0 as i32;
    c.text(TITLE, FONT, BODY.x + (BODY.w - tw) / 2 - 1, BODY.y + 7, Rgba::gray(0xD7), Hint::ClearType);
    for (i, label) in LABELS.iter().enumerate() {
        let hot = m.hot == Some(Hot::Choice(i));
        let (x, y) = (BODY.x + 16, BODY.y + 44 + 20 * i as i32);
        let fill = if hot { 0xC8 } else { 0x81 };
        c.fill(R::new(x, y, 13, 12), Rgba::gray(fill));
        if m.choice == i {
            tick(c, x, y, Rgba::gray(if hot { 0x22 } else { 0x21 }));
        }
        c.text(label, FONT, x + 19, y - 3, Rgba::gray(if hot { 0xFF } else { 0xE0 }), Hint::ClearType);
    }
    // 路径框：鼠标在框里时边和底色亮一点；路径太长时像输入框一样显示末尾
    c.fill(PATH, Rgba::gray(if m.path_hot { 0x63 } else { 0x51 }));
    c.fill(PATH.shrink(1), theme::g(if m.path_hot { 56 } else { 32 }));
    c.push_clip(PATH_CLIP);
    if m.path.is_empty() {
        if !m.dimmed {
            let pw = text::measure(PLACEHOLDER, FONT).0 as i32;
            c.text(PLACEHOLDER, FONT, PATH_TEXT.x + (PATH_TEXT.w - pw) / 2, PATH.y + 2, Rgba::gray(0x8F), Hint::ClearType);
        }
    } else {
        let pw = text::measure(m.path, FONT).0 as i32;
        let x = (PATH_CLIP.x + 1).min(PATH_CLIP.right() - pw);
        c.text(m.path, FONT, x, PATH.y + 3, Rgba::gray(if m.hot == Some(Hot::Path) { 0xFF } else { 0xE0 }), Hint::ClearType);
    }
    c.pop_clip();
    let arm = if m.hot == Some(Hot::Plus) { 0xFF } else if m.path_hot { 0xB5 } else { 0xAD };
    plus(c, PLUS.x + 2, PLUS.y + 2, Rgba::gray(arm));
    if !m.status.is_empty() {
        c.push_clip(R::new(BODY.x + 16, BTN_OK.y, BTN_OK.x - BODY.x - 22, BTN_OK.h));
        c.text(m.status, FONT, BODY.x + 16, BTN_OK.y + 4, theme::g(150), Hint::ClearType);
        c.pop_clip();
    }
    if m.can_ok {
        button_ex(c, BTN_OK, "确认", if m.hot == Some(Hot::Ok) { BTN_HOT } else { BTN });
    }
    button_ex(c, BTN_CANCEL, "取消", if m.hot == Some(Hot::Cancel) { BTN_HOT } else { BTN });
    // 选文件夹时原版整个窗口画成不可用：所有颜色向底色靠拢（剩约 24%）
    if m.dimmed {
        let g = theme::g(32);
        c.fill(BODY, Rgba(g.0, g.1, g.2, 195));
    }
}

/// 勾（13 × 12 的方块里，从原版量出的覆盖率，96 = 全覆盖）
fn tick(c: &mut Canvas, x: i32, y: i32, col: Rgba) {
    const T: [[u8; 13]; 8] = [
        [0, 0, 0, 0, 0, 0, 0, 0, 1, 51, 18, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 57, 96, 87, 31, 0],
        [0, 3, 6, 16, 3, 0, 6, 56, 96, 96, 96, 43, 0],
        [0, 31, 87, 93, 29, 3, 71, 96, 96, 86, 29, 0, 0],
        [0, 5, 81, 96, 58, 63, 96, 96, 75, 17, 0, 0, 0],
        [0, 0, 34, 96, 93, 96, 91, 62, 3, 0, 0, 0, 0],
        [0, 0, 0, 67, 96, 83, 24, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 18, 64, 12, 0, 0, 0, 0, 0, 0, 0],
    ];
    for (dy, row) in T.iter().enumerate() {
        for (dx, &v) in row.iter().enumerate() {
            if v > 0 {
                c.blend(x + dx as i32, y + 2 + dy as i32, col, v as f32 / 96.0);
            }
        }
    }
}

/// 「+」：两条 9 × 3 的粗线，外面一圈淡淡的黑晕（透明度按 3 × 3 邻域里有几格是加号）
fn plus(c: &mut Canvas, x: i32, y: i32, col: Rgba) {
    let on = |dx: i32, dy: i32| (1..=9).contains(&dx) && (4..=6).contains(&dy) || (4..=6).contains(&dx) && (1..=9).contains(&dy);
    for dy in 0..11 {
        for dx in 0..11 {
            if on(dx, dy) {
                c.fill(R::new(x + dx, y + dy, 1, 1), col);
                continue;
            }
            let n = (-1..=1).flat_map(|j| (-1..=1).map(move |i| (i, j))).filter(|&(i, j)| on(dx + i, dy + j)).count();
            let a = match n {
                0 => continue,
                1 => 32,
                2 => 72,
                3 => 88,
                _ => 128,
            };
            c.fill(R::new(x + dx, y + dy, 1, 1), Rgba(0, 0, 0, a));
        }
    }
}
