use cadenza_core::{AppCore, Command, Event};
use cadenza_domain_score::{export_midi_path, import_musicxml_path};
use cadenza_infra_audio_cpal::CpalAudioOutputPort;
use cadenza_infra_midi_midir::MidirMidiInputPort;
use cadenza_infra_storage_fs::FsStorage;
use cadenza_infra_synth_rustysynth::RustySynth;
use cadenza_ports::storage::StoragePort;
use parking_lot::Mutex;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

#[derive(Clone)]
struct AppState {
    core: Arc<Mutex<AppCore>>,
    pdf_job: Arc<Mutex<Option<PdfJob>>>,
}

struct PdfJob {
    cancel_tx: mpsc::Sender<()>,
}

#[tauri::command]
fn send_command(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    command: Command,
) -> Result<(), String> {
    match command {
        Command::ConvertPdfToMidi {
            pdf_path,
            output_path,
            audiveris_path,
        } => {
            start_pdf_to_midi_job(app, state, pdf_path, output_path, audiveris_path)?;
            Ok(())
        }
        Command::CancelPdfToMidi => {
            cancel_pdf_to_midi_job(state);
            Ok(())
        }
        other => {
            let mut core = state.core.lock();
            core.handle_command(other).map_err(|err| err.to_string())
        }
    }
}

#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("Empty path".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("open -R failed (exit: {})", status));
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let status = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path))
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("explorer failed (exit: {})", status));
        }
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let status = std::process::Command::new("xdg-open")
            .arg(path)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("xdg-open failed (exit: {})", status));
        }
        return Ok(());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("Unsupported platform".to_string())
    }
}

fn main() {
    let audio_port = Box::new(CpalAudioOutputPort::new());
    let midi_port = Box::new(MidirMidiInputPort::new("Cadenza"));
    let synth = Arc::new(RustySynth::default());
    let omr = None;
    let storage: Option<Box<dyn StoragePort>> = Some(Box::new(FsStorage::default()));

    let core = AppCore::new(audio_port, midi_port, synth, omr, storage)
        .expect("failed to initialize core");
    let state = AppState {
        core: Arc::new(Mutex::new(core)),
        pdf_job: Arc::new(Mutex::new(None)),
    };

    tauri::Builder::default()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![send_command, reveal_path])
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

fn start_pdf_to_midi_job(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    pdf_path: String,
    output_path: String,
    audiveris_path: Option<String>,
) -> Result<(), String> {
    let resolved_output_path = resolve_output_path(&pdf_path, &output_path)?;
    let resolved_output_path = resolved_output_path.to_string_lossy().into_owned();

    {
        let mut job = state.pdf_job.lock();
        if job.is_some() {
            return Err("PDF conversion already running".to_string());
        }
        let (cancel_tx, cancel_rx) = mpsc::channel();
        *job = Some(PdfJob { cancel_tx });
        drop(job);

        let job_state = state.pdf_job.clone();
        std::thread::spawn(move || {
            let _ = app.emit_all(
                "core_event",
                Event::OmrProgress {
                    page: 0,
                    total: 0,
                    stage: "Starting".to_string(),
                },
            );

            let result = run_pdf_to_midi(
                &pdf_path,
                &resolved_output_path,
                audiveris_path.as_deref(),
                &cancel_rx,
                |stage| {
                    let _ = app.emit_all(
                        "core_event",
                        Event::OmrProgress {
                            page: 0,
                            total: 0,
                            stage: stage.to_string(),
                        },
                    );
                },
            );

            match result {
                Ok(done) => {
                    let _ = app.emit_all(
                        "core_event",
                        Event::OmrDiagnostics {
                            severity: "info".to_string(),
                            message: done.message.clone(),
                            page: None,
                        },
                    );
                    let _ = app.emit_all(
                        "core_event",
                        Event::PdfToMidiFinished {
                            ok: true,
                            pdf_path: pdf_path.clone(),
                            output_path: resolved_output_path.clone(),
                            musicxml_path: done
                                .musicxml_path
                                .as_ref()
                                .map(|p| p.to_string_lossy().into_owned()),
                            diagnostics_path: done
                                .diagnostics_path
                                .as_ref()
                                .map(|p| p.to_string_lossy().into_owned()),
                            message: done.message,
                        },
                    );
                }
                Err(err) => {
                    let _ = app.emit_all(
                        "core_event",
                        Event::OmrDiagnostics {
                            severity: "error".to_string(),
                            message: err.message.clone(),
                            page: None,
                        },
                    );
                    let _ = app.emit_all(
                        "core_event",
                        Event::PdfToMidiFinished {
                            ok: false,
                            pdf_path: pdf_path.clone(),
                            output_path: resolved_output_path.clone(),
                            musicxml_path: None,
                            diagnostics_path: err
                                .diagnostics_path
                                .as_ref()
                                .map(|p| p.to_string_lossy().into_owned()),
                            message: err.message,
                        },
                    );
                }
            }

            let mut job = job_state.lock();
            *job = None;
        });
    }
    Ok(())
}

