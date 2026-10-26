//! 界面上的图标、像素数字、标志：全部在程序里自己画（`art.rs`，按原版的样子设计，不使用原版的图片）。
//! 图集与原版的排列相同（格子大小、位置），绘制时再按颜色着色：白色图形 + 外面一圈淡淡的暗边。

mod art;
pub(crate) mod sdf;

use std::sync::OnceLock;

#[derive(Clone)]
pub struct Sprite {
    pub w: u32,
    pub h: u32,
    /// 非预乘 RGBA
    pub px: Vec<[u8; 4]>,
}

impl Sprite {
    pub fn new(w: u32, h: u32) -> Sprite {
        Sprite { w, h, px: vec![[0; 4]; (w * h) as usize] }
    }

    #[inline]
    pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
        self.px[(y * self.w + x) as usize]
    }
}

/// 图集中的一个格子
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub sheet: Sheet,
    pub col: u32,
    pub row: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sheet {
    Player,
    Mini,
    Gui10,
    Gui15,
    Icon15,
}

impl Sheet {
    /// 格子尺寸
    pub fn cell(self) -> u32 {
        match self {
            Sheet::Player => 32,
            Sheet::Mini => 16,
            Sheet::Gui10 => 10,
            Sheet::Gui15 | Sheet::Icon15 => 15,
        }
    }
}

pub struct Skin {
    pub player: Sprite,
    pub mini: Sprite,
    pub gui10: Sprite,
    pub gui15: Sprite,
    pub icon15: Sprite,
    /// 像素数字 6×8：0-9 : . （空） + -
    pub num6x8: Sprite,
    /// 「airplay」字样（灰色渐变）与上方的弧线（白色，按焦点颜色着色）
    pub logo: Sprite,
    pub logo_arc: Sprite,
    /// 没有封面时的默认图案（白色，绘制时变暗）
    pub cover: Sprite,
    /// 画这套图时的缩放（1.0 = 逻辑像素；布局用的尺寸都按 1.0 的 skin() 算）
    pub scale: f32,
}

/// 程序自己画的整张图
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Art {
    Logo,
    LogoArc,
    Cover,
}

/// 逻辑像素的图（布局量尺寸用它）
pub fn skin() -> &'static Skin {
    static S: OnceLock<Skin> = OnceLock::new();
    S.get_or_init(|| art::build_at(1.0))
}

/// 按缩放画好的图（每种缩放画一次，留着）
pub fn skin_at(scale: f32) -> &'static Skin {
    if scale == 1.0 {
        return skin();
    }
    static M: std::sync::Mutex<Vec<(u32, &'static Skin)>> = std::sync::Mutex::new(Vec::new());
    let key = (scale * 1000.0).round() as u32;
    let mut m = M.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, s)) = m.iter().find(|(k, _)| *k == key) {
        return s;
    }
    let s: &'static Skin = Box::leak(Box::new(art::build_at(scale)));
    m.push((key, s));
    s
}

impl Skin {
    pub fn sheet(&self, s: Sheet) -> &Sprite {
        match s {
            Sheet::Player => &self.player,
            Sheet::Mini => &self.mini,
            Sheet::Gui10 => &self.gui10,
            Sheet::Gui15 => &self.gui15,
            Sheet::Icon15 => &self.icon15,
        }
    }

    pub fn art(&self, a: Art) -> &Sprite {
        match a {
            Art::Logo => &self.logo,
            Art::LogoArc => &self.logo_arc,
            Art::Cover => &self.cover,
        }
    }

    /// 格子在这套图集里的位置（实际像素；格子的边按缩放各自取整）
    pub fn cell_rect(&self, c: Cell) -> crate::gui::R {
        let n = c.sheet.cell() as f32;
        let e = |i: u32| art::px(i as f32 * n, self.scale) as i32;
        let (x0, y0) = (e(c.col), e(c.row));
        crate::gui::R::new(x0, y0, e(c.col + 1) - x0, e(c.row + 1) - y0)
    }
}

