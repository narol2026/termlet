# Debian / Ubuntu packaging

The package metadata and installed-file list are in
`[package.metadata.deb]` in `Cargo.toml`. `packaging/debian/copyright` is
generated because Debian expects a machine-readable copyright file that
reproduces the bundled license texts.

Install the build dependencies and packaging tool, then run the entry point
from the repository root:

```bash
sudo apt install libdbus-1-dev pkg-config dpkg-dev
cargo install cargo-deb --locked
packaging/debian/build.sh
```

The script builds the release binary with the pinned toolchain and writes the
`.deb` to `target/debian/`. It builds for the host architecture; run it on an
ARM64 host to make the ARM64 package.

Useful options:

```bash
# Reuse an existing target/release/termlet.
packaging/debian/build.sh --skip-build

# Use Debian's prerelease ordering for a development package.
packaging/debian/build.sh --deb-version 1.0.0~dev

# Choose the output file or directory.
packaging/debian/build.sh --output dist/termlet.deb
packaging/debian/build.sh --output dist/
```

Before packaging, the script verifies that `packaging/debian/copyright` is
current. Pass `--skip-copyright-check` only when reproducing a package in an
environment where Python is unavailable; CI never skips this check.

After changing a bundled asset, its license, or the generated stanzas, run:

```bash
python3 tools/licenses/gen_deb_copyright.py
```

CI checks that the checked-in copyright file is current with `--check`.
