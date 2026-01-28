use crate::audio_params::AudioParams;
use cadenza_ports::audio::AudioRenderCallback;
use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::playback::ScheduledEvent;
use cadenza_ports::synth::SynthPort;
use cadenza_ports::types::{Bus, SampleTime};
use rtrb::Consumer;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

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

impl Default for AudioClock {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AudioGraph {
    synth: Arc<dyn SynthPort>,
    params: Arc<AudioParams>,
    clock: Arc<AudioClock>,
    consumer: Consumer<ScheduledEvent>,
    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
    events: Vec<ScheduledEvent>,
    pending: Option<ScheduledEvent>,
    limiter_gain: f32,
}

impl AudioGraph {
    pub fn new(
        synth: Arc<dyn SynthPort>,
        params: Arc<AudioParams>,
        consumer: Consumer<ScheduledEvent>,
        clock: Arc<AudioClock>,
        max_frames: usize,
    ) -> Self {
        Self {
            synth,
            params,
            clock,
            consumer,
            scratch_l: vec![0.0; max_frames],
            scratch_r: vec![0.0; max_frames],
            events: Vec::with_capacity(512),
            pending: None,
            limiter_gain: 1.0,
        }
    }

    fn collect_events(&mut self, sample_time_end: SampleTime) {
        self.events.clear();

        if let Some(event) = self.pending.take() {
            if event.sample_time < sample_time_end {
                self.events.push(event);
            } else {
                self.pending = Some(event);
                return;
            }
        }

        while let Ok(event) = self.consumer.pop() {
            if event.sample_time < sample_time_end {
                self.events.push(event);
            } else {
                self.pending = Some(event);
                break;
            }
        }

        self.events.sort_by(|a, b| {
            a.sample_time
                .cmp(&b.sample_time)
                .then_with(|| midi_event_rank(&a.event).cmp(&midi_event_rank(&b.event)))
                .then_with(|| midi_event_note_key(&a.event).cmp(&midi_event_note_key(&b.event)))
        });
    }

    fn ensure_scratch(&mut self, frames: usize) {
        if self.scratch_l.len() < frames {
            self.scratch_l.resize(frames, 0.0);
            self.scratch_r.resize(frames, 0.0);
        }
    }

    fn render_segment(&mut self, frames: usize, out_l: &mut [f32], out_r: &mut [f32]) {
        let scratch_l = &mut self.scratch_l[..frames];
        let scratch_r = &mut self.scratch_r[..frames];

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

        let limit = 0.98_f32;
        let mut peak = 0.0_f32;
        for i in 0..frames {
            peak = peak.max(out_l[i].abs());
            peak = peak.max(out_r[i].abs());
        }

        let target_gain = if peak > limit { limit / peak } else { 1.0 };
        let current_gain = self.limiter_gain;
        let coeff = if target_gain < current_gain {
            0.25
        } else {
            0.01
        };
        let new_gain = (current_gain + coeff * (target_gain - current_gain)).clamp(0.0, 1.0);
        self.limiter_gain = new_gain;

        if new_gain < 0.999 {
            for i in 0..frames {
                out_l[i] *= new_gain;
                out_r[i] *= new_gain;
            }
        }
    }
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

impl AudioRenderCallback for AudioGraph {
    fn render(&mut self, sample_time_start: SampleTime, out_l: &mut [f32], out_r: &mut [f32]) {
        let frames = out_l.len().min(out_r.len());
        let sample_time_end = sample_time_start.saturating_add(frames as u64);

        self.ensure_scratch(frames);
        self.collect_events(sample_time_end);

        let playback_enabled = self.params.playback_enabled();
        let mut cursor_sample = sample_time_start;
        let mut cursor_frame = 0usize;

        let events_len = self.events.len();
        for idx in 0..events_len {
            let event = self.events[idx];
            if event.sample_time >= sample_time_end {
                continue;
            }

            if !playback_enabled
                && matches!(event.bus, Bus::Autopilot | Bus::MetronomeFx)
                && matches!(event.event, MidiLikeEvent::NoteOn { .. })
            {
                continue;
            }

            let event_sample = event.sample_time.max(cursor_sample);
            let event_frame = (event_sample - cursor_sample) as usize;
            if event_frame > 0 {
                let end = cursor_frame + event_frame;
                self.render_segment(
                    event_frame,
                    &mut out_l[cursor_frame..end],
                    &mut out_r[cursor_frame..end],
                );
                cursor_frame = end;
                cursor_sample = event_sample;
            }
            self.synth
                .handle_event(event.bus, event.event, event_sample);
        }

        if cursor_frame < frames {
            self.render_segment(
                frames - cursor_frame,
                &mut out_l[cursor_frame..frames],
                &mut out_r[cursor_frame..frames],
            );
        }

        self.clock.set(sample_time_end);
    }
}
