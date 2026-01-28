# Architecture

This repository is a Rust workspace with a Tauri shell and a static (no-bundler) UI.
The goal is to keep domain logic platform-agnostic and push all I/O into infra crates.

## Workspace layout

- `crates/cadenza-ports/`: stable traits + shared DTO types (audio, MIDI, synth, storage, OMR).
- `crates/cadenza-domain-score/`: score model + import/export (MIDI, MusicXML).
- `crates/cadenza-domain-eval/`: judging/scoring logic (pure, deterministic).
- `crates/cadenza-core/`: AppCore orchestration (commands/events, transport, scheduler, audio graph).
- `crates/cadenza-infra-*`: platform backends (cpal audio, midir MIDI, rustysynth synth, fs storage, audiveris OMR).
- `src-tauri/`: Tauri shell hosting the UI and bridging to AppCore via IPC.
- `ui/`: static HTML/CSS/JS frontend (Tauri `withGlobalTauri=true`).

## Thread model (current)

- UI thread (WebView): sends commands via `invoke("send_command", ...)`.
- Core thread (Rust): a background loop ticks `AppCore` ~60Hz and emits `Event` via `emit_all("core_event", ...)`.
- Audio thread (cpal): pulls scheduled events, feeds the synth, renders stereo PCM.
- MIDI callback thread (midir): normalizes raw MIDI bytes into `MidiLikeEvent` and pushes into a ring buffer.
- OMR job thread (PDF -> MIDI): spawns Audiveris, writes logs, then imports MusicXML and exports MIDI.

## Data flow

1. UI -> Rust: `Command` (see `crates/cadenza-core/src/ipc.rs`)
2. AppCore mutates state and enqueues `Event` for UI (same file).
3. For playback, AppCore schedules `ScheduledEvent { sample_time, bus, event }` into a ring buffer.
4. AudioGraph consumes scheduled events and calls `SynthPort::handle_event/render`.

## Buses (routing)

Audio is mixed from three buses:

- `UserMonitor`: live monitoring of user MIDI input (toggleable).
- `Autopilot`: playback of the loaded score.
- `MetronomeFx`: reserved for click/FX (not fully implemented yet).

Each bus maintains its own sustain state (CC64) inside the synth.

## Score pipeline

- MIDI import/export: `crates/cadenza-domain-score/src/midi_{import,export}.rs`
- MusicXML import (subset): `crates/cadenza-domain-score/src/musicxml_import.rs`
- Score view derivation for UI (note spans + pedal spans): `crates/cadenza-core/src/app.rs`

## OMR (PDF -> MIDI)

The Tauri shell currently runs Audiveris directly to avoid deadlocks when capturing large stdout/stderr.
Workflow:

`PDF` -> `Audiveris` -> `MusicXML (.mxl/.xml)` -> `Score` -> `MIDI (.mid)`

See `docs/Audiveris.md`.
