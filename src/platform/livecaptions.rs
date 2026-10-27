//! Windows 11「实时辅助字幕」(Live Captions)：在本机识别系统正在播放的声音，把识别出的文字交给界面。
//! 它没有公开接口，做法与开源的 LiveCaptions-Translator 相同：启动 LiveCaptions.exe，用 UI 自动化读它窗口里
//! `CaptionsTextBlock` 的文字。我们把它放在一个看不见的桌面上启动，屏幕上、任务栏上都不会出现它的窗口；
//! 用户自己已经开着实时辅助字幕时直接读那个窗口，不另开，用完也不关。
//!
//! 每次开启在一个新线程里跑一轮（新线程才能换到隐藏桌面）：检查 → 启动或附着 → 每 0.15 秒读一次文字 → 关闭。
//! 关闭在后台完成，不卡界面；下一轮等上一轮关完才开始。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
use windows::Win32::System::JobObjects::*;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW};
use windows::Win32::System::StationsAndDesktops::{CreateDesktopW, DESKTOP_CONTROL_FLAGS, HDESK, SetThreadDesktop};
use windows::Win32::System::Threading::*;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId, IsWindow, PostMessageW, WM_CLOSE};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    #[default]
    Off,
    /// 正在启动，等它的字幕框出来
    Starting,
    /// 正在识别
    Running,
    /// 出错停了（原因写给用户看）
    Failed(String),
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub status: Status,
    /// 最近几段识别结果（段与段之间是换行，越后越新）。识别时会回头改前面的字，所以每次都是完整的一份
    pub text: String,
    /// status 或 text 变了就 +1
    pub rev: u64,
}

/// 字幕识别的开关：`set(true)` 在后台启动，`set(false)` 让它停下
#[derive(Default)]
pub struct Captions {
    shared: Arc<Mutex<State>>,
    /// 当前这一轮：停止标志、线程（出错停了以后线程已结束，但仍留在这里，见 `retry`）
    run: Option<(Arc<AtomicBool>, JoinHandle<()>)>,
    /// 正在关的上一轮
    closing: Option<JoinHandle<()>>,
}

impl Captions {
    /// 状态变了（rev 与 `seen` 不同）时返回一份
    pub fn changed(&self, seen: u64) -> Option<State> {
        let s = self.shared.lock();
        (s.rev != seen).then(|| s.clone())
    }

    /// 开 / 关。已经开着（或出错停了）时再开什么也不做
    pub fn set(&mut self, on: bool) {
        if on && self.run.is_none() {
            self.start();
        } else if !on {
            self.stop();
        }
    }

    /// 出错停了的话重新开一轮（换台时用）
    pub fn retry(&mut self) {
        if self.run.as_ref().is_some_and(|r| r.1.is_finished()) {
            self.start();
        }
    }

    fn start(&mut self) {
        let stop = Arc::new(AtomicBool::new(false));
        let prev = self.closing.take().or_else(|| self.run.take().map(|r| r.1));
        let (shared, s) = (self.shared.clone(), stop.clone());
        let h = std::thread::Builder::new().name("live-captions".into()).spawn(move || {
            // 上一轮还在关：等它关完，免得这一轮附着到正在关的窗口上
            if let Some(p) = prev {
                let _ = p.join();
            }
            session(&shared, &s);
        });
        match h {
            Ok(h) => self.run = Some((stop, h)),
            Err(e) => publish(&self.shared, Status::Failed(format!("无法启动：{e}")), String::new()),
        }
    }

    fn stop(&mut self) {
        if let Some((stop, h)) = self.run.take() {
            stop.store(true, Ordering::Release);
            self.closing = Some(h);
        }
    }

    /// 退出程序时：让它停下，最多等 `max` 让它关完
    pub fn shutdown(&mut self, max: Duration) {
        self.stop();
        let Some(h) = self.closing.take() else { return };
        let t0 = Instant::now();
        while !h.is_finished() && t0.elapsed() < max {
            std::thread::sleep(Duration::from_millis(20));
        }
        if h.is_finished() {
            let _ = h.join();
        }
    }
}

impl Drop for Captions {
    fn drop(&mut self) {
        self.shutdown(Duration::from_secs(4));
    }
}

fn publish(shared: &Mutex<State>, status: Status, text: String) {
    let mut s = shared.lock();
    if s.status != status || s.text != text {
        (s.status, s.text) = (status, text);
        s.rev += 1;
    }
}

