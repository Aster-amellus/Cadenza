use cadenza_domain_score::TargetEvent;
use cadenza_ports::types::Tick;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug)]
pub struct TimingWindowTicks {
    pub perfect: i64,
    pub good: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct ChordRollTicks(pub i64);

#[derive(Clone, Copy, Debug)]
pub enum WrongNotePolicy {
    RecordOnly,
    DegradePerfect,
}

#[derive(Clone, Copy, Debug)]
pub enum AdvanceMode {
    OnResolve,
    Aggressive,
}

#[derive(Clone, Copy, Debug)]
pub struct JudgeConfig {
    pub window: TimingWindowTicks,
    pub chord_roll: ChordRollTicks,
    pub wrong_note_policy: WrongNotePolicy,
    pub advance: AdvanceMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grade {
    Perfect,
    Good,
    Miss,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissReason {
    Timeout,
    Skipped,
}

#[derive(Clone, Debug)]
pub enum JudgeEvent {
    FocusChanged { target_id: Option<u64> },
    Hit {
        target_id: u64,
        grade: Grade,
        delta_tick: i64,
        wrong_notes: u32,
    },
    Miss {
        target_id: u64,
        reason: MissReason,
        missing_notes: u32,
        wrong_notes: u32,
    },
    Stats {
        combo: u32,
        score: i64,
        hit: u32,
        miss: u32,
        wrong: u32,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct PlayerNoteOn {
    pub tick: Tick,
    pub note: u8,
    pub velocity: u8,
}

#[derive(Default, Debug)]
struct StatsState {
    combo: u32,
    score: i64,
    hit: u32,
    miss: u32,
    wrong: u32,
}

#[derive(Debug)]
struct TargetState {
    expected: HashSet<u8>,
    matched: HashMap<u8, Tick>,
    wrong_notes: u32,
    first_match_tick: Option<Tick>,
}

pub struct Judge {
    cfg: JudgeConfig,
    targets: Vec<TargetEvent>,
    idx: usize,
    state: Option<TargetState>,
    stats: StatsState,
}

impl Judge {
    pub fn new(cfg: JudgeConfig) -> Self {
        Self {
            cfg,
            targets: Vec::new(),
            idx: 0,
            state: None,
            stats: StatsState::default(),
        }
    }

    pub fn load_targets(&mut self, targets: Vec<TargetEvent>) -> Vec<JudgeEvent> {
        self.targets = targets;
        self.idx = 0;
        self.state = self.build_state();
        vec![JudgeEvent::FocusChanged {
            target_id: self.current_focus(),
        }]
    }

    pub fn on_note_on(&mut self, e: PlayerNoteOn) -> Vec<JudgeEvent> {
        let mut events = self.advance_to(e.tick);
        let Some(target) = self.current_target() else {
            return events;
        };

        let target_id = target.id;
        let target_tick = target.tick;
        let good = self.cfg.window.good;
        let perfect = self.cfg.window.perfect;
        let window_start = target_tick - good;
        let window_end = target_tick + good;
        let mut resolved: Option<(Grade, i64, u32)> = None;

        if e.tick < window_start {
            return events;
        }

        if let Some(state) = self.state.as_mut() {
            if e.tick <= window_end {
                if state.expected.contains(&e.note) && !state.matched.contains_key(&e.note) {
                    let within_roll = match state.first_match_tick {
                        Some(first) => (e.tick - first).abs() <= self.cfg.chord_roll.0,
                        None => true,
                    };
                    if within_roll {
                        state.matched.insert(e.note, e.tick);
                        if state.first_match_tick.is_none() {
                            state.first_match_tick = Some(e.tick);
                        }
                    }
                } else if !state.expected.contains(&e.note) {
                    state.wrong_notes += 1;
                }
            }

            if state.matched.len() == state.expected.len() && !state.expected.is_empty() {
                let first_match = state.first_match_tick.unwrap_or(target_tick);
                let delta = first_match - target_tick;
                let mut grade = if delta.abs() <= perfect {
                    Grade::Perfect
                } else {
                    Grade::Good
                };

                if matches!(self.cfg.wrong_note_policy, WrongNotePolicy::DegradePerfect)
                    && state.wrong_notes > 0
                    && grade == Grade::Perfect
                {
                    grade = Grade::Good;
                }

                resolved = Some((grade, delta, state.wrong_notes));
            }
        }

        if let Some((grade, delta, wrong_notes)) = resolved {
            events.push(JudgeEvent::Hit {
                target_id,
                grade,
                delta_tick: delta,
                wrong_notes,
            });

            self.update_stats_on_hit(grade, wrong_notes, &mut events);
            self.advance_focus(&mut events);
        }

        events
    }

    pub fn advance_to(&mut self, now_tick: Tick) -> Vec<JudgeEvent> {
        let mut events = Vec::new();
        loop {
            let Some(target) = self.current_target() else {
                break;
            };
            let Some(state) = self.state.as_ref() else {
                break;
            };

            let good = self.cfg.window.good;
            if now_tick <= target.tick + good {
                break;
            }

            let missing_notes = state.expected.len().saturating_sub(state.matched.len()) as u32;
            let wrong_notes = state.wrong_notes;
            let target_id = target.id;

            events.push(JudgeEvent::Miss {
                target_id,
                reason: MissReason::Timeout,
                missing_notes,
                wrong_notes,
            });

            self.update_stats_on_miss(wrong_notes, &mut events);
            self.advance_focus(&mut events);
        }

        events
    }

    pub fn current_focus(&self) -> Option<u64> {
        self.targets.get(self.idx).map(|t| t.id)
    }

    fn current_target(&self) -> Option<&TargetEvent> {
        self.targets.get(self.idx)
    }

    fn build_state(&self) -> Option<TargetState> {
        let target = self.targets.get(self.idx)?;
        let expected: HashSet<u8> = target.notes.iter().copied().collect();
        Some(TargetState {
            expected,
            matched: HashMap::new(),
            wrong_notes: 0,
            first_match_tick: None,
        })
    }

    fn advance_focus(&mut self, events: &mut Vec<JudgeEvent>) {
        self.idx = self.idx.saturating_add(1);
        self.state = self.build_state();
        events.push(JudgeEvent::FocusChanged {
            target_id: self.current_focus(),
        });
    }

    fn update_stats_on_hit(&mut self, grade: Grade, wrong_notes: u32, events: &mut Vec<JudgeEvent>) {
        self.stats.hit += 1;
        self.stats.combo += 1;
        self.stats.wrong += wrong_notes;
        self.stats.score += match grade {
            Grade::Perfect => 100,
            Grade::Good => 70,
            Grade::Miss => 0,
        };
        events.push(self.stats_event());
    }

    fn update_stats_on_miss(&mut self, wrong_notes: u32, events: &mut Vec<JudgeEvent>) {
        self.stats.miss += 1;
        self.stats.combo = 0;
        self.stats.wrong += wrong_notes;
        events.push(self.stats_event());
    }

    fn stats_event(&self) -> JudgeEvent {
        JudgeEvent::Stats {
            combo: self.stats.combo,
            score: self.stats.score,
            hit: self.stats.hit,
            miss: self.stats.miss,
            wrong: self.stats.wrong,
        }
    }
}
