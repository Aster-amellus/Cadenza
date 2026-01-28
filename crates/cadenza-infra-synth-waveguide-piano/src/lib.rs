use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::synth::{SoundFontInfo, SynthError, SynthPort};
use cadenza_ports::types::{Bus, SampleTime};
use parking_lot::Mutex;

const MAX_DELAY_SAMPLES: usize = 4096;
const MAX_VOICES: usize = 64;
const MAX_STRINGS_PER_NOTE: usize = 3;
const HAMMER_SHAPER_MAX: usize = 512;
const SOUNDBOARD_MODES: usize = 6;

pub struct WaveguidePianoSynth {
    inner: Mutex<Inner>,
}

struct Inner {
    sample_rate_hz: u32,
    buses: [BusState; 3],
}

struct BusState {
    sustain_down: bool,
    note_counter: u64,
    voices: Vec<Voice>,
    soundboard: Soundboard,
}

struct Voice {
    active: bool,
    note: u8,
    velocity: f32,
    key_down: bool,
    sustained: bool,
    gain: f32,
    out_gain: f32,
    damper: f32,
    age: u64,
    pan: f32,
    hammer: HammerModel,
    strings: [StringModel; MAX_STRINGS_PER_NOTE],
    string_count: usize,
}

struct HammerModel {
    active: bool,
    pos: f32,
    vel: f32,
    mass: f32,
    k: f32,
    p: f32,
    dt: f32,
    prev_force: f32,
    exc_gain: f32,
    shaper: HammerShaper,
    click: HammerClick,
}

struct HammerShaper {
    buf: [f32; HAMMER_SHAPER_MAX],
    idx: usize,
    delay: usize,
    sum: f32,
}

struct HammerClick {
    rng: u32,
    lp: f32,
    remaining: u32,
    total: u32,
    amp: f32,
    lp_coeff: f32,
}

struct StringModel {
    delay: Vec<f32>,
    idx: usize,
    frac: f32,
    strike_offset: usize,
    lp_state: f32,
    lp_attack: f32,
    lp_sustain: f32,
    feedback: f32,
    last: f32,
    gain: f32,
    tone: f32,
    tone_decay: f32,
    avg_coeff: f32,
    pickup_mix: f32,
    ap1_x1: f32,
    ap1_y1: f32,
    ap1_coeff: f32,
    ap2_x1: f32,
    ap2_y1: f32,
    ap2_coeff: f32,
}

struct Soundboard {
    sample_rate_hz: u32,
    mix: f32,
    color_mix: f32,
    comb_l: [CombFilter; 4],
    comb_r: [CombFilter; 4],
    allpass_l: [AllpassFilter; 2],
    allpass_r: [AllpassFilter; 2],
    modes_l: [Resonator; SOUNDBOARD_MODES],
    modes_r: [Resonator; SOUNDBOARD_MODES],
}

struct CombFilter {
    buf: Vec<f32>,
    idx: usize,
    feedback: f32,
    damp: f32,
    filter_store: f32,
}

struct AllpassFilter {
    buf: Vec<f32>,
    idx: usize,
    feedback: f32,
}

struct Resonator {
    a1: f32,
    a2: f32,
    b: f32,
    y1: f32,
    y2: f32,
    gain: f32,
}

