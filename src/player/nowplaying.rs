//! 正在播放：标题、歌词、封面。曲目切换时后台加载（主窗口、桌面歌词、系统媒体控制共用）。

use std::path::{Path, PathBuf};
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

/// 手动下载图片的结果
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverFound {
    Album,
    /// 没有专辑图片，用了艺人图片
    Artist,
    None,
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

fn cover_image(raw: Vec<u8>) -> Option<CoverImage> {
    let (w, h, rgba) = decode_rgba(&raw, 640)?;
    Some(CoverImage { size: [w as usize, h as usize], rgba: Arc::new(rgba), raw: Arc::new(raw) })
}

/// 专辑图片（内嵌 / 本地 / 关联的）；没有时用已下载的艺人图片。返回 (图片, 是否是专辑图片)
fn load_cover(t: &Track, ls: &LyricSettings, artist_dir: &Path) -> (Option<CoverImage>, bool) {
    let req = CoverRequest { audio: &t.path, linked: t.cover_path.as_deref(), unlinked: t.cover_unlinked, embedded: ls.embedded_pic, local: ls.local_pic };
    if let Some(img) = find_cover(&req).and_then(cover_image) {
        return (Some(img), true);
    }
    // 取消了图片关联的歌不显示任何图片
    if t.cover_unlinked {
        return (None, false);
    }
    (crate::media::artist::cached(artist_dir, &t.artist).flatten().and_then(cover_image), false)
}

/// 下载（或从缓存取）艺人图片，正在播放这首且还没有图片时显示。返回是否找到
fn fetch_artist_pic(p: &Player, now: &SharedNow, t: &Track, net: &crate::config::NetworkSettings, dir: &Path, manual: bool) -> Result<bool, String> {
    let Some(bytes) = crate::media::artist::find(net, dir, &t.artist, manual)? else { return Ok(false) };
    let img = cover_image(bytes).ok_or("图片无法解码")?;
    {
        let mut n = now.lock();
        if n.track == Some(t.id) && (manual || n.cover.is_none()) {
            n.cover = Some(img);
            n.rev += 1;
        }
    }
    p.repaint();
    Ok(true)
}

/// 「下载后自动修改歌曲信息」：歌名一致、时长相差 3 秒内时，补全空着的艺人、专辑。
/// 已有的信息不改（上传者的翻唱常把艺人写成上传者，改了反而错），只改曲库、不改音乐文件
pub fn fill_info(t: &Track, c: &crate::lyrics::online::Candidate) -> Option<(Option<String>, Option<String>)> {
    use crate::lyrics::query::clean_title;
    use crate::util::text::normalize_key;
    let same_title = normalize_key(&clean_title(&t.display_title())) == normalize_key(&clean_title(&c.title));
    let same_len = t.duration > 0.0 && c.duration > 0.0 && (t.duration - c.duration).abs() <= 3.0;
    if !same_title || !same_len {
        return None;
    }
    let artist = (t.artist.trim().is_empty() && !c.artist.trim().is_empty()).then(|| c.artist.trim().to_string());
    let album = (t.album.trim().is_empty() && !c.album.trim().is_empty()).then(|| c.album.trim().to_string());
    (artist.is_some() || album.is_some()).then_some((artist, album))
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
        // 听电台时由界面按电台信息更新（gui::radiowin）
        if g.radio.is_some() {
            return;
        }
        let id = self.current(&g);
        if !force && now.lock().track == id {
            return;
        }
        let Some(t) = id.and_then(|id| g.lib.track(id)).cloned() else {
            let mut n = now.lock();
            *n = NowPlaying { rev: n.rev + 1, ..Default::default() };
            return;
        };
        let (dirs, ls, net, lyric_dir, cover_dir, artist_dir) =
            (g.lyric_dirs(), g.settings.lyrics.clone(), g.settings.network.clone(), save_lyric_dir(&g), g.dirs.cover_dir(), g.dirs.artist_dir());
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
            let (cover, is_album) = load_cover(&t, &ls, &artist_dir);
            {
                let mut n = now.lock();
                if n.track == Some(t.id) {
                    n.cover = cover;
                    n.loaded = true;
                    n.rev += 1;
                }
            }
            p.repaint();
            let mut t = t;
            if !found && ls.auto_download && ls.download_lyric && !t.lyric_unlinked {
                match crate::lyrics::online::best_match(&net, ls.source, &t.display_title(), &t.artist, t.duration) {
                    Ok(Some((c, text))) => match crate::lyrics::online::save(&text, &c, &lyric_dir, &t.display_title(), &t.artist) {
                        // 下载后保存到本地并关联到这首歌，下次直接用本地文件
                        Ok(path) => {
                            p.link_lyric_file(t.id, &path);
                            set_lyrics(&now, t.id, crate::lyrics::read_lrc_file(&path).map(|l| (l, LyricSource::File(path))));
                            if ls.auto_tag {
                                if let Some((artist, album)) = p.fill_track_info(t.id, &c, &now) {
                                    // 补全的艺人马上用来找艺人图片
                                    (t.artist, t.album) = (artist.unwrap_or(t.artist), album.unwrap_or(t.album));
                                }
                            }
                            p.repaint();
                        }
                        Err(e) => log::warn!("保存歌词失败: {e}"),
                    },
                    Ok(None) => {}
                    Err(e) => log::info!("自动下载歌词失败: {e}"),
                }
            }
            // 没有专辑图片（正在显示的可能是艺人图片）：先下载专辑图片，找不到再下载艺人图片
            if !is_album && ls.auto_download && !t.cover_unlinked {
                let album = ls.album_pic
                    && fetch_cover(&p, &now, &t, &net, &cover_dir, false).unwrap_or_else(|e| {
                        log::info!("自动下载专辑图片失败: {e}");
                        false
                    });
                if !album && ls.artist_pic {
                    if let Err(e) = fetch_artist_pic(&p, &now, &t, &net, &artist_dir, false) {
                        log::info!("自动下载艺人图片失败: {e}");
                    }
                }
            }
        });
    }

    /// 「下载后自动修改歌曲信息」：用下载歌词的结果补全空着的艺人、专辑（只改曲库），同时更新正在播放的信息。
    /// 返回补全的 (艺人, 专辑)
    pub fn fill_track_info(&self, id: TrackId, c: &crate::lyrics::online::Candidate, now: &SharedNow) -> Option<(Option<String>, Option<String>)> {
        let filled = {
            let mut g = self.core.lock();
            let t = g.lib.track(id)?.clone();
            let f = fill_info(&t, c)?;
            let tr = g.lib.track_mut(id)?;
            if let Some(a) = &f.0 {
                tr.artist = a.clone();
            }
            if let Some(a) = &f.1 {
                tr.album = a.clone();
            }
            f
        };
        log::info!("下载歌词后补全歌曲信息: 艺人 {:?} 专辑 {:?}", filled.0, filled.1);
        {
            let mut n = now.lock();
            if n.track == Some(id) {
                if let Some(a) = &filled.0 {
                    n.artist = a.clone();
                }
                if let Some(a) = &filled.1 {
                    n.album = a.clone();
                }
                n.rev += 1;
            }
        }
        self.repaint();
        Some(filled)
    }

    /// 关联歌词文件但不重新加载（后台下载完成时用）
    fn link_lyric_file(&self, id: TrackId, path: &std::path::Path) {
        if let Some(t) = self.core.lock().lib.track_mut(id) {
            (t.lyric_path, t.lyric_unlinked) = (Some(path.to_path_buf()), false);
        }
    }

    /// 手动「下载专辑图片」：在调用线程上联网，找到后保存到本地并关联。返回是否找到。
    /// 手动「下载专辑图片」：先找专辑图片，找不到再找艺人图片
    pub fn download_cover(&self, id: TrackId, now: &SharedNow) -> Result<CoverFound, String> {
        let (t, net, dir, artist_dir) = {
            let g = self.core.lock();
            let Some(t) = g.lib.track(id).cloned() else { return Ok(CoverFound::None) };
            (t, g.settings.network.clone(), g.dirs.cover_dir(), g.dirs.artist_dir())
        };
        let album = fetch_cover(self, now, &t, &net, &dir, true);
        if let Ok(true) = album {
            return Ok(CoverFound::Album);
        }
        match fetch_artist_pic(self, now, &t, &net, &artist_dir, true) {
            Ok(true) => Ok(CoverFound::Artist),
            Ok(false) => album.map(|_| CoverFound::None),
            // 两个都失败时报专辑图片的错误
            Err(e) => Err(album.err().unwrap_or(e)),
        }
    }

    /// 「下载歌词」窗口：按所选的搜索结果下载专辑图片（结果自带图片地址时直接用，否则按它的歌名、艺人找）、
    /// 艺人图片，保存并关联到这首歌。在调用线程上联网，返回各项出的错
    pub fn download_pics_for(&self, id: TrackId, c: &crate::lyrics::online::Candidate, album: bool, artist: bool, now: &SharedNow) -> Vec<String> {
        let mut errors = Vec::new();
        let (t, net, dir, artist_dir) = {
            let g = self.core.lock();
            let Some(t) = g.lib.track(id).cloned() else { return errors };
            (t, g.settings.network.clone(), g.dirs.cover_dir(), g.dirs.artist_dir())
        };
        if album {
            let bytes = if c.cover.is_empty() {
                crate::media::artwork::find(&net, &c.title, &c.artist, c.duration, true)
            } else {
                crate::lyrics::lrclib::agent(&net).and_then(|a| crate::media::artwork::download(&a, &c.cover)).map(Some)
            };
            match bytes {
                Ok(Some(b)) => match crate::media::artwork::save(&b, &dir, &c.artist, &c.album, &c.title) {
                    Ok(path) => self.set_cover_link(id, Link::File(path), now),
                    Err(e) => errors.push(format!("保存专辑图片失败：{e}")),
                },
                Ok(None) => errors.push("没有找到专辑图片".into()),
                Err(e) => errors.push(format!("下载专辑图片失败：{e}")),
            }
        }
        if artist {
            let mut ta = t.clone();
            if !c.artist.trim().is_empty() {
                ta.artist = c.artist.clone();
            }
            match fetch_artist_pic(self, now, &ta, &net, &artist_dir, true) {
                Ok(true) => {}
                Ok(false) => errors.push("没有找到艺人图片".into()),
                Err(e) => errors.push(format!("下载艺人图片失败：{e}")),
            }
        }
        errors
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::online::{Candidate, Provider};

    fn cand(title: &str, artist: &str, album: &str, d: f64) -> Candidate {
        Candidate { provider: Provider::LrcApi, key: String::new(), title: title.into(), artist: artist.into(), album: album.into(), duration: d, lyrics: None, cover: String::new() }
    }

    #[test]
    fn fill_only_empty_fields() {
        let t = Track { path: "a/01. 冬眠.mp3".into(), duration: 196.0, ..Default::default() };
        // 没有标签的文件（标题取自文件名，带曲目号）：补全艺人和专辑
        assert_eq!(fill_info(&t, &cand("冬眠", "严浩翔", "冬眠", 196.2)), Some((Some("严浩翔".into()), Some("冬眠".into()))));
        // 已有的艺人不改（上传者翻唱），只补专辑
        let t2 = Track { title: "冬眠【六倍音质】".into(), artist: "起点音乐".into(), duration: 196.0, ..Default::default() };
        assert_eq!(fill_info(&t2, &cand("冬眠", "严浩翔", "冬眠", 197.0)), Some((None, Some("冬眠".into()))));
        // 时长差太多、歌名不同、没有可补的：都不改
        assert_eq!(fill_info(&t, &cand("冬眠", "严浩翔", "冬眠", 230.0)), None);
        assert_eq!(fill_info(&t, &cand("夏眠", "严浩翔", "冬眠", 196.0)), None);
        let full = Track { title: "冬眠".into(), artist: "a".into(), album: "b".into(), duration: 196.0, ..Default::default() };
        assert_eq!(fill_info(&full, &cand("冬眠", "严浩翔", "冬眠", 196.0)), None);
    }
}
