//! 网络电台的播放源：下载线程（fetch）→ TS 解复用（HLS）→ 分帧（ADTS / MP3）→ 系统解码器（mfdec）→ f32。
//! 直播流没有总长、不能跳转；网络慢时 `read` 暂时返回 0 帧但不算结束（`live()`），断线自动重连。

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use parking_lot::Mutex;

use super::source::{Source, Spec};
use crate::config::NetworkSettings;

pub mod fetch;
pub mod frame;
pub mod mfdec;
pub mod ts;

use fetch::{Msg, Wrap};

/// 播放地址是不是网络电台
pub fn is_url(path: &Path) -> bool {
    path.to_str().is_some_and(|s| s.starts_with("http://") || s.starts_with("https://"))
}

/// 正在播放的电台：节目名（ICY）、码流说明、出错信息。界面每帧读取
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Status {
    pub url: String,
    pub title: String,
    pub info: String,
    pub error: String,
    /// 正在连接 / 缓冲
    pub buffering: bool,
}

static STATUS: Mutex<Option<Status>> = Mutex::new(None);
static NET: Mutex<Option<NetworkSettings>> = Mutex::new(None);

pub fn status() -> Status {
    STATUS.lock().clone().unwrap_or_default()
}

fn update(url: &str, f: impl FnOnce(&mut Status)) {
    let mut g = STATUS.lock();
    let s = g.get_or_insert_with(Status::default);
    if s.url != url {
        *s = Status { url: url.to_string(), ..Default::default() };
    }
    f(s);
}

/// 电台用的网络设置（代理）；播放电台前由播放器设置
pub fn set_network(net: NetworkSettings) {
    *NET.lock() = Some(net);
}

/// 连接超时、没有数据多久算断线、最多连续重连几次
const OPEN_TIMEOUT: Duration = Duration::from_secs(12);
const STALL: Duration = Duration::from_secs(15);
const MAX_RETRY: u32 = 6;

pub struct StreamSource {
    url: String,
    net: NetworkSettings,
    rx: Receiver<Msg>,
    ts: ts::TsDemux,
    framer: frame::Framer,
    dec: Option<(frame::Header, mfdec::MfDec)>,
    spec: Spec,
    pending: Vec<f32>,
    last_data: Instant,
    connected_at: Instant,
    retries: u32,
    ended: bool,
}

impl StreamSource {
    pub fn open(url: &str) -> Result<Self> {
        let net = NET.lock().clone().unwrap_or_default();
        let (tx, rx) = crossbeam_channel::bounded(512);
        fetch::spawn(url.to_string(), net.clone(), tx);
        update(url, |s| (s.buffering, s.error) = (true, String::new()));
        let now = Instant::now();
        let mut s = StreamSource {
            url: url.to_string(),
            net,
            rx,
            ts: Default::default(),
            framer: Default::default(),
            dec: None,
            spec: Spec { rate: 44100, channels: 2, mask: 0 },
            pending: Vec::new(),
            last_data: now,
            connected_at: now,
            retries: 0,
            ended: false,
        };
        // 等到解出第一段声音，拿到真实的采样率 / 声道
        let mut first = Vec::new();
        while first.is_empty() {
            let left = OPEN_TIMEOUT.checked_sub(now.elapsed()).ok_or_else(|| anyhow!("连接电台超时"))?;
            match s.rx.recv_timeout(left) {
                Ok(Msg::Error(e)) => bail!("{e}"),
                Ok(Msg::End) => bail!("电台没有声音数据"),
                Ok(m) => s.handle(m, &mut first)?,
                Err(_) => bail!("连接电台超时"),
            }
        }
        s.pending = first;
        update(url, |st| st.buffering = false);
        Ok(s)
    }

    /// 处理下载线程的一条消息；解出的声音追加到 `out`
    fn handle(&mut self, m: Msg, out: &mut Vec<f32>) -> Result<()> {
        match m {
            Msg::Data(wrap, b) => {
                self.last_data = Instant::now();
                match wrap {
                    Wrap::Ts => {
                        let es = self.ts.push(&b);
                        self.framer.push(&es);
                    }
                    Wrap::Raw => self.framer.push(&b),
                }
                while let Some((h, f)) = self.framer.next() {
                    self.decode(h, &f, out)?;
                }
            }
            Msg::Title(t) => update(&self.url, |s| s.title = t),
            Msg::Info(i) if !i.is_empty() => update(&self.url, |s| s.info = i),
            Msg::Info(_) | Msg::Error(_) | Msg::End => {}
        }
        Ok(())
    }

