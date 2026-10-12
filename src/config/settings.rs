//! 持久化设置。字段对应原版 AirPlay3 的配置项（`track_mode`、`audio_volume`、`wasapi_exclusive`、
//! `list_show_*`、`display_lyric`、`extra_lyric_path`、`proxy` 等），改为 JSON 存储。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayMode {
    #[default]
    Sequential,
    Shuffle,
    RepeatAll,
    RepeatOne,
}

impl PlayMode {
    pub const ALL: [PlayMode; 4] = [PlayMode::Sequential, PlayMode::Shuffle, PlayMode::RepeatAll, PlayMode::RepeatOne];

    pub fn label(self) -> &'static str {
        match self {
            PlayMode::Sequential => "顺序播放",
            PlayMode::Shuffle => "随机播放",
            PlayMode::RepeatAll => "全部循环",
            PlayMode::RepeatOne => "单曲循环",
        }
    }

    pub fn cycle(self) -> Self {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProxyKind {
    #[default]
    Http,
    Socks4,
    Socks5,
}

impl ProxyKind {
    pub const ALL: [ProxyKind; 3] = [ProxyKind::Http, ProxyKind::Socks4, ProxyKind::Socks5];
    pub fn label(self) -> &'static str {
        match self {
            ProxyKind::Http => "HTTP",
            ProxyKind::Socks4 => "SOCKS4",
            ProxyKind::Socks5 => "SOCKS5",
        }
    }
}

/// 全局热键对应的动作（与原版"全局热键"页一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HotkeyAction {
    PlayPause,
    Prev,
    Next,
    VolumeUp,
    VolumeDown,
    Mute,
    Favorite,
    Ban,
    DeskLyric,
    ShowWindow,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 10] = [
        HotkeyAction::PlayPause,
        HotkeyAction::Prev,
        HotkeyAction::Next,
        HotkeyAction::VolumeUp,
        HotkeyAction::VolumeDown,
        HotkeyAction::Mute,
        HotkeyAction::Favorite,
        HotkeyAction::Ban,
        HotkeyAction::DeskLyric,
        HotkeyAction::ShowWindow,
    ];

    pub fn label(self) -> &'static str {
        match self {
            HotkeyAction::PlayPause => "播放 / 暂停",
            HotkeyAction::Prev => "上一首",
            HotkeyAction::Next => "下一首",
            HotkeyAction::VolumeUp => "音量增高",
            HotkeyAction::VolumeDown => "音量降低",
            HotkeyAction::Mute => "静音",
            HotkeyAction::Favorite => "喜爱",
            HotkeyAction::Ban => "不再播放",
            HotkeyAction::DeskLyric => "迷你歌词",
            HotkeyAction::ShowWindow => "显示 / 隐藏主窗口",
        }
    }

    /// 默认组合键：CTRL + ALT + 键。
    pub fn default_combo(self) -> Option<KeyCombo> {
        // Win32 虚拟键码
        let vk = match self {
            HotkeyAction::PlayPause => 0x50,  // P
            HotkeyAction::Prev => 0x25,       // ←
            HotkeyAction::Next => 0x27,       // →
            HotkeyAction::VolumeUp => 0x26,   // ↑
            HotkeyAction::VolumeDown => 0x28, // ↓
            HotkeyAction::Mute => 0x4D,       // M
            HotkeyAction::Favorite => 0x4C,   // L
            HotkeyAction::Ban => 0x42,        // B
            HotkeyAction::DeskLyric => 0x44,  // D
            HotkeyAction::ShowWindow => 0x57, // W
        };
        Some(KeyCombo { ctrl: true, alt: true, shift: false, win: false, vk })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    /// 窗体始终置顶
    pub topmost: bool,
    /// 最小化到托盘 (不显示任务栏窗体)
    pub minimize_to_tray: bool,
    /// 点关闭按钮时缩到托盘而不是退出
    pub close_to_tray: bool,
    /// 开机时自动启动
    pub autostart: bool,
    /// 拖拽添加文件(夹)后立即播放
    pub play_after_drop: bool,
}

