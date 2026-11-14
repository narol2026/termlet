[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$manifest = Get-Content -LiteralPath (Join-Path $repo "Cargo.toml") -Raw
if ($manifest -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw "Could not read the package version from Cargo.toml"
}
$version = $Matches[1]
$versionSuffix = $env:TERMLET_VERSION_SUFFIX
if ($null -eq $versionSuffix) {
    $versionSuffix = ""
}
$artifactVersion = "$version$versionSuffix"
$dist = Join-Path $repo "dist"
$zip = Join-Path $dist "Termlet-$artifactVersion-windows-x64.zip"
$installer = Join-Path $dist "Termlet-$artifactVersion-windows-x64-setup.exe"
$checksumFile = "$installer.sha256"

foreach ($path in @($zip, $installer, $checksumFile)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Missing package artifact: $path"
    }
}

$checksumLine = (Get-Content -LiteralPath $checksumFile -Raw).Trim()
$parts = $checksumLine -split '\s+', 2
if ($parts.Count -ne 2 -or $parts[1] -ne [IO.Path]::GetFileName($installer)) {
    throw "Malformed installer checksum file: $checksumFile"
}
$actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToLowerInvariant()
if ($actualHash -ne $parts[0].ToLowerInvariant()) {
    throw "Installer checksum mismatch: expected $($parts[0]), got $actualHash"
}

$smokeRoot = Join-Path $env:TEMP "termlet-windows-smoke-$([Guid]::NewGuid())"
$portableRoot = Join-Path $smokeRoot "portable"
$installDir = Join-Path $smokeRoot "installed"
$dataDir = Join-Path $env:USERPROFILE ".termlet"
$dataMarker = Join-Path $dataDir "smoke-preserve-$([Guid]::NewGuid()).txt"
$dataDirExisted = Test-Path -LiteralPath $dataDir
$uninstaller = Join-Path $installDir "unins000.exe"
$appPathsKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\termlet.exe"

try {
    New-Item -ItemType Directory -Force -Path $portableRoot, $dataDir | Out-Null
    Set-Content -LiteralPath $dataMarker -Value "preserve" -Encoding ascii

    Expand-Archive -LiteralPath $zip -DestinationPath $portableRoot
    $portableDir = Join-Path $portableRoot "Termlet-$artifactVersion-windows-x64"
    foreach (
        $name in @(
            "termlet.exe",
            "termlet-bridge.exe",
            "termlet.ico",
            "LICENSE",
            "NOTICE",
            "THIRD_PARTY_LICENSES.md"
        )
    ) {
        if (-not (Test-Path -LiteralPath (Join-Path $portableDir $name))) {
            throw "Portable package is missing $name"
        }
    }

    $install = Start-Process -FilePath $installer -Wait -PassThru -ArgumentList @(
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/SP-",
        "/DIR=`"$installDir`""
    )
    if ($install.ExitCode -ne 0) {
        throw "Installer exited with code $($install.ExitCode)"
    }

    foreach ($name in @("termlet.exe", "termlet-bridge.exe", "termlet.ico", "unins000.exe")) {
        if (-not (Test-Path -LiteralPath (Join-Path $installDir $name))) {
            throw "Installed package is missing $name"
        }
    }

    $appPathKey = Get-Item -LiteralPath $appPathsKey
    if ($appPathKey.GetValue("") -ne (Join-Path $installDir "termlet.exe")) {
        throw "Installer did not register the Termlet App Paths entry"
    }

    $uninstall = Start-Process -FilePath $uninstaller -Wait -PassThru -ArgumentList @(
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART"
    )
    if ($uninstall.ExitCode -ne 0) {
        throw "Uninstaller exited with code $($uninstall.ExitCode)"
    }
    if (Test-Path -LiteralPath (Join-Path $installDir "termlet.exe")) {
        throw "Uninstaller left termlet.exe behind"
    }
    if (-not (Test-Path -LiteralPath $dataMarker)) {
        throw "Uninstaller removed existing user data"
    }
} finally {
    if (Test-Path -LiteralPath $uninstaller) {
        Start-Process -FilePath $uninstaller -Wait -ArgumentList @(
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART"
        )
    }
    if (Test-Path -LiteralPath $dataMarker) {
        Remove-Item -LiteralPath $dataMarker -Force
    }
    if (-not $dataDirExisted -and (Test-Path -LiteralPath $dataDir)) {
        Remove-Item -LiteralPath $dataDir -Recurse -Force
    }
    if (Test-Path -LiteralPath $smokeRoot) {
        Remove-Item -LiteralPath $smokeRoot -Recurse -Force
    }
}

Write-Host "Windows ZIP and installer smoke tests passed."
