//! 任务栏歌词：作为子窗口嵌入任务栏（Shell_TrayWnd），显示当前与下一句歌词。
//! 位置 = 对齐方式（靠左 / 居中 / 靠通知区域）+ 可拖动的水平偏移；资源管理器重启后自动重建。

use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use super::app::App;
use super::canvas::{Canvas, R, Rgba};
use super::desklyric::LineState;
use super::menu::{Act, Item, check, item};
use super::text::{self, Font, Hint};
use super::window::{CLASS, Surface};

pub struct TaskbarLyric {
    pub hwnd: HWND,
    pub parent: HWND,
    canvas: Canvas,
    surface: Surface,
    /// 在任务栏客户区中的位置
    pub pos: (i32, i32),
    drag: Option<(i32, i32)>,
    last: LineState,
    light: bool,
    ticks: u32,
}

fn taskbar() -> Option<(HWND, RECT)> {
    unsafe {
        let tb = FindWindowW(w!("Shell_TrayWnd"), None).ok()?;
        let mut r = RECT::default();
        GetClientRect(tb, &mut r).ok()?;
        Some((tb, r))
    }
}

/// 通知区域（托盘图标）在任务栏客户区中的左边界
fn tray_left(tb: HWND) -> Option<i32> {
    unsafe {
        let n = FindWindowExW(Some(tb), None, w!("TrayNotifyWnd"), None).ok()?;
        let mut r = RECT::default();
        GetWindowRect(n, &mut r).ok()?;
        let mut p = POINT { x: r.left, y: r.top };
        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(tb, &mut p);
        Some(p.x)
    }
}

/// 任务栏是否为浅色主题（决定文字颜色）
fn light_taskbar() -> bool {
    use windows::Win32::System::Registry::*;
    let mut v = 0u32;
    let mut n = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut n),
        )
    };
    r.is_ok() && v != 0
}

impl App {
    fn taskbar_layout(&self) -> Option<(HWND, i32, i32, i32, i32)> {
        let (tb, r) = taskbar()?;
        let (tw, th) = (r.right - r.left, r.bottom - r.top);
        if th > tw {
            return None; // 竖直任务栏不显示
        }
        let s = self.player.core.lock().settings.taskbar_lyric.clone();
        let w = s.width.clamp(120, (tw / 2).max(120));
        let h = th.min(64);
        let base = match s.align {
            0 => 60,
            1 => (tw - w) / 2,
            _ => tray_left(tb).unwrap_or(tw - 200) - w,
        };
        let x = (base + s.offset).clamp(0, (tw - w).max(0));
        Some((tb, x, (th - h) / 2, w, h))
    }

