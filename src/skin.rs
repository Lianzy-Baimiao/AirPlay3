//! 原版皮肤图片：`RLE\xF1` 格式（来自原版 exe 的 RT_RCDATA 资源）。
//!
//! 头部：魔数 + u32 宽、高、版本；之后是按字节的操作码（高 2 位）：
//! 00 = n 个透明像素，01 = n 个 RGBA，10 = n 个不透明 RGB，11 = 1 个 RGBA 重复 n 次；n = (b & 0x3F) + 1。
//! 图片都是灰度 + alpha（白色图形、黑色阴影），绘制时再按主题颜色着色。

use std::sync::OnceLock;

#[derive(Clone)]
pub struct Sprite {
    pub w: u32,
    pub h: u32,
    /// 非预乘 RGBA
    pub px: Vec<[u8; 4]>,
}

impl Sprite {
    pub fn decode(data: &[u8]) -> Option<Sprite> {
        if data.len() < 16 || &data[..4] != b"RLE\xF1" {
            return None;
        }
        let rd = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        let (w, h) = (rd(4), rd(8));
        let total = (w as usize).checked_mul(h as usize)?;
        let mut px = Vec::with_capacity(total);
        let mut i = 16;
        while px.len() < total && i < data.len() {
            let b = data[i];
            i += 1;
            let n = (b & 0x3F) as usize + 1;
            match b >> 6 {
                0 => px.extend(std::iter::repeat_n([0u8; 4], n)),
                1 => {
                    for _ in 0..n {
                        let c = data.get(i..i + 4)?;
                        px.push([c[0], c[1], c[2], c[3]]);
                        i += 4;
                    }
                }
                2 => {
                    for _ in 0..n {
                        let c = data.get(i..i + 3)?;
                        px.push([c[0], c[1], c[2], 255]);
                        i += 3;
                    }
                }
                _ => {
                    let c = data.get(i..i + 4)?;
                    px.extend(std::iter::repeat_n([c[0], c[1], c[2], c[3]], n));
                    i += 4;
                }
            }
        }
        px.resize(total, [0; 4]);
        Some(Sprite { w, h, px })
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
    Icon20,
    Icon30,
    Loading,
}

impl Sheet {
    /// 格子尺寸
    pub fn cell(self) -> u32 {
        match self {
            Sheet::Player => 32,
            Sheet::Mini => 16,
            Sheet::Gui10 => 10,
            Sheet::Gui15 | Sheet::Icon15 => 15,
            Sheet::Icon20 | Sheet::Loading => 20,
            Sheet::Icon30 => 30,
        }
    }
}
pub struct Skin {
    pub player: Sprite,
    pub mini: Sprite,
    pub gui10: Sprite,
    pub gui15: Sprite,
    pub icon15: Sprite,
    pub icon20: Sprite,
    pub icon30: Sprite,
    pub loading: Sprite,
    pub logo: Sprite,
    pub num4x5: Sprite,
    pub num6x8: Sprite,
    pub album: Sprite,
    pub artist: Sprite,
    pub cover: Sprite,
}

macro_rules! res {
    ($n:literal) => {
        Sprite::decode(include_bytes!(concat!("../assets/skin/", $n, ".rle"))).expect(concat!("skin ", $n))
    };
}

pub fn skin() -> &'static Skin {
    static S: OnceLock<Skin> = OnceLock::new();
    S.get_or_init(|| Skin {
        player: res!("PLAYER"),
        mini: res!("MINIPLAYER"),
        gui10: res!("GUI10"),
        gui15: res!("GUI15"),
        icon15: res!("ICON15"),
        icon20: res!("ICON20"),
        icon30: res!("ICON30"),
        loading: res!("LOADING"),
        logo: res!("LOGO"),
        num4x5: res!("NUMFONT4X5"),
        num6x8: res!("NUMFONT6X8"),
        album: res!("ALBUM"),
        artist: res!("ARTIST"),
        cover: res!("COVER"),
    })
}

impl Skin {
    pub fn sheet(&self, s: Sheet) -> &Sprite {
        match s {
            Sheet::Player => &self.player,
            Sheet::Mini => &self.mini,
            Sheet::Gui10 => &self.gui10,
            Sheet::Gui15 => &self.gui15,
            Sheet::Icon15 => &self.icon15,
            Sheet::Icon20 => &self.icon20,
            Sheet::Icon30 => &self.icon30,
            Sheet::Loading => &self.loading,
        }
    }
}

