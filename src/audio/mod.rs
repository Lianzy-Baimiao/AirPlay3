//! 播放引擎：解码（symphonia / Media Foundation）→ 样本队列 → 均衡器/音量 → WASAPI（共享/独占）。

mod decoder;
pub mod devices;
pub mod dsp;
mod engine;
mod output;
pub mod presets;
mod render;
mod ring;
pub mod source;
mod source_mf;

pub use devices::{DeviceInfo, list_devices};
pub use engine::{Engine, Event, NextProvider, PlayRequest, PlayState};
pub use source::probe_duration;
