//! 「配置」窗口的数据模型：各个开关、输入框如何读写设置（分页的布局见 `settings_layout`）。

use crate::config::{ProxyKind, Settings};

/// 分页（与原版相同的顺序；原版的「音乐分享」是 MSN Messenger 签名，早已停用，换成「桌面歌词」）
pub const TABS: [&str; 8] = ["基本设置", "音频设备", "网络连接", "歌词图片", "桌面歌词", "文件关联", "全局热键", "颜色透明"];
pub const TAB_GENERAL: usize = 0;
pub const TAB_AUDIO: usize = 1;
pub const TAB_NETWORK: usize = 2;
pub const TAB_LYRICS: usize = 3;
/// 桌面歌词 + 任务栏歌词（原版没有这两个功能）
pub const TAB_DESK: usize = 4;
pub const TAB_ASSOC: usize = 5;
pub const TAB_HOTKEYS: usize = 6;
/// 整体颜色、焦点颜色、边框透明、透明雾化
pub const TAB_COLOR: usize = 7;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Topmost,
    MinToTray,
    CloseToTray,
    Autostart,
    ResumeLast,
    Autoplay,
    PlayAfterDrop,
    /// 添加文件夹时忽略压缩文件（本来就不读压缩文件，总是勾着）
    IgnoreArchives,
    Exclusive,
    ShowLyric,
    ShowCover,
    EmbeddedPic,
    LocalPic,
    AutoDownload,
    DownloadLyric,
    AlbumPic,
    ArtistPic,
    AutoTag,
    UseCustomDir,
    /// 不使用代理服务器 / 使用自定义的代理服务器（二选一）
    ProxyOff,
    ProxyEnabled,
    HotkeysEnabled,
    MediaKeys,
    MediaPlay,
    MediaPrev,
    MediaNext,
    MediaMute,
    MediaVolDown,
    MediaVolUp,
    GlobalWheel,
    DeskLyric,
    DeskLyricLocked,
    TaskbarLyric,
    TaskbarTwoLines,
    Frost,
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
    /// 桌面歌词字号
    DeskFont,
    /// 自建 LrcAPI 的地址、访问密钥
    LrcApiUrl,
    LrcApiKey,
}

impl Field {
    /// 数字输入框：输入过程中（如只打了 "-"）解析不了时先不改设置
    pub fn numeric(self) -> bool {
        matches!(self, Field::ProxyPort | Field::TaskbarX | Field::TaskbarY | Field::TaskbarFont | Field::TaskbarWidth | Field::DeskFont)
    }

    /// 显示为 * 的输入框（密码、密钥）
    pub fn secret(self) -> bool {
        matches!(self, Field::ProxyPass | Field::LrcApiKey)
    }

    /// 代理服务器的设置（只在使用自定义的代理服务器时能改）
    pub fn proxy(self) -> bool {
        matches!(self, Field::ProxyHost | Field::ProxyPort | Field::ProxyUser | Field::ProxyPass)
    }

