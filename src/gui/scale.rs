//! 界面缩放：程序里的坐标、尺寸都是逻辑像素（原版 1:1 的像素）；画布和窗口按缩放比例换成屏幕上的实际像素。

use std::sync::atomic::{AtomicU32, Ordering};

/// 用户能选的缩放（25% 一档）
pub const MIN: u32 = 100;
pub const MAX: u32 = 300;
pub const STEP: u32 = 25;

static PERCENT: AtomicU32 = AtomicU32::new(100);

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

/// 系统显示缩放（主显示器的 DPI，100% = 96）
pub fn system_percent() -> u32 {
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    unsafe {
        let m = MonitorFromPoint(windows::Win32::Foundation::POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let (mut x, mut y) = (96u32, 96u32);
        if GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut x, &mut y).is_ok() && x > 0 { (x * 100 + 48) / 96 } else { 100 }
    }
}

/// 设置里的缩放（None = 跟随系统）换成实际用的百分比
pub fn effective(setting: Option<u32>) -> u32 {
    setting.unwrap_or_else(system_percent).clamp(MIN, MAX)
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