fn session(shared: &Mutex<State>, stop: &AtomicBool) {
    if stop.load(Ordering::Acquire) {
        return;
    }
    publish(shared, Status::Starting, String::new());
    let status = match run(shared, stop) {
        Ok(()) => Status::Off,
        Err(e) => {
            log::warn!("实时辅助字幕：{e}");
            Status::Failed(e)
        }
    };
    publish(shared, status, String::new());
}

const EXE_NAME: &str = "LiveCaptions.exe";
const KEY: &str = r"Software\Microsoft\LiveCaptions";
const UI_KEY: &str = r"Software\Microsoft\LiveCaptions\UI";
/// 它关闭时记下的窗口位置和大小：我们开过一次以后改回原样（不影响用户自己用它时的位置）
const SAVED: [&str; 5] = ["xPosition", "yPosition", "LiveCaptionWidth", "LiveCaptionHeight", "WindowPosition"];

fn exe_path() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(root).join("System32").join(EXE_NAME)
}

/// 能不能用：这台电脑有没有实时辅助字幕、用户有没有同意过它的说明。不能用时返回给用户看的原因
pub fn check() -> Result<(), String> {
    if !exe_path().exists() {
        return Err("需要 Windows 11 的「实时辅助字幕」".into());
    }
    // 第一次用时它先显示一页说明，要用户点「是，继续」：在隐藏桌面上点不到，所以先让用户自己打开一次
    if reg_dword(UI_KEY, "HasUserConsented") != Some(1) {
        return Err("先按 Win+Ctrl+L 打开一次「实时辅助字幕」并同意".into());
    }
    Ok(())
}

fn run(shared: &Mutex<State>, stop: &AtomicBool) -> Result<(), String> {
    check()?;
    let exe = exe_path();
    // 先决定附着还是启动，再建 UI 自动化对象：换桌面要在这个线程建任何窗口之前
    let attached = find_window(None);
    let mut lc = None;
    if attached.is_none() {
        if process_running(EXE_NAME) {
            return Err("实时辅助字幕正在别处运行，关掉它再试".into());
        }
        let (desk, name) = hidden_desktop()?;
        unsafe { SetThreadDesktop(desk) }.map_err(|e| format!("无法切换到隐藏桌面：{e}"))?;
        lc = Some(Launched::start(&exe, &name)?);
    }
    let _com = Com::init()?;
    let uia: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.map_err(|e| format!("UI 自动化不可用：{e}"))?;
    let hwnd = match (attached, lc.as_mut()) {
        (Some(h), _) => h,
        (None, Some(l)) => match l.wait_window(stop)? {
            Some(h) => h,
            None => return Ok(()),
        },
        (None, None) => return Ok(()),
    };
    log::info!("实时辅助字幕：{}", if lc.is_some() { "在隐藏桌面上启动" } else { "读取已打开的窗口" });
    read_loop(shared, stop, &uia, hwnd, lc.as_ref())
    // lc 在这里关闭（Drop），uia 在 _com 之前释放
}

fn read_loop(shared: &Mutex<State>, stop: &AtomicBool, uia: &IUIAutomation, hwnd: HWND, lc: Option<&Launched>) -> Result<(), String> {
    let root = unsafe { uia.ElementFromHandle(hwnd) }.map_err(|e| format!("读不到它的窗口：{e}"))?;
    let cond = unsafe { uia.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from("CaptionsTextBlock")) }.map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    let mut block: Option<IUIAutomationElement> = None;
    let mut running = false;
    while !stop.load(Ordering::Acquire) {
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() || lc.is_some_and(|l| l.exited()) {
            return Err("实时辅助字幕被关掉了".into());
        }
        if block.is_none() {
            block = unsafe { root.FindFirst(TreeScope_Descendants, &cond) }.ok();
        }
        match block.as_ref().map(|b| unsafe { b.CurrentName() }) {
            Some(Ok(name)) => {
                running = true;
                publish(shared, Status::Running, tail(&name.to_string()));
            }
            // 字幕框重建了：下次重新找
            Some(Err(_)) => block = None,
            None if !running && t0.elapsed() > Duration::from_secs(15) => {
                return Err("实时辅助字幕没有准备好（可能要先在它的设置里下载语言文件）".into());
            }
            None => {}
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Ok(())
}

/// 只留最近 3 段；一段太长时（连续说了几分钟没停）从句末标点后截掉前面的
pub fn tail(s: &str) -> String {
    let paras: Vec<&str> = s.split('\n').map(str::trim).filter(|p| !p.is_empty()).collect();
    paras[paras.len().saturating_sub(3)..].iter().map(|p| clip(p, 1000)).collect::<Vec<_>>().join("\n")
}

fn clip(p: &str, max: usize) -> &str {
    let n = p.chars().count();
    if n <= max {
        return p;
    }
    // 截掉的字数按 400 字一档变：截的位置不会每来一个字就动（否则各处换行的位置跟着一直变）
    let cut = (n - max).div_ceil(400) * 400;
    let start = p.char_indices().nth(cut).map_or(p.len(), |(i, _)| i);
    match p[start..].char_indices().find(|(_, c)| "。！？；!?;".contains(*c)) {
        Some((i, c)) => p[start + i + c.len_utf8()..].trim_start(),
        None => &p[start..],
    }
}

/// 实时辅助字幕的窗口（只在当前线程所在的桌面上找）；`pid` = 只要这个进程的
fn find_window(pid: Option<u32>) -> Option<HWND> {
    let mut after = None;
    loop {
        let h = unsafe { FindWindowExW(None, after, w!("LiveCaptionsDesktopWindow"), None) }.ok()?;
        let mut p = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut p as *mut u32)) };
        if pid.is_none_or(|x| x == p) {
            return Some(h);
        }
        after = Some(h);
    }
}