/// 常用图标在图集中的位置（按原版图集排列）
pub mod icons {
    use super::{Cell, Sheet};
    const fn c(sheet: Sheet, col: u32, row: u32) -> Cell {
        Cell { sheet, col, row }
    }
    // PLAYER 32×32：大号播放控制
    pub const BIG_PREV: Cell = c(Sheet::Player, 0, 0);
    pub const BIG_PLAY: Cell = c(Sheet::Player, 1, 0);
    pub const BIG_PAUSE: Cell = c(Sheet::Player, 2, 0);
    pub const BIG_NEXT: Cell = c(Sheet::Player, 3, 0);
    // MINIPLAYER 16×16
    pub const PREV: Cell = c(Sheet::Mini, 0, 0);
    pub const PLAY: Cell = c(Sheet::Mini, 1, 0);
    pub const PAUSE: Cell = c(Sheet::Mini, 2, 0);
    pub const NEXT: Cell = c(Sheet::Mini, 3, 0);
    pub const FULL: Cell = c(Sheet::Mini, 4, 0);
    pub const SEARCH: Cell = c(Sheet::Mini, 0, 1);
    pub const LYRIC_PIC: Cell = c(Sheet::Mini, 1, 1);
    pub const LIST: Cell = c(Sheet::Mini, 2, 1);
    pub const EQ: Cell = c(Sheet::Mini, 3, 1);
    pub const MODE_SEQ: Cell = c(Sheet::Mini, 0, 2);
    pub const MODE_SHUFFLE: Cell = c(Sheet::Mini, 1, 2);
    pub const MODE_REPEAT: Cell = c(Sheet::Mini, 2, 2);
    pub const MODE_ONE: Cell = c(Sheet::Mini, 3, 2);
    pub const SHARE: Cell = c(Sheet::Mini, 0, 3);
    pub const HEART: Cell = c(Sheet::Mini, 1, 3);
    pub const BAN: Cell = c(Sheet::Mini, 2, 3);
    // GUI10 10×10：迷你条窗口按钮
    pub const S_MIN: Cell = c(Sheet::Gui10, 0, 0);
    pub const S_RESTORE: Cell = c(Sheet::Gui10, 1, 0);
    pub const S_CLOSE: Cell = c(Sheet::Gui10, 2, 0);
    // GUI15 15×15：窗口按钮、箭头、勾选
    pub const W_MIN: Cell = c(Sheet::Gui15, 0, 0);
    pub const W_MAX: Cell = c(Sheet::Gui15, 1, 0);
    pub const W_CLOSE: Cell = c(Sheet::Gui15, 2, 0);
    pub const W_PIN: Cell = c(Sheet::Gui15, 4, 0);
    pub const W_PINNED: Cell = c(Sheet::Gui15, 3, 0);
    pub const W_MINIBAR: Cell = c(Sheet::Gui15, 5, 0);
    pub const ARROW_UP: Cell = c(Sheet::Gui15, 0, 1);
    pub const ARROW_DOWN: Cell = c(Sheet::Gui15, 1, 1);
    pub const TRI_RIGHT: Cell = c(Sheet::Gui15, 2, 1);
    pub const TRI_UP: Cell = c(Sheet::Gui15, 3, 1);
    pub const TRI_LEFT: Cell = c(Sheet::Gui15, 4, 1);
    pub const TRI_DOWN: Cell = c(Sheet::Gui15, 5, 1);
    pub const CLEAR: Cell = c(Sheet::Gui15, 0, 2);
    pub const FIND: Cell = c(Sheet::Gui15, 1, 2);
    pub const CHECK: Cell = c(Sheet::Gui15, 2, 2);
    pub const PLUS: Cell = c(Sheet::Gui15, 0, 3);
    pub const CROSS: Cell = c(Sheet::Gui15, 1, 3);
    // ICON15 15×15：列表工具栏与分栏图标
    pub const T_ADD: Cell = c(Sheet::Icon15, 0, 0);
    pub const T_DEL: Cell = c(Sheet::Icon15, 1, 0);
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

    #[test]
    fn decodes_all_sheets() {
        let s = skin();
        assert_eq!((s.player.w, s.player.h), (128, 64));
        assert_eq!((s.mini.w, s.mini.h), (80, 64));
        assert_eq!((s.gui15.w, s.gui15.h), (135, 105));
        assert_eq!((s.num6x8.w, s.num6x8.h), (96, 8));
        assert_eq!((s.logo.w, s.logo.h), (120, 66));
        // 播放按钮中心是白色不透明
        let p = s.player.at(32 + 16, 16);
        assert!(p[3] > 200 && p[0] > 200, "{p:?}");
    }
}
