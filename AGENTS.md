# Repository Guidelines

Termlet is a cross-platform desktop terminal built with iced, focused on SSH
session management (Rust 2024; toolchain pinned in `rust-toolchain.toml`).

## Project Structure & Module Organization

`src/main.rs` is the entry point. Features live in domain modules under `src/`
(`ssh/`, `terminal/`, `sftp/`, `hosts/`, `keychain/`, `snippets/`,
`port_forward/`, `known_hosts/`, `workspace/`); most split into `mod.rs`,
`message.rs`, `update.rs` and `view.rs`. Supporting layers are `src/app/` (state,
keybindings), `src/views/` (screen-level), `src/ui/` (reusable widgets) and
`src/core/` (data model, crypto, theme, i18n).

Assets live in `assets/`, fonts in `fonts/`, translations in `locales/app.yml`.
Tests sit next to the code in `#[cfg(test)] mod tests` blocks; the app-level ones
are `src/e2e.rs` and `src/core/test_support.rs`. There is no `tests/` directory.

## Build, Test, and Development Commands

- `cargo build` / `cargo run` — debug build, launch the GUI.
- `cargo build --release` — release build; `cargo deb` (Linux) and
  `cargo bundle --release` (macOS) package it.
- `cargo fmt --all`, `cargo clippy --all-targets --all-features` and
  `cargo test` — the checks CI runs (`-D warnings` on clippy); keep them clean.
- `python3 tools/licenses/gen_third_party_licenses.py` — after dependency changes.

Linux builds need `libdbus-1-dev` and `pkg-config`.

## Coding Style & Naming Conventions

rustfmt defaults (4-space indent). `snake_case` files and functions,
`CamelCase` types, modules named for the domain (`sftp/update/file_ops.rs`). The
lint set in `Cargo.toml` is load-bearing: clippy `pedantic`, selected
`restriction` lints, and a `// SAFETY:` comment on every `unsafe` block. Prefer
fixing a lint over silencing it; a deliberate shape gets an `allow` with a
comment saying why.

Every user-facing string is keyed in `locales/app.yml` for `en`, `zh-CN` and
`zh-TW`.

## Testing Guidelines

Inline tests use a `mod tests` block with behaviour-named functions
(`exact_match_scores_highest`). End-to-end tests drive the real app headlessly
through `iced_test` (`Simulator`, `Emulator`) and must never touch `~/.termlet/`;
use the `core::test_support` fixtures. No coverage threshold — new behaviour
should arrive with a test.

## Commit & Pull Request Guidelines

Commits follow Conventional Commits: `feat(sftp): add drag-and-drop upload from
the file table`. Types: `feat`, `fix`, `docs`, `refactor`, `style`, `perf`,
`test`, `build`, `chore`; the scope names the module touched.

Pull requests use the repository template — summary, linked issues, how it was
tested (name the platforms), and a checklist: fmt, clippy and test clean, new
strings in all three locales, license files regenerated. Keep each PR to one
concern.

## Security & Configuration Tips

Report vulnerabilities privately as described in `SECURITY.md`, never as a public
issue. App data lives under `~/.termlet/` and must not be committed.

Full setup, packaging and tooling detail is in `CONTRIBUTING.md`; the five
patched crate forks are inventoried in `PATCHES.md`.
