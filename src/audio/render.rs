//! 渲染线程：从样本队列取数据 → 均衡器/音量 → WASAPI。
//! 负责暂停/继续、格式切换（排空后重开设备）、默认设备切换、设备丢失重连，
//! 并按"已写入 - 设备缓冲"计算真正听到的位置（延迟补偿），在听到新曲目时发出 Started。

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;

use super::dsp::{Equalizer, adapt_channels, apply_gain_ramp, volume_gain};
use super::engine::Event;
use super::output::Stream;
use super::ring::{Chunk, Mark, Shared};
use super::source::Spec;

/// 一段在设备时间轴上的起点
struct Bound {
    /// 设备帧位置（自本次打开设备以来写入的帧数）
    at: u64,
    mark: Mark,
}

pub(super) struct Render {
    shared: Arc<Shared>,
    events: Sender<Event>,
    stream: Option<Stream>,
    /// 当前设备打开所用的源格式
    spec: Option<Spec>,
    /// 等待切换格式的新段
    pending: Option<Mark>,
    bounds: VecDeque<Bound>,
    written: u64,
    epoch: u64,
    /// 设备已 Start（未暂停）
    started: bool,
    last_token: u64,
    gain: f32,
    eq: Option<(u64, Equalizer)>,
    device_rev: u64,
    /// 打开设备时的系统默认设备，用于跟随默认设备切换
    default_id: Option<String>,
    excl_failed: bool,
    error_sent: bool,
    retry_at: Option<Instant>,
    next_default_check: Instant,
    scratch: Vec<f32>,
    adapted: Vec<f32>,
}

pub(super) fn run(shared: Arc<Shared>, events: Sender<Event>, quit: Arc<std::sync::atomic::AtomicBool>) {
    crate::platform::com::init_mta();
    crate::platform::com::boost_audio_thread();
    let device_rev = shared.device_rev.load(Ordering::Acquire);
    let mut r = Render {
        shared,
        events,
        stream: None,
        spec: None,
        pending: None,
        bounds: VecDeque::new(),
        written: 0,
        epoch: 0,
        started: false,
        last_token: 0,
        gain: 0.0,
        eq: None,
        device_rev,
        default_id: None,
        excl_failed: false,
        error_sent: false,
        retry_at: None,
        next_default_check: Instant::now(),
        scratch: Vec::new(),
        adapted: Vec::new(),
    };
    while !quit.load(Ordering::Acquire) {
        r.tick();
    }
}

impl Render {
    fn tick(&mut self) {
        // 设备设置改变：关闭后按当前位置重开
        let rev = self.shared.device_rev.load(Ordering::Acquire);
        if rev != self.device_rev {
            self.device_rev = rev;
            self.excl_failed = false;
            self.rebase_after_loss();
            self.close();
            self.retry_at = None;
        }
        // 切歌/跳转：丢弃设备中的旧数据
        let epoch = self.shared.ring.lock().epoch;
        if epoch != self.epoch {
            self.epoch = epoch;
            if let Some(s) = &self.stream {
                let _ = s.stop();
                let _ = s.reset();
            }
            self.started = false;
            self.written = 0;
            self.bounds.clear();
            self.pending = None;
            self.gain = 0.0;
            if self.stream.is_none() {
                self.spec = None;
            }
        }
        if self.pending.is_some() {
            self.handle_pending();
        } else if self.stream.is_none() {
            self.try_open();
        }
        self.check_default_device();
        let paused = self.shared.paused.load(Ordering::Acquire);
        // 暂停时设备停止消耗，缓冲满后 fill 不会再写入；跳转后则会预先填好一个缓冲
        if self.stream.is_some() && self.pending.is_none() {
            self.fill();
        }
        self.sync_device_state(paused);
        self.publish();
        self.check_finished();
        self.wait(paused);
    }

    /// 新段格式与设备不同：共享模式等旧数据播完；独占模式缓冲很短，直接切换。
    fn handle_pending(&mut self) {
        if let Some(s) = &self.stream {
            if !s.exclusive && self.started && s.padding().map(|p| p > 0).unwrap_or(false) {
                return;
            }
            self.close();
        }
        let mark = self.pending.unwrap();
        if self.open_for(mark.spec) {
            self.pending = None;
            self.bounds.clear();
            self.bounds.push_back(Bound { at: 0, mark });
        }
    }

