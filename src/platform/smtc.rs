//! 系统媒体控制（SMTC）：键盘上的播放 / 暂停 / 上一首 / 下一首键，以及 Windows 音量浮层、锁屏上显示的
//! 标题、艺人、专辑图片。对应原版「启用全局多媒体键盘支持」。

use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
};
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::core::{HSTRING, factory};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    PlayPause,
    Play,
    Pause,
    Next,
    Prev,
    Stop,
}

pub struct Smtc {
    controls: SystemMediaTransportControls,
    token: i64,
    /// 上次提交的 (标题, 艺人, 专辑, 图片指针)，没变时不重复提交
    shown: Option<(String, String, String, usize)>,
    playing: Option<bool>,
}

impl Smtc {
    /// 为播放条窗口创建；按键回调在系统线程上调用
    pub fn new(hwnd: HWND, on_button: impl Fn(Button) + Send + 'static) -> windows::core::Result<Smtc> {
        let interop = factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
        let controls: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd)? };
        controls.SetIsEnabled(true)?;
        controls.SetIsPlayEnabled(true)?;
        controls.SetIsPauseEnabled(true)?;
        controls.SetIsNextEnabled(true)?;
        controls.SetIsPreviousEnabled(true)?;
        controls.SetIsStopEnabled(true)?;
        let handler = windows::Foundation::TypedEventHandler::<SystemMediaTransportControls, SystemMediaTransportControlsButtonPressedEventArgs>::new(
            move |_, args| {
                if let Some(a) = args.as_ref() {
                    let b = match a.Button()? {
                        SystemMediaTransportControlsButton::Play => Some(Button::Play),
                        SystemMediaTransportControlsButton::Pause => Some(Button::Pause),
                        SystemMediaTransportControlsButton::Next => Some(Button::Next),
                        SystemMediaTransportControlsButton::Previous => Some(Button::Prev),
                        SystemMediaTransportControlsButton::Stop => Some(Button::Stop),
                        _ => None,
                    };
                    if let Some(b) = b {
                        on_button(b);
                    }
                }
                Ok(())
            },
        );
        let token = controls.ButtonPressed(&handler)?;
        controls.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
        Ok(Smtc { controls, token, shown: None, playing: None })
    }

    /// 同步播放状态与歌曲信息（每帧调用，内容没变时不做任何事）
    pub fn update(&mut self, playing: bool, stopped: bool, title: &str, artist: &str, album: &str, cover: Option<&[u8]>) {
        let key = (title.to_string(), artist.to_string(), album.to_string(), cover.map_or(0, |c| c.as_ptr() as usize));
        if self.shown.as_ref() != Some(&key) {
            if let Err(e) = self.show(title, artist, album, cover) {
                log::debug!("SMTC 更新歌曲信息失败: {e}");
            }
            self.shown = Some(key);
        }
        if self.playing != Some(playing) || stopped {
            let st = if stopped && title.is_empty() {
                MediaPlaybackStatus::Closed
            } else if stopped {
                MediaPlaybackStatus::Stopped
            } else if playing {
                MediaPlaybackStatus::Playing
            } else {
                MediaPlaybackStatus::Paused
            };
            let _ = self.controls.SetPlaybackStatus(st);
            self.playing = Some(playing);
        }
    }

    fn show(&self, title: &str, artist: &str, album: &str, cover: Option<&[u8]>) -> windows::core::Result<()> {
        let d = self.controls.DisplayUpdater()?;
        d.SetType(MediaPlaybackType::Music)?;
        let m = d.MusicProperties()?;
        m.SetTitle(&HSTRING::from(title))?;
        m.SetArtist(&HSTRING::from(artist))?;
        m.SetAlbumTitle(&HSTRING::from(album))?;
        match cover {
            Some(bytes) if !bytes.is_empty() => {
                let stream = InMemoryRandomAccessStream::new()?;
                let w = DataWriter::CreateDataWriter(&stream.GetOutputStreamAt(0)?)?;
                w.WriteBytes(bytes)?;
                w.StoreAsync()?.join()?;
                w.FlushAsync()?.join()?;
                let _ = w.DetachStream();
                d.SetThumbnail(&RandomAccessStreamReference::CreateFromStream(&stream)?)?;
            }
            _ => d.SetThumbnail(None::<&RandomAccessStreamReference>)?,
        }
        d.Update()
    }
}

impl Drop for Smtc {
    fn drop(&mut self) {
        let _ = self.controls.RemoveButtonPressed(self.token);
        let _ = self.controls.SetPlaybackStatus(MediaPlaybackStatus::Closed);
        let _ = self.controls.SetIsEnabled(false);
    }
}