/// 图标在图集中的位置
pub mod icons {
    use super::{Cell, Sheet};
    const fn c(sheet: Sheet, col: u32, row: u32) -> Cell {
        Cell { sheet, col, row }
    }
    // 32×32：大号播放控制
    pub const BIG_PREV: Cell = c(Sheet::Player, 0, 0);
    pub const BIG_PLAY: Cell = c(Sheet::Player, 1, 0);
    pub const BIG_PAUSE: Cell = c(Sheet::Player, 2, 0);
    pub const BIG_NEXT: Cell = c(Sheet::Player, 3, 0);
    // 16×16
    pub const PREV: Cell = c(Sheet::Mini, 0, 0);
    pub const PLAY: Cell = c(Sheet::Mini, 1, 0);
    pub const PAUSE: Cell = c(Sheet::Mini, 2, 0);
    pub const NEXT: Cell = c(Sheet::Mini, 3, 0);
    pub const LYRIC_PIC: Cell = c(Sheet::Mini, 1, 1);
    pub const LIST: Cell = c(Sheet::Mini, 2, 1);
    pub const EQ: Cell = c(Sheet::Mini, 3, 1);
    pub const MODE_SEQ: Cell = c(Sheet::Mini, 0, 2);
    pub const MODE_SHUFFLE: Cell = c(Sheet::Mini, 1, 2);
    pub const MODE_REPEAT: Cell = c(Sheet::Mini, 2, 2);
    pub const MODE_ONE: Cell = c(Sheet::Mini, 3, 2);
    // 10×10：迷你条窗口按钮
    pub const S_RESTORE: Cell = c(Sheet::Gui10, 1, 0);
    pub const S_CLOSE: Cell = c(Sheet::Gui10, 2, 0);
    // 15×15：窗口按钮、箭头、勾选
    pub const W_MIN: Cell = c(Sheet::Gui15, 0, 0);
    pub const W_CLOSE: Cell = c(Sheet::Gui15, 2, 0);
    pub const W_PIN: Cell = c(Sheet::Gui15, 4, 0);
    pub const W_PINNED: Cell = c(Sheet::Gui15, 3, 0);
    pub const W_MINIBAR: Cell = c(Sheet::Gui15, 5, 0);
    pub const TRI_RIGHT: Cell = c(Sheet::Gui15, 2, 1);
    pub const TRI_DOWN: Cell = c(Sheet::Gui15, 5, 1);
    pub const FIND: Cell = c(Sheet::Gui15, 1, 2);
    pub const CHECK: Cell = c(Sheet::Gui15, 2, 2);
    pub const CROSS: Cell = c(Sheet::Gui15, 1, 3);
    /// 音乐列表搜索框里有字时的「清除」
    pub const CLEAR: Cell = c(Sheet::Gui15, 2, 3);
    // 15×15：列表工具栏与分栏图标
    pub const T_ADD: Cell = c(Sheet::Icon15, 0, 0);
    pub const T_SORT: Cell = c(Sheet::Icon15, 2, 0);
    pub const T_EDIT: Cell = c(Sheet::Icon15, 3, 0);
    pub const NOTE: Cell = c(Sheet::Icon15, 0, 1);
    pub const MIC: Cell = c(Sheet::Icon15, 1, 1);
    pub const DISC: Cell = c(Sheet::Icon15, 2, 1);
    pub const HEADPHONE: Cell = c(Sheet::Icon15, 10, 1);
    pub const ROW_PLAY: Cell = c(Sheet::Icon15, 1, 2);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个用到的图标都画出来了（格子里有不透明的白色像素）
    #[test]
    fn every_icon_is_drawn() {
        use icons::*;
        let all = [
            BIG_PREV, BIG_PLAY, BIG_PAUSE, BIG_NEXT, PREV, PLAY, PAUSE, NEXT, LYRIC_PIC, LIST, EQ, MODE_SEQ, MODE_SHUFFLE, MODE_REPEAT, MODE_ONE,
            S_RESTORE, S_CLOSE, W_MIN, W_CLOSE, W_PIN, W_PINNED, W_MINIBAR, TRI_RIGHT, TRI_DOWN, FIND, CHECK, CROSS, CLEAR, T_ADD, T_SORT, T_EDIT, NOTE,
            MIC, DISC, HEADPHONE, ROW_PLAY,
        ];
        let s = skin();
        for c in all {
            let (sp, n) = (s.sheet(c.sheet), c.sheet.cell());
            let solid = (0..n * n).filter(|i| {
                let p = sp.at(c.col * n + i % n, c.row * n + i / n);
                p[3] > 200 && p[0] > 200
            });
            assert!(solid.count() >= 4, "{c:?} 是空的");
        }
        // 像素数字：0-9 都有笔画，第 12 格（空格）是空的
        for i in 0..10 {
            assert!((0..48).any(|k| s.num6x8.at(i * 6 + k % 6, k / 6)[3] > 0), "数字 {i}");
        }
        assert!((0..48).all(|k| s.num6x8.at(12 * 6 + k % 6, k / 6)[3] == 0));
        assert!(s.logo.px.iter().any(|p| p[3] > 200) && s.logo_arc.px.iter().any(|p| p[3] > 100) && s.cover.px.iter().any(|p| p[3] > 200));
    }

    /// 开发用：把所有图集放大存成 PNG 查看（`AP3_SKIN_DUMP=<目录> cargo test --lib dump_sheets -- --ignored`）
    #[test]
    #[ignore]
    fn dump_sheets() {
        let Some(dir) = std::env::var_os("AP3_SKIN_DUMP") else { return };
        let dir = std::path::PathBuf::from(dir);
        let s = skin();
        let all: [(&str, &Sprite, u32); 9] = [
            ("player", &s.player, 32), ("mini", &s.mini, 16), ("gui10", &s.gui10, 10), ("gui15", &s.gui15, 15), ("icon15", &s.icon15, 15),
            ("num6x8", &s.num6x8, 6), ("logo", &s.logo, 0), ("logo_arc", &s.logo_arc, 0), ("cover", &s.cover, 0),
        ];
        for (name, sp, cell) in all {
            let k = if sp.w > 100 { 3 } else { 8 };
            let mut img = image::RgbaImage::new(sp.w * k, sp.h * k);
            for (x, y, p) in img.enumerate_pixels_mut() {
                let (u, v) = (x / k, y / k);
                let q = sp.at(u, v);
                let a = q[3] as u32;
                let bg = if cell > 0 && (u % cell == 0 || v % cell == 0) && (x % k == 0 || y % k == 0) { [255, 0, 255] } else { [40, 44, 52] };
                let mix = |c: u8, b: u8| ((c as u32 * a + b as u32 * (255 - a)) / 255) as u8;
                *p = image::Rgba([mix(q[0], bg[0]), mix(q[1], bg[1]), mix(q[2], bg[2]), 255]);
            }
            img.save(dir.join(format!("{name}.png"))).unwrap();
        }
    }
}
