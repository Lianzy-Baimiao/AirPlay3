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

/// 下载的歌词保存位置：设置了自定义歌词目录时存到那里，否则存到程序的 LYRIC 目录
pub fn save_lyric_dir(core: &super::Core) -> PathBuf {
    core.settings.lyrics.custom_dir.as_ref().filter(|d| !d.trim().is_empty()).map(PathBuf::from).unwrap_or_else(|| core.dirs.lyric_dir())
}

/// 从网上找专辑图片，保存到 `dir` 并关联到曲目，正在播放这首时立即显示。返回是否找到。
fn fetch_cover(p: &Player, now: &SharedNow, t: &Track, net: &crate::config::NetworkSettings, dir: &std::path::Path, loose: bool) -> Result<bool, String> {
    let Some(bytes) = crate::media::artwork::find(net, &t.display_title(), &t.artist, t.duration, loose)? else { return Ok(false) };
    let path = crate::media::artwork::save(&bytes, dir, &t.artist, &t.album, &t.display_title()).map_err(|e| format!("保存图片失败: {e}"))?;
    if let Some(tr) = p.core.lock().lib.track_mut(t.id) {
        (tr.cover_path, tr.cover_unlinked) = (Some(path.clone()), false);
    }
    let (w, h, rgba) = decode_rgba(&bytes, 640).ok_or("图片无法解码")?;
    {
        let mut n = now.lock();
        if n.track == Some(t.id) {
            n.cover = Some(CoverImage { size: [w as usize, h as usize], rgba: Arc::new(rgba), raw: Arc::new(bytes) });
            n.rev += 1;
        }
    }
    p.repaint();
    Ok(true)
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
        let (dirs, ls, net, lyric_dir, cover_dir) =
            (g.lyric_dirs(), g.settings.lyrics.clone(), g.settings.network.clone(), save_lyric_dir(&g), g.dirs.cover_dir());
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
            let no_cover = now.lock().cover.is_none();
            p.repaint();
            if !found && ls.auto_download && !t.lyric_unlinked {
                match crate::lyrics::lrclib::best_match(&net, &t.display_title(), &t.artist, &t.album, t.duration) {
                    Ok(Some(rec)) => match crate::lyrics::lrclib::save(&rec, &lyric_dir, &t.display_title(), &t.artist) {
                        // 下载后保存到本地并关联到这首歌，下次直接用本地文件
                        Ok(path) => {
                            p.link_lyric_file(t.id, &path);
                            set_lyrics(&now, t.id, crate::lyrics::read_lrc_file(&path).map(|l| (l, LyricSource::File(path))));
                            p.repaint();
                        }
                        Err(e) => log::warn!("保存歌词失败: {e}"),
                    },
                    Ok(None) => {}
                    Err(e) => log::info!("自动下载歌词失败: {e}"),
                }
            }
            if no_cover && ls.auto_download && ls.album_pic && !t.cover_unlinked {
                if let Err(e) = fetch_cover(&p, &now, &t, &net, &cover_dir, false) {
                    log::info!("自动下载专辑图片失败: {e}");
                }
            }
        });
    }

    /// 关联歌词文件但不重新加载（后台下载完成时用）
    fn link_lyric_file(&self, id: TrackId, path: &std::path::Path) {
        if let Some(t) = self.core.lock().lib.track_mut(id) {
            (t.lyric_path, t.lyric_unlinked) = (Some(path.to_path_buf()), false);
        }
    }

    /// 手动「下载专辑图片」：在调用线程上联网，找到后保存到本地并关联。返回是否找到。
    pub fn download_cover(&self, id: TrackId, now: &SharedNow) -> Result<bool, String> {
        let (t, net, dir) = {
            let g = self.core.lock();
            let Some(t) = g.lib.track(id).cloned() else { return Ok(false) };
            (t, g.settings.network.clone(), g.dirs.cover_dir())
        };
        fetch_cover(self, now, &t, &net, &dir, true)
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
