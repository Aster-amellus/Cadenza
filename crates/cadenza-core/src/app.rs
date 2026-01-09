use crate::audio_graph::{AudioClock, AudioGraph};
use crate::audio_params::AudioParams;
use crate::diagnostics::export_diagnostics;
use crate::ipc::{Command, Event, ScoreSource, SessionState};
use crate::scheduler::{Scheduler, SchedulerConfig};
use crate::transport::Transport;
use cadenza_domain_eval::{
    AdvanceMode, ChordRollTicks, Grade, Judge, JudgeConfig, JudgeEvent, PlayerNoteOn,
    TimingWindowTicks, WrongNotePolicy,
};
use cadenza_domain_score::{import_midi_path, Score, TargetEvent};
use cadenza_ports::audio::{AudioError, AudioOutputPort, AudioRenderCallback, AudioStreamHandle};
use cadenza_ports::midi::{MidiError, MidiInputPort, MidiInputStream, MidiLikeEvent, PlayerEvent};
use cadenza_ports::playback::{LoopRange, ScheduledEvent};
use cadenza_ports::storage::{SettingsDto, StorageError, StoragePort};
use cadenza_ports::synth::{SynthError, SynthPort};
use cadenza_ports::types::{AudioConfig, Bus, DeviceId, SampleTime, Tick};
use parking_lot::Mutex;
use rtrb::{Consumer, Producer, RingBuffer};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("audio error: {0}")]
    Audio(#[from] AudioError),
    #[error("midi error: {0}")]
    Midi(#[from] MidiError),
    #[error("synth error: {0}")]
    Synth(#[from] SynthError),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("score load failed: {0}")]
    ScoreLoad(String),
}

pub struct AppCore {
    audio_port: Box<dyn AudioOutputPort>,
    midi_port: Box<dyn MidiInputPort>,
    synth: Arc<dyn SynthPort>,
    storage: Option<Box<dyn StoragePort>>,
    settings: SettingsDto,
    session_state: SessionState,
    transport: Transport,
    scheduler: Scheduler,
    judge: Judge,
    score: Option<Score>,
    targets: HashMap<u64, TargetEvent>,
    audio_params: Arc<AudioParams>,
    audio_clock: Arc<AudioClock>,
    audio_stream: Option<Box<dyn AudioStreamHandle>>,
    audio_queue_tx: Option<Producer<ScheduledEvent>>,
    midi_stream: Option<Box<dyn MidiInputStream>>,
    midi_queue_rx: Option<Consumer<PlayerEvent>>,
    events: VecDeque<Event>,
    recent_inputs: VecDeque<MidiLikeEvent>,
    last_transport_emit: Instant,
    last_input_emit: Instant,
}

impl AppCore {
    pub fn new(
        audio_port: Box<dyn AudioOutputPort>,
        midi_port: Box<dyn MidiInputPort>,
        synth: Arc<dyn SynthPort>,
        storage: Option<Box<dyn StoragePort>>,
    ) -> Result<Self, AppError> {
        let settings = if let Some(storage) = storage.as_ref() {
            storage.load_settings().unwrap_or_default()
        } else {
            SettingsDto::default()
        };

        let audio_params = Arc::new(AudioParams::new(&settings));
        let audio_clock = Arc::new(AudioClock::new());

        let transport = Transport::new(480, 48_000, Vec::new());
        let scheduler = Scheduler::new(48_000, SchedulerConfig { lookahead_ms: 30 });
        let judge = Judge::new(default_judge_config());

        Ok(Self {
            audio_port,
            midi_port,
            synth,
            storage,
            settings,
            session_state: SessionState::Idle,
            transport,
            scheduler,
            judge,
            score: None,
            targets: HashMap::new(),
            audio_params,
            audio_clock,
            audio_stream: None,
            audio_queue_tx: None,
            midi_stream: None,
            midi_queue_rx: None,
            events: VecDeque::new(),
            recent_inputs: VecDeque::with_capacity(32),
            last_transport_emit: Instant::now(),
            last_input_emit: Instant::now(),
        })
    }

    pub fn handle_command(&mut self, cmd: Command) -> Result<(), AppError> {
        match cmd {
            Command::ListMidiInputs => {
                let devices = self.midi_port.list_inputs()?;
                self.events.push_back(Event::MidiInputsUpdated { devices });
            }
            Command::SelectMidiInput { device_id } => {
                self.open_midi_input(device_id)?;
            }
            Command::ListAudioOutputs => {
                let devices = self.audio_port.list_outputs()?;
                self.events.push_back(Event::AudioOutputsUpdated { devices });
            }
            Command::SelectAudioOutput { device_id, config } => {
                self.open_audio_output(device_id, config)?;
            }
            Command::SetMonitorEnabled { enabled } => {
                self.settings.monitor_enabled = enabled;
                self.audio_params.set_monitor_enabled(enabled);
                self.emit_session_state();
                self.save_settings();
            }
            Command::SetBusVolume { bus, volume } => {
                match bus {
                    Bus::UserMonitor => self.settings.bus_user_volume = volume,
                    Bus::Autopilot => self.settings.bus_autopilot_volume = volume,
                    Bus::MetronomeFx => self.settings.bus_metronome_volume = volume,
                }
                self.audio_params.set_bus(bus, volume);
                self.emit_session_state();
                self.save_settings();
            }
            Command::SetMasterVolume { volume } => {
                self.settings.master_volume = volume;
                self.audio_params.set_master(volume);
                self.emit_session_state();
                self.save_settings();
            }
            Command::LoadSoundFont { path } => {
                self.synth.load_soundfont_from_path(&path)?;
                self.settings.default_sf2_path = Some(path);
                self.save_settings();
            }
            Command::SetProgram { bus, gm_program } => {
                self.synth.set_program(bus, gm_program)?;
            }
            Command::LoadScore { source } => {
                self.load_score(source)?;
            }
            Command::SetPracticeRange { start_tick, end_tick } => {
                self.set_loop(Some(LoopRange {
                    start_tick,
                    end_tick,
                }));
            }
            Command::StartPractice => {
                self.session_state = SessionState::Running;
                self.transport.play();
                self.emit_session_state();
            }
            Command::PausePractice => {
                self.session_state = SessionState::Paused;
                self.transport.pause();
                self.emit_session_state();
            }
            Command::StopPractice => {
                self.session_state = SessionState::Ready;
                self.transport.stop();
                self.emit_session_state();
            }
            Command::Seek { tick } => {
                self.transport.seek(tick);
                self.scheduler.seek(tick);
                self.flush_audio_notes();
                self.emit_transport(true);
            }
            Command::SetLoop {
                enabled,
                start_tick,
                end_tick,
            } => {
                let range = if enabled {
                    Some(LoopRange {
                        start_tick,
                        end_tick,
                    })
                } else {
                    None
                };
                self.set_loop(range);
            }
            Command::SetTempoMultiplier { x } => {
                self.transport.set_tempo_multiplier(x);
                self.emit_transport(true);
            }
            Command::SetPlaybackMode { mode } => {
                self.scheduler.set_mode(mode);
            }
            Command::SetAccompanimentRoute { play_left, play_right } => {
                self.scheduler.set_accompaniment_route(play_left, play_right);
            }
            Command::SetInputOffsetMs { ms } => {
                self.settings.input_offset_ms = ms;
                self.save_settings();
            }
            Command::ExportDiagnostics { path } => {
                let midi_inputs = self.midi_port.list_inputs()?;
                let audio_outputs = self.audio_port.list_outputs()?;
                export_diagnostics(
                    Path::new(&path),
                    &self.settings,
                    midi_inputs,
                    audio_outputs,
                    self.recent_inputs.iter().copied().collect(),
                )?;
            }
        }
        Ok(())
    }

    pub fn tick(&mut self) {
        self.sync_transport();
        self.process_midi_inputs();
        self.advance_judge();
        self.schedule_autopilot();
        self.emit_transport(false);
        self.emit_recent_inputs();
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        self.events.drain(..).collect()
    }

    fn open_audio_output(
        &mut self,
        device_id: DeviceId,
        config: Option<AudioConfig>,
    ) -> Result<(), AppError> {
        if let Some(stream) = self.audio_stream.take() {
            stream.close();
        }

        let config = config.unwrap_or(AudioConfig {
            sample_rate_hz: 48_000,
            channels: 2,
            buffer_size_frames: None,
        });

        self.transport.set_sample_rate(config.sample_rate_hz);
        self.scheduler = Scheduler::new(config.sample_rate_hz, SchedulerConfig { lookahead_ms: 30 });
        if let Some(score) = self.score.as_ref() {
            if let Some(track) = score.tracks.first() {
                self.scheduler.set_score(track.playback_events.clone());
            }
        }

        let (producer, consumer) = RingBuffer::new(4096);
        let audio_graph = AudioGraph::new(
            self.synth.clone(),
            self.audio_params.clone(),
            consumer,
            self.audio_clock.clone(),
        );

        let stream = self.audio_port.open_output(
            &device_id,
            config,
            Arc::new(audio_graph) as Arc<dyn AudioRenderCallback>,
        )?;

        self.audio_stream = Some(stream);
        self.audio_queue_tx = Some(producer);
        self.settings.selected_audio_out = Some(device_id);
        self.emit_session_state();
        self.save_settings();
        Ok(())
    }

    fn open_midi_input(&mut self, device_id: DeviceId) -> Result<(), AppError> {
        if let Some(stream) = self.midi_stream.take() {
            stream.close();
        }

        let (producer, consumer) = RingBuffer::new(2048);
        let producer = Arc::new(Mutex::new(producer));
        let cb = Arc::new(move |event: PlayerEvent| {
            if let Some(mut guard) = producer.try_lock() {
                let _ = guard.push(event);
            }
        });

        let stream = self.midi_port.open_input(&device_id, cb)?;
        self.midi_stream = Some(stream);
        self.midi_queue_rx = Some(consumer);
        self.settings.selected_midi_in = Some(device_id);
        self.emit_session_state();
        self.save_settings();
        Ok(())
    }

    fn load_score(&mut self, source: ScoreSource) -> Result<(), AppError> {
        let score = match source {
            ScoreSource::MidiFile(path) => {
                import_midi_path(std::path::Path::new(&path))
                    .map_err(|e| AppError::ScoreLoad(e.to_string()))?
            }
            ScoreSource::MusicXmlFile(_path) => {
                return Err(AppError::ScoreLoad(
                    "MusicXML import not implemented".to_string(),
                ))
            }
            ScoreSource::InternalDemo(_id) => {
                return Err(AppError::ScoreLoad("demo score not implemented".to_string()))
            }
        };

        self.apply_score(score);
        Ok(())
    }

    fn apply_score(&mut self, score: Score) {
        let tempo_map: Vec<_> = score
            .tempo_map
            .iter()
            .map(|point| cadenza_domain_score::TempoPoint {
                tick: point.tick,
                us_per_quarter: point.us_per_quarter,
            })
            .collect();

        self.transport.update_tempo_map(tempo_map);
        self.transport.seek(0);

        let mut targets = Vec::new();
        let mut playback_events = Vec::new();

        if let Some(track) = score.tracks.first() {
            targets = track.targets.clone();
            playback_events = track.playback_events.clone();
        }

        self.targets = targets.iter().map(|t| (t.id, t.clone())).collect();
        let judge_events = self.judge.load_targets(targets);
        for event in judge_events {
            self.handle_judge_event(event);
        }

        self.scheduler.set_score(playback_events);
        self.score = Some(score);
        self.session_state = SessionState::Ready;
        self.emit_session_state();
        self.emit_transport(true);
    }

    fn schedule_autopilot(&mut self) {
        if self.session_state != SessionState::Running {
            return;
        }
        let Some(producer) = self.audio_queue_tx.as_mut() else {
            return;
        };
        let scheduled = self.scheduler.schedule(&mut self.transport);
        for event in scheduled {
            let _ = producer.push(event);
        }
    }

    fn process_midi_inputs(&mut self) {
        let Some(mut consumer) = self.midi_queue_rx.take() else {
            return;
        };
        let Some(mut producer) = self.audio_queue_tx.take() else {
            self.midi_queue_rx = Some(consumer);
            return;
        };

        let mut pending = Vec::new();
        loop {
            match consumer.pop() {
                Ok(event) => pending.push(event),
                Err(_) => break,
            }
        }

        for event in pending {
            self.record_recent_input(event.event);
            if let Some((tick, sample_time)) = self.map_player_event(&event) {
                self.route_player_event(event.event, tick, sample_time, &mut producer);
            }
        }

        self.audio_queue_tx = Some(producer);
        self.midi_queue_rx = Some(consumer);
    }

    fn route_player_event(
        &mut self,
        event: MidiLikeEvent,
        tick: Tick,
        sample_time: SampleTime,
        producer: &mut Producer<ScheduledEvent>,
    ) {
        match event {
            MidiLikeEvent::NoteOn { note, velocity } => {
                let judge_events = self.judge.on_note_on(PlayerNoteOn { tick, note, velocity });
                for event in judge_events {
                    self.handle_judge_event(event);
                }
            }
            MidiLikeEvent::NoteOff { .. } | MidiLikeEvent::Cc64 { .. } => {}
        }

        if self.settings.monitor_enabled {
            let scheduled = ScheduledEvent {
                sample_time,
                bus: Bus::UserMonitor,
                event,
            };
            let _ = producer.push(scheduled);
        }
    }

    fn advance_judge(&mut self) {
        if self.session_state != SessionState::Running {
            return;
        }
        let now_tick = self.transport.now_tick();
        let judge_events = self.judge.advance_to(now_tick);
        for event in judge_events {
            self.handle_judge_event(event);
        }
    }

    fn handle_judge_event(&mut self, event: JudgeEvent) {
        match event {
            JudgeEvent::Hit {
                target_id,
                grade,
                delta_tick,
                ..
            } => {
                let expected_notes = self
                    .targets
                    .get(&target_id)
                    .map(|t| t.notes.clone())
                    .unwrap_or_default();
                self.events.push_back(Event::JudgeFeedback {
                    target_id,
                    grade,
                    delta_tick,
                    expected_notes,
                    played_notes: Vec::new(),
                });
            }
            JudgeEvent::Miss {
                target_id, ..
            } => {
                let expected_notes = self
                    .targets
                    .get(&target_id)
                    .map(|t| t.notes.clone())
                    .unwrap_or_default();
                self.events.push_back(Event::JudgeFeedback {
                    target_id,
                    grade: Grade::Miss,
                    delta_tick: 0,
                    expected_notes,
                    played_notes: Vec::new(),
                });
            }
            JudgeEvent::Stats {
                combo,
                score,
                hit,
                miss,
                ..
            } => {
                let total = hit + miss;
                let accuracy = if total == 0 {
                    0.0
                } else {
                    hit as f32 / total as f32
                };
                self.events.push_back(Event::ScoreSummaryUpdated {
                    combo,
                    score,
                    accuracy,
                });
            }
            JudgeEvent::FocusChanged { .. } => {}
        }
    }

    fn map_player_event(&self, _event: &PlayerEvent) -> Option<(Tick, SampleTime)> {
        let offset_ticks = self.transport.ms_to_ticks(self.settings.input_offset_ms);
        let tick = self.transport.now_tick() + offset_ticks;
        let sample_time = self.transport.tick_to_sample(tick);
        Some((tick, sample_time))
    }

    fn record_recent_input(&mut self, event: MidiLikeEvent) {
        if self.recent_inputs.len() >= 20 {
            self.recent_inputs.pop_front();
        }
        self.recent_inputs.push_back(event);
    }

    fn emit_recent_inputs(&mut self) {
        if self.last_input_emit.elapsed() < Duration::from_millis(50) {
            return;
        }
        if !self.recent_inputs.is_empty() {
            self.events.push_back(Event::RecentInputEvents {
                events: self.recent_inputs.iter().copied().collect(),
            });
        }
        self.last_input_emit = Instant::now();
    }

    fn emit_session_state(&mut self) {
        self.events.push_back(Event::SessionStateUpdated {
            state: self.session_state,
            settings: self.settings.clone(),
        });
    }

    fn emit_transport(&mut self, force: bool) {
        let now = Instant::now();
        if !force && now.duration_since(self.last_transport_emit) < Duration::from_millis(33) {
            return;
        }
        self.events.push_back(Event::TransportUpdated {
            tick: self.transport.now_tick(),
            sample_time: self.transport.now_sample(),
            playing: self.session_state == SessionState::Running,
            tempo_multiplier: self.transport.tempo_multiplier(),
            loop_range: self.scheduler.loop_range(),
        });
        self.last_transport_emit = now;
    }

    fn set_loop(&mut self, range: Option<LoopRange>) {
        self.scheduler.set_loop(range);
        self.transport.set_loop(range);
        self.emit_transport(true);
    }

    fn sync_transport(&mut self) {
        if self.session_state != SessionState::Running {
            return;
        }
        let sample_time = self.audio_clock.get();
        self.transport.sync_to_sample_time(sample_time);
    }

    fn flush_audio_notes(&mut self) {
        let Some(producer) = self.audio_queue_tx.as_mut() else {
            return;
        };
        let now = self.transport.now_sample();
        let mut events = Vec::new();
        for note in 0..128u8 {
            events.push(ScheduledEvent {
                sample_time: now,
                bus: Bus::Autopilot,
                event: MidiLikeEvent::NoteOff { note },
            });
            events.push(ScheduledEvent {
                sample_time: now,
                bus: Bus::UserMonitor,
                event: MidiLikeEvent::NoteOff { note },
            });
        }
        events.push(ScheduledEvent {
            sample_time: now,
            bus: Bus::Autopilot,
            event: MidiLikeEvent::Cc64 { value: 0 },
        });
        events.push(ScheduledEvent {
            sample_time: now,
            bus: Bus::UserMonitor,
            event: MidiLikeEvent::Cc64 { value: 0 },
        });

        for event in events {
            let _ = producer.push(event);
        }
    }

    fn save_settings(&self) {
        if let Some(storage) = self.storage.as_ref() {
            let _ = storage.save_settings(&self.settings);
        }
    }
}

fn default_judge_config() -> JudgeConfig {
    JudgeConfig {
        window: TimingWindowTicks { perfect: 30, good: 80 },
        chord_roll: ChordRollTicks(24),
        wrong_note_policy: WrongNotePolicy::DegradePerfect,
        advance: AdvanceMode::OnResolve,
    }
}
