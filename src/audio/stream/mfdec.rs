//! 用 Windows 自带的 Media Foundation 解码器解一帧帧的 AAC / MP3（网络电台）。
//! symphonia 的 AAC 只支持 LC，电台常用的 HE-AAC（SBR / PS）要靠系统的 AAC 解码器才是完整音质。

use anyhow::{Result, anyhow};
use windows::Win32::Media::MediaFoundation::*;
use windows::core::GUID;

use super::frame::{Header, Kind};

pub struct MfDec {
    mft: IMFTransform,
    pub rate: u32,
    pub channels: u16,
    float: bool,
    out_size: u32,
}

// 只在解码线程（MTA）里使用
unsafe impl Send for MfDec {}

/// WAVEFORMATEX + 格式专有的尾部（AAC：HEAACWAVEINFO；MP3：MPEGLAYER3WAVEFORMAT）
fn wave_format(h: &Header) -> Vec<u8> {
    let (tag, extra): (u16, Vec<u8>) = match h.kind {
        // wPayloadType = 1（ADTS），wAudioProfileLevelIndication = 0xFE（未知），wStructType = 0，保留
        Kind::Aac => (0x1610, [1u16, 0xFE, 0, 0, 0, 0].iter().flat_map(|v| v.to_le_bytes()).collect()),
        // wID = MPEGLAYER3_ID_MPEG，fdwFlags = 关闭填充，nBlockSize，nFramesPerBlock = 1，nCodecDelay = 1393
        Kind::Mp3 => {
            let mut e = 1u16.to_le_bytes().to_vec();
            e.extend(2u32.to_le_bytes());
            e.extend((h.len as u16).to_le_bytes());
            e.extend(1u16.to_le_bytes());
            e.extend(1393u16.to_le_bytes());
            (0x0055, e)
        }
    };
    let bytes_per_sec = if h.kbps > 0 { h.kbps * 125 } else { 16_000 };
    let mut v = Vec::with_capacity(18 + extra.len());
    v.extend(tag.to_le_bytes());
    v.extend(h.channels.to_le_bytes());
    v.extend(h.rate.to_le_bytes());
    v.extend(bytes_per_sec.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(0u16.to_le_bytes());
    v.extend((extra.len() as u16).to_le_bytes());
    v.extend(extra);
    v
}

impl MfDec {
    pub fn new(h: &Header) -> Result<Self> {
        crate::platform::com::init_mta();
        crate::audio::source_mf::mf_startup();
        let sub = match h.kind {
            Kind::Aac => MFAudioFormat_AAC,
            Kind::Mp3 => MFAudioFormat_MP3,
        };
        unsafe {
            let input = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Audio, guidSubtype: sub };
            let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut n = 0u32;
            MFTEnumEx(MFT_CATEGORY_AUDIO_DECODER, MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_LOCALMFT | MFT_ENUM_FLAG_SORTANDFILTER, Some(&input), None, &mut list, &mut n)?;
            let acts = std::slice::from_raw_parts(list, n as usize).to_vec();
            windows::Win32::System::Com::CoTaskMemFree(Some(list as *const _));
            let act = acts.into_iter().flatten().next().ok_or_else(|| anyhow!("系统没有 {:?} 解码器", h.kind))?;
            let mft: IMFTransform = act.ActivateObject()?;
            let wf = wave_format(h);
            let t = MFCreateMediaType()?;
            MFInitMediaTypeFromWaveFormatEx(&t, wf.as_ptr() as *const _, wf.len() as u32)?;
            if h.kind == Kind::Aac {
                t.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 1)?;
            }
            mft.SetInputType(0, &t, 0)?;
            let mut d = MfDec { mft, rate: h.rate, channels: h.channels, float: true, out_size: 0 };
            d.pick_output()?;
            d.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            Ok(d)
        }
    }

    /// 选输出格式：优先 32 位浮点，没有就 16 位 PCM
    fn pick_output(&mut self) -> Result<()> {
        unsafe {
            let mut fallback = None;
            for i in 0.. {
                let Ok(t) = self.mft.GetOutputAvailableType(0, i) else { break };
                let sub: GUID = t.GetGUID(&MF_MT_SUBTYPE)?;
                if sub == MFAudioFormat_Float {
                    return self.set_output(&t, true);
                }
                if sub == MFAudioFormat_PCM && t.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE).unwrap_or(0) == 16 && fallback.is_none() {
                    fallback = Some(t);
                }
            }
            let t = fallback.ok_or_else(|| anyhow!("解码器没有可用的输出格式"))?;
            self.set_output(&t, false)
        }
    }

    fn set_output(&mut self, t: &IMFMediaType, float: bool) -> Result<()> {
        unsafe {
            self.mft.SetOutputType(0, t, 0)?;
            self.rate = t.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)?;
            self.channels = t.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)? as u16;
            self.float = float;
            self.out_size = self.mft.GetOutputStreamInfo(0)?.cbSize.max(1 << 16);
        }
        Ok(())
    }

    /// 送入一帧，解出的样本（交错 f32）追加到 `out`
    pub fn decode(&mut self, frame: &[u8], out: &mut Vec<f32>) -> Result<()> {
        unsafe {
            let buf = MFCreateMemoryBuffer(frame.len() as u32)?;
            let mut p = std::ptr::null_mut();
            buf.Lock(&mut p, None, None)?;
            std::ptr::copy_nonoverlapping(frame.as_ptr(), p, frame.len());
            buf.Unlock()?;
            buf.SetCurrentLength(frame.len() as u32)?;
            let s = MFCreateSample()?;
            s.AddBuffer(&buf)?;
            match self.mft.ProcessInput(0, &s, 0) {
                Ok(()) => {}
                // 解码器满了：先取输出再送（一般不会发生）
                Err(e) if e.code() == MF_E_NOTACCEPTING => {
                    self.drain(out)?;
                    self.mft.ProcessInput(0, &s, 0)?;
                }
                Err(e) => return Err(e.into()),
            }
            self.drain(out)
        }
    }

    /// 把解码器里已有的输出都取出来
    fn drain(&mut self, out: &mut Vec<f32>) -> Result<()> {
        loop {
            unsafe {
                let buf = MFCreateMemoryBuffer(self.out_size)?;
                let s = MFCreateSample()?;
                s.AddBuffer(&buf)?;
                let mut db = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: std::mem::ManuallyDrop::new(Some(s.clone())), dwStatus: 0, pEvents: std::mem::ManuallyDrop::new(None) }];
                let mut status = 0u32;
                let r = self.mft.ProcessOutput(0, &mut db, &mut status);
                // 结构里的 COM 指针要手动释放
                drop(std::mem::ManuallyDrop::take(&mut db[0].pSample));
                drop(std::mem::ManuallyDrop::take(&mut db[0].pEvents));
                match r {
                    Ok(()) => self.read_sample(&s, out)?,
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                    // HE-AAC：解出第一帧后才知道真正的采样率 / 声道，重新选输出格式
                    Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => self.pick_output()?,
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }

    fn read_sample(&self, s: &IMFSample, out: &mut Vec<f32>) -> Result<()> {
        unsafe {
            let buf = s.ConvertToContiguousBuffer()?;
            let (mut p, mut len) = (std::ptr::null_mut(), 0u32);
            buf.Lock(&mut p, None, Some(&mut len))?;
            let bytes = std::slice::from_raw_parts(p, len as usize);
            if self.float {
                out.extend(bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])));
            } else {
                out.extend(bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0));
            }
            buf.Unlock()?;
        }
        Ok(())
    }
}
