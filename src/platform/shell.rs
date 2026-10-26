//! Shell 集成：打开所在文件夹、放入回收站、开机启动、文件关联（只写 HKCU，不需要管理员权限）。

use std::path::Path;

use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, REG_SZ, RRF_RT_ANY, RRF_RT_REG_SZ, RegCloseKey, RegDeleteKeyValueW, RegDeleteTreeW, RegEnumValueW, RegGetValueW, RegOpenKeyExW,
    RegSetKeyValueW,
};
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

/// 读一个字符串值（name = None 读默认值）；没有时返回 None
fn get_str(key: &str, name: Option<&str>) -> Option<String> {
    let name_h = name.map(HSTRING::from);
    let name_p = name_h.as_ref().map_or(PCWSTR::null(), |h| PCWSTR(h.as_ptr()));
    let key = HSTRING::from(key);
    let mut buf = vec![0u16; 1024];
    let mut cb = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, &key, name_p, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut cb)) };
    if r.is_err() {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]))
}

/// 有没有这个值（任何类型）
fn has_value(key: &str, name: &str) -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name), RRF_RT_ANY, None, None, None) }.is_ok()
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

/// 程序本身的登记（ProgID、打开命令、图标、默认程序能力），都指向正在运行的 exe。
/// NoOpenWith：系统不再把本程序列进所有文件（图片、文档……）的「打开方式」，只在关联了的格式里出现（经 ProgID）
fn register_program() -> bool {
    let exe = exe();
    let cmd = format!("\"{exe}\" \"%1\"");
    let icon = format!("\"{exe}\",0");
    let hk = HKEY_CURRENT_USER;
    let mut ok = set_str(hk, &format!(r"Software\Classes\{PROGID}"), None, "AirPlay3 音频文件");
    ok &= set_str(hk, &format!(r"Software\Classes\{PROGID}\DefaultIcon"), None, &icon);
    ok &= set_str(hk, &format!(r"Software\Classes\{PROGID}\shell\open\command"), None, &cmd);
    ok &= set_str(hk, &format!(r"{APP_KEY}\shell\open\command"), None, &cmd);
    ok &= set_str(hk, APP_KEY, Some("FriendlyAppName"), "AirPlay3");
    ok &= set_str(hk, APP_KEY, Some("NoOpenWith"), "");
    ok &= set_str(hk, CAPS_KEY, Some("ApplicationName"), "AirPlay3");
    ok &= set_str(hk, CAPS_KEY, Some("ApplicationDescription"), "AirPlay3 本地音乐播放器");
    ok
}

/// 注册文件关联（ProgID + OpenWithProgids + 默认程序能力）。Windows 10/11 不允许程序直接改默认程序，
/// 用户在「打开方式」或系统的「默认应用」里选了才生效
pub fn register_associations(exts: &[&str]) -> bool {
    let hk = HKEY_CURRENT_USER;
    let mut ok = register_program();
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

/// 默认程序能力里登记过的扩展名（小写、不带点），包括旧版本登记的
fn registered_exts() -> Vec<String> {
    let mut out = Vec::new();
    let mut key = HKEY::default();
    let path = HSTRING::from(format!(r"{CAPS_KEY}\FileAssociations"));
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, &path, None, KEY_READ, &mut key).is_err() {
            return out;
        }
        let mut name = vec![0u16; 256];
        for i in 0.. {
            let mut n = name.len() as u32;
            if RegEnumValueW(key, i, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut n, None, None, None, None).is_err() {
                break;
            }
            let v = String::from_utf16_lossy(&name[..n as usize]);
            out.push(v.trim_start_matches('.').to_lowercase());
        }
        let _ = RegCloseKey(key);
    }
    out
}

/// 打开命令（`"C:\...\AirPlay3.exe" "%1"`）里的程序路径
fn command_exe(cmd: &str) -> &str {
    let cmd = cmd.trim();
    match cmd.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(""),
        None => cmd.split_whitespace().next().unwrap_or(""),
    }
}

