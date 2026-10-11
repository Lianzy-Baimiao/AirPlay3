//! WASAPI 输出流。共享模式用 AUTOCONVERTPCM 让系统做重采样/声道转换；
//! 独占模式按 f32 → 32bit → 24in32 → 24bit → 16bit 依次尝试设备支持的格式（位完美输出）。

use windows::Win32::Foundation::{CloseHandle, HANDLE, S_OK, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE};
use windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
use windows::Win32::System::Com::CLSCTX_ALL;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::Result;

use super::source::Spec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFmt {
    F32,
    I32,
    I24in32,
    I24,
    I16,
}

impl SampleFmt {
    /// (容器位数, 有效位数)
    fn bits(self) -> (u16, u16) {
        match self {
            SampleFmt::F32 | SampleFmt::I32 => (32, 32),
            SampleFmt::I24in32 => (32, 24),
            SampleFmt::I24 => (24, 24),
            SampleFmt::I16 => (16, 16),
        }
    }
}

pub struct Stream {
    client: IAudioClient,
    render: IAudioRenderClient,
    event: HANDLE,
    pub buffer_frames: u32,
    pub channels: usize,
    pub rate: u32,
    pub fmt: SampleFmt,
    pub exclusive: bool,
    rng: u32,
}

impl Drop for Stream {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
            let _ = CloseHandle(self.event);
        }
    }
}

fn default_mask(ch: u16) -> u32 {
    match ch {
        1 => 0x4,
        2 => 0x3,
        3 => 0x7,
        4 => 0x33,
        5 => 0x37,
        6 => 0x3F,
        7 => 0x13F,
        8 => 0x63F,
        _ => 0,
    }
}

fn wfx(rate: u32, channels: u16, mask: u32, fmt: SampleFmt) -> WAVEFORMATEXTENSIBLE {
    let (container, valid) = fmt.bits();
    let block = channels * container / 8;
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_EXTENSIBLE as u16,
            nChannels: channels,
            nSamplesPerSec: rate,
            nAvgBytesPerSec: rate * block as u32,
            nBlockAlign: block,
            wBitsPerSample: container,
            cbSize: 22,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 { wValidBitsPerSample: valid },
        dwChannelMask: if mask != 0 && mask.count_ones() == channels as u32 { mask } else { default_mask(channels) },
        SubFormat: if fmt == SampleFmt::F32 { KSDATAFORMAT_SUBTYPE_IEEE_FLOAT } else { KSDATAFORMAT_SUBTYPE_PCM },
    }
}

impl Stream {
    pub fn open(device_id: &str, spec: Spec, exclusive: bool, buffer_ms: u32) -> Result<Stream> {
        let device = super::devices::get_device(device_id)?;
        unsafe {
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            if exclusive { Self::open_exclusive(&device, client, spec) } else { Self::open_shared(client, spec, buffer_ms) }
        }
    }

    unsafe fn finish(client: IAudioClient, rate: u32, channels: usize, fmt: SampleFmt, exclusive: bool) -> Result<Stream> {
        unsafe {
            let event = CreateEventW(None, false, false, None)?;
            client.SetEventHandle(event)?;
            let render: IAudioRenderClient = client.GetService()?;
            let buffer_frames = client.GetBufferSize()?;
            Ok(Stream { client, render, event, buffer_frames, channels, rate, fmt, exclusive, rng: 0x1234_5678 })
        }
    }

