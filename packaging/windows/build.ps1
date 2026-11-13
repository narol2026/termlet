[CmdletBinding()]
param(
    [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$manifest = Get-Content -LiteralPath (Join-Path $repo "Cargo.toml") -Raw
if ($manifest -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw "Could not read the package version from Cargo.toml"
}
$version = $Matches[1]
$dist = Join-Path $repo "dist"
$stage = Join-Path $dist "Termlet-$version-windows-x64"
$exe = Join-Path $repo "target\release\termlet.exe"
$bridge = Join-Path $repo "target\release\termlet-bridge.exe"
$icon = Join-Path $repo "assets\app-icons\windows\termlet.ico"

if (-not $SkipBuild) {
    Write-Host "Building self-contained release binary..."
    # The installer is distributed to machines that may not have the Microsoft
    # Visual C++ Redistributable. Statically link the CRT so termlet.exe runs
    # without that separate prerequisite.
    $previousRustflags = $env:RUSTFLAGS
    try {
        $staticCrt = "-C target-feature=+crt-static"
        if ([string]::IsNullOrWhiteSpace($previousRustflags)) {
            $env:RUSTFLAGS = $staticCrt
        } else {
            $env:RUSTFLAGS = "$previousRustflags $staticCrt"
        }
        & cargo build --release --locked
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    } finally {
        if ($null -eq $previousRustflags) {
            Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
        } else {
            $env:RUSTFLAGS = $previousRustflags
        }
    }
}

if (-not (Test-Path -LiteralPath $exe)) {
    throw "Missing $exe; run this script without -SkipBuild first."
}
if (-not (Test-Path -LiteralPath $bridge)) {
    throw "Missing $bridge; run this script without -SkipBuild first."
}
if (-not (Test-Path -LiteralPath $icon)) {
    & (Join-Path $repo "tools\icons\build_windows_icon.ps1")
    if ($LASTEXITCODE -ne 0) {
        throw "Windows icon generation failed"
    }
}

# Keep the staging cleanup scoped to the named dist\Termlet-* directory.
$distFull = [IO.Path]::GetFullPath($dist)
$stageFull = [IO.Path]::GetFullPath($stage)
$distPrefix = $distFull.TrimEnd('\') + '\'
if (-not $stageFull.StartsWith($distPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to clean a staging path outside dist: $stageFull"
}
if (Test-Path -LiteralPath $stageFull) {
    Remove-Item -LiteralPath $stageFull -Recurse -Force
}

New-Item -ItemType Directory -Force -Path $stageFull | Out-Null
Copy-Item -LiteralPath $exe -Destination $stageFull
Copy-Item -LiteralPath $bridge -Destination $stageFull
Copy-Item -LiteralPath $icon -Destination $stageFull
foreach ($name in @("LICENSE", "NOTICE", "THIRD_PARTY_LICENSES.md")) {
    Copy-Item -LiteralPath (Join-Path $repo $name) -Destination $stageFull
}
$licenseDir = Join-Path $stageFull "licenses"
New-Item -ItemType Directory -Force -Path $licenseDir | Out-Null
Copy-Item -Path (Join-Path $repo "fonts\licenses\*.txt") -Destination $licenseDir

$zip = Join-Path $dist "Termlet-$version-windows-x64.zip"
Compress-Archive -Path $stageFull -DestinationPath $zip -Force
Write-Host "Portable ZIP: $zip"

$iscc = $null
$command = Get-Command iscc.exe -ErrorAction SilentlyContinue
if ($command) {
    $iscc = $command.Source
}
if (-not $iscc) {
    $candidates = @()
    if (${env:ProgramFiles(x86)}) {
        $candidates += (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe")
    }
    if ($env:ProgramFiles) {
        $candidates += (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe")
    }
    if ($env:LOCALAPPDATA) {
        $candidates += (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
    }
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            $iscc = $candidate
            break
        }
    }
}
if (-not $iscc) {
    throw "Inno Setup 6 not found. Install it with: winget install -e --id JRSoftware.InnoSetup"
}

Write-Host "Building installer..."
& $iscc "/DMyAppVersion=$version" (Join-Path $PSScriptRoot "termlet.iss")
if ($LASTEXITCODE -ne 0) {
    throw "ISCC failed with exit code $LASTEXITCODE"
}

$installer = Join-Path $dist "Termlet-$version-windows-x64-setup.exe"
if (-not (Test-Path -LiteralPath $installer)) {
    throw "Installer was not produced at $installer"
}
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToLowerInvariant()
"$hash  $([IO.Path]::GetFileName($installer))" |
    Set-Content -LiteralPath "$installer.sha256" -Encoding ascii
Write-Host "Installer: $installer"
Write-Host "SHA256: $hash"
