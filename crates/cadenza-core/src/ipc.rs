use cadenza_domain_eval::Grade;
use cadenza_domain_score::Hand;
use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::playback::{LoopRange, PlaybackMode};
use cadenza_ports::storage::SettingsDto;
use cadenza_ports::types::{
    AudioConfig, AudioOutputDevice, Bus, DeviceId, MidiInputDevice, SampleTime, Tick, Volume01,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PianoRollNoteDto {
    pub note: u8,
    pub start_tick: Tick,
    pub end_tick: Tick,
    pub velocity: u8,
    pub hand: Option<Hand>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PianoRollPedalDto {
    pub start_tick: Tick,
    pub end_tick: Tick,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PianoRollTargetDto {
    pub id: u64,
    pub tick: Tick,
    pub notes: Vec<u8>,
}

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
    GetSessionState,
    ListMidiInputs,
    SelectMidiInput {
        device_id: DeviceId,
    },
    ListAudioOutputs,
    SelectAudioOutput {
        device_id: DeviceId,
        config: Option<AudioConfig>,
    },
    TestAudio,
    SetMonitorEnabled {
        enabled: bool,
    },
    SetBusVolume {
        bus: Bus,
        volume: Volume01,
    },
    SetMasterVolume {
        volume: Volume01,
    },
    LoadSoundFont {
        path: String,
    },
    SetProgram {
        bus: Bus,
        gm_program: u8,
    },
    LoadScore {
        source: ScoreSource,
    },
    SetPracticeRange {
        start_tick: Tick,
        end_tick: Tick,
    },
    StartPractice,
    PausePractice,
    StopPractice,
    Seek {
        tick: Tick,
    },
    SetLoop {
        enabled: bool,
        start_tick: Tick,
        end_tick: Tick,
    },
    SetTempoMultiplier {
        x: f32,
    },
    SetPlaybackMode {
        mode: PlaybackMode,
    },
    SetAccompanimentRoute {
        play_left: bool,
        play_right: bool,
    },
    SetInputOffsetMs {
        ms: i32,
    },
    SetAudiverisPath {
        path: String,
    },
    ConvertPdfToMidi {
        pdf_path: String,
        output_path: String,
        audiveris_path: Option<String>,
    },
    CancelPdfToMidi,
    ExportDiagnostics {
        path: String,
    },
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
    ScoreViewUpdated {
        title: Option<String>,
        ppq: u16,
        notes: Vec<PianoRollNoteDto>,
        targets: Vec<PianoRollTargetDto>,
        pedal: Vec<PianoRollPedalDto>,
    },
    MidiInputsUpdated {
        devices: Vec<MidiInputDevice>,
    },
    AudioOutputsUpdated {
        devices: Vec<AudioOutputDevice>,
    },
    SessionStateUpdated {
        state: SessionState,
        settings: SettingsDto,
    },
    SoundFontStatus {
        loaded: bool,
        path: Option<String>,
        name: Option<String>,
        preset_count: Option<u32>,
        message: Option<String>,
    },
    OmrProgress {
        page: u32,
        total: u32,
        stage: String,
    },
    OmrDiagnostics {
        severity: String,
        message: String,
        page: Option<u32>,
    },
    PdfToMidiFinished {
        ok: bool,
        pdf_path: String,
        output_path: String,
        musicxml_path: Option<String>,
        diagnostics_path: Option<String>,
        message: String,
    },
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
    ScoreSummaryUpdated {
        combo: u32,
        score: i64,
        accuracy: f32,
    },
    MidiInputEvent {
        event: MidiLikeEvent,
    },
    RecentInputEvents {
        events: Vec<MidiLikeEvent>,
    },
}