impl Soundboard {
    fn new(sample_rate_hz: u32) -> Self {
        const COMB_L_BASE: [usize; 4] = [1116, 1188, 1277, 1356];
        const COMB_R_BASE: [usize; 4] = [1139, 1211, 1300, 1379];
        const ALLPASS_L_BASE: [usize; 2] = [556, 441];
        const ALLPASS_R_BASE: [usize; 2] = [579, 464];
        const MODE_FREQS: [f32; SOUNDBOARD_MODES] = [120.0, 240.0, 520.0, 880.0, 1450.0, 2300.0];
        const MODE_DECAYS_S: [f32; SOUNDBOARD_MODES] = [1.2, 1.0, 0.9, 0.75, 0.55, 0.42];
        const MODE_GAINS: [f32; SOUNDBOARD_MODES] = [0.42, 0.34, 0.26, 0.20, 0.16, 0.12];

        let feedback = 0.78;
        let damp = 0.22;
        let allpass_feedback = 0.5;

        let comb_l = std::array::from_fn(|i| {
            CombFilter::new(scale_len(COMB_L_BASE[i], sample_rate_hz), feedback, damp)
        });
        let comb_r = std::array::from_fn(|i| {
            CombFilter::new(scale_len(COMB_R_BASE[i], sample_rate_hz), feedback, damp)
        });
        let allpass_l = std::array::from_fn(|i| {
            AllpassFilter::new(
                scale_len(ALLPASS_L_BASE[i], sample_rate_hz),
                allpass_feedback,
            )
        });
        let allpass_r = std::array::from_fn(|i| {
            AllpassFilter::new(
                scale_len(ALLPASS_R_BASE[i], sample_rate_hz),
                allpass_feedback,
            )
        });

        let modes_l = std::array::from_fn(|i| {
            Resonator::new(
                sample_rate_hz,
                MODE_FREQS[i],
                MODE_DECAYS_S[i],
                MODE_GAINS[i],
            )
        });
        let modes_r = std::array::from_fn(|i| {
            Resonator::new(
                sample_rate_hz,
                MODE_FREQS[i] * 1.004,
                MODE_DECAYS_S[i] * 0.97,
                MODE_GAINS[i],
            )
        });

        Self {
            sample_rate_hz,
            mix: 0.06,
            color_mix: 0.07,
            comb_l,
            comb_r,
            allpass_l,
            allpass_r,
            modes_l,
            modes_r,
        }
    }

    fn reset(&mut self, sample_rate_hz: u32) {
        if self.sample_rate_hz == sample_rate_hz {
            for comb in self.comb_l.iter_mut().chain(self.comb_r.iter_mut()) {
                comb.clear();
            }
            for ap in self.allpass_l.iter_mut().chain(self.allpass_r.iter_mut()) {
                ap.clear();
            }
            for mode in self.modes_l.iter_mut().chain(self.modes_r.iter_mut()) {
                mode.clear();
            }
            return;
        }

        *self = Self::new(sample_rate_hz);
    }

    fn process(&mut self, frames: usize, out_l: &mut [f32], out_r: &mut [f32]) {
        let frames = frames.min(out_l.len()).min(out_r.len());
        if frames == 0 {
            return;
        }

        let mix = self.mix.clamp(0.0, 1.0);
        let color_mix = self.color_mix.clamp(0.0, 0.5);
        if mix <= 0.0001 && color_mix <= 0.0001 {
            return;
        }
        let apply_reverb = mix > 0.0001;
        let dry = 1.0 - mix;
        let wet_gain = 0.18;

        for i in 0..frames {
            let dry_l = out_l[i];
            let dry_r = out_r[i];
            let input = (dry_l + dry_r) * 0.5;

            let (mut out_l_sample, mut out_r_sample) = if apply_reverb {
                let mut wet_l = 0.0_f32;
                for comb in self.comb_l.iter_mut() {
                    wet_l += comb.process(input);
                }
                let mut wet_r = 0.0_f32;
                for comb in self.comb_r.iter_mut() {
                    wet_r += comb.process(input);
                }

                wet_l *= wet_gain;
                wet_r *= wet_gain;

                for ap in self.allpass_l.iter_mut() {
                    wet_l = ap.process(wet_l);
                }
                for ap in self.allpass_r.iter_mut() {
                    wet_r = ap.process(wet_r);
                }

                (dry_l * dry + wet_l * mix, dry_r * dry + wet_r * mix)
            } else {
                (dry_l, dry_r)
            };

            if color_mix > 0.0001 {
                let mut color_l = 0.0_f32;
                for mode in self.modes_l.iter_mut() {
                    color_l += mode.process(input);
                }
                let mut color_r = 0.0_f32;
                for mode in self.modes_r.iter_mut() {
                    color_r += mode.process(input);
                }
                out_l_sample += color_l * color_mix;
                out_r_sample += color_r * color_mix;
            }

            out_l[i] = out_l_sample;
            out_r[i] = out_r_sample;
        }
    }
}

