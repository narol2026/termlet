# Windows packaging

Windows packaging is covered by native x86_64 CI, which builds and smoke-tests
both the portable ZIP and per-user installer. Windows release packages remain
release candidates until the clean-machine validation matrix is complete.

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
- Windows 10 version 1809 (`10.0.17763`) or newer at runtime.

No external OpenSSH client is required. SSH sessions are handled by the bundled
`termlet-bridge.exe`, which the build script stages next to `termlet.exe`.

Termlet stores its data under `%USERPROFILE%\.termlet\`. The app applies a
protected DACL that grants only the current Windows user access, and new files
inherit that boundary. Credential-bearing files and SQLite sidecars are
restricted explicitly as well.

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

- `Termlet-<artifact-version>-windows-x64.zip` — portable build.
- `Termlet-<artifact-version>-windows-x64-setup.exe` — per-user installer.
- `Termlet-<artifact-version>-windows-x64-setup.exe.sha256` — installer
  checksum.

`TERMLET_VERSION_SUFFIX` is appended to the artifact version. CI sets it to
`-dev` outside `main` while passing the unsuffixed semantic version to the
installer metadata.

After building, verify the packages with:

```powershell
packaging\windows\smoke.ps1
```

The smoke test checks the installer checksum, extracts the portable ZIP,
installs silently into a temporary directory, verifies the registered
`termlet.exe` App Paths entry, then uninstalls and confirms that existing
`%USERPROFILE%\.termlet\` data was preserved.

`build.ps1 -SkipBuild` reuses an existing `target\release\termlet.exe`; the
wrapper passes the switch through:

```bat
packaging\windows\build.cmd -SkipBuild
```

The installer installs under `%LOCALAPPDATA%\Programs\Termlet` without an
administrator prompt. Uninstalling does not delete `%USERPROFILE%\.termlet\`,
so hosts, keys, and settings are retained.
