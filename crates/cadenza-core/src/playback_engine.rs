use crate::scheduler::{Scheduler, SchedulerConfig};
use crate::transport::Transport;
use cadenza_domain_score::{Hand, PlaybackMidiEvent, TempoPoint};
use cadenza_ports::playback::{
    LoopRange, PlaybackError, PlaybackMode, PlaybackPort, PlaybackRouteHint, PlaybackScore,
    ScheduledEvent,
};
use cadenza_ports::types::Tick;
use parking_lot::Mutex;

struct PlaybackState {
    transport: Transport,
    scheduler: Scheduler,
    loop_range: Option<LoopRange>,
}

pub struct PlaybackEngine {
    state: Mutex<PlaybackState>,
}

impl PlaybackEngine {
    pub fn new(sample_rate_hz: u32) -> Self {
        Self {
            state: Mutex::new(PlaybackState {
                transport: Transport::new(480, sample_rate_hz, Vec::new()),
                scheduler: Scheduler::new(sample_rate_hz, SchedulerConfig { lookahead_ms: 30 }),
                loop_range: None,
            }),
        }
    }
}

impl PlaybackPort for PlaybackEngine {
    fn load_score(&self, score: PlaybackScore) -> Result<(), PlaybackError> {
        let mut state = self.state.lock();
        let tempo_map = score
            .tempo_map
            .into_iter()
            .map(|point| TempoPoint {
                tick: point.tick,
                us_per_quarter: point.us_per_quarter,
            })
            .collect::<Vec<_>>();

        let events = score
            .events
            .into_iter()
            .map(|event| PlaybackMidiEvent {
                tick: event.tick,
                event: event.event,
                hand: match event.route_hint {
                    PlaybackRouteHint::Left => Some(Hand::Left),
                    PlaybackRouteHint::Right => Some(Hand::Right),
                    PlaybackRouteHint::None => None,
                },
            })
            .collect::<Vec<_>>();

        state.transport.update_tempo_map(tempo_map);
        state.transport.seek(0);
        state.scheduler.set_score(events);
        let loop_range = state.loop_range;
        state.scheduler.set_loop(loop_range);
        Ok(())
    }

    fn play(&self) -> Result<(), PlaybackError> {
        let mut state = self.state.lock();
        state.transport.play();
        Ok(())
    }

    fn pause(&self) -> Result<(), PlaybackError> {
        let mut state = self.state.lock();
        state.transport.pause();
        Ok(())
    }

    fn stop(&self) -> Result<(), PlaybackError> {
        let mut state = self.state.lock();
        state.transport.stop();
        Ok(())
    }

    fn seek(&self, tick: Tick) -> Result<(), PlaybackError> {
        let mut state = self.state.lock();
        state.transport.seek(tick);
        state.scheduler.seek(tick);
        Ok(())
    }

    fn set_loop(&self, range: Option<LoopRange>) -> Result<(), PlaybackError> {
        let mut state = self.state.lock();
        state.loop_range = range;
        state.scheduler.set_loop(range);
        state.transport.set_loop(range);
        Ok(())
    }

    fn set_tempo_multiplier(&self, multiplier: f32) -> Result<(), PlaybackError> {
        let mut state = self.state.lock();
        state.transport.set_tempo_multiplier(multiplier);
        Ok(())
    }

    fn set_mode(&self, mode: PlaybackMode) -> Result<(), PlaybackError> {
        let mut state = self.state.lock();
        state.scheduler.set_mode(mode);
        Ok(())
    }

    fn poll_scheduled_events(&self, _window_samples: u64) -> Result<Vec<ScheduledEvent>, PlaybackError> {
        let mut state = self.state.lock();
        let PlaybackState {
            transport,
            scheduler,
            ..
        } = &mut *state;
        Ok(scheduler.schedule(transport))
    }
}
