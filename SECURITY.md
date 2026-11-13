# Security Policy

Termlet handles sensitive material: SSH private keys and passphrases, host
credentials, and the encrypted at-rest store that holds them. We take security
reports seriously and appreciate responsible disclosure.

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Preferred: use GitHub's private vulnerability reporting on this repository —
**Security → Advisories → Report a vulnerability**
(<https://github.com/narol2026/termlet/security/advisories/new>). This keeps the
report private and gives us a place to coordinate a fix and an advisory.

If you cannot use GitHub, email **narolcat@gmail.com** with `[termlet security]`
in the subject.

Please include, as far as you can:

- A description of the issue and its impact.
- Steps to reproduce, or a proof-of-concept.
- The affected version — the app shows `version (build date)` on the About
  screen, and the Git commit if you built from source.
- The platform and how you installed it (source, `.deb`, `.dmg`, …).
- Any relevant logs. Crash reports live under `~/.termlet/crashes/`; inspect them
  before sharing, since they may contain paths or other local detail.

## What to expect

- We aim to acknowledge a report within a few days.
- We will confirm the issue, work on a fix, and agree on a disclosure timeline
  with you (typically once a patched release is available).
- We are happy to credit you in the advisory unless you prefer otherwise.

## Supported versions

Security fixes target the latest release and the default branch. Older builds
are not maintained.

## Scope and design notes

The most security-relevant areas of the codebase are:

- **Credential and key storage** — `src/core/crypto.rs`, `src/core/data.rs`,
  `src/keychain/`: age-encrypted secrets with the master key held in the OS
  keychain.
- **SSH transport and host verification** — `src/ssh/`, `src/ssh_connect/`,
  `src/known_hosts/`.
- **Remote file handling** — `src/sftp/`: path handling and transfer logic.
- **Shell integration** — the OSC 7/633 scripts injected into remote sessions
  (`src/ssh/shell_integration.rs`).
- **Export/import** — `src/hosts/export.rs`, `src/workspace/export.rs`.

For context: Termlet ships no telemetry and makes no outbound connections on its
own. Network traffic only happens for hosts you configure, and crash reports are
written to local disk only.

Vulnerabilities in a third-party dependency should be reported upstream, but
let us know if Termlet needs to bump or work around it.
