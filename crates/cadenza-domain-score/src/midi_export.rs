use crate::model::{PlaybackMidiEvent, Score};
use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::types::Tick;
use midly::num::{u28, u4, u7};
use midly::{Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use std::path::Path;

#[derive(thiserror::Error, Debug)]
pub enum MidiExportError {
    #[error("io error: {0}")]
    Io(String),
    #[error("invalid score: {0}")]
    InvalidScore(String),
}

pub fn export_midi_path(score: &Score, path: &Path) -> Result<(), MidiExportError> {
    let track = score
        .tracks
        .first()
        .ok_or_else(|| MidiExportError::InvalidScore("no tracks".to_string()))?;

    let mut events = build_events(score, &track.playback_events);
    events.sort_by(|a, b| {
        a.tick
            .cmp(&b.tick)
            .then_with(|| track_event_rank(&a.kind).cmp(&track_event_rank(&b.kind)))
    });

    let mut track_events = Vec::new();
    let mut last_tick: Tick = 0;
    for event in events {
        let delta = (event.tick - last_tick).max(0) as u32;
        last_tick = event.tick;
        let delta = u28::new(delta);
        track_events.push(TrackEvent {
            delta,
            kind: event.kind,
        });
    }

    track_events.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    let smf = Smf {
        header: Header {
            format: midly::Format::SingleTrack,
            timing: Timing::Metrical(score.ppq.into()),
        },
        tracks: vec![track_events],
    };

    let mut data = Vec::new();
    smf.write(&mut data)
        .map_err(|e| MidiExportError::Io(e.to_string()))?;
    std::fs::write(path, data).map_err(|e| MidiExportError::Io(e.to_string()))
}

struct MidiEvent {
    tick: Tick,
    kind: TrackEventKind<'static>,
}

fn track_event_rank(kind: &TrackEventKind<'static>) -> (u8, u8, u8) {
    match kind {
        TrackEventKind::Meta(MetaMessage::Tempo(_)) => (0, 0, 0),
        TrackEventKind::Meta(_) => (0, 1, 0),
        TrackEventKind::Midi { message, .. } => match message {
            MidiMessage::Controller { controller, value } if controller.as_int() == 64 => {
                let rank = if value.as_int() >= 64 { 0 } else { 3 };
                (1, rank, 0)
            }
            MidiMessage::NoteOff { key, .. } => (1, 1, key.as_int()),
            MidiMessage::NoteOn { key, vel } => {
                if vel.as_int() == 0 {
                    (1, 1, key.as_int())
                } else {
                    (1, 2, key.as_int())
                }
            }
            _ => (1, 4, 0),
        },
        _ => (2, 0, 0),
    }
}

fn build_events(score: &Score, playback_events: &[PlaybackMidiEvent]) -> Vec<MidiEvent> {
    let mut events = Vec::new();
    let channel = u4::new(0);

    for tempo in &score.tempo_map {
        let tick = tempo.tick;
        let tempo = MetaMessage::Tempo(midly::num::u24::new(tempo.us_per_quarter));
        events.push(MidiEvent {
            tick,
            kind: TrackEventKind::Meta(tempo),
        });
    }

    for event in playback_events {
        let kind = match event.event {
            MidiLikeEvent::NoteOn { note, velocity } => TrackEventKind::Midi {
                channel,
                message: MidiMessage::NoteOn {
                    key: u7::new(note),
                    vel: u7::new(velocity.max(1)),
                },
            },
            MidiLikeEvent::NoteOff { note } => TrackEventKind::Midi {
                channel,
                message: MidiMessage::NoteOff {
                    key: u7::new(note),
                    vel: u7::new(64),
                },
            },
            MidiLikeEvent::Cc64 { value } => TrackEventKind::Midi {
                channel,
                message: MidiMessage::Controller {
                    controller: u7::new(64),
                    value: u7::new(value),
                },
            },
        };
        events.push(MidiEvent {
            tick: event.tick,
            kind,
        });
    }

    events
}
