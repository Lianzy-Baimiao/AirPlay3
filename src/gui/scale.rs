//! 界面缩放：程序里的坐标、尺寸都是逻辑像素（原版 1:1 的像素）；画布和窗口按缩放比例换成屏幕上的实际像素。

use std::sync::atomic::{AtomicU32, Ordering};

/// 用户能选的缩放（25% 一档）
pub const MIN: u32 = 100;
pub const MAX: u32 = 300;
pub const STEP: u32 = 25;

static PERCENT: AtomicU32 = AtomicU32::new(100);
/// 系统的显示缩放（播放条所在显示器的；启动时取一次，WM_DPICHANGED 时更新）
static SYSTEM: AtomicU32 = AtomicU32::new(0);

/// 现在的缩放比例（1.0 = 100%）
pub fn get() -> f32 {
    PERCENT.load(Ordering::Relaxed) as f32 / 100.0
}

pub fn percent() -> u32 {
    PERCENT.load(Ordering::Relaxed)
}

pub fn set_percent(p: u32) {
    PERCENT.store(p.clamp(MIN, MAX), Ordering::Relaxed);
}

/// 四舍五入取整（负数也向 +∞ 方向取半，换回逻辑像素时与它一致）
pub fn rnd(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

/// 逻辑像素 → 屏幕上的实际像素
pub fn to_px(v: i32) -> i32 {
    rnd(v as f32 * get())
}

/// 屏幕上的实际像素 → 逻辑像素（四舍五入：窗口位置换来换去不会漂）
pub fn to_logical(p: i32) -> i32 {
    rnd(p as f32 / get())
}

/// 屏幕上的点 → 它所在的逻辑像素（鼠标命中：取所在的那一格）
pub fn point_logical(p: i32) -> i32 {
    ((p as f32 + 0.5) / get()).ceil() as i32 - 1
}

/// 屏幕上某一点（实际像素）所在显示器的显示缩放（DPI，100% = 96）
pub fn monitor_percent(x: i32, y: i32) -> u32 {
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    unsafe {
        let m = MonitorFromPoint(windows::Win32::Foundation::POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_ok() && dx > 0 { (dx * 100 + 48) / 96 } else { 100 }
    }
}

/// 现在的系统显示缩放（播放条所在显示器）
pub fn system() -> u32 {
    match SYSTEM.load(Ordering::Relaxed) {
        0 => monitor_percent(0, 0),
        p => p,
    }
}

pub fn set_system(p: u32) {
    SYSTEM.store(p, Ordering::Relaxed);
}

/// 设置里的缩放（None = 跟随系统）换成实际用的百分比
pub fn effective(setting: Option<u32>) -> u32 {
    setting.unwrap_or_else(system).clamp(MIN, MAX)
}

/// 鼠标在屏幕上的位置（实际像素）
pub fn cursor_px() -> (i32, i32) {
    let mut p = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// 鼠标在屏幕上的位置（逻辑像素）
pub fn cursor() -> (i32, i32) {
    let (x, y) = cursor_px();
    (point_logical(x), point_logical(y))
}

/// 屏幕上的矩形（实际像素）→ 逻辑像素（四条边各自换算）
pub fn rect_logical(r: super::canvas::R) -> super::canvas::R {
    let (x0, y0) = (to_logical(r.x), to_logical(r.y));
    super::canvas::R::new(x0, y0, to_logical(r.right()) - x0, to_logical(r.bottom()) - y0)
}

/// 逻辑矩形 → 屏幕上的实际像素
pub fn rect_px(r: super::canvas::R) -> super::canvas::R {
    let (x0, y0) = (to_px(r.x), to_px(r.y));
    super::canvas::R::new(x0, y0, to_px(r.right()) - x0, to_px(r.bottom()) - y0)
}

/// 鼠标消息里的客户区坐标（实际像素）→ 所在的逻辑像素
pub fn lx(lp: windows::Win32::Foundation::LPARAM) -> i32 {
    point_logical(super::wnd::lo(lp))
}

pub fn ly(lp: windows::Win32::Foundation::LPARAM) -> i32 {
    point_logical(super::wnd::hi(lp))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 逻辑 → 实际 → 逻辑 不变；鼠标所在的格子与画布的取整一致
    #[test]
    fn round_trip_and_cells() {
        for p in [100, 125, 150, 175, 200, 250, 300] {
            let s = p as f32 / 100.0;
            for v in -50..2000 {
                assert_eq!(rnd(rnd(v as f32 * s) as f32 / s), v, "{p}% {v}");
            }
            // 实际像素 x 落在逻辑像素 k 里 ⇔ rnd(k·s) ≤ x < rnd((k+1)·s)
            for x in 0..1000 {
                let k = ((x as f32 + 0.5) / s).ceil() as i32 - 1;
                assert!(rnd(k as f32 * s) <= x && x < rnd((k + 1) as f32 * s), "{p}% {x} -> {k}");
            }
        }
    }
}