impl CombFilter {
    fn new(len: usize, feedback: f32, damp: f32) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            idx: 0,
            feedback: feedback.clamp(0.0, 0.999),
            damp: damp.clamp(0.0, 0.99),
            filter_store: 0.0,
        }
    }

    fn clear(&mut self) {
        for v in self.buf.iter_mut() {
            *v = 0.0;
        }
        self.idx = 0;
        self.filter_store = 0.0;
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.buf[self.idx];
        self.filter_store = output + (self.filter_store - output) * self.damp;
        self.buf[self.idx] = input + self.filter_store * self.feedback;
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        output
    }
}

impl AllpassFilter {
    fn new(len: usize, feedback: f32) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            idx: 0,
            feedback: feedback.clamp(0.0, 0.999),
        }
    }

    fn clear(&mut self) {
        for v in self.buf.iter_mut() {
            *v = 0.0;
        }
        self.idx = 0;
    }

    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.buf[self.idx];
        let output = -input + buf_out;
        self.buf[self.idx] = input + buf_out * self.feedback;
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        output
    }
}

impl Resonator {
    fn new(sample_rate_hz: u32, freq_hz: f32, decay_s: f32, gain: f32) -> Self {
        let sr = sample_rate_hz.max(1) as f32;
        let freq = freq_hz.clamp(20.0, sr * 0.45);
        let w = 2.0 * std::f32::consts::PI * freq / sr;
        let decay_s = decay_s.max(0.05);
        let r = (-1.0 / (decay_s * sr)).exp();
        let a1 = 2.0 * r * w.cos();
        let a2 = -r * r;
        let b = 1.0 - r;
        Self {
            a1,
            a2,
            b,
            y1: 0.0,
            y2: 0.0,
            gain,
        }
    }

    fn clear(&mut self) {
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b * x + self.a1 * self.y1 + self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y * self.gain
    }
}

fn scale_len(base_len: usize, sample_rate_hz: u32) -> usize {
    let sr = sample_rate_hz.max(1) as f32;
    let scaled = (base_len as f32 * (sr / 48_000.0)).round() as usize;
    scaled.clamp(32, 48_000)
}

impl Default for WaveguidePianoSynth {
    fn default() -> Self {
        Self::new(48_000)
    }
}

impl WaveguidePianoSynth {
    pub fn new(sample_rate_hz: u32) -> Self {
        Self {
            inner: Mutex::new(Inner::new(sample_rate_hz)),
        }
    }
}

impl Inner {
    fn new(sample_rate_hz: u32) -> Self {
        Self {
            sample_rate_hz,
            buses: [
                BusState::new(sample_rate_hz),
                BusState::new(sample_rate_hz),
                BusState::new(sample_rate_hz),
            ],
        }
    }

    fn bus_index(bus: Bus) -> usize {
        match bus {
            Bus::UserMonitor => 0,
            Bus::Autopilot => 1,
            Bus::MetronomeFx => 2,
        }
    }
}

impl BusState {
    fn new(sample_rate_hz: u32) -> Self {
        let mut voices = Vec::with_capacity(MAX_VOICES);
        for _ in 0..MAX_VOICES {
            voices.push(Voice::new());
        }
        Self {
            sustain_down: false,
            note_counter: 0,
            voices,
            soundboard: Soundboard::new(sample_rate_hz),
        }
    }

    fn reset(&mut self, sample_rate_hz: u32) {
        self.sustain_down = false;
        self.note_counter = 0;
        for voice in self.voices.iter_mut() {
            voice.reset();
        }
        self.soundboard.reset(sample_rate_hz);
    }

    fn allocate_voice(&mut self) -> &mut Voice {
        if let Some(idx) = self.voices.iter().position(|v| !v.active) {
            return &mut self.voices[idx];
        }

        let mut best_idx = 0usize;
        let mut best_gain = self.voices[0].gain;
        for (idx, voice) in self.voices.iter().enumerate().skip(1) {
            if voice.gain < best_gain {
                best_idx = idx;
                best_gain = voice.gain;
            }
        }

        &mut self.voices[best_idx]
    }

