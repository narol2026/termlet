//! Compile-time version information for the in-app About screen.
//!
//! The version is the crate version from `Cargo.toml`; the date is baked in by
//! `build.rs` at compile time as a UTC `YYYY-MM-DD` string (overridable via
//! `SOURCE_DATE_EPOCH` for reproducible builds). An optional prerelease suffix
//! is read from the `TERMLET_VERSION_SUFFIX` environment variable, which CI
//! sets to `"-dev"` for builds from any branch other than `main`.

/// Crate version from `Cargo.toml`, e.g. `"1.0.0"`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Optional prerelease suffix, e.g. `"-dev"`. Empty for official builds.
///
/// Read at compile time, so a changed value recompiles this crate; the same
/// build without the variable set produces the plain release version.
pub const VERSION_SUFFIX: &str = match option_env!("TERMLET_VERSION_SUFFIX") {
    Some(suffix) => suffix,
    None => "",
};

/// UTC date (`YYYY-MM-DD`) the binary was compiled.
pub const BUILD_DATE: &str = env!("TERMLET_BUILD_DATE");

/// Version string for the UI: the `Cargo.toml` version, the optional prerelease
/// suffix, and the build date, e.g. `"1.0.0 (2026-10-04)"` for a release build
/// or `"1.0.0-dev (2026-10-04)"` for a dev build.
pub fn display() -> String {
    format!("{VERSION}{VERSION_SUFFIX} ({BUILD_DATE})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_combines_version_suffix_and_build_date() {
        let shown = display();
        assert!(
            shown.starts_with(&format!("{VERSION}{VERSION_SUFFIX}")),
            "got {shown}"
        );
        assert!(shown.contains(BUILD_DATE), "got {shown}");
        assert_eq!(shown, format!("{VERSION}{VERSION_SUFFIX} ({BUILD_DATE})"));
    }
}
