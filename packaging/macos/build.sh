#!/usr/bin/env bash
#
# Build a macOS .app or .dmg through cargo-bundle.
#
#   packaging/macos/build.sh
#   packaging/macos/build.sh --format dmg
#   packaging/macos/build.sh --format dmg --universal
#   packaging/macos/build.sh --format dmg --target x86_64-apple-darwin
#   packaging/macos/build.sh --format dmg --output target/Termlet-dev.dmg
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
format=app
output=
targets=()

usage() {
    cat <<'EOF'
usage: packaging/macos/build.sh [options]

Options:
  --format app|dmg         bundle format (default: app)
  --target TRIPLE          build one target; may be repeated
  --universal              build Apple Silicon and Intel together
  --output PATH            move the finished DMG to PATH
  -h, --help               show this help
EOF
}

die() {
    echo "error: $*" >&2
    exit 2
}

while (($#)); do
    case "$1" in
        --format)
            (($# >= 2)) || die "--format requires app or dmg"
            format=$2
            shift 2
            ;;
        --target)
            (($# >= 2)) || die "--target requires a Rust target triple"
            targets+=("$2")
            shift 2
            ;;
        --universal)
            targets+=(aarch64-apple-darwin x86_64-apple-darwin)
            shift
            ;;
        --output)
            (($# >= 2)) || die "--output requires a path"
            output=$2
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            die "unknown argument: $1 (run with --help)"
            ;;
    esac
done

[[ "$format" == app || "$format" == dmg ]] || die "--format must be app or dmg"
[[ -z "$output" || "$format" == dmg ]] || die "--output is only supported for DMGs"
command -v cargo-bundle >/dev/null 2>&1 ||
    die "cargo-bundle not found; install it with: cargo install cargo-bundle"

bundle_args=(--release --format "$format")
if ((${#targets[@]})); then
    for target in "${targets[@]}"; do
        bundle_args+=(-t "$target")
    done
fi

(
    cd "$root"
    cargo bundle "${bundle_args[@]}"
)

case "$format" in
    app)
        artifact="$root/target/release/bundle/osx/Termlet.app"
        ;;
    dmg)
        artifact="$root/target/release/bundle/dmg/Termlet.dmg"
        if [[ -n "$output" ]]; then
            mkdir -p -- "$(dirname -- "$output")"
            mv -f -- "$artifact" "$output"
            artifact=$output
        fi
        ;;
esac

printf 'macOS package: %s\n' "$artifact"
