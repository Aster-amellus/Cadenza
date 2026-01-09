use cadenza_domain_eval::Grade;
use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::playback::{LoopRange, PlaybackMode};
use cadenza_ports::storage::SettingsDto;
use cadenza_ports::types::{AudioConfig, AudioOutputDevice, Bus, DeviceId, MidiInputDevice, SampleTime, Tick, Volume01};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum ScoreSource {
    MidiFile(String),
    MusicXmlFile(String),
    InternalDemo(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Command {
    ListMidiInputs,
    SelectMidiInput { device_id: DeviceId },
    ListAudioOutputs,
    SelectAudioOutput { device_id: DeviceId, config: Option<AudioConfig> },
    SetMonitorEnabled { enabled: bool },
    SetBusVolume { bus: Bus, volume: Volume01 },
    SetMasterVolume { volume: Volume01 },
    LoadSoundFont { path: String },
    SetProgram { bus: Bus, gm_program: u8 },
    LoadScore { source: ScoreSource },
    SetPracticeRange { start_tick: Tick, end_tick: Tick },
    StartPractice,
    PausePractice,
    StopPractice,
    Seek { tick: Tick },
    SetLoop { enabled: bool, start_tick: Tick, end_tick: Tick },
    SetTempoMultiplier { x: f32 },
    SetPlaybackMode { mode: PlaybackMode },
    SetAccompanimentRoute { play_left: bool, play_right: bool },
    SetInputOffsetMs { ms: i32 },
    ExportDiagnostics { path: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Idle,
    Ready,
    Running,
    Paused,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Event {
    MidiInputsUpdated { devices: Vec<MidiInputDevice> },
    AudioOutputsUpdated { devices: Vec<AudioOutputDevice> },
    SessionStateUpdated { state: SessionState, settings: SettingsDto },
    TransportUpdated {
        tick: Tick,
        sample_time: SampleTime,
        playing: bool,
        tempo_multiplier: f32,
        loop_range: Option<LoopRange>,
    },
    JudgeFeedback {
        target_id: u64,
        grade: Grade,
        delta_tick: i64,
        expected_notes: Vec<u8>,
        played_notes: Vec<u8>,
    },
    ScoreSummaryUpdated { combo: u32, score: i64, accuracy: f32 },
    RecentInputEvents { events: Vec<MidiLikeEvent> },
}
