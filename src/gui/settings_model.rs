//! 「配置」窗口的数据模型：每个分页有哪些设置项、如何读写（与原版的分页与文字一致）。

use crate::config::{HotkeyAction, ProxyKind, Settings};

pub const TABS: [&str; 7] = ["基本设置", "音频设备", "歌词图片", "桌面歌词", "网络连接", "全局热键", "文件关联"];
/// 「桌面歌词」分页（桌面歌词 + 任务栏歌词；原版没有这两个功能）
pub const TAB_DESK: usize = 3;
/// 「全局热键」分页（行更密）
pub const TAB_HOTKEYS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Topmost,
    MinToTray,
    CloseToTray,
    Autostart,
    ResumeLast,
    Autoplay,
    PlayAfterDrop,
    Exclusive,
    ShowLyric,
    ShowCover,
    EmbeddedPic,
    LocalPic,
    AutoDownload,
    AlbumPic,
    UseCustomDir,
    ProxyEnabled,
    HotkeysEnabled,
    MediaKeys,
    DeskLyric,
    DeskLyricLocked,
    TaskbarLyric,
    TaskbarTwoLines,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    CustomDir,
    ProxyHost,
    ProxyPort,
    ProxyUser,
    ProxyPass,
    /// 任务栏歌词：水平偏移、垂直偏移、字号、宽度（数字）
    TaskbarX,
    TaskbarY,
    TaskbarFont,
    TaskbarWidth,
}

impl Field {
    /// 数字输入框：输入过程中（如只打了 "-"）解析不了时先不改设置
    pub fn numeric(self) -> bool {
        matches!(self, Field::ProxyPort | Field::TaskbarX | Field::TaskbarY | Field::TaskbarFont | Field::TaskbarWidth)
    }

