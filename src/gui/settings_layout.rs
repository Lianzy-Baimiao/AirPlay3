//! 「配置」窗口各分页的布局：每个控件放在哪（与原版截图量出的位置相同，单位是窗口里的像素；
//! 画布 504 × 386：上面 19 像素是标题条，左边 2 ~ 100 是分页栏，102 ~ 500 是内容）。
//! 原版没有的设置（独占模式、歌词来源、LrcAPI、桌面歌词等）按原版的样子补在空着的地方。

use super::panels::pill::Tone;
use super::settings_model::{Field, Key, Swatch, TAB_ASSOC, TAB_AUDIO, TAB_COLOR, TAB_DESK, TAB_GENERAL, TAB_HOTKEYS, TAB_LYRICS, TAB_NETWORK};
use crate::config::HotkeyAction;
use crate::gui::canvas::R;

/// 按下去做的事
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    /// 打开配置（用户数据）文件夹
    DataDir,
    BrowseLyricDir,
    ResetTaskbarPos,
    ResetColors,
    /// 改这个热键（等着按键）
    Capture(HotkeyAction),
}

/// 药丸点了以后改什么
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Toggle {
    Key(Key),
    /// 开 / 关某个热键
    Hotkey(HotkeyAction),
    /// 关联一种格式（FORMATS 里的第几个）
    Format(usize),
    /// 关联一组格式（None = 全部）
    Group(Option<Tone>),
    /// 不是开关，是按钮（没有勾选框）
    Act(Btn),
}

