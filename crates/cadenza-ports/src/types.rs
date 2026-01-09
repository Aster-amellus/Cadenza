use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc};

pub type Tick = i64; // musical time, monotonic in score
pub type SampleTime = u64; // audio sample index, monotonic while stream running

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Bus {
    UserMonitor,
    Autopilot,
    MetronomeFx,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MidiInputDevice {
    pub id: DeviceId,
    pub name: String,
    pub is_available: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioOutputDevice {
    pub id: DeviceId,
    pub name: String,
    pub default_config: AudioConfig,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct AudioConfig {
    pub sample_rate_hz: u32,
    pub channels: u16, // v1 fixed 2
    pub buffer_size_frames: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Volume01(pub f32);

impl Volume01 {
    pub fn new(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub type Shared<T> = Arc<T>;
