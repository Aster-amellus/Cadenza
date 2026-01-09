use cadenza_ports::audio::{AudioError, AudioOutputPort, AudioRenderCallback, AudioStreamHandle};
use cadenza_ports::types::{AudioConfig, AudioOutputDevice, DeviceId};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, SampleRate, StreamConfig, SupportedStreamConfigRange};
use std::sync::{mpsc, Arc};
use std::thread;

pub struct CpalAudioOutputPort {
    host: cpal::Host,
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

    fn select_stream_config(device: &cpal::Device, desired: AudioConfig) -> Result<StreamConfig, AudioError> {
        if desired.channels != 2 {
            return Err(AudioError::UnsupportedConfig(
                "only stereo output is supported in v0.1".to_string(),
            ));
        }

        let mut supported = device
            .supported_output_configs()
            .map_err(|e| AudioError::Backend(e.to_string()))?;

        let chosen = select_supported_config(&mut supported, desired)?;

        let mut config = chosen.config();

        config.buffer_size = match desired.buffer_size_frames {
            Some(frames) => BufferSize::Fixed(frames),
            None => BufferSize::Default,
        };

        Ok(config)
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
            let default_config = device
                .default_output_config()
                .map_err(|e| AudioError::DeviceUnavailable(e.to_string()))?;

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
        cb: Arc<dyn AudioRenderCallback>,
    ) -> Result<Box<dyn AudioStreamHandle>, AudioError> {
        let device_id = device_id.clone();
        let desired = config;
        let cb = cb.clone();
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

            let channels = stream_config.channels as usize;
            let mut left: Vec<f32> = Vec::new();
            let mut right: Vec<f32> = Vec::new();
            let mut sample_time: u64 = 0;
            let cb = cb.clone();

            let data_callback = move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                let frames = data.len() / channels;
                if left.len() != frames {
                    left.resize(frames, 0.0);
                    right.resize(frames, 0.0);
                }

                for value in left.iter_mut() {
                    *value = 0.0;
                }
                for value in right.iter_mut() {
                    *value = 0.0;
                }

                cb.render(sample_time, &mut left, &mut right);

                for frame in 0..frames {
                    let base = frame * channels;
                    data[base] = left[frame];
                    if channels > 1 {
                        data[base + 1] = right[frame];
                    }
                    for ch in 2..channels {
                        data[base + ch] = 0.0;
                    }
                }

                sample_time = sample_time.saturating_add(frames as u64);
            };

            let error_callback = |err| {
                eprintln!("cpal stream error: {}", err);
            };

            let stream = match device.build_output_stream(
                &stream_config,
                data_callback,
                error_callback,
                None,
            ) {
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

        match ready_rx.recv().map_err(|e| AudioError::Backend(e.to_string()))? {
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
    for config_range in supported {
        if config_range.channels() != desired.channels {
            continue;
        }
        if config_range.sample_format() != SampleFormat::F32 {
            continue;
        }
        let min = config_range.min_sample_rate().0;
        let max = config_range.max_sample_rate().0;
        if desired.sample_rate_hz < min || desired.sample_rate_hz > max {
            continue;
        }
        return Ok(config_range.with_sample_rate(SampleRate(desired.sample_rate_hz)));
    }

    Err(AudioError::UnsupportedConfig(
        "no matching f32 stereo stream config".to_string(),
    ))
}
