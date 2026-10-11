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
            for x in cr.x..cr.right() {
                // 圆角矩形的有符号距离（内部为负）
                let qx = (x as f32 + 0.5 - mx).abs() - (hw - radius);
                let qy = (y as f32 + 0.5 - my).abs() - (hh - radius);
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
