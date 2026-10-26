//! 绘制回归对照（只在手动运行时用）：用固定的输入画各个面板，把画布的原始像素存下来，改绘制代码前后逐字节比较。
//!   AP3_GOLDEN=<目录> cargo test --lib golden -- --ignored
//! 每个场景存成 <名字>.raw（预乘 BGRA，前 8 字节是宽高）和 <名字>.png（看图用）。

use std::path::PathBuf;

use super::canvas::{Canvas, R, Rgba};
use super::panels::{bar, cfgdir, cover, eq, frame, lyricdl, playlist, settings};
use super::settings_layout as lay;
use super::text::{Font, Hint};

fn save(dir: &PathBuf, name: &str, c: &Canvas) {
    let mut raw = Vec::with_capacity(8 + c.px.len() * 4);
    raw.extend_from_slice(&(c.pw as u32).to_le_bytes());
    raw.extend_from_slice(&(c.ph as u32).to_le_bytes());
    for p in &c.px {
        raw.extend_from_slice(p);
    }
    std::fs::write(dir.join(format!("{name}.raw")), raw).unwrap();
    c.save_png(&dir.join(format!("{name}.png"))).unwrap();
}

/// 一张 64×64 的假封面（渐变 + 斜线）
fn fake_cover() -> Vec<u8> {
    let mut v = Vec::with_capacity(64 * 64 * 4);
    for y in 0..64u32 {
        for x in 0..64u32 {
            let s = if (x + y) % 16 < 3 { 255 } else { 0 };
            v.extend_from_slice(&[(x * 4) as u8, (y * 4) as u8, s as u8, 255]);
        }
    }
    v
}

fn bar_model<'a>(cover: Option<(&'a [u8], u32, u32)>, hover: f32, hot: bar::Hot, slogan: f32) -> bar::BarModel<'a> {
    bar::BarModel {
        cover,
        title: "晴天 (Live)",
        artist: "周杰伦",
        playing: true,
        pos: 83.0,
        dur: 269.0,
        volume: 0.85,
        muted: false,
        mode: crate::config::PlayMode::RepeatAll,
        topmost: false,
        show_lyric: true,
        show_list: true,
        show_eq: false,
        hover,
        hot,
        slogan,
    }
}

