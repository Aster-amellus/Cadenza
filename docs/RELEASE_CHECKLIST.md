# Release checklist (v0.1)

This checklist defines the minimum bar for a "shippable" Cadenza build on macOS.

## Build and smoke

- `cargo fmt`
- `cargo check`
- `cargo test`
- `cargo clippy --all-targets`
- `cargo run -p cadenza-app` launches and the UI renders.

## Audio + MIDI

- `Settings -> Audio Output`: selectable; `Test Sound` is audible.
- `Settings -> MIDI Input`: device appears after `Refresh` and NoteOn/Off updates the keyboard highlight.
- `Monitor`: toggling works (no audible monitoring when off).
- `Calibration -> Input Offset`: slider updates and persists (restart app to confirm).

## Practice loop

- Load a `.mid/.midi` file via `Browse -> Load` (no path issues).
- `Play/Pause/Stop` works; piano-roll advances smoothly.
- Piano-roll interaction:
  - Click seeks.
  - Drag sets loop; `Clear Loop` disables it.
- Tempo buttons (0.5x/0.8x/1.0x) update playback + UI.

## Sound quality

- Optional: load a `.sf2` SoundFont and confirm piano tone improves.
- Bus volumes + master volume do not produce hard clipping at default settings.

## PDF -> MIDI (Audiveris)

- Audiveris installed and set in `Settings -> Audiveris`.
- PDF conversion runs to completion; app stays responsive; `Cancel` works.
- Output MIDI saved automatically to `~/Downloads/Cadenza/` when output path is empty.
- Status shows paths for:
  - MIDI output
  - MusicXML output
  - `audiveris.log`
- `Reveal` buttons open Finder at the corresponding artifacts.

## Diagnostics

- `Settings -> Diagnostics -> Export bundle` writes a folder successfully (settings + device list + recent inputs).