    fn note_on(&mut self, sample_rate_hz: u32, note: u8, velocity: u8) {
        let vel = (velocity as f32 / 127.0).clamp(0.02, 1.0);
        self.note_counter = self.note_counter.wrapping_add(1);
        let age = self.note_counter;

        let voice = self.allocate_voice();
        voice.reset();
        voice.active = true;
        voice.note = note;
        voice.velocity = vel;
        voice.key_down = true;
        voice.sustained = false;
        voice.age = age;

        voice.pan = note_to_pan(note);
        voice.out_gain = vel.powf(1.25) * 0.32;

        let (string_count, detunes) = string_plan(note);
        voice.string_count = string_count;

        let base_freq = midi_note_to_hz(note);
        let base_delay_len =
            (sample_rate_hz as f32 / base_freq).clamp(8.0, (MAX_DELAY_SAMPLES - 1) as f32);
        let seed = 0xA5A5_1234u32 ^ ((note as u32) << 8) ^ (velocity as u32);

        voice
            .hammer
            .start(sample_rate_hz, note, vel, base_delay_len, seed);

        for (idx, string) in voice.strings.iter_mut().enumerate() {
            if idx >= string_count {
                string.clear();
                continue;
            }
            let detune = detunes[idx];
            let freq = base_freq * (1.0 + detune);
            let delay_len =
                (sample_rate_hz as f32 / freq).clamp(8.0, (MAX_DELAY_SAMPLES - 1) as f32);
            string.init(delay_len, vel, note);
        }
    }

    fn note_off(&mut self, note: u8) {
        for voice in self.voices.iter_mut() {
            if !voice.active || voice.note != note || !voice.key_down {
                continue;
            }
            voice.key_down = false;
            if self.sustain_down {
                voice.sustained = true;
            }
        }
    }

    fn sustain(&mut self, down: bool) {
        self.sustain_down = down;
        if down {
            return;
        }
        for voice in self.voices.iter_mut() {
            if voice.active && !voice.key_down && voice.sustained {
                voice.sustained = false;
            }
        }
    }

    fn render(&mut self, frames: usize, out_l: &mut [f32], out_r: &mut [f32]) {
        for value in out_l.iter_mut() {
            *value = 0.0;
        }
        for value in out_r.iter_mut() {
            *value = 0.0;
        }

        let frames = frames.min(out_l.len()).min(out_r.len());
        if frames == 0 {
            return;
        }

        for voice in self.voices.iter_mut() {
            if !voice.active {
                continue;
            }
            voice.render(frames, out_l, out_r);
        }

        self.soundboard.process(frames, out_l, out_r);

        for voice in self.voices.iter_mut() {
            if voice.active && !voice.key_down && !voice.sustained && voice.gain < 0.0008 {
                voice.reset();
            }
        }
    }
}

impl Voice {
    fn new() -> Self {
        Self {
            active: false,
            note: 60,
            velocity: 0.0,
            key_down: false,
            sustained: false,
            gain: 0.0,
            out_gain: 0.0,
            damper: 0.0,
            age: 0,
            pan: 0.0,
            hammer: HammerModel::new(),
            strings: [StringModel::new(), StringModel::new(), StringModel::new()],
            string_count: 0,
        }
    }

    fn reset(&mut self) {
        self.active = false;
        self.key_down = false;
        self.sustained = false;
        self.gain = 0.0;
        self.out_gain = 0.0;
        self.damper = 0.0;
        self.hammer.reset();
        self.string_count = 0;
        for string in self.strings.iter_mut() {
            string.clear();
        }
    }

