# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0-rc.1] - 2026-11-14

Termlet's first Windows release candidate adds an x86_64 portable ZIP and
per-user installer. Windows remains a release candidate until it passes manual
testing on clean Windows 10 and Windows 11 systems. The Windows binaries are
not code-signed, so SmartScreen can warn on first launch.

The Linux and macOS packages continue to be included with this prerelease.

### Added

- Windows x86_64 now runs native build, test, Clippy, portable ZIP, and
  installer jobs in CI. The package smoke test verifies the installer checksum,
  performs a silent per-user install, checks the App Paths registration, and
  confirms uninstalling preserves existing Termlet user data.
- The release workflow now publishes Windows x86_64 portable ZIP and Inno Setup
  installer assets alongside the Linux and macOS packages.

### Fixed

- Windows now applies a protected current-user DACL to the Termlet data
  directory, credential-bearing files, and SQLite sidecars instead of treating
  permission hardening as a no-op. Directory entries inherit to newly created
  app files, and reparse points are rejected at the same boundaries as Unix
  symlinks.
- Windows bug-report and external links now open through `ShellExecuteExW`
  instead of passing the URL through `cmd.exe`, so query-string characters
  cannot be interpreted as shell syntax.
- The Windows installer now requires Windows 10 1809 (`10.0.17763`) or newer,
  matching the minimum version that provides ConPTY.

## [1.0.0] - 2026-11-14

Termlet's first public release provides official Linux and macOS packages.
Windows source builds remain available, but Windows is not part of this
release.

The release includes Debian/Ubuntu packages for x86_64 and aarch64, macOS disk
images for Intel and Apple Silicon, and a `SHA256SUMS` file. The macOS images
are not signed or notarized yet, so Gatekeeper warns on the first launch.

### Added

- SSH terminals backed by russh, alongside local terminals, split panes, tabs,
  shell integration, command history, in-terminal search, and a command
  palette.
- SFTP browsing and transfers with queueing, conflict handling, previews,
  drag-and-drop, and local/remote panes.
- Host vaults, age-encrypted keychain entries, known-hosts management, reusable
  snippets, and local, remote, and dynamic port forwarding.
- Saved workspaces, persistent logs, crash reports, six built-in themes plus
  automatic system theme selection, and English, Simplified Chinese, and
  Traditional Chinese localization.
- Linux packages for x86_64 and aarch64, and macOS disk images for Intel and
  Apple Silicon.
- An automated tag workflow that builds the release packages, publishes their
  SHA-256 checksums, and creates the GitHub Release.
- The Logs page is persistent. Every entry is appended to
  `~/.termlet/logs/termlet-YYYY-MM-DD.log` — one file per day, the newest seven
  kept, `0600` — and the page opens on the newest entries of the previous run
  instead of starting empty. Unlike `terminal_diagnostics.log`, this file is
  written whatever the diagnostics level is set to, so on a release build the
  events the user can see are on disk too.
- `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, and this changelog.
- An `msrv` CI job that compiles the crate with the `rust-version` declared in
  `Cargo.toml`, so the advertised MSRV is verified instead of assumed.
- `PATCHES.md`, an inventory of what each patched fork carries: the upstream
  revision its patch branch is based on, the patch set, and what Termlet uses it
  for.

### Changed

- Source documentation now matches the current support matrix and release
  state: Windows is documented as a source-build target without CI or an
  official package, unsupported reconnect-`cd` behavior was removed from the
  README and Shell Integration hint, and the fork inventory consistently names
  all five patched forks.
- The Logs page no longer lists raw terminal backend traffic — a line per
  keystroke, per pty chunk and per mouse hover. That trace now lands only in the
  diagnostics file, and only at its `trace` level, so the page keeps the entries
  that carry meaning: the command the shell announced, a non-zero exit code
  together with the command it belongs to, and a change of working directory.
- The README gained a Contributing section that points at `CONTRIBUTING.md`,
  `SECURITY.md`, and `CODE_OF_CONDUCT.md`, so the build/test expectations, the
  private vulnerability-reporting route, and the code of conduct are reachable
  from the front page.
- Moved the Logs entry below Profiles in the sidebar navigation.
- Pinned every patched fork dependency to an exact commit — `narol2026/iced`
  (`5f543772`), `/iced_drop` (`1ad7551`), `/iced_term` (`a0964fc`), `/vte`
  (`c5a8ad1`), and `/alacritty` (`bdf4728`) — instead of the moving
  `*-patched` branches, so builds are reproducible.

### Fixed

- A command that fails is reported with its exit code again. The shell sends
  that status over OSC 633 ; D in the same batch as the prompt markers that
  follow it, and the handler read only the last report of a batch, so most exit
  codes never reached the Logs page. The statuses now come from the raw events,
  the way the executed command already did.
- The Debian package's long description. `[package.metadata.deb]` now sets
  `extended-description`, so the control file carries plain text instead of the
  README's markdown — `apt show termlet` no longer prints badge URLs and raw
  HTML tags.
- Remote deletion no longer refuses paths the app treated as system-owned
  (`/etc`, `/usr`, `/usr/local`, `/usr/local/bin`). The confirmation prompt is
  the only gate, so a file the user put under `/usr/local/bin` can be deleted.

### Security

- Updated the SSH, SQLite, XML, TLS, async runtime, and supporting dependency
  graph before the first release. `cargo audit` reports no vulnerabilities in
  the locked dependency set.
- Upgraded to `russh` 0.65, including its fixes for malformed packet sizes,
  decompression limits, unexpected channel callbacks, and confirm-constrained
  agent keys.
- Pinned the upstream `russh` commit that moves to RSA 0.10 and SSH key
  support with the current constant-time mitigations.
- Refreshed the Node icon-generation toolchain and overrode its vulnerable
  transitive glob dependencies; `npm audit` now reports no vulnerabilities.

<!--
Release entries are grouped under Added / Changed / Deprecated / Removed /
Fixed / Security. Keep the newest version at the top. When cutting a release,
move the relevant items from Unreleased into a new `## [x.y.z] - YYYY-MM-DD`
section and add the comparison link below.
-->

[Unreleased]: https://github.com/narol2026/termlet/compare/v1.1.0-rc.1...HEAD
[1.1.0-rc.1]: https://github.com/narol2026/termlet/releases/tag/v1.1.0-rc.1
[1.0.0]: https://github.com/narol2026/termlet/releases/tag/v1.0.0
