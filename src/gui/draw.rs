//! 画布上的图标、图片、文字、像素数字。

use super::canvas::{Canvas, R, Rgba};
use super::text::{self, Font, Hint, Mask};
use crate::skin::{Cell, Sprite, skin};

impl Canvas {
    /// 画图集中的一个格子，按 tint 着色（图集为白色/灰色 + alpha）
    pub fn icon(&mut self, cell: Cell, x: i32, y: i32, tint: Rgba) {
        let s = skin();
        let sheet = s.sheet(cell.sheet);
        let n = cell.sheet.cell();
        self.sprite_part(sheet, (cell.col * n) as i32, (cell.row * n) as i32, n as i32, n as i32, x, y, tint);
    }

    /// 画精灵图的一部分（不缩放）
    #[allow(clippy::too_many_arguments)]
    pub fn sprite_part(&mut self, sp: &Sprite, sx: i32, sy: i32, w: i32, h: i32, x: i32, y: i32, tint: Rgba) {
        let clip = self.clip_rect().inter(&R::new(x, y, w, h));
        for py in clip.y..clip.bottom() {
            for px in clip.x..clip.right() {
                let (u, v) = (sx + px - x, sy + py - y);
                if u < 0 || v < 0 || u >= sp.w as i32 || v >= sp.h as i32 {
                    continue;
                }
                let p = sp.at(u as u32, v as u32);
                if p[3] == 0 {
                    continue;
                }
                let m = |a: u8, b: u8| ((a as u16 * b as u16 + 127) / 255) as u8;
                let c = Rgba(m(p[0], tint.0), m(p[1], tint.1), m(p[2], tint.2), tint.3);
                self.blend(px, py, c, p[3] as f32 / 255.0);
            }
        }
    }

    /// 画整张精灵图
    pub fn sprite(&mut self, sp: &Sprite, x: i32, y: i32, tint: Rgba) {
        self.sprite_part(sp, 0, 0, sp.w as i32, sp.h as i32, x, y, tint);
    }

    /// 倒影：精灵图第 `floor` 行以上的部分上下翻转画在 (x, y + floor) 以下，`len` 行内从 `alpha` 渐隐到 0
    #[allow(clippy::too_many_arguments)]
    pub fn sprite_reflect(&mut self, sp: &Sprite, x: i32, y: i32, floor: i32, len: i32, alpha: f32, tint: Rgba) {
        for dy in 0..len.min(floor) {
            let a = alpha * (1.0 - dy as f32 / len as f32);
            let v = (floor - 1 - dy) as u32;
            for u in 0..sp.w {
                let p = sp.at(u, v);
                if p[3] == 0 {
                    continue;
                }
                let m = |a: u8, b: u8| ((a as u16 * b as u16 + 127) / 255) as u8;
                let (px, py) = (x + u as i32, y + floor + dy);
                if self.clip_rect().contains(px, py) {
                    self.blend(px, py, Rgba(m(p[0], tint.0), m(p[1], tint.1), m(p[2], tint.2), tint.3), p[3] as f32 / 255.0 * a);
                }
            }
        }
    }

    /// 精灵图缩放到 dst（双线性），按 tint 着色
    pub fn sprite_scaled(&mut self, sp: &Sprite, dst: R, tint: Rgba) {
        if dst.w <= 0 || dst.h <= 0 {
            return;
        }
        let clip = self.clip_rect().inter(&dst);
        let (kx, ky) = (sp.w as f32 / dst.w as f32, sp.h as f32 / dst.h as f32);
        let get = |x: i32, y: i32| sp.at(x.clamp(0, sp.w as i32 - 1) as u32, y.clamp(0, sp.h as i32 - 1) as u32);
        for py in clip.y..clip.bottom() {
            let fy = ((py - dst.y) as f32 + 0.5) * ky - 0.5;
            let (y0, ty) = (fy.floor() as i32, fy - fy.floor());
            for px in clip.x..clip.right() {
                let fx = ((px - dst.x) as f32 + 0.5) * kx - 0.5;
                let (x0, tx) = (fx.floor() as i32, fx - fx.floor());
                let (a, b, c, d) = (get(x0, y0), get(x0 + 1, y0), get(x0, y0 + 1), get(x0 + 1, y0 + 1));
                let l = |k: usize| (a[k] as f32 * (1.0 - tx) + b[k] as f32 * tx) * (1.0 - ty) + (c[k] as f32 * (1.0 - tx) + d[k] as f32 * tx) * ty;
                let (cov, v) = (l(3) / 255.0, l(0) / 255.0);
                if cov > 0.0 {
                    let m = |t: u8| (t as f32 * v).round() as u8;
                    self.blend(px, py, Rgba(m(tint.0), m(tint.1), m(tint.2), tint.3), cov);
                }
            }
        }
    }