    fn try_open(&mut self) {
        if let Some(spec) = self.spec {
            // 设备丢失/切换后继续当前段
            self.open_for(spec);
            return;
        }
        let mut ring = self.shared.ring.lock();
        if ring.epoch != self.epoch {
            return;
        }
        if let Chunk::Mark(m) = ring.pop(&mut self.scratch, 0, 1) {
            drop(ring);
            self.pending = Some(m);
            self.handle_pending();
        }
    }

    fn open_for(&mut self, spec: Spec) -> bool {
        if self.retry_at.is_some_and(|t| Instant::now() < t) {
            return false;
        }
        let cfg = self.shared.audio_cfg.lock().clone();
        let mut res = if cfg.exclusive && !self.excl_failed {
            Stream::open(&cfg.device_id, spec, true, cfg.buffer_ms)
        } else {
            Stream::open(&cfg.device_id, spec, false, cfg.buffer_ms)
        };
        if cfg.exclusive && !self.excl_failed {
            if let Err(e) = &res {
                log::warn!("独占模式失败，改用共享模式: {e}");
                self.excl_failed = true;
                let _ = self.events.send(Event::DeviceError(format!("独占模式启动失败 ({})，已切换到共享模式。", e.message())));
                res = Stream::open(&cfg.device_id, spec, false, cfg.buffer_ms);
            }
        }
        match res {
            Ok(s) => {
                log::info!("打开输出设备: {}", s.describe());
                *self.shared.output_desc.lock() = s.describe();
                self.stream = Some(s);
                self.spec = Some(spec);
                self.default_id = if cfg.device_id.is_empty() { super::devices::default_device_id() } else { None };
                self.written = 0;
                self.started = false;
                self.gain = 0.0;
                self.eq = None;
                self.retry_at = None;
                self.error_sent = false;
                true
            }
            Err(e) => {
                log::error!("打开输出设备失败: {e}");
                if !self.error_sent {
                    self.error_sent = true;
                    let _ = self.events.send(Event::DeviceError(format!("设备启动失败: {}。如果设备工作异常，请尝试切换共享和独占模式。", e.message())));
                }
                self.retry_at = Some(Instant::now() + Duration::from_secs(2));
                false
            }
        }
    }

    /// 从队列取数据写入设备。独占模式每次必须写满一个缓冲。
    fn fill(&mut self) {
        let Some(s) = &self.stream else { return };
        let pad = match s.padding() {
            Ok(p) => p,
            Err(e) => return self.device_lost(e),
        };
        let (bufsize, exclusive) = (s.buffer_frames, s.exclusive);
        let avail = if exclusive { if pad == 0 { bufsize } else { 0 } } else { bufsize.saturating_sub(pad) } as usize;
        if avail == 0 {
            return;
        }
        let cur_ch = self.spec.map_or(2, |sp| sp.channels as usize);
        self.scratch.clear();
        let mut frames = 0usize;
        {
            let mut ring = self.shared.ring.lock();
            if ring.epoch != self.epoch {
                return;
            }
            while frames < avail {
                match ring.pop(&mut self.scratch, avail - frames, cur_ch) {
                    Chunk::Data(n) => frames += n,
                    Chunk::Mark(m) if Some(m.spec) == self.spec => {
                        self.bounds.push_back(Bound { at: self.written + frames as u64, mark: m });
                    }
                    Chunk::Mark(m) => {
                        self.pending = Some(m);
                        break;
                    }
                    Chunk::Empty => break,
                }
            }
        }
        if frames == 0 && !(exclusive && self.started) {
            return;
        }
        self.sync_eq();
        if let Some((_, eq)) = &mut self.eq {
            eq.process(&mut self.scratch, cur_ch);
        }
        let target = volume_gain(self.shared.volume());
        self.gain = apply_gain_ramp(&mut self.scratch, cur_ch, self.gain, target);
        let s = self.stream.as_mut().unwrap();
        adapt_channels(&self.scratch, cur_ch, s.channels, &mut self.adapted);
        let total = if exclusive { avail } else { frames };
        self.adapted.resize(total * s.channels, 0.0);
        match s.write(&self.adapted) {
            Ok(()) => self.written += total as u64,
            Err(e) => self.device_lost(e),
        }
    }

    fn sync_device_state(&mut self, paused: bool) {
        let Some(s) = &self.stream else { return };
        let want = !paused && self.written > 0;
        if want != self.started {
            let r = if want { s.start() } else { s.stop() };
            match r {
                Ok(()) => self.started = want,
                Err(e) => self.device_lost(e),
            }
        }
        self.shared.status.running.store(self.started, Ordering::Relaxed);
    }

