//! 解码源：统一输出交错 f32。symphonia 为主，Media Foundation 兜底（WMA 等）。

use std::fs::File;
use std::path::Path;

use anyhow::{Result, anyhow};
use symphonia::core::audio::Channels;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymErr;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase, Timestamp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub rate: u32,
    pub channels: u16,
    /// WAVEFORMATEXTENSIBLE 声道掩码（0 = 未知）
    pub mask: u32,
}

pub trait Source: Send {
    fn spec(&self) -> Spec;
    /// 文件总帧数
    fn total_frames(&self) -> Option<u64>;
    /// 解码下一段（交错 f32），返回帧数；0 表示结束。
    fn read(&mut self, out: &mut Vec<f32>) -> Result<usize>;
    /// 精确跳转到文件内第 `frame` 帧。
    fn seek(&mut self, frame: u64) -> Result<()>;
}

pub fn open_source(path: &Path) -> Result<Box<dyn Source>> {
    // symphonia / Media Foundation 不支持的格式（TTA 等）用自带的解码器
    if let Some(r) = super::codec::open(path) {
        return r;
    }
    match SymphSource::open(path) {
        Ok(s) => Ok(Box::new(s)),
        Err(e) => match super::source_mf::MfSource::open(path) {
            Ok(s) => Ok(Box::new(s)),
            Err(e2) => Err(anyhow!("{e}; MF: {e2}")),
        },
    }
}

/// 打开文件求时长（秒），供没有标签信息的格式使用。
pub fn probe_duration(path: &Path) -> Option<f64> {
    let s = open_source(path).ok()?;
    Some(s.total_frames()? as f64 / s.spec().rate as f64)
}

pub struct SymphSource {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    spec: Spec,
    tb: Option<TimeBase>,
    total: Option<u64>,
    skip: u64,
    pending: Vec<f32>,
}

impl SymphSource {
    pub fn open(path: &Path) -> Result<Self> {
        let mss = MediaSourceStream::new(Box::new(File::open(path)?), MediaSourceStreamOptions::default());
        let mut hint = Hint::new();
        if let Some(e) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(e);
        }
        let reader = symphonia::default::get_probe().probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())?;
        let track = reader.default_track(TrackType::Audio).ok_or_else(|| anyhow!("没有音频轨"))?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else { return Err(anyhow!("未知编码")) };
        let (track_id, tb) = (track.id, track.time_base);
        let rate = params.sample_rate.ok_or_else(|| anyhow!("未知采样率"))?;
        let total = track.num_frames.or_else(|| {
            let secs = tb?.calc_duration(track.duration?)?.as_secs_f64();
            Some((secs * rate as f64).round() as u64)
        });
        let decoder = symphonia::default::get_codecs().make_audio_decoder(&params, &AudioDecoderOptions::default())?;
        let spec = Spec { rate, channels: params.channels.as_ref().map_or(2, |c| c.count() as u16), mask: mask_of(params.channels.as_ref()) };
        let mut s = SymphSource { reader, decoder, track_id, spec, tb, total, skip: 0, pending: Vec::new() };
        // 先解出第一包：确认能解码，并拿到真实的输出格式（AAC SBR 等可能与参数不同）
        let mut first = Vec::new();
        if s.read(&mut first)? == 0 {
            return Err(anyhow!("没有可解码的音频数据"));
        }
        s.pending = first;
        Ok(s)
    }

    fn frames_to_ts(&self, frames: u64) -> Option<Timestamp> {
        match self.tb {
            Some(tb) if tb != TimeBase::try_from_recip(self.spec.rate)? => {
                tb.calc_timestamp(Time::try_from_secs_f64(frames as f64 / self.spec.rate as f64)?)
            }
            _ => Some(Timestamp::new(frames as i64)),
        }
    }

    fn ts_delta_to_frames(&self, delta: i64) -> u64 {
        match self.tb {
            Some(tb) if Some(tb) != TimeBase::try_from_recip(self.spec.rate) => tb
                .calc_time(Timestamp::new(delta))
                .map_or(0, |t| (t.as_secs_f64() * self.spec.rate as f64).round().max(0.0) as u64),
            _ => delta.max(0) as u64,
        }
    }
}

fn mask_of(ch: Option<&Channels>) -> u32 {
    match ch {
        Some(Channels::Positioned(p)) => p.bits() as u32,
        _ => 0,
    }
}

impl Source for SymphSource {
    fn spec(&self) -> Spec {
        self.spec
    }

    fn total_frames(&self) -> Option<u64> {
        self.total
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<usize> {
        if !self.pending.is_empty() {
            std::mem::swap(out, &mut self.pending);
            self.pending.clear();
            return Ok(out.len() / self.spec.channels.max(1) as usize);
        }
        let mut errors = 0;
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) | Err(SymErr::ResetRequired) => return Ok(0),
                Err(SymErr::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(0),
                Err(e) => return Err(e.into()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buf) if buf.frames() > 0 => {
                    let s = buf.spec();
                    let ch = s.channels().count().max(1);
                    if s.rate() != self.spec.rate || ch != self.spec.channels as usize {
                        self.spec = Spec { rate: s.rate(), channels: ch as u16, mask: mask_of(Some(s.channels())) };
                    }
                    buf.copy_to_vec_interleaved::<f32>(out);
                    if self.skip > 0 {
                        let drop = (self.skip as usize).min(out.len() / ch);
                        out.drain(..drop * ch);
                        self.skip -= drop as u64;
                    }
                    if !out.is_empty() {
                        return Ok(out.len() / ch);
                    }
                }
                Ok(_) => {}
                Err(SymErr::DecodeError(_)) if errors < 64 => errors += 1,
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn seek(&mut self, frame: u64) -> Result<()> {
        self.pending.clear();
        let ts = self.frames_to_ts(frame).ok_or_else(|| anyhow!("无法换算时间戳"))?;
        let r = self.reader.seek(SeekMode::Accurate, SeekTo::Timestamp { ts, track_id: self.track_id })?;
        self.decoder.reset();
        self.skip = self.ts_delta_to_frames(r.required_ts.get() - r.actual_ts.get());
        Ok(())
    }
}