    fn decode(&mut self, h: frame::Header, f: &[u8], out: &mut Vec<f32>) -> Result<()> {
        // 第一帧，或换台 / 重连后格式变了：重新建解码器
        if self.dec.as_ref().is_none_or(|(old, _)| old.kind != h.kind || old.rate != h.rate || old.channels != h.channels) {
            let d = mfdec::MfDec::new(&h).map_err(|e| anyhow!("无法解码这个电台（{:?}）：{e}", h.kind))?;
            let kind = match h.kind {
                frame::Kind::Aac => "AAC",
                frame::Kind::Mp3 => "MP3",
            };
            update(&self.url, |s| {
                let extra = if s.info.is_empty() || s.info == "HLS" { String::new() } else { format!(" {}", s.info) };
                s.info = format!("{kind}{extra}");
            });
            self.dec = Some((h, d));
        }
        let (_, d) = self.dec.as_mut().unwrap();
        let before = out.len();
        if let Err(e) = d.decode(f, out) {
            // 坏帧：丢掉这一帧继续
            out.truncate(before);
            log::debug!("电台解码出错: {e}");
        }
        let spec = Spec { rate: d.rate, channels: d.channels, mask: 0 };
        if spec != self.spec {
            self.spec = spec;
        }
        Ok(())
    }

    /// 断线 / 卡住：重新连接（换一个下载线程；旧的会在下次发送时发现没人收而退出）
    fn reconnect(&mut self, why: &str) {
        if self.connected_at.elapsed() > Duration::from_secs(60) {
            self.retries = 0; // 稳定播放过一阵，重新计数
        }
        self.retries += 1;
        if self.retries > MAX_RETRY {
            log::warn!("电台多次重连失败，停止: {why}");
            update(&self.url, |s| (s.error, s.buffering) = (why.to_string(), false));
            self.ended = true;
            return;
        }
        log::info!("电台重连（第 {} 次）: {why}", self.retries);
        update(&self.url, |s| s.buffering = true);
        let (tx, rx) = crossbeam_channel::bounded(512);
        fetch::spawn(self.url.clone(), self.net.clone(), tx);
        self.rx = rx;
        self.ts = Default::default();
        self.framer = Default::default();
        let now = Instant::now();
        (self.last_data, self.connected_at) = (now, now);
    }
}

impl Source for StreamSource {
    fn spec(&self) -> Spec {
        self.spec
    }

    fn total_frames(&self) -> Option<u64> {
        None
    }

    fn live(&self) -> bool {
        !self.ended
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<usize> {
        out.clear();
        if !self.pending.is_empty() {
            std::mem::swap(out, &mut self.pending);
        }
        let ch = self.spec.channels.max(1) as usize;
        let deadline = Instant::now() + Duration::from_millis(40);
        while out.len() < 1024 * ch && !self.ended {
            let wait = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(wait) {
                Ok(Msg::Error(e)) => self.reconnect(&e),
                Ok(Msg::End) => self.reconnect("电台断开了"),
                Ok(m) => self.handle(m, out)?,
                Err(RecvTimeoutError::Disconnected) => self.reconnect("下载中断"),
                Err(RecvTimeoutError::Timeout) => {
                    if self.last_data.elapsed() > STALL {
                        self.reconnect("长时间没有数据");
                    } else if out.is_empty() {
                        update(&self.url, |s| s.buffering = true);
                    }
                    break;
                }
            }
            if !out.is_empty() && Instant::now() >= deadline {
                break;
            }
        }
        if !out.is_empty() {
            update(&self.url, |s| s.buffering = false);
        }
        Ok(out.len() / ch)
    }

    fn seek(&mut self, _frame: u64) -> Result<()> {
        Err(anyhow!("网络电台不能跳转"))
    }
}