/// 药丸左边的小图标（全局热键页）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    PlayPause,
    Prev,
    Next,
    Mute,
    VolDown,
    VolUp,
    Lyric,
    Heart,
    Ban,
    Window,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Device,
    ProxyKind,
    LyricSource,
    TaskbarAlign,
    DeskStyle,
    DeskLines,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Widget {
    Pill { tone: Tone, label: String, glyph: Option<Glyph>, act: Toggle },
    /// 灰色方块的勾选框
    Check(Key, &'static str),
    Label(&'static str),
    /// 居中的标题
    Title(&'static str),
    Drop(Choice),
    /// 输入框（没内容时显示提示文字）
    Edit(Field, &'static str),
    /// 选文件夹的框
    Path(Field, &'static str),
    Swatches(Swatch),
    Slider,
    /// 凹槽分隔线（中间的亮度）
    Groove(u8),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    pub r: R,
    pub w: Widget,
}

/// 文件关联页的格式：(显示的名字, 颜色组, 对应的扩展名（空 = 不支持，灰掉）, 列, 行)
pub static FORMATS: [(&str, Tone, &[&str], i32, i32); 31] = [
    ("AAC", Tone::Khaki, &["aac"], 0, 0),
    ("AC3", Tone::Khaki, &["ac3"], 1, 0),
    ("AIFF", Tone::Khaki, &["aif", "aiff", "aifc"], 2, 0),
    ("ASF", Tone::Khaki, &["asf"], 3, 0),
    ("AU", Tone::Khaki, &[], 0, 1),
    ("DTS", Tone::Khaki, &["dts", "dtshd"], 1, 1),
    ("MP3", Tone::Khaki, &["mp3", "mp2", "mp1", "mpa"], 2, 1),
    ("MP4", Tone::Khaki, &["mp4", "m4a", "m4b"], 3, 1),
    ("MPC", Tone::Khaki, &["mpc", "mp+", "mpp"], 0, 2),
    ("OGG", Tone::Khaki, &["ogg", "oga"], 1, 2),
    ("WMA", Tone::Khaki, &["wma"], 2, 2),
    ("ALAC", Tone::Teal, &["alac"], 0, 3),
    ("APE", Tone::Teal, &["ape"], 1, 3),
    ("FLAC", Tone::Teal, &["flac"], 2, 3),
    ("TAK", Tone::Teal, &["tak"], 3, 3),
    ("TTA", Tone::Teal, &["tta"], 0, 4),
    ("WAV", Tone::Teal, &["wav"], 1, 4),
    ("WV", Tone::Teal, &["wv"], 2, 4),
    ("APL", Tone::Lavender, &["apl"], 0, 5),
    ("ASX", Tone::Lavender, &["asx"], 1, 5),
    ("FPL", Tone::Lavender, &[], 2, 5),
    ("M3U", Tone::Lavender, &["m3u", "m3u8"], 3, 5),
    ("PLS", Tone::Lavender, &["pls"], 0, 6),
    ("WPL", Tone::Lavender, &["wpl"], 1, 6),
    ("TTBL", Tone::Lavender, &[], 2, 6),
    ("TTPL", Tone::Lavender, &["ttpl"], 3, 6),
    ("CUE", Tone::Pink, &["cue"], 0, 7),
    ("XSPF", Tone::Lavender, &["xspf"], 4, 9),
    ("CAF", Tone::Teal, &["caf"], 4, 9),
    ("MKA", Tone::Khaki, &["mka"], 4, 9),
    ("WEBM", Tone::Khaki, &["webm"], 4, 9),
];

/// 文件关联页上显示的格式（FORMATS 的前 27 个；后面几个跟着同组的一起关联）
pub const SHOWN_FORMATS: usize = 27;

/// 一组格式的扩展名（None = 全部）
pub fn group_exts(g: Option<Tone>) -> Vec<&'static str> {
    FORMATS.iter().filter(|f| g.is_none_or(|t| t == f.1)).flat_map(|f| f.2.iter().copied()).collect()
}

/// 外圈坐标（含两端）的药丸区域
const fn pr(x0: i32, y0: i32, x1: i32) -> R {
    R::new(x0, y0, x1 - x0 + 1, 21)
}

fn glyph_of(a: HotkeyAction) -> Glyph {
    match a {
        HotkeyAction::PlayPause => Glyph::PlayPause,
        HotkeyAction::Prev => Glyph::Prev,
        HotkeyAction::Next => Glyph::Next,
        HotkeyAction::Mute => Glyph::Mute,
        HotkeyAction::VolumeDown => Glyph::VolDown,
        HotkeyAction::VolumeUp => Glyph::VolUp,
        HotkeyAction::DeskLyric => Glyph::Lyric,
        HotkeyAction::Favorite => Glyph::Heart,
        HotkeyAction::Ban => Glyph::Ban,
        HotkeyAction::ShowWindow => Glyph::Window,
    }
}

fn pill(r: R, tone: Tone, label: &str, act: Toggle) -> Item {
    Item { r, w: Widget::Pill { tone, label: label.to_string(), glyph: None, act } }
}

fn key(r: R, tone: Tone, label: &str, k: Key) -> Item {
    pill(r, tone, label, Toggle::Key(k))
}

fn it(r: R, w: Widget) -> Item {
    Item { r, w }
}

/// 某个分页上的控件
pub fn layout(tab: usize) -> Vec<Item> {
    use Tone::*;
    use Widget::*;
    match tab {
        TAB_GENERAL => vec![
            key(pr(126, 36, 476), Grey, "开机时自动启动", Key::Autostart),
            key(pr(126, 61, 311), Khaki, "启动时自动播放", Key::Autoplay),
            key(pr(311, 61, 476), Khaki, "从上次中断位置继续", Key::ResumeLast),
            key(pr(126, 86, 476), Teal, "最小化到托盘 (不显示任务栏窗体)", Key::MinToTray),
            key(pr(126, 106, 311), Teal, "窗体始终置顶", Key::Topmost),
            key(pr(311, 106, 476), Teal, "点关闭按钮缩到托盘", Key::CloseToTray),
            key(pr(126, 131, 476), Lavender, "拖拽添加文件(夹)到\"音乐列表\"后立即播放", Key::PlayAfterDrop),
            key(pr(126, 151, 476), Lavender, "添加文件夹时忽略压缩文件 (提高添加速度)", Key::IgnoreArchives),
            pill(pr(126, 346, 476), Grey, "打开用户配置数据及歌词图片缓存的保存位置", Toggle::Act(Btn::DataDir)),
        ],
        TAB_AUDIO => vec![
            it(R::new(122, 39, 360, 19), Drop(Choice::Device)),
            it(R::new(123, 70, 358, 18), Check(Key::Exclusive, "使用独占模式 (WASAPI Exclusive)")),
            it(R::new(143, 92, 338, 18), Label("设备启动失败或工作异常时，请试试切换共享和独占模式")),
        ],
        TAB_NETWORK => vec![
            it(R::new(123, 40, 300, 18), Check(Key::ProxyOff, "不使用代理服务器")),
            it(R::new(123, 60, 300, 18), Check(Key::ProxyEnabled, "使用自定义的代理服务器")),
            it(R::new(142, 84, 99, 19), Drop(Choice::ProxyKind)),
            it(R::new(245, 84, 159, 19), Edit(Field::ProxyHost, "服务器地址")),
            it(R::new(408, 84, 56, 19), Edit(Field::ProxyPort, "")),
            it(R::new(142, 108, 159, 19), Edit(Field::ProxyUser, "用户名")),
            it(R::new(305, 108, 159, 19), Edit(Field::ProxyPass, "密码")),
            it(R::new(142, 132, 330, 18), Label("不使用代理服务器时，按系统的代理设置连接")),
            it(R::new(123, 166, 340, 18), Label("歌词服务 LrcAPI（自建时填写；地址留空用公开服务 api.lrc.cx）")),
            it(R::new(142, 188, 199, 19), Edit(Field::LrcApiUrl, "LrcAPI 地址")),
            it(R::new(345, 188, 119, 19), Edit(Field::LrcApiKey, "访问密钥")),
        ],
        TAB_LYRICS => vec![
            key(pr(171, 36, 431), Grey, "启用自动下载", Key::AutoDownload),
            key(pr(171, 61, 431), Khaki, "下载歌词", Key::DownloadLyric),
            key(pr(171, 81, 431), Khaki, "下载专辑图片", Key::AlbumPic),
            key(pr(171, 101, 431), Khaki, "下载艺人图片", Key::ArtistPic),
            key(pr(171, 121, 431), Khaki, "下载后自动修改歌曲信息", Key::AutoTag),
            key(pr(171, 146, 431), Teal, "显示歌词", Key::ShowLyric),
            key(pr(171, 166, 431), Teal, "显示图片", Key::ShowCover),
            key(pr(171, 191, 431), Lavender, "显示音乐文件内嵌图片", Key::EmbeddedPic),
            key(pr(171, 216, 431), Pink, "显示本地默认图片", Key::LocalPic),
            it(R::new(123, 265, 358, 18), Check(Key::UseCustomDir, "使用自定义歌词目录 (文件名格式 = \"艺人 - 曲目.lrc\")")),
            it(R::new(145, 284, 338, 21), Path(Field::CustomDir, "歌词文件存放位置")),
            it(R::new(145, 322, 80, 18), Label("歌词来源")),
            it(R::new(222, 321, 261, 19), Drop(Choice::LyricSource)),
        ],
        TAB_DESK => vec![
            key(pr(121, 36, 302), Grey, "显示桌面歌词", Key::DeskLyric),
            key(pr(302, 36, 482), Grey, "锁定 (鼠标穿透)", Key::DeskLyricLocked),
            it(R::new(123, 66, 90, 18), Label("配色")),
            it(R::new(222, 65, 260, 19), Drop(Choice::DeskStyle)),
            it(R::new(123, 90, 90, 18), Label("显示行数")),
            it(R::new(222, 89, 260, 19), Drop(Choice::DeskLines)),
            it(R::new(123, 114, 90, 18), Label("桌面歌词字号")),
            it(R::new(222, 113, 80, 19), Edit(Field::DeskFont, "34")),
            key(pr(121, 146, 302), Teal, "在任务栏显示歌词", Key::TaskbarLyric),
            key(pr(302, 146, 482), Teal, "第二行显示下一句", Key::TaskbarTwoLines),
            it(R::new(123, 176, 96, 18), Label("任务栏歌词位置")),
            it(R::new(222, 175, 260, 19), Drop(Choice::TaskbarAlign)),
            it(R::new(123, 200, 90, 18), Label("水平偏移")),
            it(R::new(222, 199, 80, 19), Edit(Field::TaskbarX, "0")),
            it(R::new(320, 200, 70, 18), Label("垂直偏移")),
            it(R::new(402, 199, 80, 19), Edit(Field::TaskbarY, "0")),
            it(R::new(123, 224, 90, 18), Label("任务栏字号")),
            it(R::new(222, 223, 80, 19), Edit(Field::TaskbarFont, "14")),
            it(R::new(320, 224, 70, 18), Label("宽度")),
            it(R::new(402, 223, 80, 19), Edit(Field::TaskbarWidth, "360")),
            it(R::new(123, 248, 360, 18), Label("偏移是像素：正数向右、向下；也可以在任务栏上拖动歌词")),
            pill(pr(121, 346, 482), Grey, "恢复任务栏歌词的默认位置", Toggle::Act(Btn::ResetTaskbarPos)),
        ],
        TAB_ASSOC => assoc_layout(),
        TAB_HOTKEYS => hotkey_layout(),
        TAB_COLOR => vec![
            it(R::new(104, 31, 396, 18), Title("整体颜色")),
            it(R::new(120, 59, 365, 51), Swatches(Swatch::Body)),
            it(R::new(104, 121, 396, 18), Title("焦点颜色")),
            it(R::new(120, 149, 365, 15), Swatches(Swatch::Focus)),
            it(R::new(126, 180, 60, 18), Label("边框透明")),
            it(R::new(184, 180, 200, 17), Slider),
            key(pr(380, 178, 485), Grey, "透明雾化", Key::Frost),
            it(R::new(110, 214, 386, 3), Groove(0x32)),
            pill(pr(126, 346, 485), Grey, "恢复默认颜色", Toggle::Act(Btn::ResetColors)),
        ],
        _ => Vec::new(),
    }
}

fn assoc_layout() -> Vec<Item> {
    let mut v = Vec::new();
    // 第一排：按颜色组全选（灰色 = 全部）
    for (i, g) in [None, Some(Tone::Khaki), Some(Tone::Teal), Some(Tone::Lavender), Some(Tone::Pink)].into_iter().enumerate() {
        let x = 179 + 50 * i as i32;
        v.push(pill(pr(x, 36, x + 45), g.unwrap_or(Tone::Grey), "", Toggle::Group(g)));
    }
    for (i, f) in FORMATS.iter().take(SHOWN_FORMATS).enumerate() {
        let (x, y) = (144 + 80 * f.3, 66 + 22 * f.4);
        v.push(pill(pr(x, y, x + 75), f.1, f.0, Toggle::Format(i)));
    }
    v.push(it(R::new(120, 257, 377, 3), Widget::Groove(0x32)));
    v
}

fn hotkey_layout() -> Vec<Item> {
    let glyph = |r: R, tone: Tone, g: Glyph, act: Toggle| Item { r, w: Widget::Pill { tone, label: String::new(), glyph: Some(g), act } };
    let mut v = vec![key(pr(121, 36, 296), Tone::Khaki, "多媒体键盘", Key::MediaKeys)];
    let media = [
        (Glyph::PlayPause, Key::MediaPlay),
        (Glyph::Prev, Key::MediaPrev),
        (Glyph::Next, Key::MediaNext),
        (Glyph::Mute, Key::MediaMute),
        (Glyph::VolDown, Key::MediaVolDown),
        (Glyph::VolUp, Key::MediaVolUp),
    ];
    for (i, (g, k)) in media.into_iter().enumerate() {
        let x = 121 + 60 * i as i32;
        v.push(glyph(pr(x, 58, x + 60), Tone::Khaki, g, Toggle::Key(k)));
    }
    v.push(key(pr(121, 86, 296), Tone::Teal, "全局热键", Key::HotkeysEnabled));
    for (i, a) in HotkeyAction::PAGE.iter().enumerate() {
        let y = 108 + 20 * i as i32;
        v.push(glyph(pr(121, y, 181), Tone::Teal, glyph_of(*a), Toggle::Hotkey(*a)));
        v.push(pill(pr(181, y, 421), Tone::Teal, "", Toggle::Act(Btn::Capture(*a))));
    }
    v.push(key(pr(121, 316, 296), Tone::Lavender, "全局鼠标滚轮", Key::GlobalWheel));
    v.push(it(R::new(110, 349, 386, 3), Widget::Groove(0x3C)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个分页的控件都在内容区里，药丸一样高
    #[test]
    fn every_tab_fits() {
        for tab in 0..super::super::settings_model::TABS.len() {
            for i in layout(tab) {
                assert!(i.r.x >= 102 && i.r.right() <= 501 && i.r.y >= 20 && i.r.bottom() <= 383, "{tab}: {:?}", i);
                if matches!(i.w, Widget::Pill { .. }) {
                    assert_eq!(i.r.h, 21);
                }
            }
        }
    }
}