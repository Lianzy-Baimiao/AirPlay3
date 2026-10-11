//! 全局热键（RegisterHotKey，绑定到主窗口，WM_HOTKEY 在窗口子类过程中处理）。

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey};

use crate::config::{HotkeyAction, HotkeySettings, KeyCombo};

const BASE_ID: i32 = 0x3A00;

pub fn action_of(id: i32) -> Option<HotkeyAction> {
    HotkeyAction::ALL.get((id - BASE_ID) as usize).copied()
}

pub fn unregister_all(hwnd: isize) {
    for i in 0..HotkeyAction::ALL.len() as i32 {
        unsafe {
            let _ = UnregisterHotKey(Some(HWND(hwnd as *mut _)), BASE_ID + i);
        }
    }
}

/// 注册全部热键，返回注册失败（被其它程序占用）的动作。
pub fn register_all(hwnd: isize, hk: &HotkeySettings) -> Vec<HotkeyAction> {
    unregister_all(hwnd);
    if !hk.enabled || hwnd == 0 {
        return Vec::new();
    }
    let mut failed = Vec::new();
    for (i, a) in HotkeyAction::ALL.iter().enumerate() {
        let Some(c) = hk.combo(*a) else { continue };
        let mut m = MOD_NOREPEAT;
        for (on, f) in [(c.ctrl, MOD_CONTROL), (c.alt, MOD_ALT), (c.shift, MOD_SHIFT), (c.win, MOD_WIN)] {
            if on {
                m |= f;
            }
        }
        if unsafe { RegisterHotKey(Some(HWND(hwnd as *mut _)), BASE_ID + i as i32, HOT_KEY_MODIFIERS(m.0), c.vk) }.is_err() {
            failed.push(*a);
        }
    }
    failed
}


fn vk_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x25 => "← (左)".into(),
        0x26 => "↑ (上)".into(),
        0x27 => "→ (右)".into(),
        0x28 => "↓ (下)".into(),
        0x20 => "SPACE (空格)".into(),
        0x0D => "ENTER (回车)".into(),
        0x21 => "PGUP (上页)".into(),
        0x22 => "PGDN (下页)".into(),
        0x24 => "HOME".into(),
        0x23 => "END".into(),
        0x2D => "INSERT".into(),
        0x2E => "DELETE".into(),
        0xBB => "\"=+\"".into(),
        0xBD => "\"-\" (减号)".into(),
        0xBC => "( ,< )".into(),
        0xBE => "( .> )".into(),
        0xBF => "( /? )".into(),
        0xBA => "( ;: )".into(),
        0xDB => "( [{ )".into(),
        0xDD => "( ]} )".into(),
        0xDC => "( \\| )".into(),
        0xC0 => "( `~ )".into(),
        0xDE => "( '\" )".into(),
        _ => format!("0x{vk:02X}"),
    }
}

/// 显示为 "CTRL + ALT + P"（原版格式）
pub fn combo_text(c: &KeyCombo) -> String {
    let mut s = String::new();
    for (on, name) in [(c.ctrl, "CTRL + "), (c.alt, "ALT + "), (c.shift, "SHIFT + "), (c.win, "WIN + ")] {
        if on {
            s.push_str(name);
        }
    }
    s + &vk_name(c.vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        let c = KeyCombo { ctrl: true, alt: true, shift: false, win: false, vk: 0x50 };
        assert_eq!(combo_text(&c), "CTRL + ALT + P");
        assert_eq!(action_of(BASE_ID + 2), Some(HotkeyAction::Next));
    }
}
