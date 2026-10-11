//! 解码线程：打开曲目、解码到样本队列；一首结束时向控制器要下一首并紧接着解码（无缝播放）。
//! 同一音频文件中相邻的 CUE 分轨直接续读，不重新打开文件。

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use super::engine::{Cmd, Event, NextProvider, PlayRequest};
use super::ring::{Mark, Shared};
use super::source::{Source, Spec, open_source};

/// 解码领先于渲染的目标时长
const BUFFER_AHEAD_SECS: f64 = 1.0;

struct Active {
    req: PlayRequest,
    src: Box<dyn Source>,
    spec: Spec,
    clip_start: u64,
    clip_end: Option<u64>,
    /// 文件内当前帧位置
    pos: u64,
    /// 超出分轨终点的已解码样本（留给同文件的下一分轨）
    carry: Vec<f32>,
}

fn item_total(a: &Active) -> u64 {
    match a.clip_end {
        Some(e) => e.saturating_sub(a.clip_start),
        None => a.src.total_frames().map_or(0, |t| t.saturating_sub(a.clip_start)),
    }
}

fn frames(rate: u32, secs: f64) -> u64 {
    (secs.max(0.0) * rate as f64).round() as u64
}

struct Decoder {
    shared: Arc<Shared>,
    events: Sender<Event>,
    next: NextProvider,
    active: Option<Active>,
    history: VecDeque<PlayRequest>,
    buf: Vec<f32>,
}

pub(super) fn run(shared: Arc<Shared>, cmds: Receiver<Cmd>, events: Sender<Event>, next: NextProvider) {
    crate::platform::com::init_mta();
    let mut d = Decoder { shared, events, next, active: None, history: VecDeque::new(), buf: Vec::new() };
    loop {
        let busy = d.active.is_some();
        let full = busy && d.shared.ring.lock().buffered_secs() >= BUFFER_AHEAD_SECS;
        let cmd = if busy && !full {
            cmds.try_recv().ok()
        } else if busy {
            match cmds.recv_timeout(Duration::from_millis(20)) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(_) => return,
            }
        } else {
            match cmds.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        };
        match cmd {
            Some(Cmd::Play(req)) => d.play(req),
            Some(Cmd::Seek(s)) => d.seek(s),
            Some(Cmd::Stop) => d.stop(),
            Some(Cmd::Quit) => return,
            None if busy && !full => d.decode_chunk(),
            None => {}
        }
    }
}

impl Decoder {
    fn flush(&mut self, eos: bool) {
        let mut ring = self.shared.ring.lock();
        ring.flush();
        ring.eos = eos;
        self.shared.status.token.store(0, Ordering::Release);
        drop(ring);
        self.shared.data_ready.notify_all();
    }

    fn play(&mut self, req: PlayRequest) {
        self.active = None;
        self.flush(false);
        self.start(req, None, true);
    }

    fn stop(&mut self) {
        self.active = None;
        self.flush(true);
    }

    fn set_eos(&self) {
        self.shared.ring.lock().eos = true;
        self.shared.data_ready.notify_all();
    }

    /// 打开 req；失败时依次尝试下一首（最多 64 次，防止整张列表都损坏时死循环）。
    fn start(&mut self, mut req: PlayRequest, mut prev: Option<Active>, announce: bool) {
        for _ in 0..64 {
            match self.open(&req, prev.take(), announce) {
                Ok(()) => return,
                Err(e) => {
                    log::warn!("无法播放 {}: {e}", req.path.display());
                    let _ = self.events.send(Event::Failed(req.token, e.to_string()));
                    match (self.next)(req.token) {
                        Some(n) => req = n,
                        None => break,
                    }
                }
            }
        }
        self.set_eos();
    }