fn process_running(name: &str) -> bool {
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return false };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = false;
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok && !found {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            found = String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case(name);
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
        found
    }
}

/// 放实时辅助字幕的隐藏桌面（整个程序只建一个，一直留到退出）：(句柄, 名字)
fn hidden_desktop() -> Result<(HDESK, String), String> {
    static DESK: std::sync::Mutex<isize> = std::sync::Mutex::new(0);
    let name = format!("AirPlay3-captions-{}", std::process::id());
    let mut d = DESK.lock().unwrap_or_else(|e| e.into_inner());
    if *d == 0 {
        let h = unsafe { CreateDesktopW(&HSTRING::from(&name), PCWSTR::null(), None, DESKTOP_CONTROL_FLAGS(0), GENERIC_ALL.0, None) }
            .map_err(|e| format!("无法创建隐藏桌面：{e}"))?;
        *d = h.0 as isize;
    }
    Ok((HDESK(*d as _), name))
}

struct Com;

impl Com {
    fn init() -> Result<Com, String> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().map_err(|e| format!("COM：{e}"))?;
        Ok(Com)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// 我们自己启动的实时辅助字幕。放进作业对象：本程序被结束时它跟着结束，不会留在隐藏桌面上一直运行
struct Launched {
    process: HANDLE,
    job: Option<HANDLE>,
    pid: u32,
    hwnd: Option<HWND>,
    saved: Vec<(&'static str, Option<u32>)>,
}

impl Launched {
    fn start(exe: &Path, desktop: &str) -> Result<Launched, String> {
        let saved = SAVED.iter().map(|&n| (n, reg_dword(UI_KEY, n))).collect();
        let mut desk: Vec<u16> = format!("WinSta0\\{desktop}").encode_utf16().chain([0]).collect();
        let mut cmd: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain([0]).collect();
        let si = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, lpDesktop: PWSTR(desk.as_mut_ptr()), ..Default::default() };
        let mut pi = PROCESS_INFORMATION::default();
        unsafe { CreateProcessW(&HSTRING::from(exe.as_os_str()), Some(PWSTR(cmd.as_mut_ptr())), None, None, false, CREATE_SUSPENDED, None, PCWSTR::null(), &si, &mut pi) }
            .map_err(|e| format!("无法启动实时辅助字幕：{e}"))?;
        let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.ok();
        if let Some(j) = job {
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
            unsafe {
                let _ = SetInformationJobObject(j, JobObjectExtendedLimitInformation, &info as *const _ as *const _, size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32);
                let _ = AssignProcessToJobObject(j, pi.hProcess);
            }
        }
        unsafe {
            ResumeThread(pi.hThread);
            let _ = CloseHandle(pi.hThread);
        }
        Ok(Launched { process: pi.hProcess, job, pid: pi.dwProcessId, hwnd: None, saved })
    }

    fn exited(&self) -> bool {
        unsafe { WaitForSingleObject(self.process, 0) == WAIT_OBJECT_0 }
    }

