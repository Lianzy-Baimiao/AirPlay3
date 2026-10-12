//! Media Foundation 解码源：系统自带解码器兜底（WMA/ASF、AC3、部分 AAC 等 symphonia 不支持的格式）。

use std::path::Path;
use std::sync::Once;

use anyhow::{Result, anyhow};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::core::{GUID, HSTRING};

use super::source::{Source, Spec};

static MF_INIT: Once = Once::new();

pub(crate) fn mf_startup() {
    MF_INIT.call_once(|| unsafe {
        if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
            log::error!("MFStartup 失败: {e}");
        }
    });
}

const AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;

pub struct MfSource {
    reader: IMFSourceReader,
    spec: Spec,
    total: Option<u64>,
    skip: u64,
    seek_target: Option<i64>,
    eof: bool,
}

// IMFSourceReader 在 MTA 线程中创建和使用（解码线程/扫描线程都初始化为 MTA），可以跨线程移动。
unsafe impl Send for MfSource {}

impl MfSource {
    pub fn open(path: &Path) -> Result<Self> {
        crate::platform::com::init_mta();
        mf_startup();
        unsafe {
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), None)?;
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader.SetStreamSelection(AUDIO, true)?;
            let want = MFCreateMediaType()?;
            want.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            want.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)?;
            reader.SetCurrentMediaType(AUDIO, None, &want)?;
            let mt = reader.GetCurrentMediaType(AUDIO)?;
            let rate = mt.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)?;
            let channels = mt.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)? as u16;
            let mask = mt.GetUINT32(&MF_MT_AUDIO_CHANNEL_MASK).unwrap_or(0);
            let total = reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .ok()
                .and_then(|v| u64::try_from(&v).ok())
                .map(|hns| (hns as f64 / 1e7 * rate as f64).round() as u64);
            if rate == 0 || channels == 0 {
                return Err(anyhow!("MF: 无效的音频格式"));
            }
            Ok(MfSource { reader, spec: Spec { rate, channels, mask }, total, skip: 0, seek_target: None, eof: false })
        }
    }
}

impl Source for MfSource {
    fn spec(&self) -> Spec {
        self.spec
    }

    fn total_frames(&self) -> Option<u64> {
        self.total
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<usize> {
        out.clear();
        let ch = self.spec.channels as usize;
        while out.is_empty() && !self.eof {
            let mut flags = 0u32;
            let mut ts = 0i64;
            let mut sample: Option<IMFSample> = None;
            unsafe { self.reader.ReadSample(AUDIO, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut sample))? };
            if let Some(target) = self.seek_target.take() {
                // MF 跳到目标之前的同步点：按第一个样本的实际时间戳算出需要丢弃的帧数
                self.skip = ((target - ts).max(0) as f64 / 1e7 * self.spec.rate as f64).round() as u64;
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                self.eof = true;
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                return Err(anyhow!("MF: 解码中途格式改变"));
            }
            let Some(sample) = sample else { continue };
            unsafe {
                let buf = sample.ConvertToContiguousBuffer()?;
                let mut ptr = std::ptr::null_mut();
                let mut len = 0u32;
                buf.Lock(&mut ptr, None, Some(&mut len))?;
                let floats = std::slice::from_raw_parts(ptr as *const f32, len as usize / 4);
                out.extend_from_slice(floats);
                buf.Unlock()?;
            }
            if self.skip > 0 {
                let drop = (self.skip as usize).min(out.len() / ch);
                out.drain(..drop * ch);
                self.skip -= drop as u64;
            }
        }
        Ok(out.len() / ch)
    }

    fn seek(&mut self, frame: u64) -> Result<()> {
        let hns = (frame as f64 / self.spec.rate as f64 * 1e7) as i64;
        unsafe { self.reader.SetCurrentPosition(&GUID::zeroed(), &PROPVARIANT::from(hns))? };
        self.eof = false;
        self.skip = 0;
        self.seek_target = Some(hns);
        Ok(())
    }
}
