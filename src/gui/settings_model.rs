//! 「配置」窗口的数据模型：每个分页有哪些设置项、如何读写（与原版的分页与文字一致）。

use crate::config::{HotkeyAction, ProxyKind, Settings};

pub const TABS: [&str; 6] = ["基本设置", "音频设备", "歌词图片", "网络连接", "全局热键", "文件关联"];

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
    UseCustomDir,
    ProxyEnabled,
    HotkeysEnabled,
    MediaKeys,
    DeskLyricLocked,
    TaskbarLyric,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    CustomDir,
    ProxyHost,
    ProxyPort,
    ProxyUser,
    ProxyPass,
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
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    BrowseLyricDir,
    OpenConfigDir,
    ResetHotkeys,
    Associate,
    Unassociate,
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
        Key::UseCustomDir => s.lyrics.custom_dir.is_some(),
        Key::ProxyEnabled => s.network.proxy_enabled,
        Key::HotkeysEnabled => s.hotkeys.enabled,
        Key::MediaKeys => s.hotkeys.media_keys,
        Key::DeskLyricLocked => s.desk_lyric.locked,
        Key::TaskbarLyric => s.taskbar_lyric.visible,
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
        Key::UseCustomDir => s.lyrics.custom_dir = if v { Some(s.lyrics.custom_dir.clone().unwrap_or_default()) } else { None },
        Key::ProxyEnabled => s.network.proxy_enabled = v,
        Key::HotkeysEnabled => s.hotkeys.enabled = v,
        Key::MediaKeys => s.hotkeys.media_keys = v,
        Key::DeskLyricLocked => s.desk_lyric.locked = v,
        Key::TaskbarLyric => s.taskbar_lyric.visible = v,
    }
}

pub fn text(s: &Settings, f: Field) -> String {
    match f {
        Field::CustomDir => s.lyrics.custom_dir.clone().unwrap_or_default(),
        Field::ProxyHost => s.network.host.clone(),
        Field::ProxyPort => if s.network.port == 0 { String::new() } else { s.network.port.to_string() },
        Field::ProxyUser => s.network.user.clone(),
        Field::ProxyPass => "*".repeat(s.network.pass.chars().count()),
    }
}

pub fn set_text(s: &mut Settings, f: Field, v: &str) {
    match f {
        Field::CustomDir => s.lyrics.custom_dir = Some(v.to_string()),
        Field::ProxyHost => s.network.host = v.trim().to_string(),
        Field::ProxyPort => s.network.port = v.trim().parse().unwrap_or(0),
        Field::ProxyUser => s.network.user = v.to_string(),
        Field::ProxyPass => s.network.pass = v.to_string(),
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
            Check(Key::AutoDownload, "启用自动下载歌词 (LRCLIB)"),
            Check(Key::TaskbarLyric, "在任务栏显示歌词"),
            Check(Key::UseCustomDir, "使用自定义歌词目录 (文件名格式 = \"艺人 - 曲目.lrc\")"),
            Text(Field::CustomDir, "歌词目录"),
            Button("浏览…", Btn::BrowseLyricDir),
        ],
        3 => {
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
        4 => {
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
