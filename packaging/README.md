# Packaging

Platform-specific packaging inputs and entry points live together here:

| Platform | Entry point | Output |
| --- | --- | --- |
| Debian / Ubuntu | `packaging/debian/build.sh` | `target/debian/*.deb` |
| macOS | `packaging/macos/build.sh` | `.app` or `.dmg` under `target/release/bundle/` |
| Windows | `packaging/windows/build.cmd` | portable ZIP and Inno Setup installer under `dist/` |

Each directory has a README with prerequisites and platform-specific notes.
Application icon masters and bundles live under `assets/app-icons/`; the
scripts that generate derived icons and licenses are grouped under
`tools/icons/` and `tools/licenses/`.
