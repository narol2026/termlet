# Windows packaging

Windows packaging is maintained for source builds, but Windows is not currently
covered by CI and no official Windows release package is published.

Build the release binary, portable ZIP, and per-user installer from PowerShell
or `cmd.exe`:

```bat
packaging\windows\build.cmd
```

The installer is built with Inno Setup 6. Install it first with:

```powershell
winget install -e --id JRSoftware.InnoSetup
```

Prerequisites:

- Rust 1.95 with the MSVC toolchain (pinned in `rust-toolchain.toml`).
- Visual Studio 2022 Build Tools with the **Desktop development with C++**
  workload (MSVC and the Windows SDK).

No external OpenSSH client is required. SSH sessions are handled by the bundled
`termlet-bridge.exe`, which the build script stages next to `termlet.exe`.

The build script statically links the Microsoft C runtime. The resulting
`termlet.exe` does not require the Microsoft Visual C++ Redistributable on the
target machine. The same executable also contains the application icon as a
native Windows resource, so Explorer and shell integrations do not depend on a
separate installed `.ico` file.

The checked-in `termlet.ico` contains every common shell size plus the exact
30, 36, 60, 72, 80, and 96 px entries requested by Windows 11 display scaling.
Sizes up to 40 px use the simplified terminal mark from
`assets/app-icons/windows/termlet.png`; larger entries use the full icon.
Regenerate it from the repository root with:

```powershell
.\tools\icons\build_windows_icon.ps1
```

Outputs land in `dist/`:

- `Termlet-<version>-windows-x64.zip` — portable build.
- `Termlet-<version>-windows-x64-setup.exe` — per-user installer.
- `Termlet-<version>-windows-x64-setup.exe.sha256` — installer checksum.

`build.ps1 -SkipBuild` reuses an existing `target\release\termlet.exe`; the
wrapper passes the switch through:

```bat
packaging\windows\build.cmd -SkipBuild
```

The installer installs under `%LOCALAPPDATA%\Programs\Termlet` without an
administrator prompt. Uninstalling does not delete `%USERPROFILE%\.termlet\`,
so hosts, keys, and settings are retained.
