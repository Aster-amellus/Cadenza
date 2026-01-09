use crate::midi::MidiLikeEvent;
use crate::types::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackMode {
    Demo,
    Accompaniment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopRange {
    pub start_tick: Tick,
    pub end_tick: Tick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hand {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackRouteHint {
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TempoPoint {
    pub tick: Tick,
    pub us_per_quarter: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaybackEvent {
    pub tick: Tick,
    pub event: MidiLikeEvent,
    pub route_hint: PlaybackRouteHint,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaybackScore {
    pub ppq: u16,
    pub tempo_map: Vec<TempoPoint>,
    pub events: Vec<PlaybackEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledEvent {
    pub sample_time: SampleTime,
    pub bus: Bus,
    pub event: MidiLikeEvent,
}

#[derive(thiserror::Error, Debug)]
pub enum PlaybackError {
    #[error("invalid score: {0}")]
    InvalidScore(String),
    #[error("backend error: {0}")]
    Backend(String),
}

pub trait PlaybackPort: Send + Sync {
    fn load_score(&self, score: PlaybackScore) -> Result<(), PlaybackError>;

    fn play(&self) -> Result<(), PlaybackError>;
    fn pause(&self) -> Result<(), PlaybackError>;
    fn stop(&self) -> Result<(), PlaybackError>;

    fn seek(&self, tick: Tick) -> Result<(), PlaybackError>;
    fn set_loop(&self, range: Option<LoopRange>) -> Result<(), PlaybackError>;
    fn set_tempo_multiplier(&self, multiplier: f32) -> Result<(), PlaybackError>;
    fn set_mode(&self, mode: PlaybackMode) -> Result<(), PlaybackError>;

    fn poll_scheduled_events(&self, window_samples: u64) -> Result<Vec<ScheduledEvent>, PlaybackError>;
}
