# Cadenza

Rust workspace with a Tauri shell and a static UI (`ui/`) for MIDI practice.

## Run (dev)
- `cargo run -p cadenza-app`

## First run checklist
- `Settings`: confirm `Audio Output` is selected and click `Test Sound`.
- If you hear crackles, increase `Settings -> Audio Output -> Buffer (frames)` (higher latency, more stable).
- (Optional) Plug in a MIDI keyboard and select it under `MIDI Input`.
- (Recommended) Load a `.sf2` SoundFont for a more realistic piano: `Settings` -> `SoundFont (.sf2)`.

## Current features
- Practice view includes a realtime staff view + piano-roll + keyboard highlight.
- Load MIDI (`.mid`/`.midi`) and play it (built-in synth; optional `.sf2` SoundFont for better piano).
- `Demo` button loads an internal C-major scale for quick smoke tests.
- Click the piano roll to `Seek`, or drag to set a `Loop` range.
- Tempo quick buttons (0.5x/0.8x/1.0x) and input offset calibration slider.
- Select Audio Output + MIDI Input, toggle monitor, adjust master/bus volumes.
- `Test Sound` button (Settings) verifies audio output quickly.
- PDF -> MIDI via Audiveris (external OMR): defaults to `~/Downloads/Cadenza/<score>.mid` and loads the generated MusicXML into Practice for better fidelity.
- Export a diagnostics bundle to a chosen folder.

## Audiveris (macOS)
- Homebrew does not ship Audiveris. Download it from the official releases and place `Audiveris.app` in `/Applications`.
- In-app: `Settings` -> `Audiveris` -> set path to `/Applications/Audiveris.app` (or the `Audiveris` binary inside it).
- More: `docs/Audiveris.md`

## SoundFont (optional)
- For a realistic piano, load a good `.sf2` SoundFont (files are often large; not bundled).
- Good starting points to search for:
  - `MuseScore General.sf2`
  - `FluidR3 GM.sf2`
  - `GeneralUser GS.sf2`
- In-app: `Settings` -> `SoundFont (.sf2)` -> `Browse` -> `Load`.
- More: `docs/SoundFont.md`

## MIDI keyboard (macOS)
- Plug in the keyboard via USB (e.g., Casio PX-150), confirm it appears in “Audio MIDI Setup”.
- In-app: `Settings` -> `MIDI Input` -> `Refresh` -> select the device.
- If you hear doubled/flanged sound, turn down the keyboard speakers or set “Local Control” off on the keyboard.

## Docs
- Architecture overview: `docs/ARCHITECTURE.md`
- Roadmap and priorities: `docs/ROADMAP.md`
