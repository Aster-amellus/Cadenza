use crate::audio_graph::{AudioClock, AudioGraph};
use crate::audio_params::AudioParams;
use crate::diagnostics::export_diagnostics;
use crate::ipc::{
    Command, Event, PianoRollNoteDto, PianoRollPedalDto, PianoRollTargetDto, ScoreSource,
    SessionState,
};
use crate::scheduler::{Scheduler, SchedulerConfig};
use crate::transport::Transport;
use cadenza_domain_eval::{
    AdvanceMode, ChordRollTicks, Grade, Judge, JudgeConfig, JudgeEvent, PlayerNoteOn,
    TimingWindowTicks, WrongNotePolicy,
};
use cadenza_domain_score::{
    export_midi_path, import_midi_path, import_musicxml_path, Score, TargetEvent,
};
use cadenza_ports::audio::{AudioError, AudioOutputPort, AudioRenderCallback, AudioStreamHandle};
use cadenza_ports::midi::{MidiError, MidiInputPort, MidiInputStream, MidiLikeEvent, PlayerEvent};
use cadenza_ports::omr::{OmrOptions, OmrPort};
use cadenza_ports::playback::{LoopRange, ScheduledEvent};
use cadenza_ports::storage::{SettingsDto, StorageError, StoragePort};
use cadenza_ports::synth::{SynthError, SynthPort};
use cadenza_ports::types::{AudioConfig, Bus, DeviceId, SampleTime, Tick};
use parking_lot::Mutex;
use rtrb::{Consumer, Producer, RingBuffer};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("audio error: {0}")]
    Audio(#[from] AudioError),
    #[error("midi error: {0}")]
    Midi(#[from] MidiError),
    #[error("omr error: {0}")]
    Omr(#[from] cadenza_ports::omr::OmrError),
    #[error("synth error: {0}")]
    Synth(#[from] SynthError),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("score load failed: {0}")]
    ScoreLoad(String),
}

pub struct AppCore {
    audio_port: Box<dyn AudioOutputPort>,
    midi_port: Box<dyn MidiInputPort>,
    synth: Arc<dyn SynthPort>,
    omr: Option<Box<dyn OmrPort>>,
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
    clock_anchor: Option<ClockAnchor>,
}

#[derive(Clone, Copy, Debug)]
struct ClockAnchor {
    at: Instant,
    sample_time: SampleTime,
}

impl AppCore {
    pub fn new(
        audio_port: Box<dyn AudioOutputPort>,
        midi_port: Box<dyn MidiInputPort>,
        synth: Arc<dyn SynthPort>,
        omr: Option<Box<dyn OmrPort>>,
        storage: Option<Box<dyn StoragePort>>,
    ) -> Result<Self, AppError> {
        let settings = if let Some(storage) = storage.as_ref() {
            storage.load_settings().unwrap_or_default()
        } else {
            SettingsDto::default()
        };

        let mut bootstrap_events = VecDeque::new();
        if let Some(path) = settings.default_sf2_path.clone() {
            match synth.load_soundfont_from_path(&path) {
                Ok(info) => bootstrap_events.push_back(Event::SoundFontStatus {
                    loaded: true,
                    path: Some(path),
                    name: Some(info.name),
                    preset_count: Some(info.preset_count as u32),
                    message: None,
                }),
                Err(err) => bootstrap_events.push_back(Event::SoundFontStatus {
                    loaded: false,
                    path: Some(path),
                    name: None,
                    preset_count: None,
                    message: Some(err.to_string()),
                }),
            }
        }

        let audio_params = Arc::new(AudioParams::new(&settings));
        let audio_clock = Arc::new(AudioClock::new());

        let transport = Transport::new(480, 48_000, Vec::new());
        let scheduler = Scheduler::new(48_000, SchedulerConfig { lookahead_ms: 30 });
        let judge = Judge::new(default_judge_config());

        Ok(Self {
            audio_port,
            midi_port,
            synth,
            omr,
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
            events: bootstrap_events,
            recent_inputs: VecDeque::with_capacity(32),
            last_transport_emit: Instant::now(),
            last_input_emit: Instant::now(),
            clock_anchor: None,
        })
    }

    pub fn handle_command(&mut self, cmd: Command) -> Result<(), AppError> {
        match cmd {
            Command::GetSessionState => {
                self.emit_session_state();
                self.emit_transport(true);
            }
            Command::ListMidiInputs => {
                let devices = self.midi_port.list_inputs()?;
                self.events.push_back(Event::MidiInputsUpdated { devices });
            }
            Command::SelectMidiInput { device_id } => {
                self.open_midi_input(device_id)?;
            }
            Command::ListAudioOutputs => {
                let devices = self.audio_port.list_outputs()?;
                self.events
                    .push_back(Event::AudioOutputsUpdated { devices });
            }
            Command::SelectAudioOutput { device_id, config } => {
                self.open_audio_output(device_id, config)?;
            }
            Command::TestAudio => {
                self.test_audio()?;
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
            Command::LoadSoundFont { path } => match self.synth.load_soundfont_from_path(&path) {
                Ok(info) => {
                    self.settings.default_sf2_path = Some(path.clone());
                    self.save_settings();
                    self.events.push_back(Event::SoundFontStatus {
                        loaded: true,
                        path: Some(path),
                        name: Some(info.name),
                        preset_count: Some(info.preset_count as u32),
                        message: None,
                    });
                }
                Err(err) => {
                    self.events.push_back(Event::SoundFontStatus {
                        loaded: false,
                        path: Some(path),
                        name: None,
                        preset_count: None,
                        message: Some(err.to_string()),
                    });
                    return Err(err.into());
                }
            },
            Command::SetProgram { bus, gm_program } => {
                self.synth.set_program(bus, gm_program)?;
            }
            Command::LoadScore { source } => {
                self.load_score(source)?;
            }
            Command::SetPracticeRange {
                start_tick,
                end_tick,
            } => {
                self.set_loop(Some(LoopRange {
                    start_tick,
                    end_tick,
                }));
            }
            Command::StartPractice => {
                if self.session_state == SessionState::Running {
                    return Ok(());
                }
                if self.score.is_none() {
                    return Err(AppError::InvalidState("no score loaded".to_string()));
                }
                self.ensure_audio_output_open()?;
                self.transport.align_to_sample_time(self.audio_clock.get());
                self.scheduler.seek(self.transport.now_tick());
                self.flush_audio_notes();
                self.session_state = SessionState::Running;
                self.transport.play();
                self.audio_params.set_playback_enabled(true);
                self.schedule_autopilot();
                self.emit_session_state();
            }
            Command::PausePractice => {
                self.session_state = SessionState::Paused;
                self.transport.pause();
                self.audio_params.set_playback_enabled(false);
                self.emit_session_state();
                self.flush_audio_notes();
            }
            Command::StopPractice => {
                self.session_state = SessionState::Ready;
                self.transport.stop();
                self.scheduler.seek(self.transport.now_tick());
                self.audio_params.set_playback_enabled(false);
                self.emit_session_state();
                self.flush_audio_notes();
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
            Command::SetAccompanimentRoute {
                play_left,
                play_right,
            } => {
                self.scheduler
                    .set_accompaniment_route(play_left, play_right);
            }
            Command::SetInputOffsetMs { ms } => {
                self.settings.input_offset_ms = ms;
                self.emit_session_state();
                self.save_settings();
            }
            Command::SetAudiverisPath { path } => {
                self.settings.audiveris_path = Some(path);
                self.save_settings();
            }
            Command::ConvertPdfToMidi {
                pdf_path,
                output_path,
                audiveris_path,
            } => {
                self.convert_pdf_to_midi(&pdf_path, &output_path, audiveris_path)?;
            }
            Command::CancelPdfToMidi => {}
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

    fn test_audio(&mut self) -> Result<(), AppError> {
        if !self.settings.monitor_enabled {
            return Err(AppError::InvalidState(
                "Monitor is disabled (Settings -> Monitor). Enable it to hear the test note."
                    .to_string(),
            ));
        }

        self.ensure_audio_output_open()?;
        let Some(producer) = self.audio_queue_tx.as_mut() else {
            return Err(AppError::InvalidState(
                "Audio output not initialized".to_string(),
            ));
        };

        let start = self.audio_clock.get().saturating_add(64);
        let duration_frames = (self.transport.sample_rate_hz() as f32 * 0.25).round() as u64;

        let note = 60u8;
        let velocity = 96u8;
        let _ = producer.push(ScheduledEvent {
            sample_time: start,
            bus: Bus::UserMonitor,
            event: MidiLikeEvent::NoteOn { note, velocity },
        });
        let _ = producer.push(ScheduledEvent {
            sample_time: start.saturating_add(duration_frames),
            bus: Bus::UserMonitor,
            event: MidiLikeEvent::NoteOff { note },
        });

        Ok(())
    }

    fn convert_pdf_to_midi(
        &mut self,
        pdf_path: &str,
        output_path: &str,
        audiveris_path: Option<String>,
    ) -> Result<(), AppError> {
        let Some(omr) = self.omr.as_ref() else {
            return Err(AppError::ScoreLoad("OMR engine not configured".to_string()));
        };

        let options = OmrOptions {
            enable_diagnostics: true,
            engine_path: audiveris_path.or_else(|| self.settings.audiveris_path.clone()),
        };

        let result = omr.recognize_pdf(pdf_path, options)?;
        let musicxml_path = result
            .musicxml_path
            .ok_or_else(|| AppError::ScoreLoad("OMR did not produce MusicXML".to_string()))?;
        let score =
            import_musicxml_path(&musicxml_path).map_err(|e| AppError::ScoreLoad(e.to_string()))?;
        export_midi_path(&score, Path::new(output_path))
            .map_err(|e| AppError::ScoreLoad(e.to_string()))?;
        Ok(())
    }

    fn ensure_audio_output_open(&mut self) -> Result<(), AppError> {
        if self.audio_stream.is_some() {
            return Ok(());
        }

        let device_id = if let Some(id) = self.settings.selected_audio_out.clone() {
            id
        } else {
            let devices = self.audio_port.list_outputs()?;
            let first = devices.first().ok_or_else(|| {
                AudioError::DeviceUnavailable("no audio outputs found".to_string())
            })?;
            first.id.clone()
        };

        self.open_audio_output(device_id, None)?;
        Ok(())
    }

    pub fn tick(&mut self) {
        self.update_clock_anchor();
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

        let fallback_config = AudioConfig {
            sample_rate_hz: 48_000,
            channels: 2,
            buffer_size_frames: None,
        };

        let requested_config = config;
        let mut config = match config {
            Some(config) => config,
            None => match self.audio_port.list_outputs() {
                Ok(devices) => devices
                    .into_iter()
                    .find(|d| d.id == device_id)
                    .map(|d| d.default_config)
                    .unwrap_or(fallback_config),
                Err(_) => fallback_config,
            },
        };

        if config.buffer_size_frames == Some(0) {
            config.buffer_size_frames = None;
        }

        // Persist requested buffer size selection, but keep the existing setting if the caller
        // didn't provide a config override.
        if requested_config.is_some() {
            self.settings.audio_buffer_size_frames = config.buffer_size_frames;
        }

        // Apply persisted buffer size for callers that didn't request a specific size.
        if config.buffer_size_frames.is_none() {
            if let Some(frames) = self.settings.audio_buffer_size_frames {
                config.buffer_size_frames = Some(frames);
            }
        }

        self.transport.set_sample_rate(config.sample_rate_hz);
        self.synth.set_sample_rate(config.sample_rate_hz);
        self.scheduler =
            Scheduler::new(config.sample_rate_hz, SchedulerConfig { lookahead_ms: 30 });
        if let Some(score) = self.score.as_ref() {
            if let Some(track) = score.tracks.first() {
                self.scheduler.set_score(track.playback_events.clone());
            }
        }

        let (producer, consumer) = RingBuffer::new(4096);
        let max_frames = config
            .buffer_size_frames
            .map(|f| f as usize)
            .unwrap_or(8192);
        let audio_graph = AudioGraph::new(
            self.synth.clone(),
            self.audio_params.clone(),
            consumer,
            self.audio_clock.clone(),
            max_frames,
        );

        self.audio_clock.set(0);
        self.transport.set_origin_sample(0);

        let stream = self.audio_port.open_output(
            &device_id,
            config,
            Box::new(audio_graph) as Box<dyn AudioRenderCallback>,
        )?;

        self.audio_stream = Some(stream);
        self.audio_queue_tx = Some(producer);
        self.settings.selected_audio_out = Some(device_id);
        self.audio_params
            .set_playback_enabled(self.session_state == SessionState::Running);
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
                let path = normalize_fs_path(&path);
                let path = resolve_existing_path(path, &["mid", "midi"]);
                import_midi_path(&path).map_err(|e| {
                    AppError::ScoreLoad(format!("midi load failed for {}: {e}", path.display()))
                })?
            }
            ScoreSource::MusicXmlFile(path) => {
                let path = normalize_fs_path(&path);
                let path = resolve_existing_path(path, &["mxl", "xml"]);
                import_musicxml_path(&path).map_err(|e| {
                    AppError::ScoreLoad(format!("musicxml load failed for {}: {e}", path.display()))
                })?
            }
            ScoreSource::InternalDemo(id) => build_demo_score(&id),
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
        self.audio_params.set_playback_enabled(false);
        self.emit_score_view();
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
        while let Ok(event) = consumer.pop() {
            pending.push(event);
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
                let judge_events = self.judge.on_note_on(PlayerNoteOn {
                    tick,
                    note,
                    velocity,
                });
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
            JudgeEvent::Miss { target_id, .. } => {
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

    fn map_player_event(&self, event: &PlayerEvent) -> Option<(Tick, SampleTime)> {
        let sample_time = self.estimate_sample_time(event.at);

        // Input offset should affect judging (tick alignment), but should not introduce audible
        // latency for monitoring. We therefore compute tick with the offset, while scheduling
        // monitor audio at the estimated physical sample_time.
        let offset_ticks = self.transport.ms_to_ticks(self.settings.input_offset_ms);

        let tick = if self.session_state == SessionState::Running {
            self.transport
                .sample_to_tick(sample_time)
                .saturating_add(offset_ticks)
        } else {
            self.transport.now_tick().saturating_add(offset_ticks)
        };

        Some((tick, sample_time))
    }

    fn estimate_sample_time(&self, at: Instant) -> SampleTime {
        let Some(anchor) = self.clock_anchor else {
            return self.audio_clock.get();
        };

        let sample_rate_hz = self.transport.sample_rate_hz().max(1) as f64;
        if at >= anchor.at {
            let dt_s = at.duration_since(anchor.at).as_secs_f64();
            let delta_samples = (dt_s * sample_rate_hz).round() as u64;
            anchor.sample_time.saturating_add(delta_samples)
        } else {
            let dt_s = anchor.at.duration_since(at).as_secs_f64();
            let delta_samples = (dt_s * sample_rate_hz).round() as u64;
            anchor.sample_time.saturating_sub(delta_samples)
        }
    }

    fn record_recent_input(&mut self, event: MidiLikeEvent) {
        if self.recent_inputs.len() >= 20 {
            self.recent_inputs.pop_front();
        }
        self.recent_inputs.push_back(event);
        self.events.push_back(Event::MidiInputEvent { event });
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

    fn emit_score_view(&mut self) {
        let Some(score) = self.score.as_ref() else {
            return;
        };

        let Some(track) = score.tracks.first() else {
            self.events.push_back(Event::ScoreViewUpdated {
                title: score.meta.title.clone(),
                ppq: score.ppq,
                notes: Vec::new(),
                targets: Vec::new(),
                pedal: Vec::new(),
            });
            return;
        };

        let notes = derive_note_spans(score.ppq, &track.playback_events);
        let pedal = derive_pedal_spans(&track.playback_events);
        let mut targets: Vec<PianoRollTargetDto> = track
            .targets
            .iter()
            .map(|t| PianoRollTargetDto {
                id: t.id,
                tick: t.tick,
                notes: t.notes.clone(),
            })
            .collect();
        targets.sort_by_key(|t| t.tick);

        self.events.push_back(Event::ScoreViewUpdated {
            title: score.meta.title.clone(),
            ppq: score.ppq,
            notes,
            targets,
            pedal,
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

    fn update_clock_anchor(&mut self) {
        if self.audio_stream.is_none() {
            self.clock_anchor = None;
            return;
        }

        self.clock_anchor = Some(ClockAnchor {
            at: Instant::now(),
            sample_time: self.audio_clock.get(),
        });
    }

    fn flush_audio_notes(&mut self) {
        let Some(producer) = self.audio_queue_tx.as_mut() else {
            return;
        };
        let now = self.audio_clock.get();
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

fn normalize_fs_path(raw: &str) -> PathBuf {
    let mut s = raw.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s = &s[1..s.len().saturating_sub(1)];
    }

    if let Some(decoded) = decode_file_url(s) {
        return PathBuf::from(decoded);
    }

    let expanded = expand_tilde(s);
    if expanded.is_relative() {
        if let Ok(cwd) = std::env::current_dir() {
            return cwd.join(expanded);
        }
    }
    expanded
}

fn resolve_existing_path(path: PathBuf, extensions: &[&str]) -> PathBuf {
    if path.exists() {
        return path;
    }

    if path.extension().is_some() {
        return path;
    }

    for ext in extensions {
        let candidate = path.with_extension(ext);
        if candidate.exists() {
            return candidate;
        }
    }

    path
}

fn decode_file_url(s: &str) -> Option<String> {
    let s = s.strip_prefix("file://")?;
    let s = s.strip_prefix("localhost").unwrap_or(s);
    if !s.starts_with('/') {
        return None;
    }
    Some(percent_decode(s))
}

fn build_demo_score(id: &str) -> Score {
    let ppq: u16 = 480;
    let tempo_map = vec![cadenza_domain_score::TempoPoint {
        tick: 0,
        us_per_quarter: 500_000,
    }];

    let (title, notes) = match id {
        "c_major_scale" | "scale_c_major" | "scale" => (
            "Demo: C major scale".to_string(),
            vec![60u8, 62, 64, 65, 67, 69, 71, 72],
        ),
        _ => (
            "Demo: C major scale".to_string(),
            vec![60u8, 62, 64, 65, 67, 69, 71, 72],
        ),
    };

    let mut playback_events = Vec::new();
    let mut targets = Vec::new();

    let dur = Tick::from(ppq);
    for (idx, note) in notes.into_iter().enumerate() {
        let tick = Tick::from(idx as i64) * dur;
        let velocity = 92u8;
        playback_events.push(cadenza_domain_score::PlaybackMidiEvent {
            tick,
            event: MidiLikeEvent::NoteOn { note, velocity },
            hand: None,
        });
        playback_events.push(cadenza_domain_score::PlaybackMidiEvent {
            tick: tick + dur,
            event: MidiLikeEvent::NoteOff { note },
            hand: None,
        });

        targets.push(TargetEvent {
            id: (idx as u64) + 1,
            tick,
            notes: vec![note],
            hand: None,
            measure_index: None,
        });
    }

    Score {
        meta: cadenza_domain_score::ScoreMeta {
            title: Some(title),
            source: cadenza_domain_score::ScoreSource::Internal,
        },
        ppq,
        tempo_map,
        tracks: vec![cadenza_domain_score::Track {
            id: 0,
            name: "Demo".to_string(),
            hand: None,
            targets,
            playback_events,
        }],
    }
}

fn percent_decode(s: &str) -> String {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn expand_tilde(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix("~/") else {
        return PathBuf::from(path);
    };
    let Some(home) = std::env::var_os("HOME") else {
        return PathBuf::from(path);
    };
    PathBuf::from(home).join(rest)
}

fn default_judge_config() -> JudgeConfig {
    JudgeConfig {
        window: TimingWindowTicks {
            perfect: 30,
            good: 80,
        },
        chord_roll: ChordRollTicks(24),
        wrong_note_policy: WrongNotePolicy::DegradePerfect,
        advance: AdvanceMode::OnResolve,
    }
}

fn derive_note_spans(
    ppq: u16,
    events: &[cadenza_domain_score::PlaybackMidiEvent],
) -> Vec<PianoRollNoteDto> {
    let default_len = Tick::from(ppq.max(1));
    let mut stacks: Vec<Vec<(Tick, u8, Option<cadenza_domain_score::Hand>)>> =
        vec![Vec::new(); 128];
    let mut notes: Vec<PianoRollNoteDto> = Vec::new();

    for event in events {
        match event.event {
            MidiLikeEvent::NoteOn { note, velocity } => {
                let idx = note as usize;
                if idx < stacks.len() {
                    stacks[idx].push((event.tick, velocity, event.hand));
                }
            }
            MidiLikeEvent::NoteOff { note } => {
                let idx = note as usize;
                if idx >= stacks.len() {
                    continue;
                }
                if let Some((start_tick, velocity, hand)) = stacks[idx].pop() {
                    let mut end_tick = event.tick;
                    if end_tick <= start_tick {
                        end_tick = start_tick.saturating_add(1);
                    }
                    notes.push(PianoRollNoteDto {
                        note,
                        start_tick,
                        end_tick,
                        velocity,
                        hand,
                    });
                }
            }
            MidiLikeEvent::Cc64 { .. } => {}
        }
    }

    for (note, stack) in stacks.iter_mut().enumerate() {
        while let Some((start_tick, velocity, hand)) = stack.pop() {
            let end_tick = start_tick.saturating_add(default_len);
            notes.push(PianoRollNoteDto {
                note: note as u8,
                start_tick,
                end_tick,
                velocity,
                hand,
            });
        }
    }

    notes.sort_by(|a, b| a.start_tick.cmp(&b.start_tick).then(a.note.cmp(&b.note)));
    notes
}

fn derive_pedal_spans(
    events: &[cadenza_domain_score::PlaybackMidiEvent],
) -> Vec<PianoRollPedalDto> {
    let mut cc: Vec<(Tick, bool)> = Vec::new();
    let mut last_tick: Tick = 0;

    for event in events {
        last_tick = last_tick.max(event.tick);
        if let MidiLikeEvent::Cc64 { value } = event.event {
            cc.push((event.tick, value >= 64));
        }
    }

    if cc.is_empty() {
        return Vec::new();
    }

    cc.sort_by(|a, b| a.0.cmp(&b.0));

    let mut spans = Vec::new();
    let mut down = false;
    let mut start = 0;

    for (tick, is_down) in cc {
        if is_down && !down {
            down = true;
            start = tick;
        } else if !is_down && down {
            down = false;
            if tick > start {
                spans.push(PianoRollPedalDto {
                    start_tick: start,
                    end_tick: tick,
                });
            }
        }
    }

    if down {
        let end_tick = last_tick.saturating_add(1).max(start.saturating_add(1));
        spans.push(PianoRollPedalDto {
            start_tick: start,
            end_tick,
        });
    }

    spans
}