    /// 改了以后要重新摆放任务栏歌词
    pub fn taskbar(self) -> bool {
        matches!(self, Field::TaskbarX | Field::TaskbarY | Field::TaskbarFont | Field::TaskbarWidth)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Widget {
    Label(String),
    Check(Key, &'static str),
    /// 下拉选择：标签、当前显示文字、动作
    Choice(&'static str, String, ChoiceKind),
    Text(Field, &'static str),
    Hotkey(HotkeyAction),
    Button(&'static str, Btn),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChoiceKind {
    Device,
    ProxyKind,
    LyricSource,
    TaskbarAlign,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    BrowseLyricDir,
    OpenConfigDir,
    ResetHotkeys,
    Associate,
    Unassociate,
    ResetTaskbarPos,
}

pub fn get(s: &Settings, k: Key) -> bool {
    match k {
        Key::Topmost => s.general.topmost,
        Key::MinToTray => s.general.minimize_to_tray,
        Key::CloseToTray => s.general.close_to_tray,
        Key::Autostart => s.general.autostart,
        Key::ResumeLast => s.playback.resume_last,
        Key::Autoplay => s.playback.autoplay,
        Key::PlayAfterDrop => s.general.play_after_drop,
        Key::Exclusive => s.audio.exclusive,
        Key::ShowLyric => s.lyrics.show_lyric,
        Key::ShowCover => s.lyrics.show_cover,
        Key::EmbeddedPic => s.lyrics.embedded_pic,
        Key::LocalPic => s.lyrics.local_pic,
        Key::AutoDownload => s.lyrics.auto_download,
        Key::AlbumPic => s.lyrics.album_pic,
        Key::UseCustomDir => s.lyrics.custom_dir.is_some(),
        Key::ProxyEnabled => s.network.proxy_enabled,
        Key::HotkeysEnabled => s.hotkeys.enabled,
        Key::MediaKeys => s.hotkeys.media_keys,
        Key::DeskLyric => s.desk_lyric.visible,
        Key::DeskLyricLocked => s.desk_lyric.locked,
        Key::TaskbarLyric => s.taskbar_lyric.visible,
        Key::TaskbarTwoLines => s.taskbar_lyric.two_lines,
    }
}

pub fn set(s: &mut Settings, k: Key, v: bool) {
    match k {
        Key::Topmost => s.general.topmost = v,
        Key::MinToTray => s.general.minimize_to_tray = v,
        Key::CloseToTray => s.general.close_to_tray = v,
        Key::Autostart => s.general.autostart = v,
        Key::ResumeLast => s.playback.resume_last = v,
        Key::Autoplay => s.playback.autoplay = v,
        Key::PlayAfterDrop => s.general.play_after_drop = v,
        Key::Exclusive => s.audio.exclusive = v,
        Key::ShowLyric => s.lyrics.show_lyric = v,
        Key::ShowCover => s.lyrics.show_cover = v,
        Key::EmbeddedPic => s.lyrics.embedded_pic = v,
        Key::LocalPic => s.lyrics.local_pic = v,
        Key::AutoDownload => s.lyrics.auto_download = v,
        Key::AlbumPic => s.lyrics.album_pic = v,
        Key::UseCustomDir => s.lyrics.custom_dir = if v { Some(s.lyrics.custom_dir.clone().unwrap_or_default()) } else { None },
        Key::ProxyEnabled => s.network.proxy_enabled = v,
        Key::HotkeysEnabled => s.hotkeys.enabled = v,
        Key::MediaKeys => s.hotkeys.media_keys = v,
        Key::DeskLyric => s.desk_lyric.visible = v,
        Key::DeskLyricLocked => s.desk_lyric.locked = v,
        Key::TaskbarLyric => s.taskbar_lyric.visible = v,
        Key::TaskbarTwoLines => s.taskbar_lyric.two_lines = v,
    }
}

/// 输入框里显示的文字（密码显示为 *）
pub fn text(s: &Settings, f: Field) -> String {
    match f {
        Field::ProxyPass => "*".repeat(s.network.pass.chars().count()),
        _ => raw_text(s, f),
    }
}

/// 输入框的实际内容（开始编辑时从这里取）
pub fn raw_text(s: &Settings, f: Field) -> String {
    let t = &s.taskbar_lyric;
    match f {
        Field::CustomDir => s.lyrics.custom_dir.clone().unwrap_or_default(),
        Field::ProxyHost => s.network.host.clone(),
        Field::ProxyPort => if s.network.port == 0 { String::new() } else { s.network.port.to_string() },
        Field::ProxyUser => s.network.user.clone(),
        Field::ProxyPass => s.network.pass.clone(),
        Field::TaskbarX => t.offset.to_string(),
        Field::TaskbarY => t.offset_y.to_string(),
        Field::TaskbarFont => format!("{}", t.font_size),
        Field::TaskbarWidth => t.width.to_string(),
    }
}

/// 写入输入框内容；数字解析不了（输入到一半）时不改设置，返回 false。数字超出范围时取边界值。
pub fn set_text(s: &mut Settings, f: Field, v: &str) -> bool {
    let num = |lo: i32, hi: i32| v.trim().parse::<i32>().ok().map(|n| n.clamp(lo, hi));
    let t = &mut s.taskbar_lyric;
    match f {
        Field::CustomDir => s.lyrics.custom_dir = Some(v.to_string()),
        Field::ProxyHost => s.network.host = v.trim().to_string(),
        Field::ProxyPort => match v.trim() {
            "" => s.network.port = 0,
            p => match p.parse() {
                Ok(n) => s.network.port = n,
                Err(_) => return false,
            },
        },
        Field::ProxyUser => s.network.user = v.to_string(),
        Field::ProxyPass => s.network.pass = v.to_string(),
        Field::TaskbarX => match num(-3000, 3000) {
            Some(n) => t.offset = n,
            None => return false,
        },
        Field::TaskbarY => match num(-40, 40) {
            Some(n) => t.offset_y = n,
            None => return false,
        },
        Field::TaskbarFont => match v.trim().parse::<f32>() {
            Ok(n) if n.is_finite() => t.font_size = n.clamp(9.0, 24.0),
            _ => return false,
        },
        Field::TaskbarWidth => match num(120, 2000) {
            Some(n) => t.width = n,
            None => return false,
        },
    }
    true
}

/// 任务栏歌词的位置（对齐方式）
pub fn taskbar_align_label(align: u8) -> &'static str {
    match align {
        0 => "靠左",
        1 => "居中",
        _ => "靠右（通知区域左侧）",
    }
}

/// 某个分页的设置项（自上而下排列）
pub fn widgets(tab: usize, s: &Settings, device_name: &str) -> Vec<Widget> {
    use Widget::*;
    match tab {
        0 => vec![
            Check(Key::Topmost, "窗体始终置顶"),
            Check(Key::MinToTray, "最小化到托盘 (不显示任务栏窗体)"),
            Check(Key::CloseToTray, "点击关闭按钮时缩到托盘"),
            Check(Key::Autostart, "开机时自动启动"),
            Check(Key::Autoplay, "启动时自动播放"),
            Check(Key::ResumeLast, "从上次中断位置继续"),
            Check(Key::PlayAfterDrop, "拖拽添加文件(夹)到\"音乐列表\"后立即播放"),
            Button("打开配置文件夹", Btn::OpenConfigDir),
        ],
        1 => vec![
            Choice("输出设备", device_name.to_string(), ChoiceKind::Device),
            Check(Key::Exclusive, "使用独占模式 (WASAPI Exclusive)"),
            Label("如果设备启动失败或工作异常，请尝试切换共享和独占模式".into()),
        ],
        2 => vec![
            Check(Key::ShowLyric, "显示歌词"),
            Check(Key::ShowCover, "显示图片"),
            Check(Key::EmbeddedPic, "显示音乐文件内嵌图片"),
            Check(Key::LocalPic, "显示本地默认图片"),
            Choice("歌词来源", s.lyrics.source.label().into(), ChoiceKind::LyricSource),
            Check(Key::AutoDownload, "启用自动下载歌词"),
            Check(Key::AlbumPic, "下载专辑图片 (LrcAPI / iTunes)"),
            Check(Key::UseCustomDir, "使用自定义歌词目录 (文件名格式 = \"艺人 - 曲目.lrc\")"),
            Text(Field::CustomDir, "歌词目录"),
            Button("浏览…", Btn::BrowseLyricDir),
        ],
        TAB_DESK => vec![
            Check(Key::DeskLyric, "显示桌面歌词"),
            Check(Key::DeskLyricLocked, "锁定桌面歌词 (鼠标穿透)"),
            Check(Key::TaskbarLyric, "在任务栏显示歌词"),
            Check(Key::TaskbarTwoLines, "任务栏歌词第二行显示下一句"),
            Choice("任务栏歌词位置", taskbar_align_label(s.taskbar_lyric.align).into(), ChoiceKind::TaskbarAlign),
            Text(Field::TaskbarX, "水平偏移 (像素)"),
            Text(Field::TaskbarY, "垂直偏移 (像素)"),
            Text(Field::TaskbarFont, "字号"),
            Text(Field::TaskbarWidth, "宽度 (像素)"),
            Label("偏移：正数向右 / 向下。也可以直接在任务栏上按住歌词拖动".into()),
            Button("恢复默认位置", Btn::ResetTaskbarPos),
        ],
        4 => {
            let kind = s.network.kind;
            vec![
                Check(Key::ProxyEnabled, "使用自定义的代理服务器"),
                Choice("服务器类型", proxy_label(kind).into(), ChoiceKind::ProxyKind),
                Text(Field::ProxyHost, "服务器地址"),
                Text(Field::ProxyPort, "端口"),
                Text(Field::ProxyUser, "用户名"),
                Text(Field::ProxyPass, "密码"),
                Label("未启用时使用系统代理".into()),
            ]
        }
        TAB_HOTKEYS => {
            let mut v = vec![Check(Key::HotkeysEnabled, "启用全局键盘热键支持"), Check(Key::MediaKeys, "启用全局多媒体键盘支持")];
            v.extend(HotkeyAction::ALL.iter().map(|a| Hotkey(*a)));
            v.push(Button("重置热键", Btn::ResetHotkeys));
            v
        }
        _ => vec![
            Label("把常见音乐格式关联到 AirPlay3（只写入当前用户的注册表）".into()),
            Button("关联音乐文件", Btn::Associate),
            Button("取消关联", Btn::Unassociate),
        ],
    }
}

pub fn proxy_label(k: ProxyKind) -> &'static str {
    match k {
        ProxyKind::Http => "HTTP",
        ProxyKind::Socks4 => "SOCKS4",
        ProxyKind::Socks5 => "SOCKS5",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_fields() {
        let mut s = Settings::default();
        // 输入到一半（只有负号）时不改设置
        assert!(!set_text(&mut s, Field::TaskbarX, "-"));
        assert_eq!(s.taskbar_lyric.offset, -20);
        assert!(set_text(&mut s, Field::TaskbarX, "-35"));
        assert_eq!(raw_text(&s, Field::TaskbarX), "-35");
        // 超出范围取边界值
        assert!(set_text(&mut s, Field::TaskbarY, "999"));
        assert_eq!(s.taskbar_lyric.offset_y, 40);
        assert!(set_text(&mut s, Field::TaskbarFont, "15.5"));
        assert_eq!(s.taskbar_lyric.font_size, 15.5);
        assert!(set_text(&mut s, Field::TaskbarWidth, "50"));
        assert_eq!(s.taskbar_lyric.width, 120);
        assert!(!set_text(&mut s, Field::ProxyPort, "80a"));
        assert!(set_text(&mut s, Field::ProxyPort, "8080"));
        assert!(set_text(&mut s, Field::ProxyPort, ""));
        assert_eq!(s.network.port, 0);
        // 密码显示为 *，编辑时取原文
        s.network.pass = "abc".into();
        assert_eq!((text(&s, Field::ProxyPass), raw_text(&s, Field::ProxyPass)), ("***".into(), "abc".into()));
    }

    /// 每个分页的设置项都要放得进窗口（最后一行在状态文字上方）
    #[test]
    fn every_tab_fits() {
        use crate::gui::panels::settings::{H, row_rect};
        let s = Settings::default();
        for tab in 0..TABS.len() {
            let n = widgets(tab, &s, "设备").len();
            assert!(row_rect(tab, n - 1).bottom() <= H - 30, "{} 放不下 {n} 项", TABS[tab]);
        }
    }
}
