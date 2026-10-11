//! WASAPI 输出设备枚举（原版"音频设备"设置页）。

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ};
use windows::core::{HSTRING, PWSTR};

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
}

pub(super) fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

fn take_pwstr(p: PWSTR) -> String {
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// 列出可用的输出设备。
pub fn list_devices() -> Vec<DeviceInfo> {
    let r = (|| -> windows::core::Result<Vec<DeviceInfo>> {
        unsafe {
            let col = enumerator()?.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
            let mut v = Vec::new();
            for i in 0..col.GetCount()? {
                let dev = col.Item(i)?;
                let id = take_pwstr(dev.GetId()?);
                let name = dev
                    .OpenPropertyStore(STGM_READ)
                    .and_then(|ps| ps.GetValue(&PKEY_Device_FriendlyName))
                    .map(|v| v.to_string())
                    .unwrap_or_else(|_| id.clone());
                v.push(DeviceInfo { id, name });
            }
            Ok(v)
        }
    })();
    r.unwrap_or_else(|e| {
        log::warn!("枚举音频设备失败: {e}");
        Vec::new()
    })
}

pub fn default_device_id() -> Option<String> {
    unsafe {
        let dev = enumerator().ok()?.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        dev.GetId().ok().map(take_pwstr)
    }
}

pub(super) fn get_device(id: &str) -> windows::core::Result<IMMDevice> {
    let e = enumerator()?;
    unsafe { if id.is_empty() { e.GetDefaultAudioEndpoint(eRender, eConsole) } else { e.GetDevice(&HSTRING::from(id)) } }
}