    /// 改了以后要重新摆放任务栏歌词
    pub fn taskbar(self) -> bool {
        matches!(self, Field::TaskbarX | Field::TaskbarY | Field::TaskbarFont | Field::TaskbarWidth)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Swatch {
    Body,
    Focus,
}

impl Swatch {
    pub fn presets(self) -> &'static [[u8; 3]] {
        match self {
            Swatch::Body => &crate::gui::theme::BODY_PRESETS,
            Swatch::Focus => &crate::gui::theme::FOCUS_PRESETS,
        }
    }

    pub fn rows(self) -> i32 {
        (self.presets().len() / 16) as i32
    }
}

pub fn get(s: &Settings, k: Key) -> bool {
    let m = &s.hotkeys.media;
    match k {
        Key::Topmost => s.general.topmost,
        Key::MinToTray => s.general.minimize_to_tray,
        Key::CloseToTray => s.general.close_to_tray,
        Key::Autostart => s.general.autostart,
        Key::ResumeLast => s.playback.resume_last,
        Key::Autoplay => s.playback.autoplay,
        Key::PlayAfterDrop => s.general.play_after_drop,
        Key::IgnoreArchives => true,
        Key::Exclusive => s.audio.exclusive,
        Key::ShowLyric => s.lyrics.show_lyric,
        Key::ShowCover => s.lyrics.show_cover,
        Key::EmbeddedPic => s.lyrics.embedded_pic,
        Key::LocalPic => s.lyrics.local_pic,
        Key::AutoDownload => s.lyrics.auto_download,
        Key::DownloadLyric => s.lyrics.download_lyric,
        Key::AlbumPic => s.lyrics.album_pic,
        Key::ArtistPic => s.lyrics.artist_pic,
        Key::AutoTag => s.lyrics.auto_tag,
        Key::UseCustomDir => s.lyrics.custom_dir.is_some(),
        Key::ProxyOff => !s.network.proxy_enabled,
        Key::ProxyEnabled => s.network.proxy_enabled,
        Key::HotkeysEnabled => s.hotkeys.enabled,
        Key::MediaKeys => s.hotkeys.media_keys,
        Key::MediaPlay => m.play,
        Key::MediaPrev => m.prev,
        Key::MediaNext => m.next,
        Key::MediaMute => m.mute,
        Key::MediaVolDown => m.vol_down,
        Key::MediaVolUp => m.vol_up,
        Key::GlobalWheel => s.general.global_wheel,
        Key::DeskLyric => s.desk_lyric.visible,
        Key::DeskLyricLocked => s.desk_lyric.locked,
        Key::TaskbarLyric => s.taskbar_lyric.visible,
        Key::TaskbarTwoLines => s.taskbar_lyric.two_lines,
        Key::Frost => s.theme.frost,
    }
}

pub fn set(s: &mut Settings, k: Key, v: bool) {
    let m = &mut s.hotkeys.media;
    match k {
        Key::Topmost => s.general.topmost = v,
        Key::MinToTray => s.general.minimize_to_tray = v,
        Key::CloseToTray => s.general.close_to_tray = v,
        Key::Autostart => s.general.autostart = v,
        Key::ResumeLast => s.playback.resume_last = v,
        Key::Autoplay => s.playback.autoplay = v,
        Key::PlayAfterDrop => s.general.play_after_drop = v,
        Key::IgnoreArchives => {}
        Key::Exclusive => s.audio.exclusive = v,
        Key::ShowLyric => s.lyrics.show_lyric = v,
        Key::ShowCover => s.lyrics.show_cover = v,
        Key::EmbeddedPic => s.lyrics.embedded_pic = v,
        Key::LocalPic => s.lyrics.local_pic = v,
        Key::AutoDownload => s.lyrics.auto_download = v,
        Key::DownloadLyric => s.lyrics.download_lyric = v,
        Key::AlbumPic => s.lyrics.album_pic = v,
        Key::ArtistPic => s.lyrics.artist_pic = v,
        Key::AutoTag => s.lyrics.auto_tag = v,
        Key::UseCustomDir => s.lyrics.custom_dir = if v { Some(s.lyrics.custom_dir.clone().unwrap_or_default()) } else { None },
        // 二选一：点哪个就选哪个
        Key::ProxyOff => s.network.proxy_enabled = false,
        Key::ProxyEnabled => s.network.proxy_enabled = true,
        Key::HotkeysEnabled => s.hotkeys.enabled = v,
        Key::MediaKeys => s.hotkeys.media_keys = v,
        Key::MediaPlay => m.play = v,
        Key::MediaPrev => m.prev = v,
        Key::MediaNext => m.next = v,
        Key::MediaMute => m.mute = v,
        Key::MediaVolDown => m.vol_down = v,
        Key::MediaVolUp => m.vol_up = v,
        Key::GlobalWheel => s.general.global_wheel = v,
        Key::DeskLyric => s.desk_lyric.visible = v,
        Key::DeskLyricLocked => s.desk_lyric.locked = v,
        Key::TaskbarLyric => s.taskbar_lyric.visible = v,
        Key::TaskbarTwoLines => s.taskbar_lyric.two_lines = v,
        Key::Frost => s.theme.frost = v,
    }
}

/// 这个开关现在能不能改（灰掉的不能）：依赖的上一级开关关着时
pub fn enabled(s: &Settings, k: Key) -> bool {
    match k {
        Key::ResumeLast => s.playback.autoplay,
        Key::DownloadLyric | Key::AlbumPic | Key::ArtistPic | Key::AutoTag => s.lyrics.auto_download,
        Key::MediaPlay | Key::MediaPrev | Key::MediaNext | Key::MediaMute | Key::MediaVolDown | Key::MediaVolUp => s.hotkeys.media_keys,
        _ => true,
    }
}

/// 输入框里显示的文字（密码显示为 *）
pub fn text(s: &Settings, f: Field) -> String {
    match f {
        Field::ProxyPass | Field::LrcApiKey => "*".repeat(raw_text(s, f).chars().count()),
        // 与原版一样显示 0
        Field::ProxyPort if s.network.port == 0 => "0".into(),
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
        Field::DeskFont => format!("{}", s.desk_lyric.font_size),
        Field::LrcApiUrl => s.network.lrcapi_url.clone(),
        Field::LrcApiKey => s.network.lrcapi_key.clone(),
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
        Field::DeskFont => match v.trim().parse::<f32>() {
            Ok(n) if n.is_finite() => {
                let d = &mut s.desk_lyric;
                // 改字号时行数不变（窗口高度随之变化）
                d.lines = d.line_count();
                d.font_size = n.clamp(16.0, 72.0);
            }
            _ => return false,
        },
        Field::LrcApiUrl => s.network.lrcapi_url = v.trim().to_string(),
        Field::LrcApiKey => s.network.lrcapi_key = v.trim().to_string(),
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
}
