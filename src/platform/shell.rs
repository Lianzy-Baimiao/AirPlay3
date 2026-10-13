//! Shell 集成：打开所在文件夹、放入回收站、开机启动、文件关联（只写 HKCU，不需要管理员权限）。

use std::path::Path;

use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegDeleteTreeW, RegSetKeyValueW};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

/// 在资源管理器中打开所在文件夹并选中文件。
pub fn reveal(path: &Path) {
    unsafe {
        let pidl = ILCreateFromPathW(&HSTRING::from(path.as_os_str()));
        if pidl.is_null() {
            return;
        }
        let _ = SHOpenFolderAndSelectItems(pidl, None, 0);
        ILFree(Some(pidl));
    }
}

/// 放入回收站（调用方负责确认）。
pub fn recycle(paths: &[&Path]) -> bool {
    if paths.is_empty() {
        return true;
    }
    // 以 \0 分隔、\0\0 结尾的路径列表
    let mut buf: Vec<u16> = Vec::new();
    for p in paths {
        buf.extend(p.as_os_str().encode_wide_compat());
        buf.push(0);
    }
    buf.push(0);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(buf.as_ptr()),
        fFlags: (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_SILENT.0 | FOF_NOERRORUI.0) as u16,
        ..Default::default()
    };
    unsafe { SHFileOperationW(&mut op) == 0 && !op.fAnyOperationsAborted.as_bool() }
}

trait EncodeWide {
    fn encode_wide_compat(&self) -> Vec<u16>;
}

impl EncodeWide for std::ffi::OsStr {
    fn encode_wide_compat(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().collect()
    }
}

fn set_str(root: HKEY, key: &str, name: Option<&str>, value: &str) -> bool {
    let v: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let name_h = name.map(HSTRING::from);
    let name_p = name_h.as_ref().map_or(PCWSTR::null(), |h| PCWSTR(h.as_ptr()));
    unsafe { RegSetKeyValueW(root, &HSTRING::from(key), name_p, REG_SZ.0, Some(v.as_ptr() as *const _), (v.len() * 2) as u32).is_ok() }
}

fn exe() -> String {
    std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// 开机时自动启动（启动时最小化到托盘：命令行 --tray）。
pub fn set_autostart(on: bool) -> bool {
    if on {
        set_str(HKEY_CURRENT_USER, RUN_KEY, Some("AirPlay3"), &format!("\"{}\" --tray", exe()))
    } else {
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), w!("AirPlay3")) };
        r.is_ok() || r.0 == 2
    }
}

const PROGID: &str = "AirPlay3.Audio";
const APP_KEY: &str = r"Software\Classes\Applications\AirPlay3.exe";
const CAPS_KEY: &str = r"Software\AirPlay3\Capabilities";

/// 注册文件关联（ProgID + OpenWithProgids + 默认程序能力）。Windows 10/11 不允许程序直接改默认程序，
/// 注册后需要用户在系统设置里选择，所以随后打开"默认应用"设置页。
pub fn register_associations(exts: &[&str]) -> bool {
    let exe = exe();
    let cmd = format!("\"{exe}\" \"%1\"");
    let icon = format!("\"{exe}\",0");
    let hk = HKEY_CURRENT_USER;
    let mut ok = set_str(hk, &format!(r"Software\Classes\{PROGID}"), None, "AirPlay3 音频文件");
    ok &= set_str(hk, &format!(r"Software\Classes\{PROGID}\DefaultIcon"), None, &icon);
    ok &= set_str(hk, &format!(r"Software\Classes\{PROGID}\shell\open\command"), None, &cmd);
    ok &= set_str(hk, &format!(r"{APP_KEY}\shell\open\command"), None, &cmd);
    ok &= set_str(hk, APP_KEY, Some("FriendlyAppName"), "AirPlay3");
    ok &= set_str(hk, CAPS_KEY, Some("ApplicationName"), "AirPlay3");
    ok &= set_str(hk, CAPS_KEY, Some("ApplicationDescription"), "AirPlay3 本地音乐播放器");
    for e in exts {
        ok &= set_str(hk, &format!(r"Software\Classes\.{e}\OpenWithProgids"), Some(PROGID), "");
        ok &= set_str(hk, &format!(r"{APP_KEY}\SupportedTypes"), Some(&format!(".{e}")), "");
        ok &= set_str(hk, &format!(r"{CAPS_KEY}\FileAssociations"), Some(&format!(".{e}")), PROGID);
    }
    ok &= set_str(hk, r"Software\RegisteredApplications", Some("AirPlay3"), CAPS_KEY);
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    ok
}

/// 已关联到本程序的扩展名（默认程序能力里登记过的）
pub fn associated(exts: &[&str]) -> Vec<bool> {
    use windows::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};
    let key = HSTRING::from(format!(r"{CAPS_KEY}\FileAssociations"));
    exts.iter()
        .map(|e| {
            let name = HSTRING::from(format!(".{e}"));
            unsafe { RegGetValueW(HKEY_CURRENT_USER, &key, &name, RRF_RT_REG_SZ, None, None, None) }.is_ok()
        })
        .collect()
}

/// 只去掉这些扩展名的关联（其它扩展名和程序本身的登记不动）
pub fn unassociate(exts: &[&str]) {
    let hk = HKEY_CURRENT_USER;
    unsafe {
        for e in exts {
            let dot = HSTRING::from(format!(".{e}"));
            let _ = RegDeleteKeyValueW(hk, &HSTRING::from(format!(r"Software\Classes\.{e}\OpenWithProgids")), &HSTRING::from(PROGID));
            let _ = RegDeleteKeyValueW(hk, &HSTRING::from(format!(r"{APP_KEY}\SupportedTypes")), &dot);
            let _ = RegDeleteKeyValueW(hk, &HSTRING::from(format!(r"{CAPS_KEY}\FileAssociations")), &dot);
        }
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

pub fn unregister_associations(exts: &[&str]) {
    let hk = HKEY_CURRENT_USER;
    unsafe {
        let _ = RegDeleteTreeW(hk, &HSTRING::from(format!(r"Software\Classes\{PROGID}")));
        let _ = RegDeleteTreeW(hk, &HSTRING::from(APP_KEY));
        let _ = RegDeleteTreeW(hk, &HSTRING::from(r"Software\AirPlay3"));
        let _ = RegDeleteKeyValueW(hk, &HSTRING::from(r"Software\RegisteredApplications"), w!("AirPlay3"));
        for e in exts {
            let _ = RegDeleteKeyValueW(hk, &HSTRING::from(format!(r"Software\Classes\.{e}\OpenWithProgids")), &HSTRING::from(PROGID));
        }
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

/// 用默认程序打开（网址、文件夹、系统设置页）。
pub fn open(target: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}
