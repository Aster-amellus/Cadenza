use cadenza_core::{AppCore, Command};
use cadenza_infra_audio_cpal::CpalAudioOutputPort;
use cadenza_infra_midi_midir::MidirMidiInputPort;
use cadenza_infra_storage_fs::FsStorage;
use cadenza_infra_synth_simple::SimpleSynth;
use cadenza_ports::storage::StoragePort;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

#[derive(Clone)]
struct AppState {
    core: Arc<Mutex<AppCore>>,
}

#[tauri::command]
fn send_command(state: tauri::State<'_, AppState>, command: Command) -> Result<(), String> {
    let mut core = state.core.lock();
    core.handle_command(command).map_err(|err| err.to_string())
}

fn main() {
    let audio_port = Box::new(CpalAudioOutputPort::new());
    let midi_port = Box::new(MidirMidiInputPort::new("Cadenza"));
    let synth = Arc::new(SimpleSynth::default());
    let storage: Option<Box<dyn StoragePort>> = Some(Box::new(FsStorage::default()));

    let core = AppCore::new(audio_port, midi_port, synth, storage)
        .expect("failed to initialize core");
    let state = AppState {
        core: Arc::new(Mutex::new(core)),
    };

    tauri::Builder::default()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![send_command])
        .setup(move |app| {
            let app_handle = app.handle();
            let core = state.core.clone();
            std::thread::spawn(move || loop {
                let events = {
                    let mut core = core.lock();
                    core.tick();
                    core.drain_events()
                };

                for event in events {
                    let _ = app_handle.emit_all("core_event", event);
                }

                std::thread::sleep(Duration::from_millis(16));
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