/// 启动时整理旧版本留下的登记：
/// - 去掉不在 `keep`（文件关联页上的格式）里的扩展名（旧版本一次关联了 .webm、.mka 等页面上没有的格式）；
/// - 补上 NoOpenWith（以前没有，程序出现在所有文件的「打开方式」里）；
/// - 程序被移动或改名后（登记的 exe 不在了）改指向现在这个 exe，否则关联了的文件打不开。
///
/// 没关联过任何格式时什么也不做
pub fn tidy_associations(keep: &[&str]) {
    let have = registered_exts();
    if have.is_empty() {
        return;
    }
    let stale: Vec<&str> = have.iter().map(String::as_str).filter(|e| !keep.contains(e)).collect();
    if stale.len() == have.len() {
        return unregister_associations(&[]);
    }
    let mut changed = false;
    if !stale.is_empty() {
        unassociate(&stale);
        changed = true;
    }
    if !has_value(APP_KEY, "NoOpenWith") {
        changed |= set_str(HKEY_CURRENT_USER, APP_KEY, Some("NoOpenWith"), "");
    }
    // 调试构建（自测用的副本）不抢登记
    let cmd = get_str(&format!(r"Software\Classes\{PROGID}\shell\open\command"), None).unwrap_or_default();
    if cfg!(not(debug_assertions)) && !Path::new(command_exe(&cmd)).is_file() {
        changed |= register_program();
    }
    if changed {
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    }
}

/// 删掉程序的全部登记；`exts` 之外，默认程序能力里登记过的扩展名也一起去掉
pub fn unregister_associations(exts: &[&str]) {
    let hk = HKEY_CURRENT_USER;
    let mut all: Vec<String> = registered_exts();
    all.extend(exts.iter().map(|e| e.to_string()));
    unsafe {
        let _ = RegDeleteTreeW(hk, &HSTRING::from(format!(r"Software\Classes\{PROGID}")));
        let _ = RegDeleteTreeW(hk, &HSTRING::from(APP_KEY));
        let _ = RegDeleteTreeW(hk, &HSTRING::from(r"Software\AirPlay3"));
        let _ = RegDeleteKeyValueW(hk, &HSTRING::from(r"Software\RegisteredApplications"), w!("AirPlay3"));
        for e in &all {
            let _ = RegDeleteKeyValueW(hk, &HSTRING::from(format!(r"Software\Classes\.{e}\OpenWithProgids")), &HSTRING::from(PROGID));
        }
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

/// 两个 exe 路径是不是同一个（Windows 路径不分大小写）
fn same_exe(a: &str, b: &str) -> bool {
    !a.is_empty() && a.to_lowercase() == b.to_lowercase()
}

/// 卸载时（安装包运行 `AirPlay3.exe --unregister`）：删掉指向本 exe 的文件关联和开机启动。
/// 指向别处（另一份便携版）的登记不动，免得卸载一份把另一份的关联也删了
pub fn unregister_self() {
    let me = exe();
    let cmd = get_str(&format!(r"Software\Classes\{PROGID}\shell\open\command"), None).unwrap_or_default();
    if same_exe(command_exe(&cmd), &me) {
        unregister_associations(&[]);
    }
    let run = get_str(RUN_KEY, Some("AirPlay3")).unwrap_or_default();
    if same_exe(command_exe(&run), &me) {
        set_autostart(false);
    }
}

/// 用默认程序打开（网址、文件夹、系统设置页）。
pub fn open(target: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

#[cfg(test)]
mod tests {
    use super::{command_exe, same_exe};

    #[test]
    fn same_exe_ignores_case_only() {
        assert!(same_exe(r"C:\Users\A\AppData\Local\Programs\AirPlay3\AirPlay3.exe", r"c:\users\a\appdata\local\programs\airplay3\AIRPLAY3.EXE"));
        assert!(!same_exe(r"E:\dist\AirPlay3.exe", r"C:\Users\A\AppData\Local\Programs\AirPlay3\AirPlay3.exe"));
        assert!(!same_exe("", r"C:\x.exe"));
    }

    #[test]
    fn exe_of_open_command() {
        assert_eq!(command_exe(r#""E:\AI开发\dist\AirPlay3.exe" "%1""#), r"E:\AI开发\dist\AirPlay3.exe");
        assert_eq!(command_exe(r#"  "C:\a b\x.exe""#), r"C:\a b\x.exe");
        assert_eq!(command_exe(r"C:\x.exe %1"), r"C:\x.exe");
        assert_eq!(command_exe(""), "");
    }
}
