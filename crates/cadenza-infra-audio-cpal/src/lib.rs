use cadenza_ports::audio::{AudioError, AudioOutputPort, AudioRenderCallback, AudioStreamHandle};
use cadenza_ports::types::{AudioConfig, AudioOutputDevice, DeviceId};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, SampleRate, StreamConfig, SupportedStreamConfigRange};
use std::sync::mpsc;
use std::thread;

pub struct CpalAudioOutputPort {
    host: cpal::Host,
}

struct SelectedStreamConfig {
    config: StreamConfig,
    sample_format: SampleFormat,
}

impl CpalAudioOutputPort {
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }

    pub fn with_host(host: cpal::Host) -> Self {
        Self { host }
    }

    fn list_devices_from_host(
        host: &cpal::Host,
    ) -> Result<Vec<(DeviceId, cpal::Device)>, AudioError> {
        let host_id = format!("{:?}", host.id());
        let devices = host
            .output_devices()
            .map_err(|e| AudioError::Backend(e.to_string()))?;

        let mut list = Vec::new();
        for (index, device) in devices.enumerate() {
            let name = device
                .name()
                .unwrap_or_else(|_| "Unknown Output".to_string());
            let id = DeviceId(format!("cpal:{}:{}:{}", host_id, index, name));
            list.push((id, device));
        }

        Ok(list)
    }

    fn select_stream_config(
        device: &cpal::Device,
        desired: AudioConfig,
    ) -> Result<SelectedStreamConfig, AudioError> {
        let mut supported = device
            .supported_output_configs()
            .map_err(|e| AudioError::Backend(e.to_string()))?;

        let chosen = select_supported_config(&mut supported, desired)?;

        let sample_format = chosen.sample_format();
        let mut config = chosen.config();

        config.buffer_size = match desired.buffer_size_frames {
            Some(frames) => BufferSize::Fixed(frames),
            None => BufferSize::Default,
        };

        Ok(SelectedStreamConfig {
            config,
            sample_format,
        })
    }
}

