[CmdletBinding()]
param(
    [string]$Source = "",
    [string]$WindowsSource = "",
    [string]$Destination = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
if (-not $Source) {
    $Source = Join-Path $repo "assets\app-icons\generic\termlet.png"
}
if (-not $WindowsSource) {
    $WindowsSource = Join-Path $repo "assets\app-icons\windows\termlet.png"
}
if (-not $Destination) {
    $Destination = Join-Path $repo "assets\app-icons\windows\termlet.ico"
}
if (-not (Test-Path -LiteralPath $Source)) {
    throw "Icon source not found: $Source"
}
if (-not (Test-Path -LiteralPath $WindowsSource)) {
    throw "Windows icon source not found: $WindowsSource"
}

Add-Type -AssemblyName System.Drawing
$sourceImage = [System.Drawing.Image]::FromFile($Source)
$windowsImage = [System.Drawing.Image]::FromFile($WindowsSource)
try {
    $entries = [System.Collections.Generic.List[object]]::new()
    # These cover every context-menu, taskbar, Start-list, and Start-pin size
    # Windows 11 uses from 100% through 400% display scaling.
    foreach ($size in @(16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256)) {
        $bitmap = [System.Drawing.Bitmap]::new(
            $size,
            $size,
            [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
        )
        try {
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.Clear([System.Drawing.Color]::Transparent)
                $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality

                # Below 48 px the full grid and border become noise. The Windows
                # master keeps the same tile but a large, simple terminal glyph.
                $entrySource = if ($size -le 40) { $windowsImage } else { $sourceImage }
                $graphics.DrawImage($entrySource, 0, 0, $size, $size)
            } finally {
                $graphics.Dispose()
            }

            $stream = [System.IO.MemoryStream]::new()
            try {
                $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
                $bytes = $stream.ToArray()
            } finally {
                $bitmap.Dispose()
                $stream.Dispose()
            }
            $entries.Add([pscustomobject]@{ Size = $size; Bytes = $bytes })
        } catch {
            $bitmap.Dispose()
            throw
        }
    }

    $file = [System.IO.File]::Create($Destination)
    $writer = [System.IO.BinaryWriter]::new($file)
    try {
        $writer.Write([uint16]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]$entries.Count)

        $offset = 6 + (16 * $entries.Count)
        foreach ($entry in $entries) {
            $dimension = [byte]$(if ($entry.Size -ge 256) { 0 } else { $entry.Size })
            $writer.Write($dimension)
            $writer.Write($dimension)
            $writer.Write([byte]0)
            $writer.Write([byte]0)
            $writer.Write([uint16]1)
            $writer.Write([uint16]32)
            $writer.Write([uint32]$entry.Bytes.Length)
            $writer.Write([uint32]$offset)
            $offset += $entry.Bytes.Length
        }
        foreach ($entry in $entries) {
            $writer.Write([byte[]]$entry.Bytes)
        }
    } finally {
        $writer.Dispose()
        $file.Dispose()
    }
} finally {
    $sourceImage.Dispose()
    $windowsImage.Dispose()
}

Write-Host "Wrote $Destination"
