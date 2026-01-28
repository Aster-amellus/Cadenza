# Roadmap

This document is a working plan for getting Cadenza from "prototype" to "daily-usable".

## P0: Stability and correctness (next)

- Audio: remove locking from the audio callback path (avoid `Mutex` in render), reduce allocations, add underrun/crackle diagnostics.
- Timing: map `PlayerEvent.at` (wall clock) to `SampleTime`/`Tick` using a consistent clock bridge (fix judge accuracy and input feel).
- Event ordering: keep the invariant "NoteOff before NoteOn at the same time" across import/export/scheduling/rendering.
- MIDI import: sanitize overlapping note-ons and close dangling notes (prevents stuck notes on messy files).
- OMR robustness: keep Audiveris logs + MusicXML output in the diagnostics bundle; make failure modes obvious in UI.

## P1: PDF -> MIDI quality (highest user impact)

Audiveris output quality varies; Cadenza should post-process and sanitize:

- MusicXML import: support ties, voices, and basic articulations; stop merging unrelated voices into a single long note.
- Measure awareness: derive measure boundaries when present and add an option to clamp note-offs to the measure (when OMR is messy).
- Cleanup passes: de-overlap same-pitch notes, add tiny re-articulation gaps for repeated notes, remove impossible durations.
- Fixtures: add a `fixtures/` directory with small MusicXML examples and regression tests for the importer.

## P2: Practice UX improvements

- Loop selection UI (click/drag on the roll) + clear visual loop range.
- Input offset UI (slider + calibration helper) and per-device presets.
- Better transport: tempo multiplier UI, count-in/metronome, and clear "playback vs monitor" routing presets.

## P3: Sound quality (real piano)

The best "real piano" path for MVP is SoundFont:

- Ship a better SoundFont UX (validation, presets list, and clear guidance about licensing).
- Optional: add a small built-in SF2 for quick start if licensing allows.
- Physical modeling can stay as a fallback/testing synth; iterate only after stability + OMR correctness.

## P4: Packaging and distribution

- macOS: signing/notarization, DMG packaging, and a "first-run" setup flow.
- Add a minimal CI (format + check + tests) so regressions are caught early.
