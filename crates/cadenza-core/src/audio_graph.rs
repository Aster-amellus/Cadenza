use crate::audio_params::AudioParams;
use cadenza_ports::audio::AudioRenderCallback;
use cadenza_ports::playback::ScheduledEvent;
use cadenza_ports::synth::SynthPort;
use cadenza_ports::types::{Bus, SampleTime};
use parking_lot::Mutex;
use rtrb::Consumer;
use std::sync::{atomic::{AtomicU64, Ordering}, Arc};

pub struct AudioClock {
    sample_time: AtomicU64,
}

impl AudioClock {
    pub fn new() -> Self {
        Self {
            sample_time: AtomicU64::new(0),
        }
    }

    pub fn set(&self, sample_time: SampleTime) {
        self.sample_time.store(sample_time, Ordering::Relaxed);
    }

    pub fn get(&self) -> SampleTime {
        self.sample_time.load(Ordering::Relaxed)
    }
}

pub struct AudioGraph {
    synth: Arc<dyn SynthPort>,
    params: Arc<AudioParams>,
    clock: Arc<AudioClock>,
    state: Mutex<AudioGraphState>,
}

struct AudioGraphState {
    consumer: Consumer<ScheduledEvent>,
    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
    events: Vec<ScheduledEvent>,
    pending: Option<ScheduledEvent>,
}

impl AudioGraph {
    pub fn new(
        synth: Arc<dyn SynthPort>,
        params: Arc<AudioParams>,
        consumer: Consumer<ScheduledEvent>,
        clock: Arc<AudioClock>,
    ) -> Self {
        Self {
            synth,
            params,
            clock,
            state: Mutex::new(AudioGraphState {
                consumer,
                scratch_l: Vec::new(),
                scratch_r: Vec::new(),
                events: Vec::new(),
                pending: None,
            }),
        }
    }

    fn collect_events(
        state: &mut AudioGraphState,
        sample_time_end: SampleTime,
    ) -> &mut Vec<ScheduledEvent> {
        state.events.clear();

        if let Some(event) = state.pending.take() {
            if event.sample_time < sample_time_end {
                state.events.push(event);
            } else {
                state.pending = Some(event);
                return &mut state.events;
            }
        }

        loop {
            match state.consumer.pop() {
                Ok(event) => {
                    if event.sample_time < sample_time_end {
                        state.events.push(event);
                    } else {
                        state.pending = Some(event);
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        state.events.sort_by_key(|event| event.sample_time);
        &mut state.events
    }

    fn ensure_scratch(state: &mut AudioGraphState, frames: usize) {
        if state.scratch_l.len() < frames {
            state.scratch_l.resize(frames, 0.0);
            state.scratch_r.resize(frames, 0.0);
        }
    }

    fn render_segment(
        &self,
        state: &mut AudioGraphState,
        frames: usize,
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        let scratch_l = &mut state.scratch_l[..frames];
        let scratch_r = &mut state.scratch_r[..frames];

        for value in out_l.iter_mut() {
            *value = 0.0;
        }
        for value in out_r.iter_mut() {
            *value = 0.0;
        }

        let master = self.params.master();
        let monitor_enabled = self.params.monitor_enabled();

        for bus in [Bus::UserMonitor, Bus::Autopilot, Bus::MetronomeFx] {
            if bus == Bus::UserMonitor && !monitor_enabled {
                continue;
            }
            self.synth.render(bus, frames, scratch_l, scratch_r);
            let bus_volume = self.params.bus(bus);
            for i in 0..frames {
                out_l[i] += scratch_l[i] * bus_volume;
                out_r[i] += scratch_r[i] * bus_volume;
            }
        }

        for i in 0..frames {
            out_l[i] *= master;
            out_r[i] *= master;
        }
    }
}

impl AudioRenderCallback for AudioGraph {
    fn render(&self, sample_time_start: SampleTime, out_l: &mut [f32], out_r: &mut [f32]) {
        let frames = out_l.len().min(out_r.len());
        let sample_time_end = sample_time_start.saturating_add(frames as u64);

        let mut state = self.state.lock();
        Self::ensure_scratch(&mut state, frames);

        Self::collect_events(&mut state, sample_time_end);

        let mut cursor_sample = sample_time_start;
        let mut cursor_frame = 0usize;

        let events_len = state.events.len();
        for idx in 0..events_len {
            let event = state.events[idx];
            if event.sample_time < cursor_sample || event.sample_time >= sample_time_end {
                continue;
            }
            let event_frame = (event.sample_time - cursor_sample) as usize;
            if event_frame > 0 {
                let end = cursor_frame + event_frame;
                self.render_segment(
                    &mut state,
                    event_frame,
                    &mut out_l[cursor_frame..end],
                    &mut out_r[cursor_frame..end],
                );
                cursor_frame = end;
                cursor_sample = event.sample_time;
            }
            self.synth.handle_event(event.bus, event.event, event.sample_time);
        }

        if cursor_frame < frames {
            self.render_segment(
                &mut state,
                frames - cursor_frame,
                &mut out_l[cursor_frame..frames],
                &mut out_r[cursor_frame..frames],
            );
        }

        self.clock.set(sample_time_end);
    }
}