    fn render(&mut self, frames: usize, out_l: &mut [f32], out_r: &mut [f32]) {
        let damper_coeff = 0.02;
        let amp_coeff = 0.01;
        let mut amp = self.gain;

        let pan = self.pan;
        let left_gain = (0.5 - pan * 0.5).clamp(0.0, 1.0);
        let right_gain = (0.5 + pan * 0.5).clamp(0.0, 1.0);

        for i in 0..frames {
            let target = if self.key_down || self.sustained {
                0.0
            } else {
                1.0
            };
            self.damper += (target - self.damper) * damper_coeff;

            let mut strike_disp = 0.0_f32;
            for idx in 0..self.string_count {
                strike_disp += self.strings[idx].strike_disp();
            }
            if self.string_count > 0 {
                strike_disp /= self.string_count as f32;
            }

            let hammer_exc = self.hammer.tick(strike_disp);
            let per_string = if self.string_count > 0 {
                hammer_exc / self.string_count as f32
            } else {
                0.0
            };

            for idx in 0..self.string_count {
                self.strings[idx].inject_strike(per_string);
            }

            let mut raw = 0.0_f32;
            for idx in 0..self.string_count {
                raw += self.strings[idx].tick(self.damper);
            }
            raw += self.hammer.click_tick();

            amp += (raw.abs() - amp) * amp_coeff;

            let sample = raw * self.out_gain;
            out_l[i] += sample * left_gain;
            out_r[i] += sample * right_gain;
        }

        self.gain = amp;
    }
}

impl HammerModel {
    fn new() -> Self {
        Self {
            active: false,
            pos: 0.0,
            vel: 0.0,
            mass: 1.0,
            k: 0.0,
            p: 2.5,
            dt: 1.0 / 48_000.0,
            prev_force: 0.0,
            exc_gain: 0.0,
            shaper: HammerShaper::new(),
            click: HammerClick::new(),
        }
    }

    fn reset(&mut self) {
        self.active = false;
        self.pos = 0.0;
        self.vel = 0.0;
        self.k = 0.0;
        self.p = 2.5;
        self.prev_force = 0.0;
        self.exc_gain = 0.0;
        self.shaper.reset(1);
        self.click.reset();
    }

    fn start(&mut self, sample_rate_hz: u32, note: u8, velocity: f32, _delay_len: f32, seed: u32) {
        let sr = sample_rate_hz.max(1) as f32;
        self.dt = 1.0 / sr;
        self.mass = 1.0;
        self.pos = 0.0;

        let vel = velocity.clamp(0.02, 1.0);
        let t = ((note as f32 - 21.0) / 87.0).clamp(0.0, 1.0);

        let v0 = 60.0 + 260.0 * vel.powf(1.5);
        let k = lerp(6.0e6, 2.4e7, vel.powf(1.7));
        let p = lerp(2.15, 3.25, vel.powf(0.7));

        self.vel = v0;
        self.k = k;
        self.p = p;
        self.prev_force = 0.0;
        self.exc_gain = ((0.010 + 0.030 * vel.powf(1.2)) * (0.75 + 0.55 * t)).clamp(0.003, 0.08);

        let contact_ms = hammer_contact_ms(note, vel);
        let delay = (sr * (contact_ms / 1000.0)).round() as usize;
        let delay = delay.clamp(1, HAMMER_SHAPER_MAX.saturating_sub(1));
        self.shaper.reset(delay);
        self.click.start(sample_rate_hz, note, vel, seed);

        self.active = true;
    }

    fn tick(&mut self, string_disp: f32) -> f32 {
        if !self.active {
            return 0.0;
        }

        if self.pos <= string_disp && self.vel <= 0.0 && self.prev_force.abs() < 1.0e-6 {
            self.active = false;
            self.prev_force = 0.0;
            return 0.0;
        }

        let compression = (self.pos - string_disp).max(0.0);
        let force = self.k * compression.powf(self.p);

        let acc = -force / self.mass;
        self.vel += acc * self.dt;
        self.vel *= 0.9996;
        self.pos += self.vel * self.dt;

        let df = force - self.prev_force;
        self.prev_force = force;

        let exc = (df * self.exc_gain).clamp(-0.6, 0.6);
        self.shaper.process(exc)
    }

    fn click_tick(&mut self) -> f32 {
        self.click.tick()
    }
}

impl HammerShaper {
    fn new() -> Self {
        Self {
            buf: [0.0; HAMMER_SHAPER_MAX],
            idx: 0,
            delay: 1,
            sum: 0.0,
        }
    }

