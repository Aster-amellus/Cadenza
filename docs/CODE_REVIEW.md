# Code review (current state)

This review focuses on architecture, correctness, realtime behavior, and "what blocks daily use".

## High-level summary

The workspace structure is solid (ports/domain/core/infra separation), and the app already demonstrates the end-to-end loop:
select devices -> load a score -> play -> render a piano roll -> export diagnostics.

The biggest gap between "it runs" and "it feels usable" is timing + realtime safety:
the audio callback path and synth implementations still rely on locking, and user input timing is not aligned to the audio clock.

## Strengths

- Clear crate split: domain logic is mostly isolated from platform backends.
- Minimal UI + IPC: `Command`/`Event` are simple and debuggable (`crates/cadenza-core/src/ipc.rs`).
- Robustness improvements already landed:
  - Audiveris stdout/stderr is redirected to a log file to avoid pipe deadlocks.
  - Deterministic event ordering (NoteOff before NoteOn at same time) is enforced in multiple stages.
  - MIDI import sanitizes overlapping NoteOn and closes dangling notes to avoid stuck notes.
  - Basic limiter in the audio graph reduces hard clipping.
- Tests exist for judge logic and MIDI roundtrip.
- Settings are now forward-compatible: missing fields in `settings.json` fall back to defaults instead of resetting all settings.

## Major risks / correctness issues

### 1) Realtime safety (audio thread)

Audio callback code must avoid blocking locks and dynamic allocation, but today:

- `AudioGraph` uses a `Mutex` for state (`crates/cadenza-core/src/audio_graph.rs`).
- `RustySynth` and `WaveguidePianoSynth` also lock internally from the audio thread.

This will eventually cause crackles under CPU pressure or contention, especially on macOS where CoreAudio callbacks are time-critical.

Recommended direction:
- Move audio callback state into the cpal callback closure (owned mutable state, no mutex).
- For synths, use per-bus lock-free queues for events and render from owned voice state.
- Keep parameters as atomics (already done for volumes).

### 2) Timing model for user input

`PlayerEvent.at` is captured from `Instant` and is now mapped into the audio clock via a lightweight "clock anchor" bridge.
This is a major step toward consistent judging + monitoring, but the current bridge is updated from the ~60Hz core tick loop,
so it still has jitter under load.

Recommended direction:
- Keep the bridge, but move anchor updates closer to the audio callback:
  - Store `(Instant, SampleTime)` atomically from the audio thread (or a lock-free side channel).
  - Use that anchor to convert `PlayerEvent.at` -> `SampleTime` -> `Tick`.

Current behavior:
- `Tick` uses `input_offset_ms` (judge alignment).
- Monitoring audio schedules at the estimated physical `SampleTime` (no extra audible latency).

### 3) PDF -> MIDI quality is currently bounded by OMR output

Even with better parsing, Audiveris can output messy MusicXML.
Cadenza needs importer robustness + post-processing (ties, voices, overlap cleanup, measure-aware clamping) to be usable.

## Module-by-module notes

### `src-tauri/`

- The Tauri shell is intentionally thin: one command (`send_command`) forwards to `AppCore` and emits events.
- PDF -> MIDI is handled as a background job in the shell, not in `AppCore`:
  - Good: avoids blocking core tick loop and avoids Audiveris pipe deadlocks.
  - Bad: duplicates the OMR backend (`crates/cadenza-infra-omr-audiveris` vs shell code).

Suggestion:
- Either route everything through `OmrPort` (and fix its implementation), or delete the unused OMR crate and keep shell-only.

Security note:
- `tauri.conf.json` currently uses `"allowlist": { "all": true }`. Tighten this before distributing builds.

### `ui/`

- Good: no bundler, small surface area, responsive layout.
- Piano roll rendering is fast enough and now includes pedal visualization and better contrast.

Opportunities:
- Add loop selection + tempo controls (core already supports commands).
- Add a simple "OMR quality disclaimer" and surface the generated MusicXML path for debugging.

### `crates/cadenza-core/`

- `AppCore` mixes responsibilities (device mgmt, transport, judge, scheduling, IO glue). This is acceptable for MVP.
- The core tick loop runs ~60Hz and emits events; it is easy to reason about.

Concerns:
- Audio and MIDI state crossing is partly implicit (e.g., input timing mapping, clock syncing).
- Event queues use fixed ring buffers; overflow is silently dropped in some places (acceptable, but should be counted).

### `crates/cadenza-domain-score/`

- MIDI import/export is straightforward and now has deterministic sorting.
- MusicXML importer is intentionally a subset, but should clearly document supported constructs and failure modes.

Important missing constructs for OMR:
- Ties and multi-voice rhythms.
- Basic pedal/dynamics are now partially supported, but still depends on Audiveris output.

### Infra crates

- `cadenza-infra-audio-cpal`: good MVP choice. Device IDs are based on enumeration order + name, which may not be stable.
- `cadenza-infra-midi-midir`: good MVP choice. Parsing is intentionally minimal (NoteOn/Off + CC64 only).
- `cadenza-infra-synth-rustysynth`: correct direction (SoundFont is the practical "real piano" path).
  - Needs a realtime-safe event/render path (avoid locking in audio thread).
- `cadenza-infra-synth-waveguide-piano`: useful fallback/testing synth, but should not be the "real piano" goal for v0.1.
- `cadenza-infra-storage-fs`: clean and fine for settings.

## Suggested next steps (prioritized)

See `docs/ROADMAP.md`. The short version:

1. Fix realtime safety + input timing alignment.
2. Improve PDF -> MIDI by importer robustness + post-processing.
3. Improve UX (loop/tempo/calibration) and ship a better SoundFont workflow.
