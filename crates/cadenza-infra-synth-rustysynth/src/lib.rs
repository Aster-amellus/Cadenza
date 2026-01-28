use cadenza_infra_synth_waveguide_piano::WaveguidePianoSynth;
use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::synth::{SoundFontInfo, SynthError, SynthPort};
use cadenza_ports::types::{Bus, SampleTime};
use parking_lot::Mutex;
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

pub struct RustySynth {
    fallback: WaveguidePianoSynth,
    sample_rate_hz: AtomicU32,
    enabled: AtomicBool,
    sound_font: Mutex<Option<Arc<SoundFont>>>,
    buses: [BusState; 3],
}

struct BusState {
    program: AtomicU8,
    synth: Mutex<Option<Synthesizer>>,
}

impl BusState {
    fn new() -> Self {
        Self {
            program: AtomicU8::new(0),
            synth: Mutex::new(None),
        }
    }
}

impl Default for RustySynth {
    fn default() -> Self {
        Self::new(48_000, 64)
    }
}

impl RustySynth {
    pub fn new(sample_rate_hz: u32, _max_voices: usize) -> Self {
        Self {
            fallback: WaveguidePianoSynth::new(sample_rate_hz),
            sample_rate_hz: AtomicU32::new(sample_rate_hz),
            enabled: AtomicBool::new(false),
            sound_font: Mutex::new(None),
            buses: [BusState::new(), BusState::new(), BusState::new()],
        }
    }

    fn bus_index(bus: Bus) -> usize {
        match bus {
            Bus::UserMonitor => 0,
            Bus::Autopilot => 1,
            Bus::MetronomeFx => 2,
        }
    }

    fn rebuild_synthesizers(&self, sound_font: Arc<SoundFont>) -> Result<(), SynthError> {
        let sample_rate_hz = self.sample_rate_hz.load(Ordering::Relaxed) as i32;
        let mut settings = SynthesizerSettings::new(sample_rate_hz);
        settings.enable_reverb_and_chorus = false;

        for (idx, bus) in [Bus::UserMonitor, Bus::Autopilot, Bus::MetronomeFx]
            .into_iter()
            .enumerate()
        {
            let program = self.buses[idx].program.load(Ordering::Relaxed);
            let mut synth = Synthesizer::new(&sound_font, &settings)
                .map_err(|e| SynthError::Backend(e.to_string()))?;
            synth.set_master_volume(0.25);
            // Default preset is usually Acoustic Grand Piano (GM 0). Apply if requested.
            if program != 0 {
                synth.process_midi_message(0, 0xC0, program as i32, 0);
            }
            *self.buses[Self::bus_index(bus)].synth.lock() = Some(synth);
        }

        Ok(())
    }

    fn with_active_synth<T>(&self, bus: Bus, f: impl FnOnce(&mut Synthesizer) -> T) -> Option<T> {
        let idx = Self::bus_index(bus);
        let mut guard = self.buses[idx].synth.try_lock()?;
        let synth = guard.as_mut()?;
        Some(f(synth))
    }
}

impl SynthPort for RustySynth {
    fn load_soundfont_from_path(&self, path: &str) -> Result<SoundFontInfo, SynthError> {
        let mut file = File::open(path).map_err(|e| SynthError::SoundFontLoad(e.to_string()))?;
        let sound_font = Arc::new(
            SoundFont::new(&mut file).map_err(|e| SynthError::SoundFontLoad(e.to_string()))?,
        );

        let name = sound_font.get_info().get_bank_name().trim().to_string();
        let name = if name.is_empty() {
            Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("SoundFont")
                .to_string()
        } else {
            name
        };
        let preset_count = sound_font.get_presets().len();

        *self.sound_font.lock() = Some(sound_font.clone());
        self.rebuild_synthesizers(sound_font)?;
        self.enabled.store(true, Ordering::Relaxed);

        Ok(SoundFontInfo { name, preset_count })
    }

    fn set_sample_rate(&self, sample_rate_hz: u32) {
        self.sample_rate_hz.store(sample_rate_hz, Ordering::Relaxed);
        self.fallback.set_sample_rate(sample_rate_hz);

        let sound_font = self.sound_font.lock().clone();
        if let Some(sound_font) = sound_font {
            let _ = self.rebuild_synthesizers(sound_font);
        }
    }

    fn set_program(&self, bus: Bus, gm_program: u8) -> Result<(), SynthError> {
        let idx = Self::bus_index(bus);
        self.buses[idx].program.store(gm_program, Ordering::Relaxed);
        if !self.enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.with_active_synth(bus, |synth| {
            synth.process_midi_message(0, 0xC0, gm_program as i32, 0);
        });
        Ok(())
    }

    fn handle_event(&self, bus: Bus, event: MidiLikeEvent, at: SampleTime) {
        if !self.enabled.load(Ordering::Relaxed) {
            self.fallback.handle_event(bus, event, at);
            return;
        }

        self.with_active_synth(bus, |synth| match event {
            MidiLikeEvent::NoteOn { note, velocity } => {
                synth.note_on(0, note as i32, velocity as i32);
            }
            MidiLikeEvent::NoteOff { note } => {
                synth.note_off(0, note as i32);
            }
            MidiLikeEvent::Cc64 { value } => {
                synth.process_midi_message(0, 0xB0, 0x40, value as i32);
            }
        });
    }

    fn render(&self, bus: Bus, frames: usize, out_l: &mut [f32], out_r: &mut [f32]) {
        if !self.enabled.load(Ordering::Relaxed) {
            self.fallback.render(bus, frames, out_l, out_r);
            return;
        }

        for value in out_l.iter_mut() {
            *value = 0.0;
        }
        for value in out_r.iter_mut() {
            *value = 0.0;
        }

        let _ = self.with_active_synth(bus, |synth| {
            let frames = frames.min(out_l.len()).min(out_r.len());
            synth.render(&mut out_l[..frames], &mut out_r[..frames]);
        });
    }
}