    fn reset(&mut self, delay: usize) {
        self.buf.fill(0.0);
        self.idx = 0;
        self.delay = delay.clamp(1, HAMMER_SHAPER_MAX.saturating_sub(1));
        self.sum = 0.0;
    }

    fn process(&mut self, x: f32) -> f32 {
        let len = self.buf.len();
        let read_idx = (self.idx + len - self.delay) % len;
        let outgoing = self.buf[read_idx];
        self.sum += x - outgoing;
        self.buf[self.idx] = x;
        self.idx += 1;
        if self.idx >= len {
            self.idx = 0;
        }
        self.sum / self.delay as f32
    }
}

impl HammerClick {
    fn new() -> Self {
        Self {
            rng: 0x1234_5678,
            lp: 0.0,
            remaining: 0,
            total: 0,
            amp: 0.0,
            lp_coeff: 0.1,
        }
    }

    fn reset(&mut self) {
        self.lp = 0.0;
        self.remaining = 0;
        self.total = 0;
        self.amp = 0.0;
    }

    fn start(&mut self, sample_rate_hz: u32, note: u8, velocity: f32, seed: u32) {
        let vel = velocity.clamp(0.02, 1.0);
        let t = ((note as f32 - 21.0) / 87.0).clamp(0.0, 1.0);

        self.rng = seed ^ (note as u32).wrapping_mul(0x9E37_79B9);

        let sr = sample_rate_hz.max(1) as f32;
        let fc = 1200.0 + 2400.0 * t;
        let a = (-2.0 * std::f32::consts::PI * fc / sr).exp();
        self.lp_coeff = (1.0 - a).clamp(0.01, 0.35);
        self.lp = 0.0;

        let click_ms = 0.6 + 1.0 * (1.0 - vel);
        let total = (sr * (click_ms / 1000.0)).round() as u32;
        self.total = total.clamp(16, 256);
        self.remaining = self.total;

        self.amp = (0.008 + 0.015 * t) * vel.powf(2.2);
    }

    fn tick(&mut self) -> f32 {
        if self.remaining == 0 || self.total == 0 {
            return 0.0;
        }

        let n = self.white_noise();
        self.lp += self.lp_coeff * (n - self.lp);
        let hp = n - self.lp;

        let t = self.remaining as f32 / self.total as f32;
        let env = t * t;
        self.remaining = self.remaining.saturating_sub(1);

        hp * env * self.amp
    }

    fn white_noise(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        let bits = (self.rng >> 9) | 0x3F80_0000;
        let f = f32::from_bits(bits) - 1.0;
        (f * 2.0 - 1.0).clamp(-1.0, 1.0)
    }
}

impl StringModel {
    fn new() -> Self {
        Self {
            delay: Vec::with_capacity(MAX_DELAY_SAMPLES),
            idx: 0,
            frac: 0.0,
            strike_offset: 1,
            lp_state: 0.0,
            lp_attack: 0.0,
            lp_sustain: 0.0,
            feedback: 0.0,
            last: 0.0,
            gain: 0.0,
            tone: 0.0,
            tone_decay: 0.99995,
            avg_coeff: 0.3,
            pickup_mix: 0.6,
            ap1_x1: 0.0,
            ap1_y1: 0.0,
            ap1_coeff: 0.0,
            ap2_x1: 0.0,
            ap2_y1: 0.0,
            ap2_coeff: 0.0,
        }
    }

    fn clear(&mut self) {
        self.delay.clear();
        self.idx = 0;
        self.frac = 0.0;
        self.strike_offset = 1;
        self.lp_state = 0.0;
        self.lp_attack = 0.0;
        self.lp_sustain = 0.0;
        self.feedback = 0.0;
        self.last = 0.0;
        self.gain = 0.0;
        self.tone = 0.0;
        self.tone_decay = 0.99995;
        self.avg_coeff = 0.3;
        self.pickup_mix = 0.6;
        self.ap1_x1 = 0.0;
        self.ap1_y1 = 0.0;
        self.ap1_coeff = 0.0;
        self.ap2_x1 = 0.0;
        self.ap2_y1 = 0.0;
        self.ap2_coeff = 0.0;
    }

