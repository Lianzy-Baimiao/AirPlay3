//! GDI+ 文字（与原版一致：GDI+ DrawString + GenericTypographic，字体 Segoe UI → Tahoma → Arial，中文由系统字体链接回退）。
//! 文字先画成白字黑底的覆盖率遮罩，再由 Canvas 按颜色合成（支持灰度抗锯齿与 ClearType 子像素）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::GdiPlus::*;
use windows::core::PCWSTR;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Font {
    /// 像素字号 ×10
    pub size10: u16,
    pub bold: bool,
}

impl Font {
    pub const fn px(size: f32, bold: bool) -> Font {
        Font { size10: (size * 10.0) as u16, bold }
    }

    /// 按界面缩放放大后的字号（画在实际像素上用）
    pub fn scaled(self, scale: f32) -> Font {
        if scale == 1.0 { self } else { Font { size10: (self.size10 as f32 * scale).round() as u16, bold: self.bold } }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hint {
    /// AntiAliasGridFit
    Gray,
    /// AntiAlias（不做网格对齐，字形更宽更柔和）
    Smooth,
    ClearType,
    SystemDefault,
}

impl Hint {
    fn gdip(self) -> TextRenderingHint {
        match self {
            Hint::Gray => TextRenderingHintAntiAliasGridFit,
            Hint::Smooth => TextRenderingHintAntiAlias,
            Hint::ClearType => TextRenderingHintClearTypeGridFit,
            Hint::SystemDefault => TextRenderingHintSystemDefault,
        }
    }
}

pub struct Mask {
    pub w: u32,
    pub h: u32,
    /// 每像素 R/G/B 覆盖率（灰度时三者相同）
    pub cov: Vec<[u8; 3]>,
}

struct Fonts {
    family: *mut GpFontFamily,
    fonts: HashMap<Font, *mut GpFont>,
    format: *mut GpStringFormat,
    measure_g: *mut GpGraphics,
}

thread_local! {
    static FONTS: RefCell<Option<Fonts>> = const { RefCell::new(None) };
}

fn startup() {
    static TOKEN: OnceLock<usize> = OnceLock::new();
    TOKEN.get_or_init(|| {
        let mut token = 0usize;
        let input = GdiplusStartupInput { GdiplusVersion: 1, ..Default::default() };
        unsafe { GdiplusStartup(&mut token, &input, std::ptr::null_mut()) };
        token
    });
}

fn family() -> *mut GpFontFamily {
    for name in ["Segoe UI", "Tahoma", "Arial"] {
        let w: Vec<u16> = name.encode_utf16().chain([0]).collect();
        let mut f = std::ptr::null_mut();
        if unsafe { GdipCreateFontFamilyFromName(PCWSTR(w.as_ptr()), std::ptr::null_mut(), &mut f) }.0 == 0 && !f.is_null() {
            return f;
        }
    }
    std::ptr::null_mut()
}

fn with<R>(f: impl FnOnce(&mut Fonts) -> R) -> R {
    startup();
    FONTS.with(|c| {
        let mut c = c.borrow_mut();
        let fs = c.get_or_insert_with(|| unsafe {
            let (mut generic, mut fmt) = (std::ptr::null_mut(), std::ptr::null_mut());
            GdipStringFormatGetGenericTypographic(&mut generic);
            GdipCloneStringFormat(generic, &mut fmt);
            // 保留 GenericTypographic 自带的标志（NoFitBlackBox 等；去掉的话个别字会错开 1 像素，如「取消」的「消」）
            let mut flags = 0;
            GdipGetStringFormatFlags(fmt, &mut flags);
            flags |= StringFormatFlagsMeasureTrailingSpaces.0 | StringFormatFlagsNoWrap.0;
            // 仅调试构建：AIRPLAY3_TEXT_FLAGS=<十六进制> 换一组 StringFormat 标志（和原版对比用）
            #[cfg(debug_assertions)]
            if let Some(f) = std::env::var("AIRPLAY3_TEXT_FLAGS").ok().and_then(|v| i32::from_str_radix(v.trim_start_matches("0x"), 16).ok()) {
                flags = f;
            }
            GdipSetStringFormatFlags(fmt, flags);
            let dc = CreateCompatibleDC(None);
            let mut g = std::ptr::null_mut();
            GdipCreateFromHDC(dc, &mut g);
            GdipSetTextRenderingHint(g, TextRenderingHintAntiAliasGridFit);
            Fonts { family: family(), fonts: HashMap::new(), format: fmt, measure_g: g }
        });
        f(fs)
    })
}

impl Fonts {
    fn font(&mut self, f: Font) -> *mut GpFont {
        let fam = self.family;
        *self.fonts.entry(f).or_insert_with(|| {
            let mut p = std::ptr::null_mut();
            let style = if f.bold { FontStyleBold.0 } else { FontStyleRegular.0 };
            unsafe { GdipCreateFont(fam, f.size10 as f32 / 10.0, style, UnitPixel, &mut p) };
            p
        })
    }
}

thread_local! {
    static MEASURE: RefCell<HashMap<(String, Font), (f32, f32)>> = RefCell::new(HashMap::new());
}

/// 文字宽高（像素，浮点；带缓存）
pub fn measure(text: &str, font: Font) -> (f32, f32) {
    let key = (text.to_string(), font);
    if let Some(v) = MEASURE.with(|c| c.borrow().get(&key).copied()) {
        return v;
    }
    let v = measure_raw(text, font);
    MEASURE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 4000 {
            c.clear();
        }
        c.insert(key, v);
    });
    v
}

