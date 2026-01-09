use crate::transport::Transport;
use cadenza_domain_score::{Hand, PlaybackMidiEvent};
use cadenza_ports::playback::{LoopRange, PlaybackMode, ScheduledEvent};
use cadenza_ports::types::Bus;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug)]
pub struct SchedulerConfig {
    pub lookahead_ms: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct AccompanimentRoute {
    pub play_left: bool,
    pub play_right: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct PlaybackSettings {
    pub mode: PlaybackMode,
    pub accompaniment: AccompanimentRoute,
}

pub struct Scheduler {
    config: SchedulerConfig,
    events: Vec<PlaybackMidiEvent>,
    cursor: usize,
    queue: VecDeque<ScheduledEvent>,
    loop_range: Option<LoopRange>,
    settings: PlaybackSettings,
    sample_rate_hz: u32,
}

impl Scheduler {
    pub fn new(sample_rate_hz: u32, config: SchedulerConfig) -> Self {
        Self {
            config,
            events: Vec::new(),
            cursor: 0,
            queue: VecDeque::new(),
            loop_range: None,
            settings: PlaybackSettings {
                mode: PlaybackMode::Demo,
                accompaniment: AccompanimentRoute {
                    play_left: true,
                    play_right: true,
                },
            },
            sample_rate_hz,
        }
    }

    pub fn set_score(&mut self, mut events: Vec<PlaybackMidiEvent>) {
        events.sort_by_key(|event| event.tick);
        self.events = events;
        self.cursor = 0;
        self.queue.clear();
    }

    pub fn set_loop(&mut self, range: Option<LoopRange>) {
        self.loop_range = range;
    }

    pub fn loop_range(&self) -> Option<LoopRange> {
        self.loop_range
    }

    pub fn set_mode(&mut self, mode: PlaybackMode) {
        self.settings.mode = mode;
    }

    pub fn set_accompaniment_route(&mut self, play_left: bool, play_right: bool) {
        self.settings.accompaniment = AccompanimentRoute {
            play_left,
            play_right,
        };
    }

    pub fn seek(&mut self, tick: i64) {
        self.cursor = self
            .events
            .iter()
            .position(|event| event.tick >= tick)
            .unwrap_or(self.events.len());
        self.queue.clear();
    }

    pub fn schedule(&mut self, transport: &mut Transport) -> Vec<ScheduledEvent> {
        let lookahead_samples =
            (self.config.lookahead_ms as f64 * self.sample_rate_hz as f64 / 1000.0).round() as u64;
        let window_end_sample = transport.now_sample().saturating_add(lookahead_samples);
        let window_end_tick = transport.sample_to_tick(window_end_sample);

        let mut emitted = Vec::new();
        while let Some(event) = self.events.get(self.cursor) {
            if event.tick > window_end_tick {
                break;
            }

            if let Some(loop_range) = self.loop_range {
                if event.tick >= loop_range.end_tick {
                    transport.seek(loop_range.start_tick);
                    self.seek(loop_range.start_tick);
                    break;
                }
            }

            if let Some(bus) = self.route_bus(event.hand) {
                let sample_time = transport.tick_to_sample(event.tick);
                let scheduled = ScheduledEvent {
                    sample_time,
                    bus,
                    event: event.event,
                };
                self.queue.push_back(scheduled);
            }

            self.cursor += 1;
        }

        while let Some(event) = self.queue.pop_front() {
            emitted.push(event);
        }

        emitted
    }

    fn route_bus(&self, hand: Option<Hand>) -> Option<Bus> {
        match self.settings.mode {
            PlaybackMode::Demo => Some(Bus::Autopilot),
            PlaybackMode::Accompaniment => match hand {
                Some(Hand::Left) if !self.settings.accompaniment.play_left => None,
                Some(Hand::Right) if !self.settings.accompaniment.play_right => None,
                _ => Some(Bus::Autopilot),
            },
        }
    }
}
