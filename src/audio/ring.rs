//! 解码线程 → 渲染线程的样本队列。按"段"组织：每首歌（或格式变化处）以一个 Mark 开头，
//! 渲染线程据此实现无缝衔接、格式切换和"正在听到哪首"的延迟补偿。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use parking_lot::{Condvar, Mutex};

use super::source::Spec;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    /// 在环形队列中的位置（累计写入帧数）
    pub at: u64,
    pub token: u64,
    pub spec: Spec,
    /// 此段第一帧在曲目中的位置（帧）
    pub start: u64,
    /// 曲目总帧数（CUE 分轨为分轨长度）
    pub total: u64,
}

pub enum Chunk {
    /// 交错样本 (样本数据, 帧数)
    Data(usize),
    /// 进入新的一段
    Mark(Mark),
    /// 暂时没有数据
    Empty,
}

#[derive(Default)]
pub struct Ring {
    buf: VecDeque<f32>,
    marks: VecDeque<Mark>,
    write: u64,
    read: u64,
    channels: usize,
    rate: u32,
    pub eos: bool,
    pub epoch: u64,
}

impl Ring {
    pub fn push_mark(&mut self, m: Mark) {
        self.marks.push_back(Mark { at: self.write, ..m });
        self.channels = m.spec.channels.max(1) as usize;
        self.rate = m.spec.rate;
        self.eos = false;
    }

    pub fn push(&mut self, samples: &[f32]) {
        self.buf.extend(samples.iter().copied());
        self.write += (samples.len() / self.channels.max(1)) as u64;
    }

    /// 已缓冲的时长（秒，按最后一段的采样率估算）
    pub fn buffered_secs(&self) -> f64 {
        if self.rate == 0 { 0.0 } else { (self.write - self.read) as f64 / self.rate as f64 }
    }

    pub fn is_drained(&self) -> bool {
        self.write == self.read && self.marks.is_empty()
    }

    pub fn flush(&mut self) {
        self.buf.clear();
        self.marks.clear();
        self.read = self.write;
        self.eos = false;
        self.epoch += 1;
    }

    /// 取出最多 `max_frames` 帧到 `out`（追加），不跨段。`cur_ch` 是当前段的声道数。
    pub fn pop(&mut self, out: &mut Vec<f32>, max_frames: usize, cur_ch: usize) -> Chunk {
        if let Some(m) = self.marks.front() {
            if m.at <= self.read {
                return Chunk::Mark(self.marks.pop_front().unwrap());
            }
        }
        let limit = self.marks.front().map_or(self.write, |m| m.at);
        let avail = limit.saturating_sub(self.read) as usize;
        let n = avail.min(max_frames);
        if n == 0 {
            return Chunk::Empty;
        }
        out.extend(self.buf.drain(..n * cur_ch));
        self.read += n as u64;
        Chunk::Data(n)
    }
}

/// 解码线程、渲染线程、UI 共享的状态。
pub struct Shared {
    pub ring: Mutex<Ring>,
    pub data_ready: Condvar,
    pub paused: AtomicBool,
    /// f32 位模式
    pub volume: AtomicU32,
    pub muted: AtomicBool,
    pub eq: Mutex<crate::config::EqSettings>,
    pub eq_rev: AtomicU64,
    pub audio_cfg: Mutex<crate::config::AudioSettings>,
    pub device_rev: AtomicU64,
    pub status: Status,
    pub output_desc: Mutex<String>,
}

/// 渲染线程发布的"正在听到"的状态。
#[derive(Default)]
pub struct Status {
    /// 0 = 无
    pub token: AtomicU64,
    pub pos_frames: AtomicU64,
    pub total_frames: AtomicU64,
    pub rate: AtomicU32,
    pub running: AtomicBool,
}

impl Status {
    pub fn set(&self, token: u64, pos: u64, total: u64, rate: u32) {
        self.pos_frames.store(pos, Ordering::Relaxed);
        self.total_frames.store(total, Ordering::Relaxed);
        self.rate.store(rate, Ordering::Relaxed);
        self.token.store(token, Ordering::Release);
    }

    pub fn secs(&self) -> (f64, f64) {
        let rate = self.rate.load(Ordering::Relaxed).max(1) as f64;
        (self.pos_frames.load(Ordering::Relaxed) as f64 / rate, self.total_frames.load(Ordering::Relaxed) as f64 / rate)
    }
}

impl Shared {
    pub fn new(volume: f32, eq: crate::config::EqSettings, audio: crate::config::AudioSettings) -> Shared {
        Shared {
            ring: Mutex::new(Ring::default()),
            data_ready: Condvar::new(),
            paused: AtomicBool::new(false),
            volume: AtomicU32::new(volume.to_bits()),
            muted: AtomicBool::new(false),
            eq: Mutex::new(eq),
            eq_rev: AtomicU64::new(1),
            audio_cfg: Mutex::new(audio),
            device_rev: AtomicU64::new(1),
            status: Status::default(),
            output_desc: Mutex::new(String::new()),
        }
    }

    pub fn volume(&self) -> f32 {
        if self.muted.load(Ordering::Relaxed) { 0.0 } else { f32::from_bits(self.volume.load(Ordering::Relaxed)) }
    }
}