fn measure_raw(text: &str, font: Font) -> (f32, f32) {
    let s: Vec<u16> = if text.is_empty() { vec![b' ' as u16] } else { text.encode_utf16().collect() };
    let (w, h) = with(|fs| unsafe {
        let layout = RectF { X: 0.0, Y: 0.0, Width: 100000.0, Height: 1000.0 };
        let mut bb = RectF::default();
        let fnt = fs.font(font);
        GdipMeasureString(fs.measure_g, PCWSTR(s.as_ptr()), s.len() as i32, fnt, &layout, fs.format, &mut bb, std::ptr::null_mut(), std::ptr::null_mut());
        (bb.Width, bb.Height)
    });
    (if text.is_empty() { 0.0 } else { w }, h)
}

thread_local! {
    static CACHE: RefCell<HashMap<(String, Font, u8), std::rc::Rc<Mask>>> = RefCell::new(HashMap::new());
}

/// 带缓存的 render（界面每帧重绘时避免反复调用 GDI+）
pub fn cached(text: &str, font: Font, hint: Hint) -> std::rc::Rc<Mask> {
    let key = (text.to_string(), font, hint as u8);
    if let Some(m) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return m;
    }
    let m = std::rc::Rc::new(render(text, font, hint));
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 2000 {
            c.clear();
        }
        c.insert(key, m.clone());
    });
    m
}

/// 遮罩（和 draw_on 的块）的大小：左侧留 1 像素（字形可能略微越界）
pub fn mask_size(text: &str, font: Font) -> (u32, u32) {
    let (tw, th) = measure(text, font);
    (tw.ceil() as u32 + 3, th.ceil() as u32 + 1)
}

/// 在不透明的底（BGRA，w × h，和 render 的遮罩一样大、字在同一个位置）上直接画字。原版就是这样画的：
/// GDI+ 的 ClearType 混合与前景、底色都有关，先画遮罩再合成的抗锯齿边会淡一点
pub fn draw_on(text: &str, font: Font, hint: Hint, c: [u8; 4], px: &mut [[u8; 4]], w: u32, h: u32) {
    with(|fs| unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = CreateCompatibleDC(None);
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
        if !bits.is_null() {
            let old = SelectObject(dc, bmp.into());
            std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut [u8; 4], px.len());
            let mut g = std::ptr::null_mut();
            GdipCreateFromHDC(dc, &mut g);
            GdipSetTextRenderingHint(g, hint.gdip());
            let mut brush: *mut GpSolidFill = std::ptr::null_mut();
            GdipCreateSolidFill(u32::from_be_bytes([c[3], c[0], c[1], c[2]]), &mut brush);
            let s: Vec<u16> = text.encode_utf16().collect();
            let rect = RectF { X: 1.0, Y: 0.0, Width: w as f32, Height: h as f32 };
            GdipDrawString(g, PCWSTR(s.as_ptr()), s.len() as i32, fs.font(font), &rect, fs.format, brush as *const GpBrush);
            GdipDeleteBrush(brush as *mut GpBrush);
            GdipDeleteGraphics(g);
            let _ = GdiFlush();
            std::ptr::copy_nonoverlapping(bits as *const [u8; 4], px.as_mut_ptr(), px.len());
            SelectObject(dc, old);
        }
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
    })
}

/// 把文字画成覆盖率遮罩。遮罩左侧留 1 像素（字形可能略微越界）。
pub fn render(text: &str, font: Font, hint: Hint) -> Mask {
    let (w, h) = mask_size(text, font);
    with(|fs| unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = CreateCompatibleDC(None);
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
        let old = SelectObject(dc, bmp.into());
        let mut g = std::ptr::null_mut();
        GdipCreateFromHDC(dc, &mut g);
        GdipSetTextRenderingHint(g, hint.gdip());
        GdipGraphicsClear(g, 0xFF00_0000);
        let mut brush: *mut GpSolidFill = std::ptr::null_mut();
        GdipCreateSolidFill(0xFFFF_FFFF, &mut brush);
        let s: Vec<u16> = text.encode_utf16().collect();
        let rect = RectF { X: 1.0, Y: 0.0, Width: w as f32, Height: h as f32 };
        GdipDrawString(g, PCWSTR(s.as_ptr()), s.len() as i32, fs.font(font), &rect, fs.format, brush as *const GpBrush);
        GdipDeleteBrush(brush as *mut GpBrush);
        GdipDeleteGraphics(g);
        let _ = GdiFlush();
        let cov = if bits.is_null() {
            vec![[0; 3]; (w * h) as usize]
        } else {
            std::slice::from_raw_parts(bits as *const [u8; 4], (w * h) as usize).iter().map(|p| [p[2], p[1], p[0]]).collect()
        };
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        Mask { w, h, cov }
    })
}