    /// 根据设置创建 / 移除任务栏歌词
    pub fn sync_taskbar_lyric(&mut self) {
        let visible = self.player.core.lock().settings.taskbar_lyric.visible;
        let layout = if visible { self.taskbar_layout() } else { None };
        let Some((tb, x, y, w, h)) = layout else {
            if let Some(t) = self.taskbar.take() {
                unsafe {
                    let _ = DestroyWindow(t.hwnd);
                }
            }
            return;
        };
        if self.taskbar.as_ref().is_some_and(|t| t.parent != tb || !unsafe { IsWindow(Some(t.hwnd)).as_bool() }) {
            self.taskbar = None;
        }
        if self.taskbar.is_none() {
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    CLASS,
                    PCWSTR::null(),
                    WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                    x,
                    y,
                    w,
                    h,
                    Some(tb),
                    None,
                    GetModuleHandleW(None).ok().map(|h| h.into()),
                    None,
                )
            };
            let Ok(hwnd) = hwnd else {
                log::warn!("无法嵌入任务栏");
                return;
            };
            self.taskbar = Some(TaskbarLyric { hwnd, parent: tb, canvas: Canvas::new(w, h), surface: Surface::new(w, h), pos: (x, y), drag: None, last: LineState::default(), light: light_taskbar(), ticks: 0 });
        }
        let t = self.taskbar.as_mut().unwrap();
        if t.canvas.w != w || t.canvas.h != h {
            t.canvas = Canvas::new(w, h);
        }
        if t.pos != (x, y) || t.drag.is_none() {
            t.pos = (x, y);
            unsafe {
                let _ = SetWindowPos(t.hwnd, Some(HWND_TOP), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }
        }
        self.paint_taskbar();
    }

    pub fn tick_taskbar(&mut self) {
        if self.taskbar.is_none() {
            return;
        }
        let st = self.line_state();
        let t = self.taskbar.as_mut().unwrap();
        t.ticks += 1;
        // 每 2 秒检查一次任务栏位置（通知区域图标增减、分辨率变化、主题切换）
        if t.ticks % 66 == 0 {
            t.light = light_taskbar();
            return self.sync_taskbar_lyric();
        }
        if st.idx != t.last.idx || st.text != t.last.text || (st.progress - t.last.progress).abs() > 0.01 {
            t.last = st;
            self.paint_taskbar();
        }
    }

    fn paint_taskbar(&mut self) {
        let size = self.player.core.lock().settings.taskbar_lyric.font_size;
        let Some(t) = self.taskbar.as_mut() else { return };
        let c = &mut t.canvas;
        c.clear();
        let (w, h) = (c.w, c.h);
        // 整个区域可接收鼠标（拖动调整位置），但看不见
        c.hit_margin(R::new(0, 0, w, h));
        let (fg, dim) = if t.light { (Rgba::gray(20), Rgba::gray(90)) } else { (Rgba::gray(250), Rgba::gray(170)) };
        let played = Rgba(234, 115, 43, 255);
        let f1 = Font::px(size, false);
        let f2 = Font::px((size - 3.0).max(9.0), false);
        let two = !t.last.next.is_empty() && h >= 36;
        let (tw, th) = text::measure(&t.last.text, f1);
        let y1 = if two { (h / 2 - th as i32 + 2).max(0) } else { (h - th as i32) / 2 };
        let x1 = ((w - tw as i32) / 2).max(4);
        c.push_clip(R::new(0, 0, w, h));
        let split = x1 + (tw * t.last.progress) as i32;
        c.push_clip(R::new(0, 0, split, h));
        c.text(&t.last.text, f1, x1, y1, played, Hint::Gray);
        c.pop_clip();
        c.push_clip(R::new(split, 0, w, h));
        c.text(&t.last.text, f1, x1, y1, fg, Hint::Gray);
        c.pop_clip();
        if two {
            let (nw, _) = text::measure(&t.last.next, f2);
            c.text(&t.last.next, f2, ((w - nw as i32) / 2).max(4), h / 2 + 2, dim, Hint::Gray);
        }
        c.pop_clip();
        t.surface.present_at(t.hwnd, &t.canvas, None);
    }

    pub fn taskbar_menu(&self) -> Vec<Item> {
        let s = self.player.core.lock().settings.taskbar_lyric.clone();
        vec![
            check("靠左", None, s.align == 0, Act::TaskbarAlign(0)),
            check("居中", None, s.align == 1, Act::TaskbarAlign(1)),
            check("靠近通知区域", None, s.align == 2, Act::TaskbarAlign(2)),
            Item::Sep,
            item("字号增大", Act::TaskbarFont(1.0)),
            item("字号减小", Act::TaskbarFont(-1.0)),
            Item::Sep,
            item("关闭任务栏歌词", Act::TaskbarToggle),
        ]
    }
}
impl App {
    pub fn taskbar_msg(&mut self, hwnd: HWND, msg: u32, _wp: WPARAM, _lp: LPARAM) -> Option<LRESULT> {
        if self.taskbar.as_ref()?.hwnd != hwnd {
            return None;
        }
        match msg {
            WM_LBUTTONDOWN => {
                // 按住拖动：调整水平偏移
                let (cx, _) = super::resize::cursor();
                let off = self.player.core.lock().settings.taskbar_lyric.offset;
                self.taskbar.as_mut()?.drag = Some((cx, off));
                unsafe { SetCapture(hwnd) };
            }
            WM_MOUSEMOVE => {
                if let Some((x0, off0)) = self.taskbar.as_ref()?.drag {
                    let (cx, _) = super::resize::cursor();
                    {
                        let mut g = self.player.core.lock();
                        g.settings.taskbar_lyric.offset = off0 + (cx - x0);
                    }
                    if let Some((_, x, y, w, h)) = self.taskbar_layout() {
                        let t = self.taskbar.as_mut()?;
                        t.pos = (x, y);
                        unsafe {
                            let _ = SetWindowPos(t.hwnd, Some(HWND_TOP), x, y, w, h, SWP_NOACTIVATE);
                        }
                    }
                }
            }
            WM_LBUTTONUP => {
                if self.taskbar.as_mut()?.drag.take().is_some() {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    self.player.core.lock().touch_settings();
                    self.player.save(false);
                }
            }
            WM_RBUTTONUP => self.act(Act::TaskbarMenu),
            WM_LBUTTONDBLCLK => self.restore(),
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            _ => return None,
        }
        Some(LRESULT(0))
    }
}

/// 资源管理器重启时广播的消息（需要重新加托盘图标、重新嵌入任务栏）
pub fn taskbar_created_msg() -> u32 {
    static M: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *M.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}
