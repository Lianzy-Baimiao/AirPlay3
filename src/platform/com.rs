//! COM 初始化辅助。UI 线程由 winit 初始化为 STA；工作线程（解码、渲染、扫描）用 MTA。

use std::cell::Cell;

use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Threading::{AvSetMmThreadCharacteristicsW, GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL};
use windows::core::w;

thread_local! {
    static INITED: Cell<bool> = const { Cell::new(false) };
}

/// 当前线程初始化为 MTA（每线程一次；不要在 UI 线程调用）。
pub fn init_mta() {
    INITED.with(|f| {
        if !f.get() {
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            f.set(true);
        }
    });
}

/// 音频渲染线程提升到 MMCSS "Pro Audio"，失败时退回最高线程优先级。
pub fn boost_audio_thread() {
    unsafe {
        let mut idx = 0u32;
        if AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut idx).is_err() {
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        }
    }
}
