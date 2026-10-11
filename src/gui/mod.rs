//! 原生界面：复刻原版 AirPlay3 的分层窗口 + GDI+ 外观（皮肤图片来自原版资源）。

mod actions;
mod desklyric;
pub mod app;
pub mod canvas;
pub mod draw;
mod input;
mod lyricwin;
mod menu;
mod minibar;
mod paint;
pub mod panels;
mod popup;
mod resize;
mod settings_model;
mod settingswin;
mod state;
mod taskbar;
mod tray;
mod wnd;
pub mod text;
pub mod window;

pub use app::run;

pub use canvas::{Canvas, R, Rgba};

impl Canvas {
    /// 调试：把画布存成 PNG（反预乘，透明处显示为黑）
    pub fn save_png(&self, path: &std::path::Path) -> Result<(), String> {
        let mut out = image::RgbaImage::new(self.w as u32, self.h as u32);
        for (i, p) in self.px.iter().enumerate() {
            let a = p[3] as u32;
            let un = |v: u8| if a == 0 { 0 } else { ((v as u32 * 255 + a / 2) / a).min(255) as u8 };
            let x = i as u32 % self.w as u32;
            let y = i as u32 / self.w as u32;
            out.put_pixel(x, y, image::Rgba([un(p[2]), un(p[1]), un(p[0]), p[3]]));
        }
        out.save(path).map_err(|e| e.to_string())
    }
}