    fn init(&mut self, delay_len: f32, velocity: f32, note: u8) {
        let len_int = (delay_len.floor() as usize).clamp(8, MAX_DELAY_SAMPLES - 1);
        self.frac = (delay_len - len_int as f32).clamp(0.0, 0.999);
        self.delay.resize(len_int, 0.0);
        self.idx = 0;
        for v in self.delay.iter_mut() {
            *v = 0.0;
        }

        let strike_pos = strike_position(note);
        let strike_offset = (delay_len * strike_pos).round() as usize;
        self.strike_offset = strike_offset.clamp(1, len_int.saturating_sub(1).max(1));

        self.lp_state = 0.0;
        self.last = 0.0;
        self.ap1_x1 = 0.0;
        self.ap1_y1 = 0.0;
        self.ap2_x1 = 0.0;
        self.ap2_y1 = 0.0;

        let vel = velocity.clamp(0.02, 1.0);
        let t = ((note as f32 - 21.0) / 87.0).clamp(0.0, 1.0);

        let brightness = (0.18 + 0.82 * vel).clamp(0.05, 1.0);
        let note_lp = (0.95 + 0.25 * t).clamp(0.85, 1.35);
        let base_lp = (0.018 + 0.22 * brightness) * note_lp;

        self.lp_attack = (base_lp * (1.18 + 0.22 * vel)).clamp(0.01, 0.55);
        self.lp_sustain = (base_lp * 0.55).clamp(0.005, 0.35);

        let decay = note_decay_coeff(note);
        self.feedback = (decay * (0.994 + 0.005 * vel)).clamp(0.965, 0.99995);

        self.tone = 1.0;
        self.tone_decay = (0.99997 - 0.00005 * vel - 0.00002 * t).clamp(0.99985, 0.99999);

        self.avg_coeff = (0.38 - 0.28 * t).clamp(0.04, 0.42);
        self.pickup_mix = (0.75 - 0.4 * t).clamp(0.25, 0.85);

        self.ap1_coeff = (0.03 + 0.24 * t).clamp(0.0, 0.6);
        self.ap2_coeff = (0.01 + 0.12 * t).clamp(0.0, 0.6);

        self.gain = 0.85;
    }

    fn strike_disp(&self) -> f32 {
        let len = self.delay.len();
        if len == 0 {
            return 0.0;
        }
        let idx = (self.idx + self.strike_offset) % len;
        self.delay[idx]
    }

    fn inject_strike(&mut self, amount: f32) {
        let len = self.delay.len();
        if len == 0 {
            return;
        }

        let idx = (self.idx + self.strike_offset) % len;
        let v = (self.delay[idx] + amount).clamp(-1.0, 1.0);
        self.delay[idx] = v;
    }

    fn tick(&mut self, damper: f32) -> f32 {
        let len = self.delay.len();
        if len < 2 {
            return 0.0;
        }

        let idx0 = self.idx;
        let idx1 = if idx0 + 1 < len { idx0 + 1 } else { 0 };
        let read = self.delay[idx0] * (1.0 - self.frac) + self.delay[idx1] * self.frac;

        let x = read;
        let damper = damper.clamp(0.0, 1.0);

        let mut lp_coeff = self.lp_sustain + (self.lp_attack - self.lp_sustain) * self.tone;
        lp_coeff *= 1.0 - 0.85 * damper;
        lp_coeff = lp_coeff.clamp(0.002, 0.6);

        self.lp_state += lp_coeff * (x - self.lp_state);
        let mut y = self.lp_state;

        let avg = self.avg_coeff;
        y = y * (1.0 - avg) + self.last * avg;
        self.last = y;

        y = allpass(y, self.ap1_coeff, &mut self.ap1_x1, &mut self.ap1_y1);
        y = allpass(y, self.ap2_coeff, &mut self.ap2_x1, &mut self.ap2_y1);

        let feedback = (self.feedback - 0.02 * damper).clamp(0.0, 0.99995);
        let write = y * feedback;
        self.delay[self.idx] = write;
        self.idx += 1;
        if self.idx >= len {
            self.idx = 0;
        }

        self.tone *= self.tone_decay;

        let out = read + (y - read) * self.pickup_mix;
        out * self.gain
    }
}

