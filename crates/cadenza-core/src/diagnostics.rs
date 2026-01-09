use cadenza_ports::midi::MidiLikeEvent;
use cadenza_ports::storage::{SettingsDto, StorageError};
use cadenza_ports::types::{AudioOutputDevice, MidiInputDevice};
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Serialize)]
struct AppVersion {
    name: String,
    version: String,
}

#[derive(Serialize)]
struct PlatformInfo {
    os: String,
    arch: String,
}

#[derive(Serialize)]
struct DeviceSnapshot {
    midi_inputs: Vec<MidiInputDevice>,
    audio_outputs: Vec<AudioOutputDevice>,
}

#[derive(Serialize)]
struct RecentEvents {
    events: Vec<MidiLikeEvent>,
}

pub fn export_diagnostics(
    dir: &Path,
    settings: &SettingsDto,
    midi_inputs: Vec<MidiInputDevice>,
    audio_outputs: Vec<AudioOutputDevice>,
    recent_events: Vec<MidiLikeEvent>,
) -> Result<(), StorageError> {
    fs::create_dir_all(dir).map_err(|e| StorageError::Io(e.to_string()))?;

    let app_version = AppVersion {
        name: "Cadenza".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let platform = PlatformInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    };

    write_json(&dir.join("app_version.json"), &app_version)?;
    write_json(&dir.join("platform.json"), &platform)?;
    write_json(&dir.join("settings.json"), settings)?;
    write_json(
        &dir.join("device_snapshot.json"),
        &DeviceSnapshot {
            midi_inputs,
            audio_outputs,
        },
    )?;
    write_json(
        &dir.join("recent_events.json"),
        &RecentEvents { events: recent_events },
    )?;

    fs::write(dir.join("logs.txt"), b"logs not configured\n")
        .map_err(|e| StorageError::Io(e.to_string()))?;

    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StorageError> {
    let data = serde_json::to_vec_pretty(value).map_err(|e| StorageError::Serde(e.to_string()))?;
    fs::write(path, data).map_err(|e| StorageError::Io(e.to_string()))
}
