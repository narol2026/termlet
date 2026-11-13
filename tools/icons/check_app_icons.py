#!/usr/bin/env python3
"""Check that platform icon assets have the sizes their install paths promise.

The check has no third-party dependencies so it can run before the Rust build
in CI. It validates PNG dimensions, the Windows ICO size ladder, macOS ICNS
payloads, and the Linux hicolor installation path.

Usage:

    python3 tools/icons/check_app_icons.py
"""

from __future__ import annotations

import os
import re
import struct
import sys
import zlib

REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)
APP_ICONS = "assets/app-icons"
DESKTOP_ENTRY = "assets/desktop/termlet.desktop"
GENERIC_ICON_PNG = f"{APP_ICONS}/generic/termlet.png"
LINUX_ICON_PNG = f"{APP_ICONS}/linux/256x256/termlet.png"
MACOS_ICON_ICNS = f"{APP_ICONS}/macos/termlet.icns"
WINDOWS_ICON_ICO = f"{APP_ICONS}/windows/termlet.ico"
WINDOWS_ICON_PNG = f"{APP_ICONS}/windows/termlet.png"

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
WINDOWS_ICON_SIZES = [
    16,
    20,
    24,
    30,
    32,
    36,
    40,
    48,
    60,
    64,
    72,
    80,
    96,
    128,
    256,
]
ICNS_PNG_SIZES = {
    b"icp4": 16,
    b"icp5": 32,
    b"icp6": 64,
    b"ic07": 128,
    b"ic08": 256,
    b"ic09": 512,
    b"ic10": 1024,
    b"ic11": 32,
    b"ic12": 64,
    b"ic13": 256,
    b"ic14": 512,
}


def path(relative: str) -> str:
    return os.path.join(REPO_ROOT, relative)


def png_size(data: bytes, label: str) -> tuple[int, int]:
    if not data.startswith(PNG_SIGNATURE) or len(data) < 24:
        raise ValueError(f"{label}: not a PNG file")
    return struct.unpack(">II", data[16:24])


def read_png_size(relative: str) -> tuple[int, int]:
    with open(path(relative), "rb") as handle:
        return png_size(handle.read(), relative)


def paeth(left: int, above: int, upper_left: int) -> int:
    estimate = left + above - upper_left
    left_distance = abs(estimate - left)
    above_distance = abs(estimate - above)
    upper_left_distance = abs(estimate - upper_left)
    if left_distance <= above_distance and left_distance <= upper_left_distance:
        return left
    if above_distance <= upper_left_distance:
        return above
    return upper_left


