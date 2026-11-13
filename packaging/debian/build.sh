#!/usr/bin/env bash
#
# Build a Debian package for the host architecture.
#
#   packaging/debian/build.sh
#   packaging/debian/build.sh --deb-version 1.0.0~dev
#   packaging/debian/build.sh --skip-build --output dist/
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
deb_version=
output=
skip_build=0
skip_copyright_check=0

usage() {
    cat <<'EOF'
usage: packaging/debian/build.sh [options]

Options:
  --deb-version VERSION    override the package version (without arch)
  --output PATH            write the .deb to PATH or into directory PATH
  --skip-build             reuse target/release/termlet
  --skip-copyright-check   skip the generated copyright freshness check
  -h, --help               show this help
EOF
}

die() {
    echo "error: $*" >&2
    exit 2
}

while (($#)); do
    case "$1" in
        --deb-version)
            (($# >= 2)) || die "--deb-version requires a value"
            deb_version=$2
            shift 2
            ;;
        --output)
            (($# >= 2)) || die "--output requires a path"
            output=$2
            shift 2
            ;;
        --skip-build)
            skip_build=1
            shift
            ;;
        --skip-copyright-check)
            skip_copyright_check=1
            shift
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

command -v cargo >/dev/null 2>&1 ||
    die "cargo not found; install Rust from https://rustup.rs/"
command -v cargo-deb >/dev/null 2>&1 ||
    die "cargo-deb not found; install it with: cargo install cargo-deb --locked"
command -v dpkg-deb >/dev/null 2>&1 ||
    die "dpkg-deb not found; install the dpkg package"
command -v dpkg-shlibdeps >/dev/null 2>&1 ||
    die "dpkg-shlibdeps not found; install the dpkg-dev package"

if ((!skip_copyright_check)); then
    command -v python3 >/dev/null 2>&1 ||
        die "python3 not found; use --skip-copyright-check to bypass the check"
    (
        cd "$root"
        python3 tools/licenses/gen_deb_copyright.py --check
    )
fi

if ((!skip_build)); then
    (
        cd "$root"
        cargo build --release --locked
    )
fi

[[ -x "$root/target/release/termlet" ]] ||
    die "missing target/release/termlet; run without --skip-build first"

staging=$(mktemp -d "${TMPDIR:-/tmp}/termlet-deb.XXXXXX")
cleanup() {
    rm -rf -- "$staging"
}
trap cleanup EXIT

deb_args=(deb --no-build --output "$staging")
if [[ -n "$deb_version" ]]; then
    deb_args+=(--deb-version "$deb_version")
fi

(
    cd "$root"
    cargo "${deb_args[@]}"
)

shopt -s nullglob
packages=("$staging"/*.deb)
shopt -u nullglob
((${#packages[@]} == 1)) ||
    die "expected cargo-deb to produce one .deb, found ${#packages[@]}"

artifact=${packages[0]}
filename=$(basename -- "$artifact")
if [[ -z "$output" ]]; then
    destination="$root/target/debian/$filename"
elif [[ "$output" == *.deb ]]; then
    destination=$output
else
    destination="${output%/}/$filename"
fi

mkdir -p -- "$(dirname -- "$destination")"
mv -f -- "$artifact" "$destination"

contents=$(dpkg-deb --contents "$destination")
for required in \
    ./usr/bin/termlet \
    ./usr/share/applications/termlet.desktop \
    ./usr/share/icons/hicolor/256x256/apps/termlet.png \
    ./usr/share/icons/hicolor/scalable/apps/termlet.svg \
    ./usr/share/doc/termlet/copyright; do
    if ! grep -Eq -- " $required$" <<<"$contents"; then
        die "package is missing $required"
    fi
done

package=$(dpkg-deb --field "$destination" Package)
version=$(dpkg-deb --field "$destination" Version)
architecture=$(dpkg-deb --field "$destination" Architecture)
read -r sha256 _ < <(sha256sum -- "$destination")

printf 'Debian package: %s\n' "$destination"
printf 'Package: %s %s (%s)\n' "$package" "$version" "$architecture"
printf 'SHA256: %s\n' "$sha256"