fn cancel_pdf_to_midi_job(state: tauri::State<'_, AppState>) {
    let job = state.pdf_job.lock();
    if let Some(job) = job.as_ref() {
        let _ = job.cancel_tx.send(());
    }
}

struct PdfToMidiOk {
    message: String,
    musicxml_path: Option<PathBuf>,
    diagnostics_path: Option<PathBuf>,
}

struct PdfToMidiErr {
    message: String,
    diagnostics_path: Option<PathBuf>,
}

fn run_pdf_to_midi(
    pdf_path: &str,
    output_path: &str,
    audiveris_path: Option<&str>,
    cancel_rx: &mpsc::Receiver<()>,
    mut progress: impl FnMut(&str),
) -> Result<PdfToMidiOk, PdfToMidiErr> {
    progress("Running Audiveris");

    let engine = audiveris_path.unwrap_or("audiveris");
    let engine = normalize_engine_path(engine);

    let input_path = Path::new(pdf_path);
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| PdfToMidiErr {
            message: "Invalid PDF filename".to_string(),
            diagnostics_path: None,
        })?;

    let output_dir = make_workdir().map_err(|e| PdfToMidiErr {
        message: e,
        diagnostics_path: None,
    })?;
    let diagnostics_path = output_dir.join("audiveris.log");

    let log_file = File::create(&diagnostics_path).map_err(|e| PdfToMidiErr {
        message: format!("Failed to create diagnostics log: {e}"),
        diagnostics_path: Some(diagnostics_path.clone()),
    })?;
    let log_file_err = log_file.try_clone().map_err(|e| PdfToMidiErr {
        message: format!("Failed to clone diagnostics log handle: {e}"),
        diagnostics_path: Some(diagnostics_path.clone()),
    })?;

    let mut child = std::process::Command::new(engine)
        .arg("-batch")
        .arg("-export")
        .arg("-output")
        .arg(&output_dir)
        .arg(input_path)
        // Avoid deadlocking on large Audiveris output by redirecting directly to a log file.
        .stdout(std::process::Stdio::from(log_file))
        .stderr(std::process::Stdio::from(log_file_err))
        .spawn()
        .map_err(|e| PdfToMidiErr {
            message: if e.kind() == std::io::ErrorKind::NotFound {
                "Audiveris not found. Install Audiveris and set its path in Settings → Audiveris (e.g., /Applications/Audiveris.app).".to_string()
            } else {
                format!("Failed to launch Audiveris: {e}")
            },
            diagnostics_path: Some(diagnostics_path.clone()),
        })?;

    let mut cancelled = false;
    let status = loop {
        if cancel_rx.try_recv().is_ok() {
            cancelled = true;
            let _ = child.kill();
        }
        match child.try_wait() {
            Ok(Some(done)) => {
                break done;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PdfToMidiErr {
                    message: format!("Failed waiting for Audiveris: {err}"),
                    diagnostics_path: Some(diagnostics_path),
                });
            }
        }
    };

    if cancelled {
        return Err(PdfToMidiErr {
            message: "Conversion cancelled".to_string(),
            diagnostics_path: Some(diagnostics_path),
        });
    }

    if !status.success() {
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".to_string());
        return Err(PdfToMidiErr {
            message: format!(
                "Audiveris failed (exit code: {code}). See diagnostics log for details."
            ),
            diagnostics_path: Some(diagnostics_path),
        });
    }

    progress("Import MusicXML");
    let musicxml_path = find_output_musicxml(&output_dir, stem).ok_or_else(|| PdfToMidiErr {
        message: "Audiveris did not produce MusicXML (.mxl/.xml)".to_string(),
        diagnostics_path: Some(diagnostics_path.clone()),
    })?;

    let score = import_musicxml_path(&musicxml_path).map_err(|e| PdfToMidiErr {
        message: format!("MusicXML import failed: {e}"),
        diagnostics_path: Some(diagnostics_path.clone()),
    })?;

    progress("Export MIDI");
    let output_path = Path::new(output_path);
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    export_midi_path(&score, output_path).map_err(|e| PdfToMidiErr {
        message: format!(
            "MIDI export failed writing to {}: {e}",
            output_path.display()
        ),
        diagnostics_path: Some(diagnostics_path.clone()),
    })?;

    progress("Done");
    Ok(PdfToMidiOk {
        message: format!(
            "Wrote MIDI to {} (MusicXML: {})",
            output_path.display(),
            musicxml_path.display()
        ),
        musicxml_path: Some(musicxml_path),
        diagnostics_path: Some(diagnostics_path),
    })
}