    /// 等它的窗口出来；中途要求停止时返回 None
    fn wait_window(&mut self, stop: &AtomicBool) -> Result<Option<HWND>, String> {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(15) {
            if stop.load(Ordering::Acquire) {
                return Ok(None);
            }
            if self.exited() {
                return Err("实时辅助字幕启动后马上退出了".into());
            }
            if let Some(h) = find_window(Some(self.pid)) {
                self.hwnd = Some(h);
                return Ok(Some(h));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err("实时辅助字幕的窗口没有出来".into())
    }
}

impl Drop for Launched {
    /// 关掉：先发 WM_CLOSE（它正常退出、记下自己的状态），3 秒内没退出就结束进程；再把它记下的位置改回原样
    fn drop(&mut self) {
        unsafe {
            if let Some(h) = self.hwnd.or_else(|| find_window(Some(self.pid))) {
                let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            if WaitForSingleObject(self.process, 3000) != WAIT_OBJECT_0 {
                log::warn!("实时辅助字幕 3 秒内没有退出，结束它");
                let _ = TerminateProcess(self.process, 1);
                WaitForSingleObject(self.process, 2000);
            }
        }
        for &(n, v) in &self.saved {
            set_reg_dword(UI_KEY, n, v);
        }
        // 它运行时 RunningState = 1，正常关闭后是 0。被结束的（包括本程序上次被结束时跟着结束的）来不及改：
        // 没有别的实时辅助字幕在运行时改成 0
        if reg_dword(KEY, "RunningState") == Some(1) && !process_running(EXE_NAME) {
            set_reg_dword(KEY, "RunningState", Some(0));
        }
        unsafe {
            let _ = CloseHandle(self.process);
            if let Some(j) = self.job {
                let _ = CloseHandle(j);
            }
        }
    }
}

fn reg_dword(key: &str, name: &str) -> Option<u32> {
    let (mut v, mut n) = (0u32, 4u32);
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut _), Some(&mut n)) };
    r.is_ok().then_some(v)
}

/// 写回一个 DWORD；原来没有这个值（None）时删掉
fn set_reg_dword(key: &str, name: &str, v: Option<u32>) {
    let (k, n) = (HSTRING::from(key), HSTRING::from(name));
    unsafe {
        let _ = match v {
            Some(v) => RegSetKeyValueW(HKEY_CURRENT_USER, &k, &n, REG_DWORD.0, Some(&v as *const u32 as *const _), 4),
            None => RegDeleteKeyValueW(HKEY_CURRENT_USER, &k, &n),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_last_three_paragraphs() {
        assert_eq!(tail("一\n二\n\n三\n四 \n"), "二\n三\n四");
        assert_eq!(tail(""), "");
        assert_eq!(tail("只有一段"), "只有一段");
    }

    #[test]
    fn long_paragraph_cut_after_sentence_end() {
        // 1500 字：截掉 400 的整数倍（这里 800）后，从下一个句末标点后开始
        let s: String = (0..150).map(|i| if i % 10 == 9 { "九字一句话就到这里。".to_string() } else { "十个字的一段内容啊".to_string() + "，" }).collect();
        let n = s.chars().count();
        assert!(n > 1000);
        let c = clip(&s, 1000);
        assert!(c.chars().count() <= 1000 && c.chars().count() >= 500, "{}", c.chars().count());
        assert!(c.starts_with("十个字"), "{c}");
        // 再多几个字，截的位置不变
        let longer = format!("{s}再来几个字");
        assert_eq!(clip(&longer, 1000).strip_suffix("再来几个字"), Some(c));
    }

    /// 手动检查（要 Windows 11 的实时辅助字幕，运行时放一段说话的声音）：
    /// cargo test --lib live_captions_manual -- --ignored --nocapture（AIRPLAY3_LC_SECS = 运行几秒）
    #[test]
    #[ignore]
    fn live_captions_manual() {
        let secs = std::env::var("AIRPLAY3_LC_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(15u64);
        let mut c = Captions::default();
        c.set(true);
        let (t0, mut seen) = (Instant::now(), 0);
        while t0.elapsed() < Duration::from_secs(secs) {
            if let Some(s) = c.changed(seen) {
                seen = s.rev;
                println!("{:6.2}s {:?} {}", t0.elapsed().as_secs_f64(), s.status, s.text.replace('\n', " | "));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let t1 = Instant::now();
        c.shutdown(Duration::from_secs(5));
        println!("shutdown {:.2}s, then {:?}", t1.elapsed().as_secs_f64(), c.changed(0).map(|s| s.status));
    }
}
