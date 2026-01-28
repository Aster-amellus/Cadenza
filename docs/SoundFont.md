# SoundFont (.sf2)

Cadenza can render MIDI using RustySynth when a SoundFont is loaded. Without a SoundFont, it falls back to a lightweight physical-model synth (useful for testing, not a real piano).

## Recommended files

For "real piano" tone, use a piano-focused SF2. General MIDI sets also work, but the piano patch quality varies.

Common options to search for:
- `MuseScore General.sf2`
- `FluidR3 GM.sf2`
- `GeneralUser GS.sf2`

## Where to store

Put the file somewhere stable, for example:
- `~/Music/SoundFonts/`
- `~/Documents/SoundFonts/`

The app stores the path in settings and will try to reload it on startup.

## Load in the app

1. Open `Settings`.
2. Under `SoundFont (.sf2)`, click `Browse` and choose the `.sf2` file.
3. Click `Load` and confirm the status shows `Loaded`.

## Troubleshooting

- No sound: select an `Audio Output`, ensure `Monitor` is enabled, then click `Test Sound`.
- Distortion/crackles: lower `Master Volume` and the bus volumes.
- Load fails: confirm the file ends with `.sf2` and the file is readable.