impl Default for CpalAudioOutputPort {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CpalAudioStreamHandle {
    stop_tx: mpsc::Sender<()>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl AudioStreamHandle for CpalAudioStreamHandle {
    fn close(mut self: Box<Self>) {
        let _ = self.stop_tx.send(());
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

impl AudioOutputPort for CpalAudioOutputPort {
    fn list_outputs(&self) -> Result<Vec<AudioOutputDevice>, AudioError> {
        let devices = Self::list_devices_from_host(&self.host)?;
        let mut results = Vec::new();

        for (id, device) in devices {
            let name = device
                .name()
                .unwrap_or_else(|_| "Unknown Output".to_string());
            let default_config = match device.default_output_config() {
                Ok(config) => config,
                Err(_) => continue,
            };

            let config = AudioConfig {
                sample_rate_hz: default_config.sample_rate().0,
                channels: default_config.channels(),
                buffer_size_frames: None,
            };

            results.push(AudioOutputDevice {
                id,
                name,
                default_config: config,
            });
        }

        Ok(results)
    }

    fn open_output(
        &self,
        device_id: &DeviceId,
        config: AudioConfig,
        cb: Box<dyn AudioRenderCallback>,
    ) -> Result<Box<dyn AudioStreamHandle>, AudioError> {
        let device_id = device_id.clone();
        let desired = config;
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (stop_tx, stop_rx) = mpsc::channel();

        let join_handle = thread::spawn(move || {
            let host = cpal::default_host();
            let devices = match Self::list_devices_from_host(&host) {
                Ok(list) => list,
                Err(err) => {
                    let _ = ready_tx.send(Err(err));
                    return;
                }
            };

            let device = match devices.into_iter().find(|(id, _)| id == &device_id) {
                Some((_, device)) => device,
                None => {
                    let _ = ready_tx.send(Err(AudioError::DeviceNotFound(device_id.to_string())));
                    return;
                }
            };

            let stream_config = match Self::select_stream_config(&device, desired) {
                Ok(config) => config,
                Err(err) => {
                    let _ = ready_tx.send(Err(err));
                    return;
                }
            };

            let channels = stream_config.config.channels as usize;
            let initial_frames = match stream_config.config.buffer_size {
                BufferSize::Fixed(frames) => frames as usize,
                BufferSize::Default => 8192,
            };
            let left: Vec<f32> = vec![0.0; initial_frames];
            let right: Vec<f32> = vec![0.0; initial_frames];
            let sample_time: u64 = 0;

            let error_callback = |err| {
                eprintln!("cpal stream error: {}", err);
            };

            let stream = match (stream_config.sample_format, cb) {
                (SampleFormat::F32, mut cb) => {
                    let mut sample_time = sample_time;
                    let mut left = left;
                    let mut right = right;
                    device.build_output_stream(
                        &stream_config.config,
                        move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                            let frames = data.len() / channels;
                            if frames > left.len() {
                                left.resize(frames, 0.0);
                                right.resize(frames, 0.0);
                            }
                            cb.render(sample_time, &mut left[..frames], &mut right[..frames]);
                            write_interleaved_f32(
                                data,
                                channels,
                                &left[..frames],
                                &right[..frames],
                            );
                            sample_time = sample_time.saturating_add(frames as u64);
                        },
                        error_callback,
                        None,
                    )
                }
                (SampleFormat::I16, mut cb) => {
                    let mut sample_time = sample_time;
                    let mut left = left;
                    let mut right = right;
                    device.build_output_stream(
                        &stream_config.config,
                        move |data: &mut [i16], _info: &cpal::OutputCallbackInfo| {
                            let frames = data.len() / channels;
                            if frames > left.len() {
                                left.resize(frames, 0.0);
                                right.resize(frames, 0.0);
                            }
                            cb.render(sample_time, &mut left[..frames], &mut right[..frames]);
                            write_interleaved_i16(
                                data,
                                channels,
                                &left[..frames],
                                &right[..frames],
                            );
                            sample_time = sample_time.saturating_add(frames as u64);
                        },
                        error_callback,
                        None,
                    )
                }
                (SampleFormat::U16, mut cb) => {
                    let mut sample_time = sample_time;
                    let mut left = left;
                    let mut right = right;
                    device.build_output_stream(
                        &stream_config.config,
                        move |data: &mut [u16], _info: &cpal::OutputCallbackInfo| {
                            let frames = data.len() / channels;
                            if frames > left.len() {
                                left.resize(frames, 0.0);
                                right.resize(frames, 0.0);
                            }
                            cb.render(sample_time, &mut left[..frames], &mut right[..frames]);
                            write_interleaved_u16(
                                data,
                                channels,
                                &left[..frames],
                                &right[..frames],
                            );
                            sample_time = sample_time.saturating_add(frames as u64);
                        },
                        error_callback,
                        None,
                    )
                }
                _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
            };

            let stream = match stream {
                Ok(stream) => stream,
                Err(err) => {
                    let _ = ready_tx.send(Err(AudioError::Backend(err.to_string())));
                    return;
                }
            };

            if let Err(err) = stream.play() {
                let _ = ready_tx.send(Err(AudioError::Backend(err.to_string())));
                return;
            }

            let _ = ready_tx.send(Ok(()));
            let _ = stop_rx.recv();
            drop(stream);
        });

        match ready_rx
            .recv()
            .map_err(|e| AudioError::Backend(e.to_string()))?
        {
            Ok(()) => Ok(Box::new(CpalAudioStreamHandle {
                stop_tx,
                join_handle: Some(join_handle),
            })),
            Err(err) => Err(err),
        }
    }
}

fn select_supported_config(
    supported: &mut dyn Iterator<Item = SupportedStreamConfigRange>,
    desired: AudioConfig,
) -> Result<cpal::SupportedStreamConfig, AudioError> {
    let mut best: Option<cpal::SupportedStreamConfig> = None;
    let mut best_score: i32 = -1;

    for config_range in supported {
        if config_range.channels() != desired.channels {
            continue;
        }
        let min = config_range.min_sample_rate().0;
        let max = config_range.max_sample_rate().0;
        if desired.sample_rate_hz < min || desired.sample_rate_hz > max {
            continue;
        }

        let score = match config_range.sample_format() {
            SampleFormat::F32 => 3,
            SampleFormat::I16 => 2,
            SampleFormat::U16 => 1,
            _ => 0,
        };

        if score > best_score {
            best = Some(config_range.with_sample_rate(SampleRate(desired.sample_rate_hz)));
            best_score = score;
        }
    }

    if let Some(best) = best {
        return Ok(best);
    }

    Err(AudioError::UnsupportedConfig(
        "no matching stream config".to_string(),
    ))
}

fn write_interleaved_f32(data: &mut [f32], channels: usize, left: &[f32], right: &[f32]) {
    let frames = data.len() / channels;
    for frame in 0..frames {
        let base = frame * channels;
        let l = left.get(frame).copied().unwrap_or(0.0);
        let r = right.get(frame).copied().unwrap_or(0.0);
        match channels {
            0 => {}
            1 => data[base] = (l + r) * 0.5,
            _ => {
                data[base] = l;
                data[base + 1] = r;
                for ch in 2..channels {
                    data[base + ch] = 0.0;
                }
            }
        }
    }
}

fn write_interleaved_i16(data: &mut [i16], channels: usize, left: &[f32], right: &[f32]) {
    let frames = data.len() / channels;
    for frame in 0..frames {
        let base = frame * channels;
        let l = left.get(frame).copied().unwrap_or(0.0);
        let r = right.get(frame).copied().unwrap_or(0.0);
        match channels {
            0 => {}
            1 => data[base] = f32_to_i16((l + r) * 0.5),
            _ => {
                data[base] = f32_to_i16(l);
                data[base + 1] = f32_to_i16(r);
                for ch in 2..channels {
                    data[base + ch] = 0;
                }
            }
        }
    }
}

fn write_interleaved_u16(data: &mut [u16], channels: usize, left: &[f32], right: &[f32]) {
    let frames = data.len() / channels;
    for frame in 0..frames {
        let base = frame * channels;
        let l = left.get(frame).copied().unwrap_or(0.0);
        let r = right.get(frame).copied().unwrap_or(0.0);
        match channels {
            0 => {}
            1 => data[base] = f32_to_u16((l + r) * 0.5),
            _ => {
                data[base] = f32_to_u16(l);
                data[base + 1] = f32_to_u16(r);
                for ch in 2..channels {
                    data[base + ch] = u16::MAX / 2;
                }
            }
        }
    }
}

fn f32_to_i16(value: f32) -> i16 {
    let v = value.clamp(-1.0, 1.0);
    (v * i16::MAX as f32) as i16
}

fn f32_to_u16(value: f32) -> u16 {
    let v = value.clamp(-1.0, 1.0);
    let scaled = (v * 0.5 + 0.5) * u16::MAX as f32;
    scaled.round().clamp(0.0, u16::MAX as f32) as u16
}
