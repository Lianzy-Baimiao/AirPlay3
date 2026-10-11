//! 单实例：命名互斥量；已有实例时把命令行文件通过 WM_COPYDATA 交给它，并把它的窗口调到前台。

use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

/// WM_COPYDATA 的 dwData 标识
pub const COPYDATA_OPEN: usize = 0x4150_3301;

/// 返回 true 表示本进程是第一个实例（互斥量句柄在进程结束时自动释放）
pub fn acquire() -> bool {
    unsafe {
        let h = CreateMutexW(None, true, w!("AirPlay3.Rust.SingleInstance"));
        !(h.is_err() || GetLastError() == ERROR_ALREADY_EXISTS)
    }
}

/// 找到已运行实例的主窗口（播放条：任务栏上显示的 AIRPLAY_GUI_CLASS 窗口）
fn find_main() -> Option<HWND> {
    unsafe {
        let mut h = FindWindowExW(None, None, w!("AIRPLAY_GUI_CLASS"), w!("AirPlay3")).ok();
        while let Some(cur) = h {
            let ex = GetWindowLongW(cur, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOOLWINDOW.0 == 0 {
                return Some(cur);
            }
            h = FindWindowExW(None, Some(cur), w!("AIRPLAY_GUI_CLASS"), w!("AirPlay3")).ok();
        }
        None
    }
}

/// 把文件交给已有实例（files 可为空：只激活窗口）
pub fn forward(files: &[PathBuf]) -> bool {
    let Some(hwnd) = (0..20).find_map(|_| find_main().or_else(|| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        None
    })) else {
        return false;
    };
    let text: Vec<u16> = files.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n").encode_utf16().collect();
    let cds = COPYDATASTRUCT { dwData: COPYDATA_OPEN, cbData: (text.len() * 2) as u32, lpData: text.as_ptr() as *mut _ };
    unsafe {
        // 允许已有实例把自己的窗口调到前台
        let _ = AllowSetForegroundWindow(u32::MAX);
        let mut res = 0usize;
        let _ = SendMessageTimeoutW(hwnd, WM_COPYDATA, WPARAM(0), LPARAM(&cds as *const _ as isize), SMTO_ABORTIFHUNG, 3000, Some(&mut res));
    }
    true
}

/// 解析 WM_COPYDATA（在已有实例的窗口过程中调用）
pub fn parse(lp: LPARAM) -> Option<Vec<PathBuf>> {
    let cds = unsafe { (lp.0 as *const COPYDATASTRUCT).as_ref()? };
    if cds.dwData != COPYDATA_OPEN {
        return None;
    }
    let n = cds.cbData as usize / 2;
    let s = if n == 0 { String::new() } else { String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(cds.lpData as *const u16, n) }) };
    Some(s.split('\n').filter(|l| !l.is_empty()).map(PathBuf::from).collect())
}