    unsafe fn open_shared(client: IAudioClient, spec: Spec, buffer_ms: u32) -> Result<Stream> {
        let w = wfx(spec.rate, spec.channels, spec.mask, SampleFmt::F32);
        let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        let dur = buffer_ms.clamp(20, 500) as i64 * 10_000;
        unsafe {
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, dur, 0, &w as *const _ as *const WAVEFORMATEX, None)?;
            Self::finish(client, spec.rate, spec.channels as usize, SampleFmt::F32, false)
        }
    }

    unsafe fn open_exclusive(device: &IMMDevice, mut client: IAudioClient, spec: Spec) -> Result<Stream> {
        use SampleFmt::*;
        let mut chosen = None;
        'outer: for ch in [spec.channels, 2] {
            for fmt in [F32, I32, I24in32, I24, I16] {
                let w = wfx(spec.rate, ch, if ch == spec.channels { spec.mask } else { 0 }, fmt);
                if unsafe { client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &w as *const _ as *const WAVEFORMATEX, None) } == S_OK {
                    chosen = Some((w, ch, fmt));
                    break 'outer;
                }
            }
        }
        let Some((w, ch, fmt)) = chosen else {
            return Err(windows::core::Error::new(AUDCLNT_E_UNSUPPORTED_FORMAT, format!("设备不支持 {} Hz 独占输出", spec.rate)));
        };
        let pw = &w as *const _ as *const WAVEFORMATEX;
        let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
        unsafe {
            let mut period = 0i64;
            client.GetDevicePeriod(Some(&mut period), None)?;
            if let Err(e) = client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, flags, period, period, pw, None) {
                if e.code() != AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
                    return Err(e);
                }
                // 按对齐后的缓冲大小重新计算周期再初始化
                let frames = client.GetBufferSize()?;
                period = (10_000_000.0 * frames as f64 / spec.rate as f64 + 0.5) as i64;
                client = device.Activate(CLSCTX_ALL, None)?;
                client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, flags, period, period, pw, None)?;
            }
            Self::finish(client, spec.rate, ch as usize, fmt, true)
        }
    }

    pub fn padding(&self) -> Result<u32> {
        unsafe { self.client.GetCurrentPadding() }
    }

    /// 等待设备事件，返回是否收到。
    pub fn wait(&self, ms: u32) -> bool {
        unsafe { WaitForSingleObject(self.event, ms) == WAIT_OBJECT_0 }
    }

    pub fn start(&self) -> Result<()> {
        unsafe { self.client.Start() }
    }

    pub fn stop(&self) -> Result<()> {
        unsafe { self.client.Stop() }
    }

    pub fn reset(&self) -> Result<()> {
        unsafe { self.client.Reset() }
    }

    pub fn describe(&self) -> String {
        let bits = match self.fmt {
            SampleFmt::F32 => "32 bit float",
            SampleFmt::I32 => "32 bit",
            SampleFmt::I24in32 | SampleFmt::I24 => "24 bit",
            SampleFmt::I16 => "16 bit",
        };
        let mode = if self.exclusive { "独占" } else { "共享" };
        format!("WASAPI {mode} · {} Hz · {} 声道 · {bits}", self.rate, self.channels)
    }

    fn tpdf(&mut self) -> f32 {
        let mut next = || {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 17;
            self.rng ^= self.rng << 5;
            self.rng as f32 / u32::MAX as f32
        };
        next() - next()
    }

    /// 写入交错 f32（已是设备声道数），按设备格式转换（整数格式加 TPDF 抖动）。
    pub fn write(&mut self, data: &[f32]) -> Result<()> {
        let frames = (data.len() / self.channels) as u32;
        if frames == 0 {
            return Ok(());
        }
        unsafe {
            let p = self.render.GetBuffer(frames)?;
            match self.fmt {
                SampleFmt::F32 => {
                    let dst = std::slice::from_raw_parts_mut(p as *mut f32, data.len());
                    for (d, s) in dst.iter_mut().zip(data) {
                        *d = s.clamp(-1.0, 1.0);
                    }
                }
                SampleFmt::I32 | SampleFmt::I24in32 => {
                    let dst = std::slice::from_raw_parts_mut(p as *mut i32, data.len());
                    for (d, s) in dst.iter_mut().zip(data) {
                        *d = (s.clamp(-1.0, 1.0) as f64 * 2147483647.0) as i32;
                    }
                }
                SampleFmt::I24 => {
                    let dst = std::slice::from_raw_parts_mut(p, data.len() * 3);
                    for (i, s) in data.iter().enumerate() {
                        let v = ((s + self.tpdf() / 8388608.0).clamp(-1.0, 1.0) * 8388607.0) as i32;
                        dst[i * 3..i * 3 + 3].copy_from_slice(&v.to_le_bytes()[..3]);
                    }
                }
                SampleFmt::I16 => {
                    let dst = std::slice::from_raw_parts_mut(p as *mut i16, data.len());
                    for i in 0..data.len() {
                        dst[i] = ((data[i] + self.tpdf() / 32768.0).clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                }
            }
            self.render.ReleaseBuffer(frames, 0)
        }
    }
}
