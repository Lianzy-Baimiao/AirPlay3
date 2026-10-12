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
    pub w: i32,
    pub h: i32,
    /// 预乘 BGRA
    pub px: Vec<[u8; 4]>,
    clip: Vec<R>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas {
        Canvas { w, h, px: vec![[0; 4]; (w.max(0) * h.max(0)) as usize], clip: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.px.fill([0; 4]);
    }

    /// 让完全透明的像素也能接收鼠标（alpha=1，肉眼不可见）：用于窗口边缘的调整区域
    pub fn hit_margin(&mut self, r: R) {
        let r = r.inter(&R::new(0, 0, self.w, self.h));
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let p = &mut self.px[(y * self.w + x) as usize];
                if p[3] == 0 {
                    *p = [0, 0, 0, 1];
                }
            }
        }
    }

    pub fn clip_rect(&self) -> R {
        self.clip.last().copied().unwrap_or(R::new(0, 0, self.w, self.h))
    }

    pub fn push_clip(&mut self, r: R) {
        let c = self.clip_rect().inter(&r);
        self.clip.push(c);
    }

    pub fn pop_clip(&mut self) {
        self.clip.pop();
    }

    /// 以覆盖率 cov(0..=1) 混合一个像素（source-over）
    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, c: Rgba, cov: f32) {
        let a = c.3 as f32 / 255.0 * cov;
        if a <= 0.0 {
            return;
        }
        let i = (y * self.w + x) as usize;
        let d = &mut self.px[i];
        let inv = 1.0 - a;
        d[0] = (c.2 as f32 * a + d[0] as f32 * inv + 0.5) as u8;
        d[1] = (c.1 as f32 * a + d[1] as f32 * inv + 0.5) as u8;
        d[2] = (c.0 as f32 * a + d[2] as f32 * inv + 0.5) as u8;
        d[3] = (255.0 * a + d[3] as f32 * inv + 0.5) as u8;
    }

    pub fn fill(&mut self, r: R, c: Rgba) {
        let r = r.inter(&self.clip_rect());
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                self.blend(x, y, c, 1.0);
            }
        }
    }

    /// 纵向渐变
    pub fn vgrad(&mut self, r: R, top: Rgba, bottom: Rgba) {
        let cr = r.inter(&self.clip_rect());
        for y in cr.y..cr.bottom() {
            let t = if r.h > 1 { (y - r.y) as f32 / (r.h - 1) as f32 } else { 0.0 };
            let c = top.mix(bottom, t);
            for x in cr.x..cr.right() {
                self.blend(x, y, c, 1.0);
            }
        }
    }

    /// 圆角矩形：fill 填充，border 1 像素描边（抗锯齿）
    pub fn round_rect(&mut self, r: R, radius: f32, fill: Option<Rgba>, border: Option<Rgba>) {
        let cr = r.inter(&self.clip_rect());
        let (hw, hh) = (r.w as f32 / 2.0, r.h as f32 / 2.0);
        let (mx, my) = (r.x as f32 + hw, r.y as f32 + hh);
        let radius = radius.min(hw).min(hh);
        for y in cr.y..cr.bottom() {
            let qy = (y as f32 + 0.5 - my).abs() - (hh - radius);
            for x in cr.x..cr.right() {
                let qx = (x as f32 + 0.5 - mx).abs() - (hw - radius);
                // 离边和圆角都超过 1.5 像素的内部：完全被填充、不碰描边，不用算距离（大窗口的底色快很多，结果一样）
                if qx <= -1.5 && qy <= -1.5 {
                    if let Some(f) = fill {
                        self.blend(x, y, f, 1.0);
                    }
                    continue;
                }
                // 圆角矩形的有符号距离（内部为负）
                let sd = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius;
                let outer = (0.5 - sd).clamp(0.0, 1.0);
                let inner = (-0.5 - sd).clamp(0.0, 1.0);
                if let Some(f) = fill {
                    self.blend(x, y, f, if border.is_some() { inner } else { outer });
                }
                if let Some(b) = border {
                    self.blend(x, y, b, outer - inner);
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
