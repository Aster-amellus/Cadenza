use cadenza_domain_score::{
    export_midi_path, import_midi_path, PlaybackMidiEvent, Score, ScoreMeta, ScoreSource,
    TargetEvent, TempoPoint, Track,
};
use cadenza_ports::midi::MidiLikeEvent;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_midi_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("cadenza-{name}-{nanos}.mid"))
}

#[test]
fn midi_export_import_roundtrip() {
    let path = temp_midi_path("midi-roundtrip");

    let ppq = 480u16;
    let playback_events = vec![
        PlaybackMidiEvent {
            tick: 0,
            event: MidiLikeEvent::NoteOn {
                note: 60,
                velocity: 100,
            },
            hand: None,
        },
        PlaybackMidiEvent {
            tick: 480,
            event: MidiLikeEvent::NoteOff { note: 60 },
            hand: None,
        },
    ];

    let track = Track {
        id: 0,
        name: "Test".to_string(),
        hand: None,
        targets: vec![TargetEvent {
            id: 1,
            tick: 0,
            notes: vec![60],
            hand: None,
            measure_index: None,
        }],
        playback_events,
    };

    let score = Score {
        meta: ScoreMeta {
            title: Some("Roundtrip".to_string()),
            source: ScoreSource::Internal,
        },
        ppq,
        tempo_map: vec![TempoPoint {
            tick: 0,
            us_per_quarter: 500_000,
        }],
        tracks: vec![track],
    };

    export_midi_path(&score, &path).expect("export should succeed");

    let loaded = import_midi_path(&path).expect("import should succeed");
    assert_eq!(loaded.ppq, ppq);
    assert!(!loaded.tracks.is_empty());
    assert!(loaded.tracks[0]
        .playback_events
        .iter()
        .any(|e| matches!(e.event, MidiLikeEvent::NoteOn { .. })));

    let _ = std::fs::remove_file(&path);
}
