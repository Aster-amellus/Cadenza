# Repository Guidelines

## Project Structure & Module Organization
This is a Rust workspace with a Tauri shell and a static UI.
Primary locations:
- `docs/`: design specs and drafts (e.g., `Transport.md`, `UI_spec.md`, `ScoreDomain_v0.1.md`).
- `crates/`: Rust modules separated by domain, ports, core, and infra.
  - Example: `crates/cadenza-core/` for orchestration, `crates/cadenza-infra-*` for platform backends.
- `src-tauri/`: Tauri shell that hosts the UI and bridges to `AppCore`.
- `ui/`: static HTML/CSS/JS (no bundler).
Keep new specs in `docs/` and new Rust modules under `crates/` using the `cadenza-<layer>-<name>` pattern.

## Build, Test, and Development Commands
Run from repo root:
- `cargo check`: fast compile check for the workspace.
- `cargo build -p cadenza-app`: builds the Tauri shell.
- `cargo run -p cadenza-app`: runs the desktop app (uses `ui/` as the frontend).
There are no scripted dev commands beyond Cargo yet; if you add tooling, document it here and in `README.md`.

## Coding Style & Naming Conventions
Rust code targets edition 2021 and should be formatted with `rustfmt` (`cargo fmt`).
Use idiomatic Rust module names (`snake_case`) and keep crate names in the existing `cadenza-<layer>-<name>` format.
Frontend files in `ui/` use two-space indentation and simple, readable DOM code.
Documentation should be Markdown with short paragraphs, explicit headings, and ASCII-only text.

## Testing Guidelines
No automated tests are defined yet. If you add tests, prefer `crates/<crate>/tests/` or unit tests in-module.
Name test files `*_test.rs` and run them with `cargo test`.

## Commit & Pull Request Guidelines
The existing history uses Conventional Commit style (`feat:`). Follow that pattern (`feat:`, `fix:`, `chore:`) with concise, imperative subjects.
Pull requests should include:
- A short summary of changes.
- A list of touched `docs/` or `ui/` files.
- Screenshots for UI changes or a brief GIF if behavior is visual.

## External Tools & Configuration
PDF to MIDI relies on an external OMR engine (Audiveris). The app looks for `audiveris` on PATH or a custom binary path (e.g., `/Applications/Audiveris.app/Contents/MacOS/Audiveris`) set in Settings.
Diagnostics export writes a bundle to the selected folder; keep paths absolute.
