use crate::model::{
    PlaybackMidiEvent, Score, ScoreMeta, ScoreSource, TargetEvent, TempoPoint, Track,
};
use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::types::Tick;
use midly::{Fps, MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(thiserror::Error, Debug)]
pub enum MidiImportError {
    #[error("io error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
}

pub fn import_midi_path(path: &Path) -> Result<Score, MidiImportError> {
    let data = std::fs::read(path).map_err(|e| MidiImportError::Io(e.to_string()))?;
    import_midi_bytes(&data)
}

pub fn import_midi_bytes(data: &[u8]) -> Result<Score, MidiImportError> {
    let smf = Smf::parse(data).map_err(|e| MidiImportError::Parse(e.to_string()))?;
    let (ppq, tempo_override) = match smf.header.timing {
        Timing::Metrical(ticks) => (ticks.as_int(), None),
        Timing::Timecode(fps, ticks_per_frame) => {
            let (ppq, us_per_quarter) = timecode_ppq_and_tempo(fps, ticks_per_frame);
            (ppq, Some(us_per_quarter))
        }
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
                                event: MidiLikeEvent::NoteOn { note, velocity },
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

    let tempo_map = build_tempo_map(tempo_points, tempo_override);
    let targets = build_targets(note_on_events);
    playback_events.sort_by(|a, b| {
        a.tick
            .cmp(&b.tick)
            .then_with(|| midi_event_rank(&a.event).cmp(&midi_event_rank(&b.event)))
            .then_with(|| midi_event_note_key(&a.event).cmp(&midi_event_note_key(&b.event)))
    });
    playback_events = sanitize_note_pairs(ppq, playback_events);

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

fn midi_event_rank(event: &MidiLikeEvent) -> u8 {
    match event {
        MidiLikeEvent::Cc64 { value } => {
            if *value >= 64 {
                0
            } else {
                3
            }
        }
        MidiLikeEvent::NoteOff { .. } => 1,
        MidiLikeEvent::NoteOn { .. } => 2,
    }
}

fn midi_event_note_key(event: &MidiLikeEvent) -> u8 {
    match event {
        MidiLikeEvent::NoteOn { note, .. } => *note,
        MidiLikeEvent::NoteOff { note } => *note,
        MidiLikeEvent::Cc64 { .. } => 0,
    }
}

fn build_tempo_map(
    tempo_points: BTreeMap<Tick, u32>,
    override_us_per_quarter: Option<u32>,
) -> Vec<TempoPoint> {
    if let Some(us_per_quarter) = override_us_per_quarter {
        return vec![TempoPoint {
            tick: 0,
            us_per_quarter,
        }];
    }

    let mut map: Vec<TempoPoint> = tempo_points
        .into_iter()
        .map(|(tick, us_per_quarter)| TempoPoint {
            tick,
            us_per_quarter,
        })
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

fn timecode_ppq_and_tempo(fps: Fps, ticks_per_frame: u8) -> (u16, u32) {
    let ticks_per_frame = ticks_per_frame.max(1) as u16;
    match fps {
        Fps::Fps24 => (24 * ticks_per_frame, 1_000_000),
        Fps::Fps25 => (25 * ticks_per_frame, 1_000_000),
        Fps::Fps30 => (30 * ticks_per_frame, 1_000_000),
        Fps::Fps29 => (30 * ticks_per_frame, 1_001_000),
    }
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

fn sanitize_note_pairs(ppq: u16, events: Vec<PlaybackMidiEvent>) -> Vec<PlaybackMidiEvent> {
    if events.is_empty() {
        return events;
    }

    let default_len: Tick = ppq.max(1) as Tick;
    let mut out: Vec<PlaybackMidiEvent> = Vec::with_capacity(events.len() + 64);
    let mut active: [u8; 128] = [0; 128];
    let mut last_tick: Tick = 0;

    for event in events {
        last_tick = last_tick.max(event.tick);
        match event.event {
            MidiLikeEvent::NoteOn { note, velocity: _ } => {
                let idx = note as usize;
                if idx < active.len() {
                    let count = active[idx] as usize;
                    if count > 0 {
                        for _ in 0..count {
                            out.push(PlaybackMidiEvent {
                                tick: event.tick,
                                event: MidiLikeEvent::NoteOff { note },
                                hand: event.hand,
                            });
                        }
                        active[idx] = 0;
                    }
                    active[idx] = active[idx].saturating_add(1);
                }
                out.push(event);
            }
            MidiLikeEvent::NoteOff { note } => {
                let idx = note as usize;
                if idx >= active.len() || active[idx] == 0 {
                    continue;
                }
                active[idx] = active[idx].saturating_sub(1);
                out.push(event);
            }
            MidiLikeEvent::Cc64 { .. } => out.push(event),
        }
    }

    let end_tick = last_tick.saturating_add(default_len.max(1));
    for (note, count) in active.iter().copied().enumerate() {
        for _ in 0..count {
            out.push(PlaybackMidiEvent {
                tick: end_tick,
                event: MidiLikeEvent::NoteOff { note: note as u8 },
                hand: None,
            });
        }
    }

    out.sort_by(|a, b| {
        a.tick
            .cmp(&b.tick)
            .then_with(|| midi_event_rank(&a.event).cmp(&midi_event_rank(&b.event)))
            .then_with(|| midi_event_note_key(&a.event).cmp(&midi_event_note_key(&b.event)))
    });
    out
}
