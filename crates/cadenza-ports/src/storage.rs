use crate::types::*;
use serde::{Deserialize, Serialize};

fn default_monitor_enabled() -> bool {
    true
}

fn default_master_volume() -> Volume01 {
    Volume01::new(0.8)
}

fn default_bus_user_volume() -> Volume01 {
    Volume01::new(0.8)
}

fn default_bus_autopilot_volume() -> Volume01 {
    Volume01::new(0.8)
}

fn default_bus_metronome_volume() -> Volume01 {
    Volume01::new(0.6)
}

#[derive(thiserror::Error, Debug)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(String),
    #[error("serialization error: {0}")]
    Serde(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsDto {
    pub selected_midi_in: Option<DeviceId>,
    pub selected_audio_out: Option<DeviceId>,
    pub audio_buffer_size_frames: Option<u32>,
    #[serde(default = "default_monitor_enabled")]
    pub monitor_enabled: bool,
    #[serde(default = "default_master_volume")]
    pub master_volume: Volume01,
    #[serde(default = "default_bus_user_volume")]
    pub bus_user_volume: Volume01,
    #[serde(default = "default_bus_autopilot_volume")]
    pub bus_autopilot_volume: Volume01,
    #[serde(default = "default_bus_metronome_volume")]
    pub bus_metronome_volume: Volume01,
    pub input_offset_ms: i32,
    pub default_sf2_path: Option<String>,
    pub audiveris_path: Option<String>,
}

impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            selected_midi_in: None,
            selected_audio_out: None,
            audio_buffer_size_frames: None,
            monitor_enabled: true,
            master_volume: Volume01::new(0.8),
            bus_user_volume: Volume01::new(0.8),
            bus_autopilot_volume: Volume01::new(0.8),
            bus_metronome_volume: Volume01::new(0.6),
            input_offset_ms: 0,
            default_sf2_path: None,
            audiveris_path: None,
        }
    }
}

pub trait StoragePort: Send + Sync {
    fn load_settings(&self) -> Result<SettingsDto, StorageError>;
    fn save_settings(&self, s: &SettingsDto) -> Result<(), StorageError>;
}