impl Default for General {
    fn default() -> Self {
        Self { topmost: false, minimize_to_tray: false, close_to_tray: false, autostart: false, play_after_drop: true }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LastPlayed {
    pub list_id: u64,
    pub track_id: u64,
    pub position: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Playback {
    pub mode: PlayMode,
    /// 0.0 ..= 1.0
    pub volume: f32,
    pub muted: bool,
    /// 从上次中断位置继续
    pub resume_last: bool,
    /// 启动时自动播放
    pub autoplay: bool,
    pub last: Option<LastPlayed>,
}

impl Default for Playback {
    fn default() -> Self {
        Self { mode: PlayMode::Sequential, volume: 0.8, muted: false, resume_last: true, autoplay: false, last: None }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioSettings {
    /// 空字符串 = 系统默认设备
    pub device_id: String,
    /// WASAPI 独占模式
    pub exclusive: bool,
    /// 共享模式缓冲 (毫秒)
    pub buffer_ms: u32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { device_id: String::new(), exclusive: false, buffer_ms: 60 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EqSettings {
    pub enabled: bool,
    pub preset: String,
    pub preamp_db: f32,
    pub bands: [f32; 10],
}

impl Default for EqSettings {
    fn default() -> Self {
        Self { enabled: false, preset: "自定义".into(), preamp_db: 0.0, bands: [0.0; 10] }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ListSettings {
    pub show_index: bool,
    pub show_artist: bool,
    pub show_album: bool,
    pub show_duration: bool,
    /// 艺人显示在前
    pub artist_first: bool,
    /// 显示斑马线
    pub stripes: bool,
    pub sidebar_visible: bool,
}

impl Default for ListSettings {
    fn default() -> Self {
        Self {
            show_index: true,
            show_artist: true,
            show_album: true,
            show_duration: true,
            artist_first: false,
            stripes: true,
            sidebar_visible: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LyricSettings {
    /// 显示歌词
    pub show_lyric: bool,
    /// 显示图片
    pub show_cover: bool,
    /// 显示音乐文件内嵌图片
    pub embedded_pic: bool,
    /// 显示本地默认图片 (cover.jpg / folder.jpg)
    pub local_pic: bool,
    /// 使用自定义歌词目录 (文件名格式 = "艺人 - 曲目.lrc")
    pub custom_dir: Option<String>,
    /// 启用自动下载（默认关闭：会把 歌名 / 艺人 / 时长 发送到所选歌词来源）
    pub auto_download: bool,
    /// 自动下载时也下载专辑图片（没有内嵌 / 本地图片时，从 LrcAPI / iTunes 下载并保存到本地）
    pub album_pic: bool,
    /// 在线歌词来源（默认「自动」：同时搜索 LrcAPI、LRCLIB）
    pub source: crate::lyrics::online::Provider,
}

impl Default for LyricSettings {
    fn default() -> Self {
        Self { show_lyric: true, show_cover: true, embedded_pic: true, local_pic: true, custom_dir: None, auto_download: false, album_pic: true, source: Default::default() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DeskLyricSettings {
    pub visible: bool,
    pub locked: bool,
    pub font: String,
    pub font_size: f32,
    pub played: [u8; 3],
    pub unplayed: [u8; 3],
    pub two_lines: bool,
    pub pos: Option<[i32; 2]>,
    pub width: Option<i32>,
}

impl Default for DeskLyricSettings {
    fn default() -> Self {
        Self {
            visible: false,
            locked: false,
            font: "Microsoft YaHei UI".into(),
            font_size: 34.0,
            played: [255, 214, 92],
            unplayed: [96, 196, 255],
            two_lines: true,
            pos: None,
            width: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct NetworkSettings {
    /// 使用自定义的代理服务器
    pub proxy_enabled: bool,
    pub kind: ProxyKind,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HotkeySettings {
    /// 启用全局键盘热键支持
    pub enabled: bool,
    /// 启用全局多媒体键盘支持（系统媒体控制 SMTC）
    pub media_keys: bool,
    pub bindings: BTreeMap<HotkeyAction, Option<KeyCombo>>,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        let bindings = HotkeyAction::ALL.iter().map(|a| (*a, a.default_combo())).collect();
        Self { enabled: true, media_keys: true, bindings }
    }
}

impl HotkeySettings {
    pub fn combo(&self, a: HotkeyAction) -> Option<KeyCombo> {
        match self.bindings.get(&a) {
            Some(c) => *c,
            None => a.default_combo(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ThemeSettings {
    /// 整体颜色
    pub base: [u8; 3],
    /// 焦点颜色
    pub accent: [u8; 3],
    /// 边框透明 (窗体不透明度 0.6 ..= 1.0)
    pub opacity: f32,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self { base: [36, 38, 46], accent: [56, 196, 160], opacity: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct WinRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WindowState {
    pub main: Option<WinRect>,
    pub maximized: bool,
    /// 隐藏音乐列表，只显示播放器
    pub compact: bool,
    pub mini: bool,
    pub mini_pos: Option<[f32; 2]>,
    pub sidebar_width: f32,
    /// 原版面板开关：歌词图片 / 均衡器 / 音乐列表
    pub show_cover: bool,
    pub show_eq: bool,
    pub show_list: bool,
    /// 可调整的面板尺寸（像素）：音乐列表宽高、歌词图片宽高
    pub list_size: [i32; 2],
    pub cover_h: i32,
    pub cover_w: i32,
    /// 播放条宽度（原版可在 284 ~ 804 之间调整）
    pub bar_w: i32,
    /// 歌词图片、均衡器、音乐列表相对播放条左上角的位置（面板可以拖开）；没有时用默认布局
    pub pane_offsets: Option<[[i32; 2]; 3]>,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            main: None,
            maximized: false,
            compact: false,
            mini: false,
            mini_pos: None,
            sidebar_width: 190.0,
            show_cover: true,
            show_eq: true,
            show_list: true,
            list_size: [324, 431],
            cover_h: 225,
            cover_w: 304,
            bar_w: 304,
            pane_offsets: None,
        }
    }
}

/// 任务栏歌词：嵌入任务栏显示当前歌词
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskbarLyricSettings {
    pub visible: bool,
    /// 0 = 靠左（开始按钮右侧），1 = 居中，2 = 靠右（通知区域左侧）
    pub align: u8,
    /// 相对对齐位置的水平偏移（像素，可拖动调整）
    pub offset: i32,
    pub width: i32,
    pub font_size: f32,
}

impl Default for TaskbarLyricSettings {
    fn default() -> Self {
        Self { visible: false, align: 2, offset: -20, width: 360, font_size: 14.0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub general: General,
    pub playback: Playback,
    pub audio: AudioSettings,
    pub eq: EqSettings,
    pub list: ListSettings,
    pub lyrics: LyricSettings,
    pub desk_lyric: DeskLyricSettings,
    pub taskbar_lyric: TaskbarLyricSettings,
    pub network: NetworkSettings,
    pub hotkeys: HotkeySettings,
    pub theme: ThemeSettings,
    pub window: WindowState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_partial() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.hotkeys, s.hotkeys);
        // 旧版本/手改的配置缺字段时用默认值补齐
        let partial: Settings = serde_json::from_str(r#"{"playback":{"volume":0.3}}"#).unwrap();
        assert_eq!(partial.playback.volume, 0.3);
        assert!(partial.playback.resume_last);
        assert_eq!(partial.audio.buffer_ms, 60);
    }

    #[test]
    fn mode_cycle() {
        assert_eq!(PlayMode::RepeatOne.cycle(), PlayMode::Sequential);
        assert_eq!(PlayMode::Sequential.cycle(), PlayMode::Shuffle);
    }
}