def read_png_rgba(relative: str) -> tuple[int, int, bytes]:
    """Decode the non-interlaced, 8-bit RGBA PNGs used by the icon masters."""

    with open(path(relative), "rb") as handle:
        data = handle.read()
    if not data.startswith(PNG_SIGNATURE):
        raise ValueError(f"{relative}: not a PNG file")

    width, height, depth, color_type, compression, filter_method, interlace = (
        struct.unpack(">IIBBBBB", data[16:29])
    )
    if depth != 8 or color_type != 6 or interlace != 0:
        raise ValueError(
            f"{relative}: expected 8-bit non-interlaced RGBA, "
            f"got depth={depth} color={color_type} interlace={interlace}"
        )
    if compression != 0 or filter_method != 0:
        raise ValueError(f"{relative}: unsupported PNG compression or filters")

    compressed = []
    offset = 8
    while offset + 12 <= len(data):
        chunk_size, chunk_type = struct.unpack(">I4s", data[offset : offset + 8])
        payload = data[offset + 8 : offset + 8 + chunk_size]
        if len(payload) != chunk_size:
            raise ValueError(f"{relative}: truncated {chunk_type!r} chunk")
        if chunk_type == b"IDAT":
            compressed.append(payload)
        if chunk_type == b"IEND":
            break
        offset += 12 + chunk_size
    if not compressed:
        raise ValueError(f"{relative}: no IDAT data")

    stride = width * 4
    raw = zlib.decompress(b"".join(compressed))
    if len(raw) != height * (stride + 1):
        raise ValueError(f"{relative}: unexpected decompressed image size")

    rows = bytearray()
    previous = bytearray(stride)
    offset = 0
    for _ in range(height):
        filter_type = raw[offset]
        scanline = bytearray(raw[offset + 1 : offset + 1 + stride])
        offset += stride + 1
        for index in range(stride):
            left = scanline[index - 4] if index >= 4 else 0
            above = previous[index]
            upper_left = previous[index - 4] if index >= 4 else 0
            if filter_type == 0:
                pass
            elif filter_type == 1:
                scanline[index] = (scanline[index] + left) & 0xFF
            elif filter_type == 2:
                scanline[index] = (scanline[index] + above) & 0xFF
            elif filter_type == 3:
                scanline[index] = (scanline[index] + (left + above) // 2) & 0xFF
            elif filter_type == 4:
                scanline[index] = (
                    scanline[index] + paeth(left, above, upper_left)
                ) & 0xFF
            else:
                raise ValueError(f"{relative}: unsupported PNG filter {filter_type}")
        rows.extend(scanline)
        previous = scanline
    return width, height, bytes(rows)


def check_rendered_png(relative: str) -> None:
    width, height, rgba = read_png_rgba(relative)
    colors = {rgba[offset : offset + 4] for offset in range(0, len(rgba), 4)}
    if len(colors) < 300:
        raise ValueError(
            f"{relative}: only {len(colors)} colors; gradients or clipping "
            "were probably not rendered"
        )

    def pixel(x: int, y: int) -> bytes:
        offset = (y * width + x) * 4
        return rgba[offset : offset + 4]

    background = pixel(width // 2, height // 4)
    if background[3] < 240:
        raise ValueError(f"{relative}: icon background is transparent")
    if max(background[:3]) > 200:
        raise ValueError(f"{relative}: icon background is unexpectedly bright")
    if pixel(0, 0)[3] != 0:
        raise ValueError(f"{relative}: outer corner is not transparent")
    if pixel(width // 2, height // 2)[3] < 240:
        raise ValueError(f"{relative}: terminal mark is missing")


def ico_entries(relative: str) -> list[int]:
    with open(path(relative), "rb") as handle:
        data = handle.read()
    if len(data) < 6:
        raise ValueError(f"{relative}: truncated ICO header")

    reserved, image_type, count = struct.unpack("<HHH", data[:6])
    if reserved != 0 or image_type != 1:
        raise ValueError(f"{relative}: not an ICO file")
    if len(data) < 6 + 16 * count:
        raise ValueError(f"{relative}: truncated ICO directory")

    sizes = []
    for index in range(count):
        offset = 6 + 16 * index
        width, height, _colors, _reserved, planes, bits, length, data_offset = (
            struct.unpack("<BBBBHHII", data[offset : offset + 16])
        )
        nominal = 256 if width == 0 else width
        if height != width:
            raise ValueError(f"{relative}: entry {index} is not square")
        if planes != 1 or bits != 32:
            raise ValueError(f"{relative}: entry {nominal} is not 32-bit RGBA")
        payload = data[data_offset : data_offset + length]
        if len(payload) != length:
            raise ValueError(f"{relative}: entry {nominal} has truncated data")
        actual = png_size(payload, f"{relative}[{nominal}]")
        if actual != (nominal, nominal):
            raise ValueError(
                f"{relative}: directory says {nominal}x{nominal}, "
                f"payload is {actual[0]}x{actual[1]}"
            )
        sizes.append(nominal)
    return sizes


def check_icns(relative: str) -> None:
    with open(path(relative), "rb") as handle:
        data = handle.read()
    if len(data) < 8:
        raise ValueError(f"{relative}: truncated ICNS header")

    magic, declared_size = struct.unpack(">4sI", data[:8])
    if magic != b"icns" or declared_size != len(data):
        raise ValueError(f"{relative}: invalid ICNS header")

    found = set()
    offset = 8
    while offset + 8 <= declared_size:
        chunk_type, chunk_size = struct.unpack(">4sI", data[offset : offset + 8])
        if chunk_size < 8 or offset + chunk_size > declared_size:
            raise ValueError(f"{relative}: invalid {chunk_type!r} chunk")
        payload = data[offset + 8 : offset + chunk_size]
        expected = ICNS_PNG_SIZES.get(chunk_type)
        if expected is not None:
            actual = png_size(payload, f"{relative}:{chunk_type!r}")
            if actual != (expected, expected):
                raise ValueError(
                    f"{relative}: {chunk_type!r} should be {expected}x{expected}, "
                    f"payload is {actual[0]}x{actual[1]}"
                )
            found.add(expected)
        offset += chunk_size

    missing = [size for size in (32, 64, 128, 256, 512, 1024) if size not in found]
    if missing:
        raise ValueError(f"{relative}: missing PNG sizes {missing}")


def check_linux_install_paths() -> None:
    with open(path("Cargo.toml"), encoding="utf-8") as handle:
        manifest = handle.read()
    with open(path(DESKTOP_ENTRY), encoding="utf-8") as handle:
        desktop_icon = re.search(r"^Icon=(\S+)$", handle.read(), re.MULTILINE)
    if desktop_icon is None:
        raise ValueError(f"{DESKTOP_ENTRY} has no Icon entry")
    expected_stem = desktop_icon.group(1)

    pattern = re.compile(
        r'\[\s*"([^"]+\.png)"\s*,\s*'
        r'"usr/share/icons/hicolor/(\d+)x(\d+)/apps/"'
    )
    for source, width, height in pattern.findall(manifest):
        source_stem = os.path.splitext(os.path.basename(source))[0]
        if source_stem != expected_stem:
            raise ValueError(
                f"Cargo.toml installs {source}, but termlet.desktop uses "
                f"Icon={expected_stem}"
            )
        actual = read_png_size(source)
        if actual != (int(width), int(height)):
            raise ValueError(
                f"Cargo.toml installs {source} ({actual[0]}x{actual[1]}) "
                f"under hicolor/{width}x{height}"
            )


def main() -> int:
    try:
        full_size = read_png_size(GENERIC_ICON_PNG)
        linux_size = read_png_size(LINUX_ICON_PNG)
        windows_size = read_png_size(WINDOWS_ICON_PNG)
        if full_size != (1024, 1024):
            raise ValueError(f"{GENERIC_ICON_PNG} is {full_size}, expected 1024x1024")
        if linux_size != (256, 256):
            raise ValueError(
                f"{LINUX_ICON_PNG} is {linux_size}, "
                "expected 256x256"
            )
        if windows_size != (256, 256):
            raise ValueError(
                f"{WINDOWS_ICON_PNG} is {windows_size}, expected 256x256"
            )

        check_rendered_png(LINUX_ICON_PNG)
        check_rendered_png(WINDOWS_ICON_PNG)

        ico_sizes = ico_entries(WINDOWS_ICON_ICO)
        if ico_sizes != WINDOWS_ICON_SIZES:
            raise ValueError(
                f"{WINDOWS_ICON_ICO} sizes are {ico_sizes}, "
                f"expected {WINDOWS_ICON_SIZES}"
            )

        check_icns(MACOS_ICON_ICNS)
        check_linux_install_paths()
    except (OSError, ValueError) as error:
        print(f"icon check failed: {error}", file=sys.stderr)
        return 1

    print(
        "application icons are consistent: "
        f"ICO={','.join(map(str, WINDOWS_ICON_SIZES))}; "
        "macOS=32-1024; Linux PNG=256"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
