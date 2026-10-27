//! 原生界面：复刻原版 AirPlay3 的分层窗口 + GDI+ 外观（图标、标志等在 `skin` 里自己画）。

mod actions;
mod desklyric;
#[cfg(test)]
mod golden;
pub mod app;
pub mod canvas;
pub mod draw;
mod input;
mod anim;
mod captions;
mod cfgdirwin;
mod lyricdlwin;
mod lyricwin;
mod menu;
mod minibar;
mod paint;
mod radiotab;
pub mod panels;
mod popup;
mod resize;
pub mod scale;
mod zoom;
mod settings_layout;
mod settings_model;
mod snap;
mod settingswin;
mod state;
mod taskbar;
pub mod theme;
mod tray;
mod wnd;
pub mod text;
pub mod window;

pub use app::run;

pub use canvas::{Canvas, R, Rgba};

impl Canvas {
    /// 调试：把画布存成 PNG（反预乘，透明处显示为黑）
    pub fn save_png(&self, path: &std::path::Path) -> Result<(), String> {
        let mut out = image::RgbaImage::new(self.pw as u32, self.ph as u32);
        for (i, p) in self.px.iter().enumerate() {
            let a = p[3] as u32;
            let un = |v: u8| if a == 0 { 0 } else { ((v as u32 * 255 + a / 2) / a).min(255) as u8 };
            let x = i as u32 % self.pw as u32;
            let y = i as u32 / self.pw as u32;
            out.put_pixel(x, y, image::Rgba([un(p[2]), un(p[1]), un(p[0]), p[3]]));
        }
        out.save(path).map_err(|e| e.to_string())
    }

    /// 仅调试构建：AIRPLAY3_DUMP=<目录> 时把画面存成 <目录>\<name>.png，用来和原版逐像素对比
    #[cfg(debug_assertions)]
    pub fn dump(&self, name: &str) {
        if let Some(dir) = std::env::var_os("AIRPLAY3_DUMP") {
            let dir = std::path::PathBuf::from(dir);
            let tmp = dir.join(format!("{name}.tmp.png"));
            if self.save_png(&tmp).is_ok() {
                let _ = std::fs::rename(&tmp, dir.join(format!("{name}.png")));
            }
        }
    }

    /// 仅调试构建：AIRPLAY3_DUMP 和 AIRPLAY3_DUMP_SEQ 都设了时，把动画的每一帧存成 <name>_<启动后的毫秒数>.png
    #[cfg(debug_assertions)]
    pub fn dump_seq(&self, name: &str, ms: u128) {
        if std::env::var_os("AIRPLAY3_DUMP_SEQ").is_some()
            && let Some(dir) = std::env::var_os("AIRPLAY3_DUMP")
        {
            let _ = self.save_png(&std::path::PathBuf::from(dir).join(format!("{name}_{ms:05}.png")));
        }
    }
}