    fn sync_eq(&mut self) {
        let rev = self.shared.eq_rev.load(Ordering::Acquire);
        let Some(spec) = self.spec else { return };
        if self.eq.as_ref().map(|(r, _)| *r) != Some(rev) {
            let eq = Equalizer::new(&self.shared.eq.lock(), spec.rate, spec.channels as usize);
            self.eq = Some((rev, eq));
        }
    }

    /// 计算真正听到的位置并发布；听到新曲目时发出 Started。
    fn publish(&mut self) {
        let Some(s) = &self.stream else { return };
        let pad = s.padding().unwrap_or(0) as u64;
        let played = self.written.saturating_sub(pad);
        while self.bounds.len() > 1 && self.bounds[1].at <= played {
            self.bounds.pop_front();
        }
        let Some(b) = self.bounds.front() else { return };
        let m = b.mark;
        let mut pos = m.start + played.saturating_sub(b.at);
        if m.total > 0 {
            pos = pos.min(m.total);
        }
        {
            // 在队列锁内比较 epoch，避免把切歌前的旧位置覆盖到新状态上
            let ring = self.shared.ring.lock();
            if ring.epoch != self.epoch {
                return;
            }
            self.shared.status.set(m.token, pos, m.total, m.spec.rate);
        }
        if m.token != self.last_token {
            self.last_token = m.token;
            let _ = self.events.send(Event::Started(m.token));
        }
    }

    /// 队列已结束且设备缓冲已播完：停止设备并通知。
    fn check_finished(&mut self) {
        {
            let ring = self.shared.ring.lock();
            if !(ring.eos && ring.is_drained() && ring.epoch == self.epoch) || self.pending.is_some() {
                return;
            }
        }
        if let Some(s) = &self.stream {
            if s.padding().map(|p| p > 0).unwrap_or(false) {
                return;
            }
        }
        if self.stream.is_none() && self.last_token == 0 {
            return;
        }
        self.close();
        self.bounds.clear();
        self.spec = None;
        self.shared.status.token.store(0, Ordering::Release);
        if self.last_token != 0 {
            self.last_token = 0;
            let _ = self.events.send(Event::Stopped);
        }
    }

    fn wait(&mut self, paused: bool) {
        match &self.stream {
            Some(s) if self.started => {
                s.wait(200);
            }
            _ => {
                let mut ring = self.shared.ring.lock();
                let ms = if self.pending.is_some() && !paused { 10 } else { 200 };
                if ring.epoch == self.epoch {
                    self.shared.data_ready.wait_for(&mut ring, Duration::from_millis(ms));
                }
            }
        }
    }

    /// 跟随系统默认设备的切换（未指定设备时）。
    fn check_default_device(&mut self) {
        if self.stream.is_none() || Instant::now() < self.next_default_check {
            return;
        }
        self.next_default_check = Instant::now() + Duration::from_secs(2);
        if !self.shared.audio_cfg.lock().device_id.is_empty() {
            return;
        }
        let now = super::devices::default_device_id();
        if now.is_some() && now != self.default_id {
            log::info!("系统默认输出设备已改变，重新打开");
            self.rebase_after_loss();
            self.close();
        }
    }

    /// 设备将被重开：设备缓冲中未播放的数据会丢失，把当前段的起点移到"已听到"的位置继续。
    fn rebase_after_loss(&mut self) {
        let Some(s) = &self.stream else { return };
        let pad = s.padding().unwrap_or(0) as u64;
        let played = self.written.saturating_sub(pad);
        if let Some(last) = self.bounds.back() {
            let mut mark = last.mark;
            mark.start += self.written.saturating_sub(last.at);
            self.bounds.clear();
            self.bounds.push_back(Bound { at: 0, mark });
            self.spec = Some(mark.spec);
        }
        log::debug!("rebase: written={} played={played}", self.written);
        self.written = 0;
    }

    fn device_lost(&mut self, e: windows::core::Error) {
        log::warn!("输出设备错误: {e}");
        self.rebase_after_loss();
        self.close();
        self.retry_at = Some(Instant::now() + Duration::from_millis(500));
    }

    fn close(&mut self) {
        self.stream = None;
        self.started = false;
        self.shared.status.running.store(false, Ordering::Relaxed);
    }
}
