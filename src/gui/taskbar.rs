//! 任务栏歌词：作为子窗口嵌入任务栏（Shell_TrayWnd），显示当前与下一句歌词。
//! 位置 = 对齐方式（靠左 / 居中 / 靠右）+ 水平、垂直偏移（可直接拖动，也可在「配置 → 桌面歌词」里填数字）；
//! 资源管理器重启后自动重建。

use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use super::app::App;
use super::canvas::{Canvas, R, Rgba};
use super::desklyric::LineState;
use super::menu::{Act, Item, check, item};
use super::settings_model::{TAB_DESK, taskbar_align_label};
use super::text::{self, Font, Hint};
use super::window::{CLASS, Surface};

pub struct TaskbarLyric {
    pub hwnd: HWND,
    pub parent: HWND,
    canvas: Canvas,
    surface: Surface,
    /// 在任务栏客户区中的位置
    pub pos: (i32, i32),
    /// 拖动：按下时的光标位置与 (水平, 垂直) 偏移
    drag: Option<((i32, i32), (i32, i32))>,
    last: LineState,
    light: bool,
    ticks: u32,
    /// 当前字号下垂直偏移的可用范围（上次绘制时算出）
    y_range: (i32, i32),
}

/// 任务栏歌词在任务栏里的布局
struct Layout {
    tb: HWND,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    /// 水平偏移的可用范围（超出就贴边，不再移动）
    x_range: (i32, i32),
}

/// 文字块的顶部：在高 h 的区域里垂直居中后再加偏移，不超出区域
fn text_top(h: i32, block: i32, offset_y: i32) -> i32 {
    let max = (h - block).max(0);
    (max / 2 + offset_y).clamp(0, max)
}

