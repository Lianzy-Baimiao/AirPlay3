//! 正在播放：标题、歌词、封面。曲目切换时后台加载（主窗口、桌面歌词、系统媒体控制共用）。

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::config::LyricSettings;
use crate::library::{Track, TrackId};
use crate::lyrics::{LyricRequest, LyricSource, Lyrics};
use crate::media::cover::{CoverRequest, decode_rgba, find_cover};

use super::Player;

#[derive(Clone)]
pub struct CoverImage {
    pub size: [usize; 2],
    pub rgba: Arc<Vec<u8>>,
    /// 原始图片数据（系统媒体控制的缩略图）
    pub raw: Arc<Vec<u8>>,
}

#[derive(Clone, Default)]
pub struct NowPlaying {
    pub track: Option<TrackId>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub lyrics: Option<Arc<Lyrics>>,
    pub lyric_source: Option<LyricSource>,
    /// 用户微调（毫秒，正值 = 歌词提前）
    pub lyric_offset_ms: i64,
    pub cover: Option<CoverImage>,
    pub loaded: bool,
    /// 每次变化 +1
    pub rev: u64,
}

pub type SharedNow = Arc<Mutex<NowPlaying>>;

impl NowPlaying {
    pub fn lyric_index(&self, pos: f64) -> Option<usize> {
        self.lyrics.as_ref()?.index_at(pos + self.lyric_offset_ms as f64 / 1000.0)
    }

    pub fn lyric_progress(&self, idx: usize, pos: f64) -> f32 {
        self.lyrics.as_ref().map_or(0.0, |l| l.progress_at(idx, pos + self.lyric_offset_ms as f64 / 1000.0))
    }
}

/// 歌词/图片关联方式（原版"取消歌词关联"、"取消图片关联"）
pub enum Link {
    Auto,
    File(PathBuf),
    Off,
}

fn load_lyrics(t: &Track, dirs: &[PathBuf]) -> Option<(Lyrics, LyricSource)> {
    let title = t.display_title();
    let req = LyricRequest {
        audio: &t.path,
        is_cue: t.is_cue(),
        title: &title,
        artist: &t.artist,
        linked: t.lyric_path.as_deref(),
        unlinked: t.lyric_unlinked,
        search_dirs: dirs,
    };
    crate::lyrics::load(&req)
}

fn load_cover(t: &Track, ls: &LyricSettings) -> Option<CoverImage> {
    let req = CoverRequest { audio: &t.path, linked: t.cover_path.as_deref(), unlinked: t.cover_unlinked, embedded: ls.embedded_pic, local: ls.local_pic };
    let raw = find_cover(&req)?;
    let (w, h, rgba) = decode_rgba(&raw, 640)?;
    Some(CoverImage { size: [w as usize, h as usize], rgba: Arc::new(rgba), raw: Arc::new(raw) })
}

fn set_lyrics(now: &SharedNow, id: TrackId, lyr: Option<(Lyrics, LyricSource)>) {
    let mut n = now.lock();
    if n.track == Some(id) {
        (n.lyrics, n.lyric_source) = match lyr {
            Some((l, s)) => (Some(Arc::new(l)), Some(s)),
            None => (None, None),
        };
        n.rev += 1;
    }
}

impl Player {
    /// 曲目改变（或 `force`）时更新正在播放信息，并在后台加载歌词/封面；没有歌词且开启了自动下载时从 LRCLIB 下载。
    pub fn refresh_now_playing(&self, now: &SharedNow, force: bool) {
        let g = self.core.lock();
        let id = self.current(&g);
        if !force && now.lock().track == id {
            return;
        }
        let Some(t) = id.and_then(|id| g.lib.track(id)).cloned() else {
            let mut n = now.lock();
            *n = NowPlaying { rev: n.rev + 1, ..Default::default() };
            return;
        };
        let (dirs, ls, net, lyric_dir) = (g.lyric_dirs(), g.settings.lyrics.clone(), g.settings.network.clone(), g.dirs.lyric_dir());
        drop(g);
        {
            let mut n = now.lock();
            let same = n.track == Some(t.id);
            *n = NowPlaying {
                track: Some(t.id),
                title: t.display_title(),
                artist: t.display_artist().to_string(),
                album: t.display_album().to_string(),
                duration: t.duration,
                lyrics: if same { n.lyrics.clone() } else { None },
                lyric_source: if same { n.lyric_source.clone() } else { None },
                lyric_offset_ms: t.lyric_offset_ms,
                cover: if same { n.cover.clone() } else { None },
                loaded: false,
                rev: n.rev + 1,
            };
        }
        let (now, p) = (now.clone(), self.clone());
        std::thread::spawn(move || {
            crate::platform::com::init_mta();
            let lyr = load_lyrics(&t, &dirs);
            let found = lyr.is_some();
            set_lyrics(&now, t.id, lyr);
            let cover = load_cover(&t, &ls);
            {
                let mut n = now.lock();
                if n.track == Some(t.id) {
                    n.cover = cover;
                    n.loaded = true;
                    n.rev += 1;
                }
            }
            p.repaint();
            if !found && ls.auto_download && !t.lyric_unlinked {
                match crate::lyrics::lrclib::best_match(&net, &t.display_title(), &t.artist, &t.album, t.duration) {
                    Ok(Some(rec)) => {
                        if crate::lyrics::lrclib::save(&rec, &lyric_dir, &t.display_title(), &t.artist).is_ok() {
                            set_lyrics(&now, t.id, load_lyrics(&t, &dirs));
                            p.repaint();
                        }
                    }
                    Ok(None) => {}
                    Err(e) => log::info!("自动下载歌词失败: {e}"),
                }
            }
        });
    }

    pub fn set_lyric_link(&self, id: TrackId, link: Link, now: &SharedNow) {
        if let Some(t) = self.core.lock().lib.track_mut(id) {
            (t.lyric_path, t.lyric_unlinked) = match link {
                Link::Auto => (None, false),
                Link::File(p) => (Some(p), false),
                Link::Off => (None, true),
            };
        }
        self.refresh_now_playing(now, true);
    }

    pub fn set_cover_link(&self, id: TrackId, link: Link, now: &SharedNow) {
        if let Some(t) = self.core.lock().lib.track_mut(id) {
            (t.cover_path, t.cover_unlinked) = match link {
                Link::Auto => (None, false),
                Link::File(p) => (Some(p), false),
                Link::Off => (None, true),
            };
        }
        self.refresh_now_playing(now, true);
    }

    /// 歌词时间微调，`delta_ms = 0` 表示复位。
    pub fn adjust_lyric_offset(&self, id: TrackId, delta_ms: i64, now: &SharedNow) {
        let mut g = self.core.lock();
        let Some(t) = g.lib.track_mut(id) else { return };
        t.lyric_offset_ms = if delta_ms == 0 { 0 } else { t.lyric_offset_ms + delta_ms };
        let v = t.lyric_offset_ms;
        drop(g);
        let mut n = now.lock();
        if n.track == Some(id) {
            n.lyric_offset_ms = v;
            n.rev += 1;
        }
        drop(n);
        self.repaint();
    }
}
