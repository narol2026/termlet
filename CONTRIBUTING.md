# Contributing to Termlet

Thanks for taking the time to contribute. This document covers how to set up the
project, the checks we expect a change to pass, and the conventions the codebase
follows.

## Ways to contribute

- Report bugs and request features via the issue templates.
- Improve the documentation in `README.md` or the module docs in `src/`.
- Add or fix translations in `locales/app.yml`.
- Send a pull request for a bug fix or feature.

For anything larger than a small fix, please open an issue first so we can agree
on the approach before you invest time in a patch.

## Development setup

### Prerequisites

- **Rust**: install [rustup](https://rustup.rs/). The compiler is pinned in
  `rust-toolchain.toml`, and rustup installs it — with `rustfmt` and `clippy` —
  automatically on the first `cargo` command, so there is nothing to choose by
  hand. The crate separately targets `edition = "2024"` with an MSRV of
  **1.89** (see `rust-version` in `Cargo.toml`; the floor comes from
  dependencies such as `cosmic-text`). You do not need that older compiler to
  contribute — the pinned toolchain is what builds and tests every change. A
  separate CI job only checks that the declared floor still compiles, so raising
  `rust-version` is a deliberate change, not a side effect of a dependency bump.
- **Linux only**: the `keyring` crate links `libdbus` through its
  `sync-secret-service` backend, so you need the development headers:

  ```bash
  sudo apt install libdbus-1-dev pkg-config   # Debian/Ubuntu
  sudo dnf install dbus-devel pkgconf-pkg-config   # Fedora
  ```

- **Node.js** (optional): only needed to regenerate the icon font
  (`npm install && npm run build:icons`). Uses `svgtofont`.
- **Python 3.8+** (optional): only needed to regenerate
  `THIRD_PARTY_LICENSES.md` after a dependency change.

### Build and run

```bash
cargo build            # debug build
cargo run              # launch the GUI
cargo build --release  # release build
```

See the [README](README.md#building) and [`packaging/`](packaging/README.md)
for packaging and platform-specific notes.

## Checks to run before opening a PR

```bash
cargo fmt              # format (keep the tree rustfmt-clean)
cargo clippy --all-targets --all-features
cargo test
```

CI runs these on every pull request, so running them locally first saves a round
trip. If a change is platform-specific, say which platforms you tested in the PR
description.

## Project conventions

### Commit messages

The history follows [Conventional Commits](https://www.conventionalcommits.org/).
Use a type, an optional scope, and an imperative subject:

```text
feat(sftp): add drag-and-drop upload from the file table
fix(topbar): only show the workspace save button for the active session
docs: clarify the macOS signing flow
```

Commonly used types are `feat`, `fix`, `docs`, `refactor`, `style`, `perf`,
`test`, `build`, `chore`. Scopes match the module the change touches (for
example `terminal`, `sftp`, `ssh`, `hosts`, `fonts`, `i18n`, `license`).

### Code layout

The module map lives in the [README](README.md#project-layout). Keep
screen-level code in `src/views/`, reusable widgets in `src/ui/`, and business
logic in the matching domain module. Where a design decision is non-obvious,
prefer a short `//!` module header or a doc comment on the item over a long
inline comment, and explain the reasoning in the pull request.

### Localization

Every user-facing string is keyed in `locales/app.yml`. When you add or change a
string you must add it for **all three** languages (`en`, `zh-CN`, `zh-TW`) — the
`_version: 2` file keeps them side by side, one block per key.

### Patched forks

Termlet builds dependencies from five patched forks, each pinned to an exact
commit:
the `iced` family from `github.com/narol2026/iced`, `iced_drop` from
`github.com/narol2026/iced_drop`, and `alacritty_terminal`, `vte`, and
`iced_term` from `github.com/narol2026/alacritty`, `/vte`, and `/iced_term`.
Every fork is pinned to an exact revision in the `[patch.crates-io]` table of
[`Cargo.toml`](Cargo.toml) (plus the `iced_drop` dependency); what each fork
carries — the upstream revision its patch branch is based on, the patch set, and
what Termlet uses it for — is in [`PATCHES.md`](PATCHES.md). If your change
depends on a tweak to any of these, explain what the patch does and why it
cannot live upstream, and note it in the PR.

### Third-party licenses

`THIRD_PARTY_LICENSES.md` is generated. After changing dependencies, regenerate
it and include the result in your PR:

```bash
cargo fetch
python3 tools/licenses/gen_third_party_licenses.py
```

If you add a bundled font, icon set, or other binary asset, add its license text
under `fonts/licenses/` (or the matching location) and reference it from
`NOTICE`.

That set is what both distributables ship: the `.deb` installs `LICENSE`,
`NOTICE`, `THIRD_PARTY_LICENSES.md`, and the `fonts/licenses/` texts under
`/usr/share/doc/termlet/` (with `packaging/debian/copyright` installed as the
Debian `copyright` file), and the macOS `.app` carries the same files as
resources. `packaging/debian/copyright` is generated — Debian wants the license
texts in that file itself, so the stanzas and the texts are assembled from
`LICENSE` and `fonts/licenses/` by
`python3 tools/licenses/gen_deb_copyright.py`. Re-run it in the same PR when a bundled
asset, its license, or its attribution changes, and keep the `assets` and
`resources` lists in `Cargo.toml` in step.

## Pull request process

1. Fork the repository and branch from the default branch.
2. Make your change, keeping commits focused and the messages conventional.
3. Run the checks above and fill in the PR template.
4. A maintainer will review; expect questions on anything touching credentials,
   the encryption layer, or the patched forks.

Please keep pull requests scoped to one concern. Large mixed changes are hard to
review and slow to land.

## Reporting security issues

Do **not** open a public issue for a vulnerability. Follow
[SECURITY.md](SECURITY.md) instead.

## License

Termlet is licensed under [Apache-2.0](LICENSE). By submitting a pull request you
agree that your contribution is licensed under the same terms. There is no CLA.
