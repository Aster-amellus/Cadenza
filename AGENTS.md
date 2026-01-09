# Repository Guidelines

## Project Structure & Module Organization
This repository currently contains documentation only. The primary content lives in `docs/`, which holds design and specification artifacts such as `UI_spec.md`, `Transport.md`, and domain drafts like `ScoreDomain_v0.1.md`. There is no application source code or test suite yet. If code is added later, keep `docs/` for specs and create a clear top-level directory for implementation (for example, `src/` or `app/`).

## Build, Test, and Development Commands
No build, run, or test commands are defined in this repository at the moment. When a build system is introduced, document the canonical commands here and in `README.md` (e.g., `make build`, `npm test`, or `cargo test`) with a one-line description of each.

## Coding Style & Naming Conventions
Documentation should be written in Markdown with short paragraphs and explicit headings. Keep filenames descriptive and consistent with existing patterns (e.g., `UI_spec.md`, `Transport.md`, or versioned drafts like `Judge_v0.1.md`). Prefer ASCII text and avoid heavy inline formatting. If code is added later, record indentation rules and formatting tools here (for example, `rustfmt` or `prettier`).

## Testing Guidelines
There are no tests yet. If a test suite is added, standardize on a top-level `tests/` (or language-idiomatic) directory and document naming conventions (for example, `*_test.rs` or `*.spec.ts`) along with how to run the suite.

## Commit & Pull Request Guidelines
This repository has no commit history yet, so there is no established commit message convention. Until one emerges, use concise, imperative messages (for example, “Add UI spec draft”) and keep related changes grouped. For pull requests, include a short summary, mention which `docs/` files were updated, and note any open questions or follow-ups needed by reviewers.

## Documentation Workflow Tips
When adding new specs, link to related documents in the opening section and clarify status (draft vs. proposed). Avoid duplicating content across files; prefer cross-references with consistent terminology.
