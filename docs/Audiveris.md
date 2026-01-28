# Audiveris (PDF -> MIDI)

Cadenza uses an external OMR engine (Audiveris) to convert a PDF score into MusicXML, then exports a `.mid`.

Audiveris is not bundled, and Homebrew does not provide it.

## Install (macOS)

1. Download the macOS build from the official Audiveris releases:
   - https://github.com/Audiveris/audiveris/releases
2. Move `Audiveris.app` to `/Applications`.
3. In Cadenza: `Settings` -> `Audiveris` -> `Browse`, then select either:
   - `/Applications/Audiveris.app` (recommended), or
   - `/Applications/Audiveris.app/Contents/MacOS/Audiveris` (the executable).
4. Click `Save`.

## Troubleshooting

- Error `audiveris: command not found`: Audiveris is not on PATH; set the path in Settings.
- macOS blocks the app: open `System Settings -> Privacy & Security` and allow Audiveris, then try again.
- Conversion fails: the UI shows an `audiveris.log` path; open it to see the exact error.

## Debugging output quality

On success, Cadenza also shows the generated MusicXML path (`.mxl/.xml`).
If the resulting MIDI is messy (e.g. notes glued across measures), the MusicXML file is the best artifact to inspect and share.
