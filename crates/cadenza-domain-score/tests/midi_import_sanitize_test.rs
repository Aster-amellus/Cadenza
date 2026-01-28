use cadenza_domain_score::import_midi_bytes;
use cadenza_ports::midi::MidiLikeEvent;
use midly::num::{u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};

fn build_midi(track: Vec<TrackEvent<'static>>) -> Vec<u8> {
    let smf = Smf {
        header: Header {
            format: Format::SingleTrack,
            timing: Timing::Metrical(480.into()),
        },
        tracks: vec![track],
    };
    let mut data = Vec::new();
    smf.write(&mut data).expect("midi write should succeed");
    data
}

#[test]
fn midi_import_inserts_noteoff_before_overlapping_noteon() {
    let channel = u4::new(0);
    let key = u7::new(60);
    let vel = u7::new(100);
    let track = vec![
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Midi {
                channel,
                message: MidiMessage::NoteOn { key, vel },
            },
        },
        // Second NoteOn without a NoteOff for the first note.
        TrackEvent {
            delta: u28::new(480),
            kind: TrackEventKind::Midi {
                channel,
                message: MidiMessage::NoteOn { key, vel },
            },
        },
        // Only one NoteOff exists in the source.
        TrackEvent {
            delta: u28::new(480),
            kind: TrackEventKind::Midi {
                channel,
                message: MidiMessage::NoteOff {
                    key,
                    vel: u7::new(64),
                },
            },
        },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        },
    ];

    let midi = build_midi(track);
    let score = import_midi_bytes(&midi).expect("import should succeed");
    let events = &score.tracks[0].playback_events;

    let at_480: Vec<_> = events.iter().filter(|e| e.tick == 480).collect();
    assert_eq!(at_480.len(), 2);
    assert!(matches!(
        at_480[0].event,
        MidiLikeEvent::NoteOff { note: 60 }
    ));
    assert!(matches!(
        at_480[1].event,
        MidiLikeEvent::NoteOn { note: 60, .. }
    ));
    assert!(events
        .iter()
        .any(|e| e.tick == 960 && matches!(e.event, MidiLikeEvent::NoteOff { note: 60 })));
}

#[test]
fn midi_import_closes_dangling_notes_at_end() {
    let channel = u4::new(0);
    let key = u7::new(60);
    let vel = u7::new(100);
    let track = vec![
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Midi {
                channel,
                message: MidiMessage::NoteOn { key, vel },
            },
        },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        },
    ];

    let midi = build_midi(track);
    let score = import_midi_bytes(&midi).expect("import should succeed");
    let events = &score.tracks[0].playback_events;

    assert!(events
        .iter()
        .any(|e| e.tick == 480 && matches!(e.event, MidiLikeEvent::NoteOff { note: 60 })));
}