    /// 画 RGBA 图片（非预乘）到 dst，双线性缩放
    pub fn image(&mut self, rgba: &[u8], iw: u32, ih: u32, dst: R, opacity: f32) {
        if iw == 0 || ih == 0 || dst.w <= 0 || dst.h <= 0 {
            return;
        }
        let clip = self.clip_rect().inter(&dst);
        let (sx, sy) = (iw as f32 / dst.w as f32, ih as f32 / dst.h as f32);
        let get = |x: i32, y: i32| {
            let (x, y) = (x.clamp(0, iw as i32 - 1) as usize, y.clamp(0, ih as i32 - 1) as usize);
            let i = (y * iw as usize + x) * 4;
            [rgba[i] as f32, rgba[i + 1] as f32, rgba[i + 2] as f32, rgba[i + 3] as f32]
        };
        for py in clip.y..clip.bottom() {
            let fy = ((py - dst.y) as f32 + 0.5) * sy - 0.5;
            let (y0, ty) = (fy.floor() as i32, fy - fy.floor());
            for px in clip.x..clip.right() {
                let fx = ((px - dst.x) as f32 + 0.5) * sx - 0.5;
                let (x0, tx) = (fx.floor() as i32, fx - fx.floor());
                let (a, b, c, d) = (get(x0, y0), get(x0 + 1, y0), get(x0, y0 + 1), get(x0 + 1, y0 + 1));
                let l = |k: usize| (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty;
                let col = Rgba(l(0) as u8, l(1) as u8, l(2) as u8, 255);
                self.blend(px, py, col, l(3) / 255.0 * opacity);
            }
        }
    }

    /// 合成文字遮罩：左上角对齐 (x, y)
    pub fn mask(&mut self, m: &Mask, x: i32, y: i32, c: Rgba) {
        self.mask_grad(m, x, y, c, c);
    }

    /// 合成文字遮罩，颜色从上到下由 `top` 渐变到 `bottom`（桌面歌词的配色）
    pub fn mask_grad(&mut self, m: &Mask, x: i32, y: i32, top: Rgba, bottom: Rgba) {
        let clip = self.clip_rect().inter(&R::new(x - 1, y, m.w as i32, m.h as i32));
        for py in clip.y..clip.bottom() {
            let c = if top == bottom || m.h < 2 { top } else { top.mix(bottom, (py - y) as f32 / (m.h - 1) as f32) };
            let a = c.3 as f32 / 255.0;
            for px in clip.x..clip.right() {
                let cov = m.cov[((py - y) * m.w as i32 + (px - x + 1)) as usize];
                if cov == [0; 3] {
                    continue;
                }
                let i = (py * self.w + px) as usize;
                let d = &mut self.px[i];
                // 子像素：每个通道各自的覆盖率（灰度时三者相同）
                let ch = [c.2, c.1, c.0];
                let cv = [cov[2], cov[1], cov[0]];
                for k in 0..3 {
                    let t = cv[k] as f32 / 255.0 * a;
                    d[k] = (ch[k] as f32 * t + d[k] as f32 * (1.0 - t) + 0.5) as u8;
                }
                let t = cov.iter().copied().max().unwrap_or(0) as f32 / 255.0 * a;
                d[3] = (255.0 * t + d[3] as f32 * (1.0 - t) + 0.5) as u8;
            }
        }
    }

    /// 画文字，返回宽度
    pub fn text(&mut self, s: &str, font: Font, x: i32, y: i32, c: Rgba, hint: Hint) -> i32 {
        if s.is_empty() {
            return 0;
        }
        let m = text::cached(s, font, hint);
        self.mask(&m, x, y, c);
        text::measure(s, font).0.round() as i32
    }

    /// 带柔和暗色光晕的文字（原版播放条标题）
    pub fn glow_text(&mut self, s: &str, font: Font, x: i32, y: i32, c: Rgba, glow: Rgba, radius: i32) -> i32 {
        if s.is_empty() {
            return 0;
        }
        let m = text::cached(s, font, Hint::Gray);
        let blurred = blur(&m, radius);
        self.mask(&blurred, x - radius, y - radius, glow);
        self.mask(&m, x, y, c);
        text::measure(s, font).0.round() as i32
    }

    /// 原版像素数字字体 6×8：0-9 : . + - 以及空格
    pub fn num6x8(&mut self, s: &str, x: i32, y: i32, c: Rgba) -> i32 {
        let sp = &skin().num6x8;
        let mut cx = x;
        for ch in s.chars() {
            let idx = match ch {
                '0'..='9' => Some(ch as i32 - '0' as i32),
                ':' => Some(10),
                '.' => Some(11),
                '+' => Some(13),
                '-' => Some(14),
                _ => None,
            };
            if let Some(i) = idx {
                self.sprite_part(sp, i * 6, 0, 6, 8, cx, y, c);
            }
            cx += 7;
        }
        cx - x
    }
}

/// 描边遮罩：把字形覆盖率向四周扩展 r 像素（桌面歌词的黑色描边）。结果四周各扩展 r。
pub fn outline(m: &Mask, r: i32) -> Mask {
    let (w, h) = (m.w as i32 + 2 * r, m.h as i32 + 2 * r);
    let src = |x: i32, y: i32| -> u8 {
        if x < 0 || y < 0 || x >= m.w as i32 || y >= m.h as i32 { 0 } else { m.cov[(y * m.w as i32 + x) as usize][1] }
    };
    let mut cov = vec![[0u8; 3]; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut v = 0u8;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx * dx + dy * dy <= r * r + 1 {
                        v = v.max(src(x - r + dx, y - r + dy));
                    }
                }
            }
            cov[(y * w + x) as usize] = [v, v, v];
        }
    }
    Mask { w: w as u32, h: h as u32, cov }
}

