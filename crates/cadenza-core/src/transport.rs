use cadenza_domain_score::TempoPoint;
use cadenza_ports::playback::LoopRange;
use cadenza_ports::types::{SampleTime, Tick};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Debug)]
pub struct TempoMap {
    ppq: u16,
    segments: Vec<TempoSegment>,
}

#[derive(Clone, Copy, Debug)]
struct TempoSegment {
    start_tick: Tick,
    start_us: i64,
    us_per_quarter: u32,
}

#[derive(Clone, Debug)]
pub struct Transport {
    state: TransportState,
    ppq: u16,
    sample_rate_hz: u32,
    origin_sample: SampleTime,
    tempo_map: TempoMap,
    tempo_multiplier: f32,
    position_tick: Tick,
    position_sample: SampleTime,
    loop_range: Option<LoopRange>,
}

impl TempoMap {
    pub fn new(ppq: u16, mut points: Vec<TempoPoint>) -> Self {
        if points.is_empty() || points[0].tick != 0 {
            points.insert(
                0,
                TempoPoint {
                    tick: 0,
                    us_per_quarter: 500_000,
                },
            );
        }
        points.sort_by_key(|p| p.tick);

        let mut segments = Vec::with_capacity(points.len());
        let mut current_us = 0i64;
        for (idx, point) in points.iter().enumerate() {
            if idx > 0 {
                let prev = &points[idx - 1];
                let delta_ticks = point.tick - prev.tick;
                current_us += ticks_to_us(delta_ticks, prev.us_per_quarter, ppq);
            }
            segments.push(TempoSegment {
                start_tick: point.tick,
                start_us: current_us,
                us_per_quarter: point.us_per_quarter,
            });
        }

        Self { ppq, segments }
    }

    pub fn tick_to_micros(&self, tick: Tick) -> i64 {
        let seg = self.segment_for_tick(tick);
        let delta_ticks = tick - seg.start_tick;
        seg.start_us + ticks_to_us(delta_ticks, seg.us_per_quarter, self.ppq)
    }

    pub fn micros_to_tick(&self, micros: i64) -> Tick {
        let seg = self.segment_for_micros(micros);
        let delta_us = micros - seg.start_us;
        let delta_ticks = us_to_ticks(delta_us, seg.us_per_quarter, self.ppq);
        seg.start_tick + delta_ticks
    }

    fn segment_for_tick(&self, tick: Tick) -> TempoSegment {
        let mut current = self.segments[0];
        for seg in &self.segments {
            if seg.start_tick > tick {
                break;
            }
            current = *seg;
        }
        current
    }

    fn us_per_quarter_at(&self, tick: Tick) -> u32 {
        self.segment_for_tick(tick).us_per_quarter
    }

    fn segment_for_micros(&self, micros: i64) -> TempoSegment {
        let mut current = self.segments[0];
        for seg in &self.segments {
            if seg.start_us > micros {
                break;
            }
            current = *seg;
        }
        current
    }
}

impl Transport {
    pub fn new(ppq: u16, sample_rate_hz: u32, tempo_points: Vec<TempoPoint>) -> Self {
        let tempo_map = TempoMap::new(ppq, tempo_points);
        Self {
            state: TransportState::Stopped,
            ppq,
            sample_rate_hz,
            origin_sample: 0,
            tempo_map,
            tempo_multiplier: 1.0,
            position_tick: 0,
            position_sample: 0,
            loop_range: None,
        }
    }

    pub fn state(&self) -> TransportState {
        self.state
    }

    pub fn play(&mut self) {
        self.state = TransportState::Playing;
    }

    pub fn pause(&mut self) {
        self.state = TransportState::Paused;
    }

    pub fn stop(&mut self) {
        self.state = TransportState::Stopped;
        let target_tick = self.loop_range.map(|range| range.start_tick).unwrap_or(0);
        self.seek(target_tick);
    }

    pub fn seek(&mut self, tick: Tick) {
        self.position_tick = tick;
        self.position_sample = self.tick_to_sample(tick);
    }

    pub fn align_to_sample_time(&mut self, sample_time: SampleTime) {
        let relative = self.tick_to_sample_relative(self.position_tick);
        self.origin_sample = sample_time.saturating_sub(relative);
        self.position_sample = sample_time;
    }

