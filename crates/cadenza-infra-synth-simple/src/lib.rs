use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::synth::{SoundFontInfo, SynthError, SynthPort};
use cadenza_ports::types::{Bus, SampleTime};
use parking_lot::Mutex;
use std::f32::consts::TAU;

pub struct SimpleSynth {
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    sample_rate_hz: f32,
    max_voices: usize,
    buses: [BusState; 3],
}

#[derive(Clone, Debug)]
struct BusState {
    sustain_down: bool,
    voices: Vec<Voice>,
    note_counter: u64,
}

#[derive(Clone, Debug)]
struct Voice {
    note: u8,
    freq: f32,
    phase: f32,
    velocity: f32,
    key_down: bool,
    sustained: bool,
    release_samples_left: u32,
    release_total_samples: u32,
    age: u64,
}

impl SimpleSynth {
    pub fn new(sample_rate_hz: u32, max_voices: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                sample_rate_hz: sample_rate_hz as f32,
                max_voices: max_voices.max(8),
                buses: [BusState::new(), BusState::new(), BusState::new()],
            }),
        }
    }
}

impl Default for SimpleSynth {
    fn default() -> Self {
        Self::new(48_000, 64)
    }
}

impl Inner {
    fn bus_index(bus: Bus) -> usize {
        match bus {
            Bus::UserMonitor => 0,
            Bus::Autopilot => 1,
            Bus::MetronomeFx => 2,
        }
    }

    fn note_on(&mut self, bus: Bus, note: u8, velocity: u8) {
        let index = Self::bus_index(bus);
        let state = &mut self.buses[index];
        state.note_counter = state.note_counter.wrapping_add(1);

        if state.voices.len() >= self.max_voices {
            if let Some((idx, _)) = state
                .voices
                .iter()
                .enumerate()
                .min_by_key(|(_, voice)| voice.age)
            {
                state.voices.swap_remove(idx);
            }
        }

        let freq = 440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0);
        let velocity = (velocity as f32 / 127.0).clamp(0.05, 1.0);
        let release_total_samples = (self.sample_rate_hz * 0.2) as u32;
        let voice = Voice {
            note,
            freq,
            phase: 0.0,
            velocity,
            key_down: true,
            sustained: false,
            release_samples_left: 0,
            release_total_samples: release_total_samples.max(1),
            age: state.note_counter,
        };
        state.voices.push(voice);
    }

    fn note_off(&mut self, bus: Bus, note: u8) {
        let index = Self::bus_index(bus);
        let state = &mut self.buses[index];
        for voice in &mut state.voices {
            if voice.note == note && voice.key_down {
                voice.key_down = false;
                if state.sustain_down {
                    voice.sustained = true;
                } else {
                    voice.release_samples_left = voice.release_total_samples;
                }
            }
        }
    }

    fn sustain(&mut self, bus: Bus, down: bool) {
        let index = Self::bus_index(bus);
        let state = &mut self.buses[index];
        state.sustain_down = down;

        if !down {
            for voice in &mut state.voices {
                if !voice.key_down && voice.sustained {
                    voice.sustained = false;
                    voice.release_samples_left = voice.release_total_samples;
                }
            }
        }
    }

    fn render_bus(&mut self, bus: Bus, frames: usize, out_l: &mut [f32], out_r: &mut [f32]) {
        for value in out_l.iter_mut() {
            *value = 0.0;
        }
        for value in out_r.iter_mut() {
            *value = 0.0;
        }

        let index = Self::bus_index(bus);
        let state = &mut self.buses[index];
        let amplitude = 0.2;

        for voice in &mut state.voices {
            let phase_step = TAU * voice.freq / self.sample_rate_hz;
            for i in 0..frames {
                let mut gain = voice.velocity;
                if voice.release_samples_left > 0 {
                    gain *= voice.release_samples_left as f32 / voice.release_total_samples as f32;
                    voice.release_samples_left = voice.release_samples_left.saturating_sub(1);
                }

                let sample = (voice.phase).sin() * gain * amplitude;
                out_l[i] += sample;
                out_r[i] += sample;
                voice.phase += phase_step;
                if voice.phase >= TAU {
                    voice.phase -= TAU;
                }
            }
        }

        state
            .voices
            .retain(|voice| voice.key_down || voice.sustained || voice.release_samples_left > 0);
    }
}

impl BusState {
    fn new() -> Self {
        Self {
            sustain_down: false,
            voices: Vec::new(),
            note_counter: 0,
        }
    }
}

impl SynthPort for SimpleSynth {
    fn load_soundfont_from_path(&self, _path: &str) -> Result<SoundFontInfo, SynthError> {
        Err(SynthError::UnsupportedFormat)
    }

    fn set_sample_rate(&self, sample_rate_hz: u32) {
        let mut inner = self.inner.lock();
        inner.sample_rate_hz = sample_rate_hz as f32;
    }

    fn set_program(&self, _bus: Bus, _gm_program: u8) -> Result<(), SynthError> {
        Ok(())
    }

    fn handle_event(&self, bus: Bus, event: MidiLikeEvent, _at: SampleTime) {
        let mut inner = self.inner.lock();
        match event {
            MidiLikeEvent::NoteOn { note, velocity } => inner.note_on(bus, note, velocity),
            MidiLikeEvent::NoteOff { note } => inner.note_off(bus, note),
            MidiLikeEvent::Cc64 { value } => inner.sustain(bus, value >= 64),
        }
    }

    fn render(&self, bus: Bus, frames: usize, out_l: &mut [f32], out_r: &mut [f32]) {
        let mut inner = self.inner.lock();
        inner.render_bus(bus, frames, out_l, out_r);
    }
}
