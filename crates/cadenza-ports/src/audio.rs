use crate::types::*;
use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum AudioError {
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("device unavailable: {0}")]
    DeviceUnavailable(String),
    #[error("unsupported config: {0}")]
    UnsupportedConfig(String),
    #[error("backend error: {0}")]
    Backend(String),
}

/// Audio callback: must be realtime-safe.
pub trait AudioRenderCallback: Send + Sync + 'static {
    fn render(&self, sample_time_start: SampleTime, out_l: &mut [f32], out_r: &mut [f32]);
}

pub trait AudioStreamHandle: Send {
    fn close(self: Box<Self>);
}

pub trait AudioOutputPort: Send + Sync {
    fn list_outputs(&self) -> Result<Vec<AudioOutputDevice>, AudioError>;

    fn open_output(
        &self,
        device_id: &DeviceId,
        config: AudioConfig,
        cb: Arc<dyn AudioRenderCallback>,
    ) -> Result<Box<dyn AudioStreamHandle>, AudioError>;
}