/// 垂直偏移的可用范围（与 `text_top` 一致）
fn y_range(h: i32, block: i32) -> (i32, i32) {
    let max = (h - block).max(0);
    (-(max / 2), max - max / 2)
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
    fn taskbar_layout(&self) -> Option<Layout> {
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
        let x_range = (-base, (tw - w).max(0) - base);
        let x = base + s.offset.clamp(x_range.0, x_range.1.max(x_range.0));
        Some(Layout { tb, x, y: (th - h) / 2, w, h, x_range })
    }

    /// 根据设置创建 / 移除任务栏歌词
    pub fn sync_taskbar_lyric(&mut self) {
        let visible = self.player.core.lock().settings.taskbar_lyric.visible;
        let layout = if visible { self.taskbar_layout() } else { None };
        let Some(Layout { tb, x, y, w, h, .. }) = layout else {
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
            // 任务栏歌词不跟随界面缩放（字号单独调）：一直按 100% 画
            self.taskbar = Some(TaskbarLyric {
                hwnd,
                parent: tb,
                canvas: Canvas::with_scale(w, h, 1.0),
                surface: Surface::new(w, h),
                pos: (x, y),
                drag: None,
                last: LineState::default(),
                light: light_taskbar(),
                ticks: 0,
                y_range: (0, 0),
            });
        }
        let t = self.taskbar.as_mut().unwrap();
        if t.canvas.w != w || t.canvas.h != h {
            t.canvas = Canvas::with_scale(w, h, 1.0);
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
        if st.idx != t.last.idx || st.text != t.last.text || st.caption != t.last.caption || (st.progress - t.last.progress).abs() > 0.01 {
            t.last = st;
            self.paint_taskbar();
        }
    }

    fn paint_taskbar(&mut self) {
        let (size, offset_y, two_lines) = {
            let g = self.player.core.lock();
            let s = &g.settings.taskbar_lyric;
            (s.font_size, s.offset_y, s.two_lines)
        };
        let notice = self.ui.caption.notice;
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
        // 电台字幕：只显示一行，放不下时显示最后面（最新的字）
        let caption = t.last.caption != 0 && !notice;
        let line = if caption { super::captions::tail_fit(&t.last.text, f1, (w - 8) as f32) } else { t.last.text.as_str() };
        let two = two_lines && !t.last.next.is_empty() && h >= 36;
        let (tw, th) = text::measure(line, f1);
        let th2 = if two { text::measure(&t.last.next, f2).1.max(1.0) } else { 0.0 };
        // 两行之间留 2 像素；整块文字在任务栏高度内居中，再按垂直偏移上下移动
        let block = th as i32 + if two { th2 as i32 + 2 } else { 0 };
        t.y_range = y_range(h, block);
        let y1 = text_top(h, block, offset_y);
        // 字幕靠左（居中的话每来一个字整行都要挪）
        let x1 = if caption { 4 } else { ((w - tw as i32) / 2).max(4) };
        c.push_clip(R::new(0, 0, w, h));
        let split = x1 + (tw * t.last.progress) as i32;
        c.push_clip(R::new(0, 0, split, h));
        c.text(line, f1, x1, y1, played, Hint::Gray);
        c.pop_clip();
        c.push_clip(R::new(split, 0, w, h));
        c.text(line, f1, x1, y1, fg, Hint::Gray);
        c.pop_clip();
        if two {
            let (nw, _) = text::measure(&t.last.next, f2);
            c.text(&t.last.next, f2, ((w - nw as i32) / 2).max(4), y1 + th as i32 + 2, dim, Hint::Gray);
        }
        c.pop_clip();
        #[cfg(debug_assertions)]
        t.canvas.dump("taskbar");
        t.surface.present_at(t.hwnd, &t.canvas, None);
    }

    pub fn taskbar_menu(&self) -> Vec<Item> {
        let s = self.player.core.lock().settings.taskbar_lyric.clone();
        let mut v: Vec<Item> = (0..3u8).map(|a| check(taskbar_align_label(a), None, s.align == a, Act::TaskbarAlign(a))).collect();
        v.extend([
            Item::Sep,
            check("第二行显示下一句", None, s.two_lines, Act::TaskbarTwoLines),
            item("字号增大", Act::TaskbarFont(1.0)),
            item("字号减小", Act::TaskbarFont(-1.0)),
            Item::Sep,
            item("恢复默认位置", Act::TaskbarReset),
            item("调整位置…", Act::SettingsTab(TAB_DESK)),
            Item::Sep,
            item("关闭任务栏歌词", Act::TaskbarToggle),
        ]);
        v
    }
}
impl App {
    pub fn taskbar_msg(&mut self, hwnd: HWND, msg: u32, _wp: WPARAM, _lp: LPARAM) -> Option<LRESULT> {
        if self.taskbar.as_ref()?.hwnd != hwnd {
            return None;
        }
        match msg {
            WM_LBUTTONDOWN => {
                // 按住拖动：左右移动整块歌词，上下移动文字
                let cur = super::scale::cursor_px();
                let off = {
                    let g = self.player.core.lock();
                    (g.settings.taskbar_lyric.offset, g.settings.taskbar_lyric.offset_y)
                };
                self.taskbar.as_mut()?.drag = Some((cur, off));
                unsafe { SetCapture(hwnd) };
            }
            WM_MOUSEMOVE => {
                let ((x0, y0), (ox, oy)) = self.taskbar.as_ref()?.drag?;
                let (cx, cy) = super::scale::cursor_px();
                // 拖过头时偏移停在边界上，往回拖立刻跟手
                let x_range = self.taskbar_layout().map_or((-3000, 3000), |l| l.x_range);
                let y_range = self.taskbar.as_ref()?.y_range;
                {
                    let mut g = self.player.core.lock();
                    let t = &mut g.settings.taskbar_lyric;
                    t.offset = (ox + cx - x0).clamp(x_range.0, x_range.1.max(x_range.0));
                    t.offset_y = (oy + cy - y0).clamp(y_range.0, y_range.1);
                }
                if let Some(Layout { x, y, w, h, .. }) = self.taskbar_layout() {
                    let t = self.taskbar.as_mut()?;
                    t.pos = (x, y);
                    unsafe {
                        let _ = SetWindowPos(t.hwnd, Some(HWND_TOP), x, y, w, h, SWP_NOACTIVATE);
                    }
                }
                self.paint_taskbar();
                if let Some(s) = self.settings_win.as_mut() {
                    s.win.dirty = true;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_offset_stays_inside() {
        // 40 高的任务栏、20 高的文字块：居中时顶部在 10，可上下各移 10
        assert_eq!(text_top(40, 20, 0), 10);
        assert_eq!(text_top(40, 20, -4), 6);
        assert_eq!(text_top(40, 20, 99), 20);
        assert_eq!(text_top(40, 20, -99), 0);
        assert_eq!(y_range(40, 20), (-10, 10));
        assert_eq!(y_range(41, 20), (-10, 11));
        assert_eq!(text_top(41, 20, 11), 21);
        // 文字比区域高时不能移动
        assert_eq!((text_top(20, 30, 5), y_range(20, 30)), (0, (0, 0)));
    }
}
