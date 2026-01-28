use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::types::Tick;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Hand {
    Left,
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoreMeta {
    pub title: Option<String>,
    pub source: ScoreSource,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ScoreSource {
    Midi,
    MusicXml,
    PdfOmr,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TempoPoint {
    pub tick: Tick,
    pub us_per_quarter: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Score {
    pub meta: ScoreMeta,
    pub ppq: u16,
    pub tempo_map: Vec<TempoPoint>,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub id: u32,
    pub name: String,
    pub hand: Option<Hand>,
    pub targets: Vec<TargetEvent>,
    pub playback_events: Vec<PlaybackMidiEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetEvent {
    pub id: u64,
    pub tick: Tick,
    pub notes: Vec<u8>,
    pub hand: Option<Hand>,
    pub measure_index: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaybackMidiEvent {
    pub tick: Tick,
    pub event: MidiLikeEvent,
    pub hand: Option<Hand>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoreFile {
    pub schema_version: String,
    pub score: Score,
    pub edit_log: Vec<String>,
}

impl Score {
    pub fn new(meta: ScoreMeta, ppq: u16) -> Self {
        Self {
            meta,
            ppq,
            tempo_map: vec![TempoPoint {
                tick: 0,
                us_per_quarter: 500_000,
            }],
            tracks: Vec::new(),
        }
    }
}
