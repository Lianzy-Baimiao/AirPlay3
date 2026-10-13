//! 多媒体键盘上的静音、音量减、音量加（「配置 → 全局热键」里打开时）：改为调本程序的音量。
//! 系统默认用这几个键调系统音量，所以用低级键盘钩子在系统之前拿到并吃掉它们（原版也是这样做的）；
//! 都没打开时不装钩子。钩子回调在装钩子的线程（界面线程）上运行，只给播放条窗口发一个消息。

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

/// 发给播放条窗口的消息：wParam = 0 静音、1 音量减、2 音量加
pub const WM_APP_MEDIAKEY: u32 = WM_APP + 20;

pub const MUTE: u32 = 1;
pub const VOL_DOWN: u32 = 2;
pub const VOL_UP: u32 = 4;

const VK_VOLUME_MUTE: u32 = 0xAD;
const VK_VOLUME_DOWN: u32 = 0xAE;
const VK_VOLUME_UP: u32 = 0xAF;

static MASK: AtomicU32 = AtomicU32::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static HOOK: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = unsafe { &*(lp.0 as *const KBDLLHOOKSTRUCT) };
        let (bit, which) = match k.vkCode {
            VK_VOLUME_MUTE => (MUTE, 0),
            VK_VOLUME_DOWN => (VOL_DOWN, 1),
            VK_VOLUME_UP => (VOL_UP, 2),
            _ => (0, 0),
        };
        if bit != 0 && MASK.load(Ordering::Relaxed) & bit != 0 {
            let down = matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            if down {
                let hwnd = HWND(TARGET.load(Ordering::Relaxed) as *mut _);
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_APP_MEDIAKEY, WPARAM(which), LPARAM(0));
                }
            }
            // 按下、松开都吃掉，系统就不会再调系统音量
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

/// 按设置装上 / 拆掉钩子（mask：MUTE | VOL_DOWN | VOL_UP 的组合；0 = 都交给系统）。每帧调用，没变时不做事
pub fn sync(mask: u32, hwnd: HWND) {
    MASK.store(mask, Ordering::Relaxed);
    TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    let hooked = HOOK.load(Ordering::Relaxed) != 0;
    if mask != 0 && !hooked {
        let inst = unsafe { GetModuleHandleW(None) }.ok().map(|m| m.into());
        match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), inst, 0) } {
            Ok(h) => HOOK.store(h.0 as isize, Ordering::Relaxed),
            Err(e) => {
                log::warn!("装键盘钩子失败（音量键仍由系统处理）: {e}");
                // 不再重试
                MASK.store(0, Ordering::Relaxed);
                HOOK.store(-1, Ordering::Relaxed);
            }
        }
    } else if mask == 0 && hooked {
        let h = HOOK.swap(0, Ordering::Relaxed);
        if h > 0 {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(h as *mut _));
            }
        }
    }
}
