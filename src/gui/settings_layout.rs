//! 「配置」窗口各分页的布局：每个控件放在哪（与原版截图量出的位置相同，单位是窗口里的像素；
//! 画布 504 × 386：上面 19 像素是标题条，左边 2 ~ 100 是分页栏，102 ~ 500 是内容）。
//! 原版没有的设置（独占模式、歌词来源、LrcAPI、桌面歌词等）按原版的样子补在空着的地方。

use super::panels::pill::Tone;
use super::settings_model::{Field, Key, Swatch, TAB_ABOUT, TAB_ASSOC, TAB_AUDIO, TAB_COLOR, TAB_DESK, TAB_GENERAL, TAB_HOTKEYS, TAB_LYRICS, TAB_NETWORK};
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
    ProjectHome,
    ProjectIssues,
    CheckUpdates,
    ViewRelease,
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
    Group(Option<Kind>),
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
    /// 只读文字（State.value），支持换行。
    Text,
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

/// 文件关联页的格式分组（第一排的按钮按组全选）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Common,
    Lossless,
    Playlist,
    Cue,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Common, Kind::Lossless, Kind::Playlist, Kind::Cue];

    /// 「多彩设置项」关着时写在全选按钮上的名字（开着时靠颜色区分，与原版相同）
    pub fn label(self) -> &'static str {
        match self {
            Kind::Common => "常见",
            Kind::Lossless => "无损",
            Kind::Playlist => "列表",
            Kind::Cue => "CUE",
        }
    }

    /// 这一组的颜色（原版）
    pub fn tone(self) -> Tone {
        match self {
            Kind::Common => Tone::Khaki,
            Kind::Lossless => Tone::Teal,
            Kind::Playlist => Tone::Lavender,
            Kind::Cue => Tone::Pink,
        }
    }
}

/// 文件关联页的格式：(显示的名字, 分组, 对应的扩展名（空 = 不支持，灰掉）, 列, 行)。
/// 只关联页面上看得到的格式（启动时去掉旧版本多关联的，见 shell::tidy_associations）
pub static FORMATS: [(&str, Kind, &[&str], i32, i32); 27] = [
    ("AAC", Kind::Common, &["aac"], 0, 0),
    ("AC3", Kind::Common, &["ac3"], 1, 0),
    ("AIFF", Kind::Common, &["aif", "aiff", "aifc"], 2, 0),
    ("ASF", Kind::Common, &["asf"], 3, 0),
    ("AU", Kind::Common, &[], 0, 1),
    ("DTS", Kind::Common, &["dts", "dtshd"], 1, 1),
    ("MP3", Kind::Common, &["mp3", "mp2", "mp1", "mpa"], 2, 1),
    ("MP4", Kind::Common, &["mp4", "m4a", "m4b"], 3, 1),
    ("MPC", Kind::Common, &["mpc", "mp+", "mpp"], 0, 2),
    ("OGG", Kind::Common, &["ogg", "oga"], 1, 2),
    ("WMA", Kind::Common, &["wma"], 2, 2),
    ("ALAC", Kind::Lossless, &["alac"], 0, 3),
    ("APE", Kind::Lossless, &["ape"], 1, 3),
    ("FLAC", Kind::Lossless, &["flac"], 2, 3),
    ("TAK", Kind::Lossless, &["tak"], 3, 3),
    ("TTA", Kind::Lossless, &["tta"], 0, 4),
    ("WAV", Kind::Lossless, &["wav"], 1, 4),
    ("WV", Kind::Lossless, &["wv"], 2, 4),
    ("APL", Kind::Playlist, &["apl"], 0, 5),
    ("ASX", Kind::Playlist, &["asx"], 1, 5),
    ("FPL", Kind::Playlist, &[], 2, 5),
    ("M3U", Kind::Playlist, &["m3u", "m3u8"], 3, 5),
    ("PLS", Kind::Playlist, &["pls"], 0, 6),
    ("WPL", Kind::Playlist, &["wpl"], 1, 6),
    ("TTBL", Kind::Playlist, &[], 2, 6),
    ("TTPL", Kind::Playlist, &["ttpl"], 3, 6),
    ("CUE", Kind::Cue, &["cue"], 0, 7),
];

/// 一组格式的扩展名（None = 全部）
pub fn group_exts(g: Option<Kind>) -> Vec<&'static str> {
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

