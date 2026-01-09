use cadenza_domain_eval::{
    AdvanceMode, ChordRollTicks, Grade, Judge, JudgeConfig, JudgeEvent, PlayerNoteOn,
    TimingWindowTicks, WrongNotePolicy,
};
use cadenza_domain_score::TargetEvent;

fn target(id: u64, tick: i64, notes: &[u8]) -> TargetEvent {
    TargetEvent {
        id,
        tick,
        notes: notes.to_vec(),
        hand: None,
        measure_index: None,
    }
}

#[test]
fn perfect_hit_single_note() {
    let cfg = JudgeConfig {
        window: TimingWindowTicks { perfect: 5, good: 10 },
        chord_roll: ChordRollTicks(4),
        wrong_note_policy: WrongNotePolicy::RecordOnly,
        advance: AdvanceMode::OnResolve,
    };
    let mut judge = Judge::new(cfg);
    judge.load_targets(vec![target(1, 100, &[60])]);

    let events = judge.on_note_on(PlayerNoteOn {
        tick: 100,
        note: 60,
        velocity: 100,
    });

    assert!(events.iter().any(|event| matches!(
        event,
        JudgeEvent::Hit {
            target_id: 1,
            grade: Grade::Perfect,
            ..
        }
    )));
}

#[test]
fn wrong_note_degrades_perfect() {
    let cfg = JudgeConfig {
        window: TimingWindowTicks { perfect: 3, good: 8 },
        chord_roll: ChordRollTicks(4),
        wrong_note_policy: WrongNotePolicy::DegradePerfect,
        advance: AdvanceMode::OnResolve,
    };
    let mut judge = Judge::new(cfg);
    judge.load_targets(vec![target(1, 200, &[64])]);

    judge.on_note_on(PlayerNoteOn {
        tick: 200,
        note: 65,
        velocity: 100,
    });
    let events = judge.on_note_on(PlayerNoteOn {
        tick: 200,
        note: 64,
        velocity: 100,
    });

    assert!(events.iter().any(|event| matches!(
        event,
        JudgeEvent::Hit {
            target_id: 1,
            grade: Grade::Good,
            ..
        }
    )));
}

#[test]
fn chord_roll_allows_split_hits() {
    let cfg = JudgeConfig {
        window: TimingWindowTicks { perfect: 2, good: 6 },
        chord_roll: ChordRollTicks(3),
        wrong_note_policy: WrongNotePolicy::RecordOnly,
        advance: AdvanceMode::OnResolve,
    };
    let mut judge = Judge::new(cfg);
    judge.load_targets(vec![target(1, 300, &[60, 64])]);

    judge.on_note_on(PlayerNoteOn {
        tick: 300,
        note: 60,
        velocity: 100,
    });
    let events = judge.on_note_on(PlayerNoteOn {
        tick: 302,
        note: 64,
        velocity: 100,
    });

    assert!(events.iter().any(|event| matches!(
        event,
        JudgeEvent::Hit {
            target_id: 1,
            grade: Grade::Perfect,
            ..
        }
    )));
}

#[test]
fn advance_to_emits_miss_after_window() {
    let cfg = JudgeConfig {
        window: TimingWindowTicks { perfect: 2, good: 6 },
        chord_roll: ChordRollTicks(3),
        wrong_note_policy: WrongNotePolicy::RecordOnly,
        advance: AdvanceMode::OnResolve,
    };
    let mut judge = Judge::new(cfg);
    judge.load_targets(vec![target(1, 100, &[60])]);

    let events = judge.advance_to(200);

    assert!(events.iter().any(|event| matches!(
        event,
        JudgeEvent::Miss {
            target_id: 1,
            ..
        }
    )));
}