#[test]
#[ignore]
fn golden() {
    let Some(dir) = std::env::var_os("AP3_GOLDEN").map(PathBuf::from) else { return };
    // AP3_GOLDEN_SCALE=1.25 等：按这个缩放画（看放大后的样子）
    let scale: f32 = std::env::var("AP3_GOLDEN_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    std::fs::create_dir_all(&dir).unwrap();
    let art = fake_cover();
    let cov = Some((art.as_slice(), 64u32, 64u32));

    // 播放条
    for (name, w, cover, hover, hot, slogan) in [
        ("bar_idle", 304, None, 0.0, bar::Hot::None, 0.0),
        ("bar_slogan_mid", 304, None, 0.0, bar::Hot::None, 0.4),
        ("bar_cover_hover", 304, cov, 1.0, bar::Hot::PlayPause, 0.0),
        ("bar_half_hover_wide", 520, cov, 0.5, bar::Hot::Close, 1.0),
    ] {
        let mut c = Canvas::with_scale(w, bar::H, scale);
        bar::paint(&mut c, &bar_model(cover, hover, hot, slogan));
        save(&dir, name, &c);
    }

    // 歌词图片
    let lines: Vec<String> = ["故事的小黄花", "从出生那年就飘着", "童年的荡秋千", "随记忆一直晃到现在"].iter().map(|s| s.to_string()).collect();
    for (name, active, cover, lyric_mode, info, strip) in [
        ("cover_idle", false, None, 0.0, 0.0, 0.0),
        ("cover_info", true, cov, 0.0, 1.0, 1.0),
        ("cover_lyrics", true, cov, 1.0, 0.0, 0.3),
        ("cover_lyrics_noart", true, None, 1.0, 0.5, 0.0),
    ] {
        let mut c = Canvas::with_scale(cover::W, cover::H, scale);
        let m = cover::CoverModel {
            cover,
            title: "晴天",
            artist: "周杰伦",
            album: "叶惠美",
            active,
            lines: &lines,
            current: Some(1),
            lyric_mode,
            info,
            strip_hover: strip,
            status: None,
        };
        cover::paint(&mut c, &m);
        save(&dir, name, &c);
    }

    // 均衡器
    let mut c = Canvas::with_scale(eq::W, eq::H, scale);
    eq::paint(&mut c, &eq::EqModel { enabled: true, preamp: 3.0, bands: [6.0, 4.0, 2.0, 0.0, -2.0, -4.0, -6.0, 3.0, 9.0, -12.0], preset: "摇滚", hot_button: Some(1), strip_hover: 0.6 });
    save(&dir, "eq", &c);

    // 音乐列表
    let rows: Vec<playlist::Row> = (0..12)
        .map(|i| playlist::Row { title: ["晴天", "七里香", "A Very Long English Song Title"][i % 3], artist: "周杰伦", duration: "4:29", playing: i == 2, selected: i == 4 })
        .collect();
    let tabs = [("默认", true), ("试听列表", false), ("我喜欢", false)];
    for (name, hover, search, focus, caret) in [("list", Some(3), "", false, false), ("list_search", None, "晴", true, true)] {
        let mut c = Canvas::with_scale(playlist::W, playlist::H, scale);
        let m = playlist::PlaylistModel {
            tabs: &tabs,
            rows: &rows,
            first: 0,
            scroll: 0,
            total_rows: 40,
            hover_row: hover,
            hot_tool: Some(1),
            footer: "40 / 2:58:16",
            search,
            search_focus: focus,
            strip_hover: 0.5,
            thumb_hot: false,
            strip_tabs: None,
            paste_hot: false,
            radio_empty: false,
            tab_w: playlist::TAB_W,
            tab_scroll: 0,
            search_hover: focus,
            caret_on: caret,
        };
        playlist::paint(&mut c, &m);
        save(&dir, name, &c);
    }

    // 配置：8 个分页
    for tab in 0..super::settings_model::TABS.len() {
        let items: Vec<(lay::Item, settings::State)> = lay::layout(tab)
            .into_iter()
            .enumerate()
            .map(|(i, it)| (it, settings::State { on: i % 2 == 0, enabled: i % 5 != 4, dim: i % 7 == 6, value: "示例 Value 123".into(), focused: i == 3 }))
            .collect();
        let mut c = Canvas::with_scale(settings::W, settings::H, scale);
        settings::paint(&mut c, &settings::SettingsModel { tab, items: &items, hover: Some(1), hover_tab: Some((tab + 1) % 8), hover_cell: Some(2), strip: 1.0 });
        save(&dir, &format!("settings_{tab}"), &c);
    }

    // 下载歌词
    let results = [
        lyricdl::ResultRow { title: "晴天", artist: "周杰伦", duration: "4:29", source: crate::lyrics::online::Provider::LrcApi },
        lyricdl::ResultRow { title: "晴天 (Live)", artist: "周杰伦", duration: "4:41", source: crate::lyrics::online::Provider::Lrclib },
    ];
    let preview: Vec<String> = lines.clone();
    let mut c = Canvas::with_scale(lyricdl::W, lyricdl::H, scale);
    let m = lyricdl::LyricDlModel {
        fields: ["晴天", "周杰伦", "叶惠美"],
        focus: Some(1),
        rows: &results,
        selected: Some(0),
        hover: Some(1),
        scroll: 0,
        list_note: "",
        preview: &preview,
        pics: [Some(lyricdl::Pic { rgba: &art, w: 64, h: 64 }), None],
        checks: [true, false, true, false],
        info_found: true,
        can_download: true,
        hot_button: Some(2),
        hot_check: Some(1),
    };
    lyricdl::paint(&mut c, &m);
    save(&dir, "lyricdl", &c);

    // 配置文件存放位置
    for (name, choice, path, hot, dimmed) in [("cfgdir", 2, r"D:\Music\AirPlay3\", Some(cfgdir::Hot::Ok), false), ("cfgdir_dim", 1, "", Some(cfgdir::Hot::Plus), true)] {
        let mut c = Canvas::with_scale(cfgdir::W, cfgdir::H, scale);
        cfgdir::paint(&mut c, &cfgdir::CfgDirModel { choice, path, status: "", hot, path_hot: hot == Some(cfgdir::Hot::Plus), can_ok: !path.is_empty(), dimmed });
        save(&dir, name, &c);
    }

    // 基本图元
    let mut c = Canvas::with_scale(200, 120, scale);
    frame::panel(&mut c, 0.7);
    c.round_rect(R::new(20, 40, 61, 23), 3.0, Some(Rgba::gray(52)), Some(Rgba::gray(100)));
    c.vgrad(R::new(90, 40, 40, 30), Rgba(200, 60, 20, 255), Rgba(20, 60, 200, 128));
    c.num6x8("12:34 -5.0+", 20, 75, Rgba(207, 207, 207, 200));
    c.icon(crate::skin::icons::PLAY, 140, 40, Rgba::gray(230));
    c.icon(crate::skin::icons::W_CLOSE, 160, 40, Rgba::gray(230));
    c.text("文字 Text 123", Font::px(12.0, false), 20, 90, Rgba::gray(220), Hint::ClearType);
    c.glow_text("光晕 Glow", Font::px(13.0, true), 110, 90, Rgba::gray(235), Rgba(16, 16, 16, 190), 3);
    c.hit_margin(R::new(0, 0, 200, 4));
    save(&dir, "primitives", &c);
}