fn normalize_engine_path(engine: &str) -> String {
    let engine = engine.trim();
    if engine.eq_ignore_ascii_case("audiveris") {
        if let Some(candidate) = default_audiveris_engine() {
            return candidate;
        }
    }

    let path = Path::new(engine);
    let ext_is_app = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"));

    if ext_is_app {
        let candidate = path.join("Contents").join("MacOS").join("Audiveris");
        if candidate.exists() {
            return candidate.to_string_lossy().into_owned();
        }
    }

    engine.to_string()
}

fn default_audiveris_engine() -> Option<String> {
    let candidates = [
        PathBuf::from("/Applications/Audiveris.app"),
        tauri::api::path::home_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Applications")
            .join("Audiveris.app"),
    ];

    for candidate in candidates {
        let bin = candidate.join("Contents").join("MacOS").join("Audiveris");
        if bin.exists() {
            return Some(bin.to_string_lossy().into_owned());
        }
    }
    None
}

fn make_workdir() -> Result<PathBuf, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let pid = std::process::id();
    let dir = std::env::temp_dir()
        .join("cadenza-omr")
        .join(format!("job-{}-{}", pid, now));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn find_output_musicxml(output_dir: &Path, stem: &str) -> Option<PathBuf> {
    let mxl = output_dir.join(format!("{}.mxl", stem));
    if mxl.exists() {
        return Some(mxl);
    }
    let xml = output_dir.join(format!("{}.xml", stem));
    if xml.exists() {
        return Some(xml);
    }

    find_output_musicxml_recursive(output_dir, stem, 0)
}

fn find_output_musicxml_recursive(dir: &Path, stem: &str, depth: usize) -> Option<PathBuf> {
    if depth > 6 {
        return None;
    }

    let entries = std::fs::read_dir(dir).ok()?;
    let mut best_other: Option<PathBuf> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_output_musicxml_recursive(&path, stem, depth + 1) {
                return Some(found);
            }
            continue;
        }

        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !(ext.eq_ignore_ascii_case("mxl") || ext.eq_ignore_ascii_case("xml")) {
            continue;
        }

        let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if file_stem == stem {
            return Some(path);
        }

        // Keep a fallback in case Audiveris produced a different name.
        if best_other.is_none() {
            best_other = Some(path);
        }
    }

    best_other
}

fn resolve_output_path(pdf_path: &str, output_path: &str) -> Result<PathBuf, String> {
    let pdf_path = Path::new(pdf_path);
    let default_name = pdf_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(sanitize_file_stem)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "score".to_string());

    let base_dir = default_export_dir()?;

    let output_path = output_path.trim();
    let mut candidate = if output_path.is_empty() {
        base_dir.join(format!("{default_name}.mid"))
    } else {
        expand_tilde(output_path)
    };

    if candidate.is_relative() {
        candidate = base_dir.join(candidate);
    }

    let ends_with_sep = output_path.ends_with('/') || output_path.ends_with('\\');
    if ends_with_sep || candidate.is_dir() {
        candidate = candidate.join(format!("{default_name}.mid"));
    }

    let ext = candidate.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !(ext.eq_ignore_ascii_case("mid") || ext.eq_ignore_ascii_case("midi")) {
        candidate.set_extension("mid");
    }

    if let Some(parent) = candidate.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }

    make_unique_path(candidate)
}

fn default_export_dir() -> Result<PathBuf, String> {
    let dir = tauri::api::path::download_dir()
        .or_else(|| tauri::api::path::home_dir().map(|home| home.join("Downloads")))
        .unwrap_or_else(std::env::temp_dir)
        .join("Cadenza");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn expand_tilde(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix("~/") else {
        return PathBuf::from(path);
    };
    let home = tauri::api::path::home_dir()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    home.join(rest)
}

fn sanitize_file_stem(stem: &str) -> String {
    let mut out = String::new();
    for ch in stem.chars() {
        if ch.is_control()
            || matches!(
                ch,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\u{0}'
            )
        {
            out.push('_');
            continue;
        }
        out.push(ch);
    }
    out.trim().trim_matches('.').to_string()
}

fn make_unique_path(path: PathBuf) -> Result<PathBuf, String> {
    if !path.exists() {
        return Ok(path);
    }

    let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("mid");

    for idx in 1..=999 {
        let candidate = parent.join(format!("{stem}-{idx}.{ext}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    Ok(parent.join(format!("{stem}-{now}.{ext}")))
}
