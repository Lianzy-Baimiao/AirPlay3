//! 软件画布：预乘 BGRA（与 DIB / UpdateLayeredWindow 相同的格式）。
//! 原版只用 GDI+ 画文字和图片，其余（圆角边框、渐变、着色图标）都是自己逐像素画的，这里同样处理。

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const fn gray(v: u8) -> Rgba {
        Rgba(v, v, v, 255)
    }
    pub const fn alpha(self, a: u8) -> Rgba {
        Rgba(self.0, self.1, self.2, ((self.3 as u16 * a as u16) / 255) as u8)
    }
    pub fn mix(self, o: Rgba, t: f32) -> Rgba {
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgba(l(self.0, o.0), l(self.1, o.1), l(self.2, o.2), l(self.3, o.3))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct R {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl R {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> R {
        R { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
    pub fn inter(&self, o: &R) -> R {
        let (x, y) = (self.x.max(o.x), self.y.max(o.y));
        let (r, b) = (self.right().min(o.right()), self.bottom().min(o.bottom()));
        R::new(x, y, (r - x).max(0), (b - y).max(0))
    }
    pub fn shrink(&self, d: i32) -> R {
        R::new(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d)
    }
}

pub struct Canvas {
    /// 逻辑尺寸（布局、命中测试都用它；原版 1:1 的像素）
    pub w: i32,
    pub h: i32,
    /// 缩放比例（1.0 = 逻辑像素就是实际像素）
    pub scale: f32,
    /// 实际像素尺寸
    pub pw: i32,
    pub ph: i32,
    /// 预乘 BGRA，pw × ph
    pub px: Vec<[u8; 4]>,
    /// 裁剪栈：(逻辑坐标, 实际像素)
    clip: Vec<(R, R)>,
}

use super::scale::rnd;

impl Canvas {
    /// 按现在的界面缩放新建
    pub fn new(w: i32, h: i32) -> Canvas {
        Canvas::with_scale(w, h, super::scale::get())
    }

    pub fn with_scale(w: i32, h: i32, scale: f32) -> Canvas {
        let (pw, ph) = (rnd(w.max(0) as f32 * scale), rnd(h.max(0) as f32 * scale));
        Canvas { w, h, scale, pw, ph, px: vec![[0; 4]; (pw * ph) as usize], clip: Vec::new() }
    }

    /// 同样缩放的另一块画布
    pub fn like(&self, w: i32, h: i32) -> Canvas {
        Canvas::with_scale(w, h, self.scale)
    }

    /// 逻辑坐标 → 实际像素
    #[inline]
    pub fn tx(&self, v: i32) -> i32 {
        if self.scale == 1.0 { v } else { rnd(v as f32 * self.scale) }
    }

    /// 逻辑矩形 → 实际像素（四条边各自取整：相邻的矩形之间没有缝）
    pub fn tr(&self, r: R) -> R {
        let (x0, y0) = (self.tx(r.x), self.tx(r.y));
        R::new(x0, y0, self.tx(r.right()) - x0, self.tx(r.bottom()) - y0)
    }

    /// 实际像素 p 所在的逻辑像素（与 tr 的取整一致）
    #[inline]
    pub fn lp(&self, p: i32) -> i32 {
        if self.scale == 1.0 { p } else { ((p as f32 + 0.5) / self.scale).ceil() as i32 - 1 }
    }

    /// 实际像素中心的逻辑坐标（浮点）
    #[inline]
    pub fn lf(&self, px: i32, py: i32) -> (f32, f32) {
        ((px as f32 + 0.5) / self.scale, (py as f32 + 0.5) / self.scale)
    }

    /// 1 个逻辑像素宽的细线在实际像素里多宽（100% ~ 174% 是 1，175% ~ 274% 是 2 …）
    pub fn line_px(&self) -> f32 {
        (self.scale + 0.25).floor().max(1.0)
    }

    pub fn clear(&mut self) {
        self.px.fill([0; 4]);
    }

    /// 让完全透明的像素也能接收鼠标（alpha=1，肉眼不可见）：用于窗口边缘的调整区域
    pub fn hit_margin(&mut self, r: R) {
        let r = self.tr(r).inter(&R::new(0, 0, self.pw, self.ph));
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let p = &mut self.px[(y * self.pw + x) as usize];
                if p[3] == 0 {
                    *p = [0, 0, 0, 1];
                }
            }
        }
    }

    /// 现在的裁剪范围（逻辑坐标）
    pub fn clip_rect(&self) -> R {
        self.clip.last().map_or(R::new(0, 0, self.w, self.h), |c| c.0)
    }

    /// 现在的裁剪范围（实际像素）
    pub(crate) fn clip_px(&self) -> R {
        self.clip.last().map_or(R::new(0, 0, self.pw, self.ph), |c| c.1)
    }

    pub fn push_clip(&mut self, r: R) {
        let c = (self.clip_rect().inter(&r), self.clip_px().inter(&self.tr(r)));
        self.clip.push(c);
    }

    pub fn pop_clip(&mut self) {
        self.clip.pop();
    }

    /// 以覆盖率 cov(0..=1) 混合一个逻辑像素（source-over；不管裁剪，画布外的忽略）。
    /// 缩放时盖住这个逻辑像素对应的所有实际像素（像素画的图案放大成方块）
    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, c: Rgba, cov: f32) {
        if self.scale == 1.0 {
            if x >= 0 && y >= 0 && x < self.pw && y < self.ph {
                self.blend_px(x, y, c, cov);
            }
            return;
        }
        let r = self.tr(R::new(x, y, 1, 1)).inter(&R::new(0, 0, self.pw, self.ph));
        for py in r.y..r.bottom() {
            for px in r.x..r.right() {
                self.blend_px(px, py, c, cov);
            }
        }
    }

    /// 按实际像素一个个上色：f(像素中心的逻辑坐标) → (颜色, 覆盖率)；画逻辑矩形 r 盖住的实际像素。
    /// clip = false 时不管裁剪（与 blend 相同）
    pub fn shade(&mut self, r: R, clip: bool, mut f: impl FnMut(f32, f32) -> (Rgba, f32)) {
        let lim = if clip { self.clip_px() } else { R::new(0, 0, self.pw, self.ph) };
        let p = self.tr(r).inter(&lim);
        for py in p.y..p.bottom() {
            for px in p.x..p.right() {
                let (lx, ly) = self.lf(px, py);
                let (c, cov) = f(lx, ly);
                if cov > 0.0 {
                    self.blend_px(px, py, c, cov);
                }
            }
        }
    }

    /// 把逻辑矩形 r 清成全透明
    pub fn clear_rect(&mut self, r: R) {
        let p = self.tr(r).inter(&R::new(0, 0, self.pw, self.ph));
        for y in p.y..p.bottom() {
            self.px[(y * self.pw + p.x) as usize..(y * self.pw + p.right()) as usize].fill([0; 4]);
        }
    }

    /// 把 src（同样的缩放）原样复制到逻辑坐标 (x, y)，超出的部分不画
    pub fn blit(&mut self, src: &Canvas, x: i32, y: i32) {
        let (ox, oy) = (self.tx(x), self.tx(y));
        for sy in 0..src.ph {
            let dy = oy + sy;
            if dy < 0 || dy >= self.ph {
                continue;
            }
            for sx in 0..src.pw {
                let dx = ox + sx;
                if dx >= 0 && dx < self.pw {
                    self.px[(dy * self.pw + dx) as usize] = src.px[(sy * src.pw + sx) as usize];
                }
            }
        }
    }

    /// 以覆盖率 cov 混合一个实际像素（调用方保证在画布内）
    #[inline]
    pub(crate) fn blend_px(&mut self, x: i32, y: i32, c: Rgba, cov: f32) {
        let a = c.3 as f32 / 255.0 * cov;
        if a <= 0.0 {
            return;
        }
        let i = (y * self.pw + x) as usize;
        let d = &mut self.px[i];
        let inv = 1.0 - a;
        d[0] = (c.2 as f32 * a + d[0] as f32 * inv + 0.5) as u8;
        d[1] = (c.1 as f32 * a + d[1] as f32 * inv + 0.5) as u8;
        d[2] = (c.0 as f32 * a + d[2] as f32 * inv + 0.5) as u8;
        d[3] = (255.0 * a + d[3] as f32 * inv + 0.5) as u8;
    }

    pub fn fill(&mut self, r: R, c: Rgba) {
        let r = self.tr(r).inter(&self.clip_px());
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                self.blend_px(x, y, c, 1.0);
            }
        }
    }

    /// 纵向渐变
    pub fn vgrad(&mut self, r: R, top: Rgba, bottom: Rgba) {
        let r = self.tr(r);
        let cr = r.inter(&self.clip_px());
        for y in cr.y..cr.bottom() {
            let t = if r.h > 1 { (y - r.y) as f32 / (r.h - 1) as f32 } else { 0.0 };
            let c = top.mix(bottom, t);
            for x in cr.x..cr.right() {
                self.blend_px(x, y, c, 1.0);
            }
        }
    }

    /// 圆角矩形：fill 填充，border 1 个逻辑像素的描边（抗锯齿；圆角半径是逻辑像素）
    pub fn round_rect(&mut self, r: R, radius: f32, fill: Option<Rgba>, border: Option<Rgba>) {
        let r = self.tr(r);
        let cr = r.inter(&self.clip_px());
        let (hw, hh) = (r.w as f32 / 2.0, r.h as f32 / 2.0);
        let (mx, my) = (r.x as f32 + hw, r.y as f32 + hh);
        let radius = (radius * self.scale).min(hw).min(hh);
        // 描边宽度；在它里面再 1.5 像素才是完全填充的内部
        let bw = self.line_px();
        let deep = -0.5 - bw;
        for y in cr.y..cr.bottom() {
            let qy = (y as f32 + 0.5 - my).abs() - (hh - radius);
            for x in cr.x..cr.right() {
                let qx = (x as f32 + 0.5 - mx).abs() - (hw - radius);
                // 离边和圆角都超过 1.5 像素的内部：完全被填充、不碰描边，不用算距离（大窗口的底色快很多，结果一样）
                if qx <= deep && qy <= deep {
                    if let Some(f) = fill {
                        self.blend_px(x, y, f, 1.0);
                    }
                    continue;
                }
                // 圆角矩形的有符号距离（内部为负）
                let sd = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius;
                let outer = (0.5 - sd).clamp(0.0, 1.0);
                let inner = (0.5 - bw - sd).clamp(0.0, 1.0);
                if let Some(f) = fill {
                    self.blend_px(x, y, f, if border.is_some() { inner } else { outer });
                }
                if let Some(b) = border {
                    self.blend_px(x, y, b, outer - inner);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 加快内部填充前的写法（每个像素都算距离），用来对照结果
    fn reference(c: &mut Canvas, r: R, radius: f32, fill: Option<Rgba>, border: Option<Rgba>) {
        let cr = r.inter(&c.clip_rect());
        let (hw, hh) = (r.w as f32 / 2.0, r.h as f32 / 2.0);
        let (mx, my) = (r.x as f32 + hw, r.y as f32 + hh);
        let radius = radius.min(hw).min(hh);
        for y in cr.y..cr.bottom() {
            for x in cr.x..cr.right() {
                let qx = (x as f32 + 0.5 - mx).abs() - (hw - radius);
                let qy = (y as f32 + 0.5 - my).abs() - (hh - radius);
                let sd = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius;
                let (outer, inner) = ((0.5 - sd).clamp(0.0, 1.0), (-0.5 - sd).clamp(0.0, 1.0));
                if let Some(f) = fill {
                    c.blend(x, y, f, if border.is_some() { inner } else { outer });
                }
                if let Some(b) = border {
                    c.blend(x, y, b, outer - inner);
                }
            }
        }
    }

    #[test]
    fn round_rect_same_as_reference() {
        let cases = [
            (R::new(0, 0, 700, 450), 8.0, Some(Rgba(20, 20, 20, 150)), Some(Rgba(255, 255, 255, 40))),
            (R::new(3, 2, 61, 23), 3.0, Some(Rgba::gray(52)), Some(Rgba::gray(100))),
            (R::new(5, 5, 7, 14), 2.0, Some(Rgba(240, 110, 30, 200)), None),
            (R::new(-4, 10, 40, 9), 6.0, None, Some(Rgba::gray(200))),
            (R::new(1, 1, 3, 3), 8.0, Some(Rgba::gray(90)), Some(Rgba::gray(10))),
        ];
        for (r, radius, fill, border) in cases {
            let (mut a, mut b) = (Canvas::new(720, 470), Canvas::new(720, 470));
            a.fill(R::new(0, 0, 720, 470), Rgba(10, 60, 90, 255));
            b.fill(R::new(0, 0, 720, 470), Rgba(10, 60, 90, 255));
            a.round_rect(r, radius, fill, border);
            reference(&mut b, r, radius, fill, border);
            assert!(a.px == b.px, "{r:?} 与原写法不一致");
        }
    }
}
