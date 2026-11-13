#!/usr/bin/env bash
#
# Build the .icns app icons from their SVG masters.
#
#   tools/icons/build_app_icons.sh           # rebuild the macOS .icns
#   tools/icons/build_app_icons.sh all       # rebuild both icon sets
#   tools/icons/build_app_icons.sh generic   # rebuild only the generic .icns
#
# macOS only: it uses the system `sips` to rasterise the SVGs at each size of
# the standard .iconset and `iconutil` to pack them into an .icns.
#
# Three masters exist because the platforms want different amounts of
# transparent padding and detail around the artwork:
#
#   assets/app-icons/generic/termlet.svg
#                              generic master, tile fills 87.5% of the canvas
#                              (Linux .desktop / Windows runtime window icon)
#   assets/app-icons/windows/termlet.svg
#                              Windows mark for 16-40 px entries
#   assets/app-icons/macos/termlet.svg
#                              Apple macOS (Big Sur+) grid: 824/1024 = 80.469%
#                              of the canvas, so the Dock/Finder icon matches
#                              the size of Apple's own app icons
#
# `assets/app-icons/macos/termlet.icns` is the one `cargo bundle` ships on
# macOS (see the `icon` list under [package.metadata.bundle] in Cargo.toml).
set -euo pipefail

for tool in sips iconutil; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "error: '$tool' not found — this script needs macOS" >&2
        exit 1
    }
done

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
icons="$root/assets/app-icons"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Rasterise $1 (SVG) into a standard .iconset and pack it into $2 (.icns).
build_icns() {
    local svg="$1" icns="$2"
    local name iconset logical scale px suffix

    [ -f "$svg" ] || { echo "error: missing SVG master $svg" >&2; exit 1; }

    name=$(basename "$icns" .icns)
    iconset="$work/$name.iconset"
    mkdir -p "$iconset"

    # Standard macOS iconset: <logical size>:<scale>, both 1x and 2x.
    for entry in 16:1 16:2 32:1 32:2 128:1 128:2 256:1 256:2 512:1 512:2; do
        logical=${entry%%:*}
        scale=${entry##*:}
        px=$((logical * scale))
        suffix=""
        [ "$scale" -eq 2 ] && suffix="@2x"
        # `-Z` rasterises the vector at the requested size (rather than scaling
        # a low-res bitmap), which keeps the 16/32 px icons crisp.
        sips -s format png -Z "$px" "$svg" --out "$iconset/icon_${logical}x${logical}${suffix}.png" >/dev/null
    done

    iconutil -c icns "$iconset" -o "$icns"
    printf 'built %-32s <- %s\n' "${icns#"$root"/}" "${svg#"$root"/}"
}

# Rasterise an SVG at an exact nominal PNG size.
render_png() {
    local svg="$1" png="$2" size="$3"

    [ -f "$svg" ] || { echo "error: missing SVG master $svg" >&2; exit 1; }
    sips -s format png -Z "$size" "$svg" --out "$png" >/dev/null
    printf 'built %-32s <- %s\n' "${png#"$root"/}" "${svg#"$root"/}"
}

case "${1:-macos}" in
    all)
        build_icns "$icons/macos/termlet.svg" "$icons/macos/termlet.icns"
        build_icns "$icons/generic/termlet.svg" "$icons/generic/termlet.icns"
        render_png "$icons/generic/termlet.svg" "$icons/linux/256x256/termlet.png" 256
        render_png "$icons/windows/termlet.svg" "$icons/windows/termlet.png" 256
        ;;
    macos)
        build_icns "$icons/macos/termlet.svg" "$icons/macos/termlet.icns"
        ;;
    generic)
        build_icns "$icons/generic/termlet.svg" "$icons/generic/termlet.icns"
        ;;
    *)
        echo "usage: $(basename "$0") [macos|all|generic]" >&2
        exit 2
        ;;
esac
