use crate::midi::MidiLikeEvent;
use crate::types::*;

#[derive(thiserror::Error, Debug)]
pub enum SynthError {
    #[error("soundfont load failed: {0}")]
    SoundFontLoad(String),
    #[error("unsupported soundfont format")]
    UnsupportedFormat,
    #[error("backend error: {0}")]
    Backend(String),
}

#[derive(Clone, Debug)]
pub struct SoundFontInfo {
    pub name: String,
    pub preset_count: usize,
}

/// Thread model:
/// - load_* / set_program are called from core thread (can lock internally)
/// - handle_event/render are called from audio thread (must be realtime-safe)
pub trait SynthPort: Send + Sync {
    fn load_soundfont_from_path(&self, path: &str) -> Result<SoundFontInfo, SynthError>;
    fn set_sample_rate(&self, sample_rate_hz: u32);
    fn set_program(&self, bus: Bus, gm_program: u8) -> Result<(), SynthError>;

    /// Called by audio thread: inject events into synth (per bus state, includes CC64 sustain)
    fn handle_event(&self, bus: Bus, event: MidiLikeEvent, at: SampleTime);

    /// Called by audio thread: render frames to out_l/out_r
    fn render(&self, bus: Bus, frames: usize, out_l: &mut [f32], out_r: &mut [f32]);
}
