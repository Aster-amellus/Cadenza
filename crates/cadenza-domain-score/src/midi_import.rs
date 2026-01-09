use crate::model::{PlaybackMidiEvent, Score, ScoreMeta, ScoreSource, TargetEvent, TempoPoint, Track};
use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::types::Tick;
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(thiserror::Error, Debug)]
pub enum MidiImportError {
    #[error("io error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("unsupported timing format (smpte)")]
    UnsupportedTiming,
}

pub fn import_midi_path(path: &Path) -> Result<Score, MidiImportError> {
    let data = std::fs::read(path).map_err(|e| MidiImportError::Io(e.to_string()))?;
    import_midi_bytes(&data)
}

pub fn import_midi_bytes(data: &[u8]) -> Result<Score, MidiImportError> {
    let smf = Smf::parse(data).map_err(|e| MidiImportError::Parse(e.to_string()))?;
    let ppq = match smf.header.timing {
        Timing::Metrical(ticks) => ticks.as_int(),
        Timing::Timecode(_, _) => return Err(MidiImportError::UnsupportedTiming),
    };

    let mut tempo_points: BTreeMap<Tick, u32> = BTreeMap::new();
    let mut playback_events: Vec<PlaybackMidiEvent> = Vec::new();
    let mut note_on_events: Vec<(Tick, u8)> = Vec::new();

    for track in &smf.tracks {
        let mut tick: Tick = 0;
        for event in track {
            tick += event.delta.as_int() as Tick;
            match &event.kind {
                TrackEventKind::Midi { message, .. } => match message {
                    MidiMessage::NoteOn { key, vel } => {
                        let note = key.as_int();
                        let velocity = vel.as_int();
                        if velocity == 0 {
                            playback_events.push(PlaybackMidiEvent {
                                tick,
                                event: MidiLikeEvent::NoteOff { note },
                                hand: None,
                            });
                        } else {
                            playback_events.push(PlaybackMidiEvent {
                                tick,
                                event: MidiLikeEvent::NoteOn {
                                    note,
                                    velocity,
                                },
                                hand: None,
                            });
                            note_on_events.push((tick, note));
                        }
                    }
                    MidiMessage::NoteOff { key, .. } => {
                        playback_events.push(PlaybackMidiEvent {
                            tick,
                            event: MidiLikeEvent::NoteOff { note: key.as_int() },
                            hand: None,
                        });
                    }
                    MidiMessage::Controller { controller, value } => {
                        if controller.as_int() == 64 {
                            playback_events.push(PlaybackMidiEvent {
                                tick,
                                event: MidiLikeEvent::Cc64 {
                                    value: value.as_int(),
                                },
                                hand: None,
                            });
                        }
                    }
                    _ => {}
                },
                TrackEventKind::Meta(MetaMessage::Tempo(us_per_quarter)) => {
                    tempo_points.insert(tick, us_per_quarter.as_int());
                }
                _ => {}
            }
        }
    }

    let tempo_map = build_tempo_map(tempo_points);
    let targets = build_targets(note_on_events);
    let mut playback_events = playback_events;
    playback_events.sort_by_key(|event| event.tick);

    let track = Track {
        id: 0,
        name: "Merged".to_string(),
        hand: None,
        targets,
        playback_events,
    };

    let score = Score {
        meta: ScoreMeta {
            title: None,
            source: ScoreSource::Midi,
        },
        ppq,
        tempo_map,
        tracks: vec![track],
    };

    Ok(score)
}

fn build_tempo_map(tempo_points: BTreeMap<Tick, u32>) -> Vec<TempoPoint> {
    let mut map: Vec<TempoPoint> = tempo_points
        .into_iter()
        .map(|(tick, us_per_quarter)| TempoPoint { tick, us_per_quarter })
        .collect();

    if map.is_empty() || map[0].tick != 0 {
        map.insert(
            0,
            TempoPoint {
                tick: 0,
                us_per_quarter: 500_000,
            },
        );
    }

    map.sort_by_key(|point| point.tick);
    map
}

fn build_targets(mut note_on_events: Vec<(Tick, u8)>) -> Vec<TargetEvent> {
    if note_on_events.is_empty() {
        return Vec::new();
    }

    note_on_events.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    let mut targets = Vec::new();
    let mut current_tick = note_on_events[0].0;
    let mut notes: Vec<u8> = Vec::new();
    let mut next_id: u64 = 1;

    for (tick, note) in note_on_events {
        if tick != current_tick {
            notes.sort_unstable();
            notes.dedup();
            targets.push(TargetEvent {
                id: next_id,
                tick: current_tick,
                notes: notes.clone(),
                hand: None,
                measure_index: None,
            });
            next_id += 1;
            notes.clear();
            current_tick = tick;
        }
        notes.push(note);
    }

    if !notes.is_empty() {
        notes.sort_unstable();
        notes.dedup();
        targets.push(TargetEvent {
            id: next_id,
            tick: current_tick,
            notes,
            hand: None,
            measure_index: None,
        });
    }

    targets
}
