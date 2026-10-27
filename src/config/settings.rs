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

    /// 「全局热键」页上的顺序（与原版相同；显示 / 隐藏主窗口是新加的，放在最后）
    pub const PAGE: [HotkeyAction; 10] = [
        HotkeyAction::PlayPause,
        HotkeyAction::Prev,
        HotkeyAction::Next,
        HotkeyAction::Mute,
        HotkeyAction::VolumeDown,
        HotkeyAction::VolumeUp,
        HotkeyAction::DeskLyric,
        HotkeyAction::Favorite,
        HotkeyAction::Ban,
        HotkeyAction::ShowWindow,
    ];

    /// 默认组合键：CTRL + ALT + 键（与原版相同）
    pub fn default_combo(self) -> Option<KeyCombo> {
        // Win32 虚拟键码
        let vk = match self {
            HotkeyAction::PlayPause => 0x74,  // F5
            HotkeyAction::Prev => 0x25,       // ←
            HotkeyAction::Next => 0x27,       // →
            HotkeyAction::VolumeUp => 0x26,   // ↑
            HotkeyAction::VolumeDown => 0x28, // ↓
            HotkeyAction::Mute => 0x53,       // S
            HotkeyAction::DeskLyric => 0x76,  // F7
            HotkeyAction::Favorite => 0x77,   // F8
            HotkeyAction::Ban => 0x78,        // F9
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
    /// 窗体始终置顶（主界面和迷你条；默认开着，托盘菜单、播放条的图钉、配置里都能关）。
    /// 以前叫 topmost、默认关，而迷你条总是强制置顶；改名后旧配置里的值不再读取，都从「开」开始
    pub always_on_top: bool,
    /// 最小化到托盘 (不显示任务栏窗体)
    pub minimize_to_tray: bool,
    /// 点关闭按钮时缩到托盘而不是退出
    pub close_to_tray: bool,
    /// 开机时自动启动
    pub autostart: bool,
    /// 拖拽添加文件(夹)后立即播放
    pub play_after_drop: bool,
    /// 全局鼠标滚轮：窗口不在前台时，鼠标指着它滚动滚轮也有效（原版用低级鼠标钩子做到）
    pub global_wheel: bool,
}

impl Default for General {
    fn default() -> Self {
        Self { always_on_top: true, minimize_to_tray: false, close_to_tray: false, autostart: false, play_after_drop: true, global_wheel: true }
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
    /// 启用自动下载（默认关闭：会把 歌名 / 艺人 / 时长 发送到所选歌词来源）；下面几项都在它打开时才有效
    pub auto_download: bool,
    /// 自动下载时下载歌词
    pub download_lyric: bool,
    /// 自动下载时也下载专辑图片（没有内嵌 / 本地图片时，从 LrcAPI / iTunes 下载并保存到本地）
    pub album_pic: bool,
    /// 自动下载时，找不到专辑图片就下载艺人图片（Deezer / 维基百科，按艺人缓存）
    pub artist_pic: bool,
    /// 下载歌词后自动修改歌曲信息：只补全空着的艺人、专辑（只改曲库，不改音乐文件）
    pub auto_tag: bool,
    /// 在线歌词来源（默认「自动」：同时搜索 LrcAPI、LRCLIB）
    pub source: crate::lyrics::online::Provider,
}

impl Default for LyricSettings {
    fn default() -> Self {
        Self {
            show_lyric: true,
            show_cover: true,
            embedded_pic: true,
            local_pic: true,
            custom_dir: None,
            auto_download: false,
            download_lyric: true,
            album_pic: true,
            artist_pic: true,
            auto_tag: false,
            source: Default::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DeskLyricSettings {
    pub visible: bool,
    pub locked: bool,
    pub font: String,
    pub font_size: f32,
    /// 配色（`gui::desklyric::STYLES` 的序号）
    pub style: usize,
    /// 显示几行（窗口高度随之变化；拖动窗口上下边调整）。
    /// 旧配置里没有这一项，读成 0（不用整体默认值 2），再按 `two_lines` 算
    #[serde(default)]
    pub lines: usize,
    /// 旧版本的「双行显示」，只读不写
    #[serde(skip_serializing)]
    pub two_lines: bool,
    pub pos: Option<[i32; 2]>,
    pub width: Option<i32>,
}

impl DeskLyricSettings {
    pub const MAX_LINES: usize = 12;

    /// 实际显示的行数
    pub fn line_count(&self) -> usize {
        match self.lines {
            0 if self.two_lines => 2,
            0 => 1,
            n => n.min(Self::MAX_LINES),
        }
    }
}

impl Default for DeskLyricSettings {
    fn default() -> Self {
        Self {
            visible: false,
            locked: false,
            font: "Microsoft YaHei UI".into(),
            font_size: 34.0,
            style: 0,
            lines: 2,
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
    /// 自建的 LrcAPI 地址（空 = 公开实例 api.lrc.cx）
    pub lrcapi_url: String,
    /// 自建 LrcAPI 的访问密钥（部署时 --auth 设置的值；空 = 不需要）
    pub lrcapi_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HotkeySettings {
    /// 启用全局键盘热键支持
    pub enabled: bool,
    /// 启用全局多媒体键盘支持（系统媒体控制 SMTC）
    pub media_keys: bool,
    /// 多媒体键盘上由本程序处理的键（原版的六个开关）：播放 / 暂停、上一首、下一首经系统媒体控制；
    /// 静音、音量减、音量加打开时改为调本程序的音量（不再调系统音量）
    pub media: MediaKeys,
    pub bindings: BTreeMap<HotkeyAction, Option<KeyCombo>>,
    /// 关掉的热键（保留组合键，只是不注册）。旧配置里没有这一项时都开着
    #[serde(default)]
    pub disabled: std::collections::BTreeSet<HotkeyAction>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MediaKeys {
    pub play: bool,
    pub prev: bool,
    pub next: bool,
    pub mute: bool,
    pub vol_down: bool,
    pub vol_up: bool,
}

impl Default for MediaKeys {
    fn default() -> Self {
        Self { play: true, prev: true, next: true, mute: false, vol_down: false, vol_up: false }
    }
}

impl Default for HotkeySettings {
    fn default() -> Self {
        let bindings = HotkeyAction::ALL.iter().map(|a| (*a, a.default_combo())).collect();
        // 与原版相同：迷你歌词、喜爱、不再播放默认不开
        let disabled = [HotkeyAction::DeskLyric, HotkeyAction::Favorite, HotkeyAction::Ban].into_iter().collect();
        Self { enabled: true, media_keys: true, media: MediaKeys::default(), bindings, disabled }
    }
}

impl HotkeySettings {
    /// 这个热键开着（有组合键且没关掉）
    pub fn active(&self, a: HotkeyAction) -> bool {
        !self.disabled.contains(&a) && self.combo(a).is_some()
    }

    pub fn combo(&self, a: HotkeyAction) -> Option<KeyCombo> {
        match self.bindings.get(&a) {
            Some(c) => *c,
            None => a.default_combo(),
        }
    }
}

/// 「配置 → 颜色透明」（与原版相同的四项）。旧版本留下的 base / accent / opacity 字段不再使用（读取时忽略）
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ThemeSettings {
    /// 整体颜色（面板主体；文字、分隔线、玻璃的色调都由它推出）
    pub body: [u8; 3],
    /// 焦点颜色（当前播放、当前歌词、选中项）
    pub focus: [u8; 3],
    /// 边框透明：播放条与标题条玻璃的不透明度（0 = 原版默认，完全通透）
    pub border_alpha: u8,
    /// 透明雾化：窗口后面的桌面模糊（毛玻璃）
    pub frost: bool,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        // 与原版的默认相同（透明雾化默认打开）
        Self { body: [0x20, 0x20, 0x20], focus: [0xFF, 0xA0, 0x40], border_alpha: 0, frost: true }
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
    /// 界面缩放（百分比，100 ~ 300，25 一档）；None = 跟随系统的显示缩放
    pub zoom: Option<u32>,
    /// 迷你条两条的位置（实际像素：控制条, 歌词条）；下次进入迷你条模式时放回这里
    pub mini_strips: Option<[[i32; 2]; 2]>,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            zoom: None,
            mini_strips: None,
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
    /// 相对对齐位置的水平偏移（像素，正值向右；可拖动调整）
    pub offset: i32,
    /// 相对垂直居中的偏移（像素，正值向下；可拖动调整）
    pub offset_y: i32,
    pub width: i32,
    pub font_size: f32,
    /// 第二行显示下一句（关掉后只显示当前句，上下可移动的范围更大）
    pub two_lines: bool,
}

impl TaskbarLyricSettings {
    /// 各对齐方式的默认水平偏移（靠右时离通知区域留一点空）
    pub fn default_offset(align: u8) -> i32 {
        if align == 2 { -20 } else { 0 }
    }
}

impl Default for TaskbarLyricSettings {
    fn default() -> Self {
        Self { visible: false, align: 2, offset: -20, offset_y: 0, width: 360, font_size: 14.0, two_lines: true }
    }
}

/// 网络电台。程序里没有任何电台地址：电台都来自用户自己粘贴的清单。
/// 默认关着（界面上看不到入口），在配置文件里把 `"radio": { "enabled": true }` 改成 true 才打开
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RadioSettings {
    /// 显示「网络电台」（音乐列表上方的页签、主菜单里的一项）
    pub enabled: bool,
    /// 用户粘贴进来的电台清单
    pub lists: Vec<crate::radio::StationList>,
    /// 上次听的电台
    pub last: Option<crate::radio::Station>,
    /// 电台字幕：用 Windows 11 的「实时辅助字幕」识别正在播的声音，显示在歌词的位置上
    pub captions: bool,
    /// 旧版本（v3.9 ~ v3.20）收藏的电台：读进来后并入 `lists` 的「收藏」，不再写出
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub favorites: Vec<crate::radio::Station>,
}

impl RadioSettings {
    /// 旧配置里收藏的电台并入「收藏」清单
    pub fn migrate(&mut self) {
        if self.favorites.is_empty() {
            return;
        }
        let favs = std::mem::take(&mut self.favorites);
        let i = match self.lists.iter().position(|l| l.name == "收藏") {
            Some(i) => i,
            None => {
                self.lists.push(crate::radio::StationList { name: "收藏".into(), stations: Vec::new() });
                self.lists.len() - 1
            }
        };
        for mut st in favs {
            st.group.clear();
            if !self.lists[i].stations.iter().any(|s| s.url == st.url) {
                self.lists[i].stations.push(st);
            }
        }
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
    pub radio: RadioSettings,
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
        // 旧版本的颜色设置（base / accent / opacity，从没用过）读取时忽略，用原版的默认颜色
        let old: Settings = serde_json::from_str(r#"{"theme":{"base":[36,38,46],"accent":[56,196,160],"opacity":1.0}}"#).unwrap();
        assert_eq!(old.theme, ThemeSettings::default());
        assert_eq!((old.theme.body, old.theme.focus, old.theme.border_alpha, old.theme.frost), ([0x20; 3], [0xFF, 0xA0, 0x40], 0, true));
        assert_eq!(partial.audio.buffer_ms, 60);
    }

    /// 网络电台默认关着、没有任何电台；旧版本收藏的电台并入「收藏」清单，旧的 topmost 不再读
    #[test]
    fn radio_and_topmost_migration() {
        let d = Settings::default();
        assert!(!d.radio.enabled && d.radio.lists.is_empty() && d.general.always_on_top);
        let mut old: Settings = serde_json::from_str(
            r#"{"general":{"topmost":false},"radio":{"country":"XX","favorites":[{"uuid":"u","name":"电台 A","url":"https://a.example.com/1","country":"某地","favicon":"https://a.example.com/i.png"}]}}"#,
        )
        .unwrap();
        old.radio.migrate();
        assert!(old.general.always_on_top && !old.radio.enabled);
        assert_eq!(old.radio.lists.len(), 1);
        let l = &old.radio.lists[0];
        assert_eq!((l.name.as_str(), l.stations[0].name.as_str(), l.stations[0].logo.as_str(), l.stations[0].group.as_str()), ("收藏", "电台 A", "https://a.example.com/i.png", ""));
        // 写出时不再有 favorites
        let v = serde_json::to_value(&old).unwrap();
        assert!(v["radio"].get("favorites").is_none() && v["radio"]["enabled"] == false);
    }

    /// 旧版本的桌面歌词设置（双行开关、自定义颜色）照样能读，行数从「双行显示」换算
    #[test]
    fn desk_lyric_migration() {
        let old: Settings = serde_json::from_str(r#"{"desk_lyric":{"visible":true,"two_lines":false,"played":[1,2,3],"unplayed":[4,5,6]}}"#).unwrap();
        assert_eq!((old.desk_lyric.visible, old.desk_lyric.line_count()), (true, 1));
        let old2: Settings = serde_json::from_str(r#"{"desk_lyric":{"two_lines":true}}"#).unwrap();
        assert_eq!(old2.desk_lyric.line_count(), 2);
        assert_eq!(Settings::default().desk_lyric.line_count(), 2);
        let mut s = Settings::default();
        s.desk_lyric.lines = 5;
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.desk_lyric.line_count(), 5);
        let v: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert!(v["desk_lyric"].get("two_lines").is_none(), "two_lines 不再写入");
        s.desk_lyric.lines = 99;
        assert_eq!(s.desk_lyric.line_count(), DeskLyricSettings::MAX_LINES);
    }

    #[test]
    fn mode_cycle() {
        assert_eq!(PlayMode::RepeatOne.cycle(), PlayMode::Sequential);
        assert_eq!(PlayMode::Sequential.cycle(), PlayMode::Shuffle);
    }
}