/// 遮罩模糊（两次盒式模糊近似高斯），四周扩展 radius
fn blur(m: &Mask, radius: i32) -> Mask {
    let r = radius.max(1);
    let (w, h) = (m.w as i32 + 2 * r, m.h as i32 + 2 * r);
    let mut a = vec![0f32; (w * h) as usize];
    for y in 0..m.h as i32 {
        for x in 0..m.w as i32 {
            let c = m.cov[(y * m.w as i32 + x) as usize];
            a[((y + r) * w + x + r) as usize] = c.iter().copied().max().unwrap_or(0) as f32;
        }
    }
    for _ in 0..2 {
        let mut b = vec![0f32; a.len()];
        for y in 0..h {
            for x in 0..w {
                let mut s = 0.0;
                for k in -r..=r {
                    let xx = (x + k).clamp(0, w - 1);
                    s += a[(y * w + xx) as usize];
                }
                b[(y * w + x) as usize] = s / (2 * r + 1) as f32;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let mut s = 0.0;
                for k in -r..=r {
                    let yy = (y + k).clamp(0, h - 1);
                    s += b[(yy * w + x) as usize];
                }
                a[(y * w + x) as usize] = s / (2 * r + 1) as f32;
            }
        }
    }
    let cov = a.iter().map(|v| {
        let v = (v * 2.2).min(255.0) as u8;
        [v, v, v]
    });
    Mask { w: w as u32, h: h as u32, cov: cov.collect() }
}