    fn open(&mut self, req: &PlayRequest, prev: Option<Active>, announce: bool) -> anyhow::Result<()> {
        // 同一文件、紧接着上一分轨的 CUE 分轨：继续用已打开的解码器
        let reusable = prev.filter(|p| {
            p.req.path == req.path && req.seek == 0.0 && p.clip_end.is_some_and(|e| e.abs_diff(frames(p.spec.rate, req.start)) <= 1)
        });
        let mut act = match reusable {
            Some(mut p) => {
                p.clip_start = p.clip_end.unwrap();
                p.clip_end = req.end.map(|e| frames(p.spec.rate, e));
                p.req = req.clone();
                p
            }
            None => {
                let mut src = open_source(&req.path)?;
                let spec = src.spec();
                let clip_start = frames(spec.rate, req.start);
                let target = clip_start + frames(spec.rate, req.seek);
                if target > 0 {
                    src.seek(target)?;
                }
                let clip_end = req.end.map(|e| frames(spec.rate, e));
                Active { req: req.clone(), src, spec, clip_start, clip_end, pos: target, carry: Vec::new() }
            }
        };
        act.spec = act.src.spec();
        let total = item_total(&act);
        let start = act.pos.saturating_sub(act.clip_start);
        {
            let mut ring = self.shared.ring.lock();
            ring.push_mark(Mark { at: 0, token: req.token, spec: act.spec, start, total });
            if announce {
                self.shared.status.set(req.token, start, total, act.spec.rate);
            }
        }
        self.shared.data_ready.notify_all();
        self.history.push_back(req.clone());
        while self.history.len() > 4 {
            self.history.pop_front();
        }
        self.active = Some(act);
        Ok(())
    }

    fn decode_chunk(&mut self) {
        let Some(act) = self.active.as_mut() else { return };
        self.buf.clear();
        let n = if act.carry.is_empty() {
            match act.src.read(&mut self.buf) {
                Ok(n) => n,
                Err(e) => {
                    log::warn!("解码错误 {}: {e}", act.req.path.display());
                    0
                }
            }
        } else {
            std::mem::swap(&mut self.buf, &mut act.carry);
            self.buf.len() / act.spec.channels.max(1) as usize
        };
        let spec = act.src.spec();
        let ch = spec.channels.max(1) as usize;
        let mut n = n;
        if let Some(end) = act.clip_end {
            let room = end.saturating_sub(act.pos) as usize;
            if n > room {
                act.carry = self.buf.split_off(room * ch);
                n = room;
            }
        }
        if n == 0 {
            return self.finish_item();
        }
        let mut ring = self.shared.ring.lock();
        if spec != act.spec {
            // 码流中途改变格式（如 AAC SBR）：开新段
            act.spec = spec;
            let total = item_total(act);
            let start = act.pos.saturating_sub(act.clip_start);
            ring.push_mark(Mark { at: 0, token: act.req.token, spec, start, total });
        }
        ring.push(&self.buf[..n * ch]);
        drop(ring);
        act.pos += n as u64;
        self.shared.data_ready.notify_all();
    }

    fn finish_item(&mut self) {
        let Some(act) = self.active.take() else { return };
        match (self.next)(act.req.token) {
            Some(req) => self.start(req, Some(act), false),
            None => self.set_eos(),
        }
    }

    fn seek(&mut self, secs: f64) {
        let token = self.shared.status.token.load(Ordering::Acquire);
        let req = match &self.active {
            Some(a) if a.req.token == token => Some(a.req.clone()),
            _ => self.history.iter().rev().find(|r| r.token == token).cloned(),
        };
        let Some(mut req) = req else { return };
        req.seek = secs;
        self.shared.ring.lock().flush();
        if let Some(act) = self.active.as_mut().filter(|a| a.req.token == token) {
            let target = act.clip_start + frames(act.spec.rate, secs);
            if act.clip_end.is_none_or(|e| target < e) && act.src.seek(target).is_ok() {
                act.pos = target;
                act.carry.clear();
                act.req.seek = secs;
                let (spec, total) = (act.src.spec(), item_total(act));
                let mut ring = self.shared.ring.lock();
                ring.push_mark(Mark { at: 0, token, spec, start: target - act.clip_start, total });
                self.shared.status.set(token, target - act.clip_start, total, spec.rate);
                drop(ring);
                self.shared.data_ready.notify_all();
                return;
            }
        }
        self.active = None;
        self.start(req, None, true);
    }
}
