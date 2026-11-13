# macOS packaging

macOS bundles are built with
[cargo-bundle](https://github.com/burtonageo/cargo-bundle). The application
metadata, resources, and icon list live in `[package.metadata.bundle]` in
`Cargo.toml`; `build.sh` provides the common local and CI entry point.

Install the tool and build a `.dmg` for the current Mac:

```bash
cargo install cargo-bundle
packaging/macos/build.sh --format dmg
```

Build only the `.app`:

```bash
packaging/macos/build.sh
```

Build a universal `.dmg` for Apple Silicon and Intel:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
packaging/macos/build.sh --format dmg --universal
```

Use `--target <triple>` instead of `--universal` to select one architecture,
or `--output <path>` to move the finished DMG to an explicit path. Outputs are
normally under `target/release/bundle/`.

These unsigned artifacts trigger a Gatekeeper warning outside the build
machine. Distribution requires separate `codesign` and `notarytool` steps.
This repository has no Linux-to-macOS cross-compilation setup, so builds must
run on macOS.
