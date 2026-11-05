//! 自动歌词回归：真实标签读取 → 播放刷新 → HTTP 搜索 → 保存关联 → 再次加载。
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc};
use std::time::{Duration, Instant};

use airplay3::config::{DataDirs, Settings};
use airplay3::library::{Library, Track};
use airplay3::lyrics::online::{self, Provider};
use airplay3::player::{Player, nowplaying::SharedNow};
use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::{Tag, TagType};

const SYNCED: &str = "[00:01.00]第一行\n[00:02.00]第二行\n[00:03.00]第三行\n[00:04.00]第四行\n[00:05.00]第五行";

struct Fixture {
    root: PathBuf,
    stop: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
    settings: Settings,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("airplay3-lyrics-test-{}-{}", std::process::id(), fastrand::u64(..)));
        std::fs::create_dir_all(&root).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let body = serde_json::json!([{
            "id": "test", "title": "测试歌曲", "artist": "测试歌手", "duration": 226.5, "lrc": SYNCED
        }]).to_string();
        let server = std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                        let mut request = Vec::new();
                        let mut buf = [0; 1024];
                        for _ in 0..2 {
                            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                                match stream.read(&mut buf) {
                                    Ok(0) | Err(_) => break,
                                    Ok(n) => request.extend_from_slice(&buf[..n]),
                                }
                            }
                            if !request.starts_with(b"CONNECT ") { break; }
                            stream.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").unwrap();
                            request.clear();
                        }
                        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(2)),
                    Err(e) => panic!("fixture server: {e}"),
                }
            }
        });
        let mut settings = Settings::default();
        settings.lyrics.auto_download = true;
        settings.lyrics.download_lyric = true;
        settings.lyrics.source = Provider::LrcApi;
        settings.lyrics.album_pic = false;
        settings.lyrics.artist_pic = false;
        settings.lyrics.embedded_pic = false;
        settings.lyrics.local_pic = false;
        settings.network.lrcapi_url = format!("http://127.0.0.1:{port}");
        // 显式使用本地代理，隔离宿主机的环境变量 / Windows 系统代理。
        settings.network.proxy_enabled = true;
        settings.network.host = "127.0.0.1".into();
        settings.network.port = port;
        Self { root, stop, server: Some(server), settings }
    }

    fn track(&self, embedded: &str) -> Track {
        let path = self.root.join("test.wav");
        let wav = b"RIFF\x26\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data\x02\0\0\0\0\0";
        std::fs::write(&path, wav).unwrap();
        let mut tag = Tag::new(TagType::Id3v2);
        tag.insert_text(ItemKey::UnsyncLyrics, embedded.into());
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
        assert_eq!(airplay3::media::metadata::read_embedded_lyrics(&path).as_deref(), Some(embedded));
        Track { path, title: "测试歌曲".into(), artist: "测试歌手".into(), duration: 226.441, ..Default::default() }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.server.take().unwrap().join().unwrap();
        assert!(self.root.is_absolute() && self.root.starts_with(std::env::temp_dir()));
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn automatic_download_replaces_embedded_plain_text() {
    let f = Fixture::new();
    let t = f.track("【音乐资源站】更多打包资源下载");
    let manual = online::search(&f.settings.network, &[Provider::LrcApi], &t.title, &t.artist).unwrap();
    assert_eq!(online::fetch(&f.settings.network, &manual[0]).unwrap(), SYNCED, "手动能够下载");
    let mut lib = Library::default();
    let id = lib.upsert(t);
    let (tx, rx) = mpsc::channel();
    let player = Player::new(DataDirs { root: f.root.clone() }, f.settings.clone(), lib, Arc::new(move || { let _ = tx.send(()); }));
    player.core.lock().queue.current = Some(id);
    let now: SharedNow = Default::default();
    player.refresh_now_playing(&now, false);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if now.lock().lyrics.as_ref().is_some_and(|l| l.synced) { break; }
        if rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_err() { break; }
    }
    let n = now.lock();
    assert!(n.lyrics.as_ref().is_some_and(|l| l.synced), "手动下载成功，但自动下载被内嵌纯文本拦住：{:?}", n.lyrics);
    drop(n);
    let path = player.core.lock().lib.track(id).unwrap().lyric_path.clone().expect("下载后应保存关联");
    assert!(path.is_file());
    assert!(airplay3::lyrics::read_lrc_file(&path).unwrap().synced);
    // 再次播放直接读保存的文件，仍然优先于原音频内的广告。
    let reloaded: SharedNow = Default::default();
    player.refresh_now_playing(&reloaded, false);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !reloaded.lock().loaded {
        assert!(rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_ok(), "重新加载超时");
    }
    let n = reloaded.lock();
    assert!(n.lyrics.as_ref().unwrap().synced);
    assert_eq!(n.lyric_source, Some(airplay3::lyrics::LyricSource::File(path)));
}