    pub fn set_origin_sample(&mut self, origin_sample: SampleTime) {
        self.origin_sample = origin_sample;
        self.position_sample = self.tick_to_sample(self.position_tick);
    }

    pub fn set_loop(&mut self, range: Option<LoopRange>) {
        self.loop_range = range;
    }

    pub fn set_tempo_multiplier(&mut self, multiplier: f32) {
        self.tempo_multiplier = multiplier.max(0.1);
        self.recalculate_origin();
    }

    pub fn set_sample_rate(&mut self, sample_rate_hz: u32) {
        self.sample_rate_hz = sample_rate_hz;
        self.recalculate_origin();
    }

    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    pub fn update_tempo_map(&mut self, points: Vec<TempoPoint>) {
        self.tempo_map = TempoMap::new(self.ppq, points);
        self.recalculate_origin();
    }

    pub fn advance_by_samples(&mut self, frames: u32) {
        if self.state != TransportState::Playing {
            return;
        }
        self.position_sample = self.position_sample.saturating_add(frames as u64);
        self.position_tick = self.sample_to_tick(self.position_sample);

        if let Some(loop_range) = self.loop_range {
            if self.position_tick >= loop_range.end_tick {
                self.seek(loop_range.start_tick);
            }
        }
    }

    pub fn now_tick(&self) -> Tick {
        self.position_tick
    }

    pub fn now_sample(&self) -> SampleTime {
        self.position_sample
    }

    pub fn tempo_multiplier(&self) -> f32 {
        self.tempo_multiplier
    }

    pub fn sync_to_sample_time(&mut self, sample_time: SampleTime) {
        self.position_sample = sample_time;
        self.position_tick = self.sample_to_tick(sample_time);
    }

    pub fn ms_to_ticks(&self, ms: i32) -> Tick {
        let us = ms as i64 * 1000;
        let us_per_quarter = self.tempo_map.us_per_quarter_at(self.position_tick);
        us_to_ticks(us, us_per_quarter, self.ppq)
    }

    pub fn tick_to_sample(&self, tick: Tick) -> SampleTime {
        let micros = self.tick_to_micros_scaled(tick);
        self.origin_sample
            .saturating_add(micros_to_samples(micros, self.sample_rate_hz))
    }

    pub fn sample_to_tick(&self, sample: SampleTime) -> Tick {
        let relative_sample = sample.saturating_sub(self.origin_sample);
        let micros = samples_to_micros(relative_sample, self.sample_rate_hz);
        let scaled = (micros as f64 * self.tempo_multiplier as f64).round() as i64;
        self.tempo_map.micros_to_tick(scaled)
    }

    fn tick_to_micros_scaled(&self, tick: Tick) -> i64 {
        let base = self.tempo_map.tick_to_micros(tick) as f64;
        (base / self.tempo_multiplier as f64).round() as i64
    }

    fn tick_to_sample_relative(&self, tick: Tick) -> SampleTime {
        let micros = self.tick_to_micros_scaled(tick);
        micros_to_samples(micros, self.sample_rate_hz)
    }

    fn recalculate_origin(&mut self) {
        let current_sample = self.position_sample;
        let relative = self.tick_to_sample_relative(self.position_tick);
        self.origin_sample = current_sample.saturating_sub(relative);
    }
}

fn ticks_to_us(ticks: Tick, us_per_quarter: u32, ppq: u16) -> i64 {
    let ticks = ticks as i128;
    let us_per_quarter = us_per_quarter as i128;
    let ppq = ppq as i128;
    ((ticks * us_per_quarter) / ppq) as i64
}

fn us_to_ticks(us: i64, us_per_quarter: u32, ppq: u16) -> Tick {
    let us = us as i128;
    let us_per_quarter = us_per_quarter as i128;
    let ppq = ppq as i128;
    ((us * ppq) / us_per_quarter) as Tick
}

fn micros_to_samples(micros: i64, sample_rate_hz: u32) -> SampleTime {
    if micros <= 0 {
        return 0;
    }
    let samples = (micros as f64 * sample_rate_hz as f64 / 1_000_000.0).round();
    samples as u64
}

fn samples_to_micros(sample: SampleTime, sample_rate_hz: u32) -> i64 {
    let micros = sample as f64 * 1_000_000.0 / sample_rate_hz as f64;
    micros.round() as i64
}
