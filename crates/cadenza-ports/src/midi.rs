use crate::types::*;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MidiLikeEvent {
    NoteOn {
        note: u8,
        velocity: u8,
    },
    NoteOff {
        note: u8,
    },
    /// CC64: value 0..127. pedal_down = value >= 64
    Cc64 {
        value: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventSource {
    User,
    Autopilot,
    Metronome,
}

/// Raw input from MIDI devices, not mapped to Tick yet.
#[derive(Clone, Copy, Debug)]
pub struct PlayerEvent {
    pub at: Instant,
    pub event: MidiLikeEvent,
}

#[derive(thiserror::Error, Debug)]
pub enum MidiError {
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("device unavailable: {0}")]
    DeviceUnavailable(String),
    #[error("backend error: {0}")]
    Backend(String),
}

/// MIDI input stream handle: drop closes it.
pub trait MidiInputStream: Send {
    fn close(self: Box<Self>);
}

pub type PlayerEventCallback = Arc<dyn Fn(PlayerEvent) + Send + Sync + 'static>;

pub trait MidiInputPort: Send + Sync {
    fn list_inputs(&self) -> Result<Vec<MidiInputDevice>, MidiError>;

    /// Open input stream: implementation should invoke cb from a background thread/callback.
    fn open_input(
        &self,
        device_id: &DeviceId,
        cb: PlayerEventCallback,
    ) -> Result<Box<dyn MidiInputStream>, MidiError>;
}
