//! 播放引擎对外接口。内部两个线程：解码线程（打开文件、解码、无缝衔接下一首）
//! 和渲染线程（WASAPI 输出）。控制器通过 `NextProvider` 回调提供"下一首"。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender, unbounded};

use super::ring::Shared;
use crate::config::{AudioSettings, EqSettings};

#[derive(Clone, Debug, PartialEq)]
pub struct PlayRequest {
    /// 由控制器分配，用于识别"正在播放的是哪一项"
    pub token: u64,
    pub path: PathBuf,
    /// CUE 分轨起点（秒）
    pub start: f64,
    /// CUE 分轨终点（秒）；None = 文件结尾
    pub end: Option<f64>,
    /// 从分轨内的这个位置开始（秒）
    pub seek: f64,
}

#[derive(Clone, Debug)]
pub enum Event {
    /// 开始听到这一项
    Started(u64),
    /// 这一项无法打开/解码
    Failed(u64, String),
    /// 播放列表结束或已停止
    Stopped,
    DeviceError(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
}

/// 给定刚结束的 token，返回下一项（无缝衔接）。在解码线程调用。
pub type NextProvider = Box<dyn Fn(u64) -> Option<PlayRequest> + Send>;

pub(super) enum Cmd {
    Play(PlayRequest),
    Seek(f64),
    Stop,
    Quit,
}

pub struct Engine {
    pub(super) shared: Arc<Shared>,
    cmd: Sender<Cmd>,
    pub events: Receiver<Event>,
    quit: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Engine {
    pub fn new(volume: f32, eq: EqSettings, audio: AudioSettings, next: NextProvider) -> Engine {
        let shared = Arc::new(Shared::new(volume, eq, audio));
        let (cmd_tx, cmd_rx) = unbounded();
        let (ev_tx, ev_rx) = unbounded();
        let quit = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        {
            let (shared, ev, quit) = (shared.clone(), ev_tx.clone(), quit.clone());
            threads.push(std::thread::Builder::new().name("audio-render".into()).spawn(move || super::render::run(shared, ev, quit)).unwrap());
        }
        {
            let shared = shared.clone();
            threads.push(
                std::thread::Builder::new().name("audio-decode".into()).spawn(move || super::decoder::run(shared, cmd_rx, ev_tx, next)).unwrap(),
            );
        }
        Engine { shared, cmd: cmd_tx, events: ev_rx, quit, threads }
    }

    /// 播放（替换当前播放）。`paused` 为 true 时只准备好不出声（启动时恢复上次进度）。
    pub fn play(&self, req: PlayRequest, paused: bool) {
        self.shared.paused.store(paused, Ordering::Release);
        let _ = self.cmd.send(Cmd::Play(req));
    }

    pub fn seek(&self, secs: f64) {
        let _ = self.cmd.send(Cmd::Seek(secs.max(0.0)));
    }

    pub fn stop(&self) {
        let _ = self.cmd.send(Cmd::Stop);
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Release);
        self.shared.data_ready.notify_all();
    }

    pub fn set_volume(&self, v: f32) {
        self.shared.volume.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_muted(&self, m: bool) {
        self.shared.muted.store(m, Ordering::Relaxed);
    }

    pub fn set_eq(&self, eq: &EqSettings) {
        *self.shared.eq.lock() = eq.clone();
        self.shared.eq_rev.fetch_add(1, Ordering::AcqRel);
    }

    pub fn set_audio(&self, a: &AudioSettings) {
        *self.shared.audio_cfg.lock() = a.clone();
        self.shared.device_rev.fetch_add(1, Ordering::AcqRel);
        self.shared.data_ready.notify_all();
    }

    pub fn state(&self) -> PlayState {
        if self.shared.status.token.load(Ordering::Acquire) == 0 {
            PlayState::Stopped
        } else if self.shared.paused.load(Ordering::Acquire) {
            PlayState::Paused
        } else {
            PlayState::Playing
        }
    }

    /// 当前听到的 (token, 位置秒, 总长秒)
    pub fn position(&self) -> (u64, f64, f64) {
        let st = &self.shared.status;
        let (pos, total) = st.secs();
        (st.token.load(Ordering::Acquire), pos, total)
    }

    pub fn output_desc(&self) -> String {
        self.shared.output_desc.lock().clone()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Release);
        let _ = self.cmd.send(Cmd::Quit);
        self.shared.data_ready.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}