/// 某个分页上的控件。colorful = 「多彩设置项」开着（原版的样子：开关按功能分组上色）；
/// 关着时开关都是跟着主题的素色，文件关联页改用文字和间距区分各组
pub fn layout(tab: usize, colorful: bool) -> Vec<Item> {
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
            key(pr(126, 176, 476), Pink, "鼠标离开后自动渐隐下面勾选的面板", Key::AutoFade),
            key(pr(126, 196, 243), Pink, "歌词图片", Key::FadeCover),
            key(pr(243, 196, 360), Pink, "均衡器", Key::FadeEq),
            key(pr(360, 196, 476), Pink, "音乐列表", Key::FadeList),
            it(R::new(128, 223, 64, 18), Label("渐隐前等待")),
            it(R::new(196, 222, 48, 19), Edit(Field::FadeSecs, "3")),
            it(R::new(254, 223, 222, 18), Label("秒（1 ~ 60 秒；鼠标移回来时重新显示）")),
            pill(pr(126, 346, 476), Grey, "设置用户配置数据及歌词图片缓存保存位置", Toggle::Act(Btn::DataDir)),
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
            it(R::new(145, 350, 90, 18), Label("封面展示时长")),
            it(R::new(240, 349, 48, 19), Edit(Field::CoverIntroSecs, "8")),
            it(R::new(298, 350, 185, 18), Label("秒（0 秒关闭，最多 60 秒）")),
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
        TAB_ASSOC => assoc_layout(colorful),
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
            key(pr(126, 231, 485), Lavender, "多彩设置项 (配置里的开关按功能分组上色)", Key::Colorful),
            pill(pr(126, 346, 485), Grey, "恢复默认颜色", Toggle::Act(Btn::ResetColors)),
        ],
        TAB_ABOUT => vec![
            it(R::new(126, 36, 350, 22), Title("AirPlay3 · Rust 重写版")),
            it(R::new(126, 66, 350, 18), Text),
            it(R::new(126, 96, 350, 18), Label("让经典的本地音乐播放器，在今天继续好好用。")),
            it(R::new(126, 118, 350, 18), Label("「经典软件回归计划」的第一步。")),
            it(R::new(126, 140, 350, 18), Label("以 Rust 重写，致敬原版 AirPlay3。")),
            it(R::new(126, 162, 350, 18), Label("项目维护：Lianzy-Baimiao")),
            pill(pr(126, 190, 296), Grey, "GitHub 项目主页", Toggle::Act(Btn::ProjectHome)),
            pill(pr(306, 190, 476), Grey, "问题反馈", Toggle::Act(Btn::ProjectIssues)),
            it(R::new(126, 226, 350, 3), Groove(0x32)),
            it(R::new(126, 240, 350, 18), Title("版本更新")),
            it(R::new(126, 268, 350, 40), Text),
            pill(pr(126, 318, 296), Teal, "", Toggle::Act(Btn::CheckUpdates)),
            pill(pr(306, 318, 476), Grey, "查看发行版", Toggle::Act(Btn::ViewRelease)),
            it(R::new(126, 350, 350, 18), Label("仅手动检查；下载和安装由你决定。")),
        ],
        _ => Vec::new(),
    }
}

fn assoc_layout(colorful: bool) -> Vec<Item> {
    let mut v = Vec::new();
    // 第一排：按组全选（全部 / 常见 / 无损 / 列表 / CUE）。
    // 多彩：原版的五个小色块（灰色 = 全部）；素色：按钮上写着名字，与下面的格式两端对齐
    for (i, g) in std::iter::once(None).chain(Kind::ALL.map(Some)).enumerate() {
        let (i, tone) = (i as i32, g.map_or(Tone::Grey, Kind::tone));
        let (x, w, label) = if colorful { (179 + 50 * i, 45, "") } else { (144 + 64 * i, 59, g.map_or("全部", Kind::label)) };
        v.push(pill(pr(x, 36, x + w), tone, label, Toggle::Group(g)));
    }
    for (i, f) in FORMATS.iter().enumerate() {
        // 素色：各组之间多空 3 像素，看得出哪些格式是一组
        let gap = if colorful { 0 } else { 3 * Kind::ALL.iter().position(|k| *k == f.1).unwrap_or(0) as i32 };
        let (x, y) = (144 + 80 * f.3, 66 + 22 * f.4 + gap);
        v.push(pill(pr(x, y, x + 75), f.1.tone(), f.0, Toggle::Format(i)));
    }
    let bottom = FORMATS.iter().map(|f| f.4).max().unwrap_or(0);
    let last_gap = if colorful { 0 } else { 3 * (Kind::ALL.len() as i32 - 1) };
    v.push(it(R::new(120, 66 + 22 * bottom + 21 + 16 + last_gap, 377, 3), Widget::Groove(0x32)));
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
        for (tab, colorful) in (0..super::super::settings_model::TABS.len()).flat_map(|t| [(t, true), (t, false)]) {
            for i in layout(tab, colorful) {
                assert!(i.r.x >= 102 && i.r.right() <= 501 && i.r.y >= 20 && i.r.bottom() <= 383, "{tab}: {:?}", i);
                if matches!(i.w, Widget::Pill { .. }) {
                    assert_eq!(i.r.h, 21);
                }
            }
        }
    }

    /// 两种样子只在文件关联页的摆法上不同，开关一个不少；多彩时文件关联页是原版的位置
    #[test]
    fn colorful_and_plain_have_the_same_controls() {
        for tab in 0..super::super::settings_model::TABS.len() {
            let (a, b) = (layout(tab, true), layout(tab, false));
            assert_eq!(a.len(), b.len(), "{tab}");
            for (x, y) in a.iter().zip(&b) {
                match (&x.w, &y.w) {
                    (Widget::Pill { act: p, tone: s, .. }, Widget::Pill { act: q, tone: t, .. }) => assert_eq!((p, s), (q, t)),
                    (p, q) => assert_eq!(p, q),
                }
                assert!(x.r == y.r || tab == TAB_ASSOC, "{tab}: {:?} / {:?}", x, y);
            }
        }
        let a = layout(TAB_ASSOC, true);
        assert_eq!((a[0].r, a[5].r, a.last().unwrap().r), (pr(179, 36, 224), pr(144, 66, 219), R::new(120, 257, 377, 3)));
        let b = layout(TAB_ASSOC, false);
        assert_eq!((b[0].r, b.last().unwrap().r), (pr(144, 36, 203), R::new(120, 266, 377, 3)));
    }
}