fn allpass(x: f32, coeff: f32, x1: &mut f32, y1: &mut f32) -> f32 {
    if coeff.abs() <= 0.0001 {
        return x;
    }
    let y = -coeff * x + *x1 + coeff * *y1;
    *x1 = x;
    *y1 = y;
    y
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn hammer_contact_ms(note: u8, velocity: f32) -> f32 {
    let vel = velocity.clamp(0.02, 1.0);
    let t = ((note as f32 - 21.0) / 87.0).clamp(0.0, 1.0);

    // Light touch: longer contact (darker). Hard hits: shorter contact (brighter).
    let base = lerp(2.8, 0.85, vel.powf(0.65));
    // High strings tend to shorter contact.
    let note_scale = lerp(1.25, 0.75, t);
    (base * note_scale).clamp(0.5, 4.0)
}

fn strike_position(note: u8) -> f32 {
    let t = ((note as f32 - 21.0) / 87.0).clamp(0.0, 1.0);
    // Typical grand piano strike position is around 1/7..1/9, tending higher notes closer to 1/9.
    (0.16 - 0.05 * t).clamp(0.10, 0.18)
}

fn midi_note_to_hz(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

fn note_to_pan(note: u8) -> f32 {
    let t = (note as f32 - 60.0) / 48.0;
    (t.clamp(-1.0, 1.0) * 0.5).clamp(-0.6, 0.6)
}

fn note_decay_coeff(note: u8) -> f32 {
    let t = (note as f32 - 21.0) / 87.0;
    let t = t.clamp(0.0, 1.0);
    // Low notes ring longer; high notes decay quicker.
    0.9996 - t * 0.0014
}

fn string_plan(note: u8) -> (usize, [f32; MAX_STRINGS_PER_NOTE]) {
    if note >= 55 {
        (3, [-0.0026, 0.0, 0.0019])
    } else if note >= 35 {
        (2, [-0.0018, 0.0013, 0.0])
    } else {
        (1, [0.0, 0.0, 0.0])
    }
}

impl SynthPort for WaveguidePianoSynth {
    fn load_soundfont_from_path(&self, _path: &str) -> Result<SoundFontInfo, SynthError> {
        Err(SynthError::UnsupportedFormat)
    }

    fn set_sample_rate(&self, sample_rate_hz: u32) {
        let mut inner = self.inner.lock();
        inner.sample_rate_hz = sample_rate_hz;
        for bus in inner.buses.iter_mut() {
            bus.reset(sample_rate_hz);
        }
    }

    fn set_program(&self, _bus: Bus, _gm_program: u8) -> Result<(), SynthError> {
        Ok(())
    }

    fn handle_event(&self, bus: Bus, event: MidiLikeEvent, _at: SampleTime) {
        let Some(mut inner) = self.inner.try_lock() else {
            return;
        };
        let sample_rate_hz = inner.sample_rate_hz;
        let idx = Inner::bus_index(bus);
        let bus_state = &mut inner.buses[idx];
        match event {
            MidiLikeEvent::NoteOn { note, velocity } => {
                bus_state.note_on(sample_rate_hz, note, velocity);
            }
            MidiLikeEvent::NoteOff { note } => {
                bus_state.note_off(note);
            }
            MidiLikeEvent::Cc64 { value } => {
                bus_state.sustain(value >= 64);
            }
        }
    }

    fn render(&self, bus: Bus, frames: usize, out_l: &mut [f32], out_r: &mut [f32]) {
        for value in out_l.iter_mut() {
            *value = 0.0;
        }
        for value in out_r.iter_mut() {
            *value = 0.0;
        }

        let Some(mut inner) = self.inner.try_lock() else {
            return;
        };
        let idx = Inner::bus_index(bus);
        inner.buses[idx].render(frames, out_l, out_r);
    }
}
