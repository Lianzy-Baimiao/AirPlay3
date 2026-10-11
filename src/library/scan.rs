//! 导入：展开文件夹（自然排序）、播放列表、CUE；多线程读取标签，按顺序分批交给调用方合并。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use parking_lot::Mutex;

use super::model::{CueRef, Track};
use crate::media::{cue, is_audio_ext, is_playlist_ext, metadata};

#[derive(Default)]
pub struct ImportJob {
    pub total: AtomicUsize,
    pub done: AtomicUsize,
    pub finished: AtomicBool,
    pub cancel: AtomicBool,
}

/// 自然排序："2.mp3" < "10.mp3"
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        n.push(c);
                        it.next();
                    }
                    n.trim_start_matches('0').to_string()
                };
                let (n, m) = (take(&mut x), take(&mut y));
                match n.len().cmp(&m.len()).then_with(|| n.cmp(&m)) {
                    std::cmp::Ordering::Equal => {}
                    o => return o,
                }
            }
            (Some(c), Some(d)) => {
                let (c, d) = (c.to_lowercase().next().unwrap_or(c), d.to_lowercase().next().unwrap_or(d));
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, cancel: &AtomicBool) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let (mut files, mut dirs) = (Vec::new(), Vec::new());
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        if ft.is_dir() {
            if !name.starts_with('$') && !name.eq_ignore_ascii_case("System Volume Information") {
                dirs.push((name, e.path()));
            }
        } else if ft.is_file() {
            files.push((name, e.path()));
        }
    }
    files.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    dirs.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    out.extend(files.into_iter().map(|f| f.1));
    for (_, d) in dirs {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        walk(&d, out, cancel);
    }
}

/// 展开输入为 (音频文件, CUE 文件)，保持顺序；被 CUE 引用的整轨音频不再单独导入。
fn collect(inputs: &[PathBuf], cancel: &AtomicBool) -> Vec<PathBuf> {
    let mut flat = Vec::new();
    for p in inputs {
        if p.is_dir() {
            walk(p, &mut flat, cancel);
        } else if is_playlist_ext(&crate::util::ext_lower(p)) {
            flat.extend(super::playlist_io::read_playlist(p).into_iter().filter(|f| f.is_file()));
        } else {
            flat.push(p.clone());
        }
    }
    let covered: HashSet<String> = flat
        .iter()
        .filter(|p| crate::util::ext_lower(p) == "cue")
        .filter_map(|p| cue::read_cue_file(p))
        .flat_map(|s| s.tracks.into_iter().map(|t| t.file.to_string_lossy().to_lowercase()))
        .collect();
    let mut seen = HashSet::new();
    flat.into_iter()
        .filter(|p| {
            let ext = crate::util::ext_lower(p);
            (ext == "cue" || is_audio_ext(&ext)) && !covered.contains(&p.to_string_lossy().to_lowercase())
        })
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

fn file_track(path: &Path) -> Track {
    let (g_artist, g_title) = metadata::guess_from_filename(path);
    let tag = metadata::read_tags(path).unwrap_or_default();
    let mut duration = tag.duration;
    if duration <= 0.0 {
        duration = crate::audio::probe_duration(path).unwrap_or(0.0);
    }
    Track {
        path: path.to_path_buf(),
        title: if tag.title.is_empty() { g_title } else { tag.title },
        artist: if tag.artist.is_empty() { g_artist } else { tag.artist },
        album: tag.album,
        album_artist: tag.album_artist,
        genre: tag.genre,
        year: tag.year,
        track_no: tag.track_no,
        disc_no: tag.disc_no,
        duration,
        sample_rate: tag.sample_rate,
        channels: tag.channels,
        bitrate: tag.bitrate,
        added: crate::util::now_secs(),
        ..Default::default()
    }
}

fn cue_tracks(sheet_path: &Path, durations: &mut HashMap<PathBuf, f64>) -> Vec<Track> {
    let Some(sheet) = cue::read_cue_file(sheet_path) else { return Vec::new() };
    let year = sheet.date.get(..4).and_then(|y| y.parse().ok());
    sheet
        .tracks
        .iter()
        .map(|ct| {
            let file_len = *durations.entry(ct.file.clone()).or_insert_with(|| {
                metadata::read_tags(&ct.file).map(|t| t.duration).filter(|d| *d > 0.0).or_else(|| crate::audio::probe_duration(&ct.file)).unwrap_or(0.0)
            });
            let end = ct.end.unwrap_or(file_len);
            Track {
                path: ct.file.clone(),
                cue: Some(CueRef { sheet: sheet_path.to_path_buf(), index: ct.number, start: ct.start, end: ct.end }),
                title: if ct.title.is_empty() { format!("Track {:02}", ct.number) } else { ct.title.clone() },
                artist: ct.performer.clone(),
                album: sheet.title.clone(),
                album_artist: sheet.performer.clone(),
                genre: sheet.genre.clone(),
                year,
                track_no: Some(ct.number),
                duration: (end - ct.start).max(0.0),
                added: crate::util::now_secs(),
                ..Default::default()
            }
        })
        .collect()
}

/// 在后台线程导入文件/文件夹/列表。`sink` 按输入顺序收到分批的曲目（在后台线程调用）。
pub fn spawn_import(inputs: Vec<PathBuf>, sink: impl Fn(Vec<Track>) + Send + 'static) -> Arc<ImportJob> {
    let job = Arc::new(ImportJob::default());
    let j = job.clone();
    let spawn = std::thread::Builder::new().name("import".into()).spawn(move || {
        crate::platform::com::init_mta();
        let files = collect(&inputs, &j.cancel);
        j.total.store(files.len(), Ordering::Relaxed);
        let results: Vec<Mutex<Option<Vec<Track>>>> = files.iter().map(|_| Mutex::new(None)).collect();
        let (next, durations) = (AtomicUsize::new(0), Mutex::new(HashMap::new()));
        let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 4);
        let active = AtomicUsize::new(workers);
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| {
                    crate::platform::com::init_mta();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= files.len() || j.cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let p = &files[i];
                        let t = if crate::util::ext_lower(p) == "cue" { cue_tracks(p, &mut durations.lock()) } else { vec![file_track(p)] };
                        *results[i].lock() = Some(t);
                        j.done.fetch_add(1, Ordering::Relaxed);
                    }
                    active.fetch_sub(1, Ordering::AcqRel);
                });
            }
            let mut flushed = 0;
            loop {
                let all_done = active.load(Ordering::Acquire) == 0;
                let mut batch = Vec::new();
                while flushed < files.len() {
                    match results[flushed].lock().take() {
                        Some(t) => batch.extend(t),
                        None if all_done => {}
                        None => break,
                    }
                    flushed += 1;
                }
                if !batch.is_empty() {
                    sink(batch);
                }
                if all_done || flushed >= files.len() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
        });
        j.finished.store(true, Ordering::Release);
    });
    if spawn.is_err() {
        job.finished.store(true, Ordering::Release);
    }
    job
}
