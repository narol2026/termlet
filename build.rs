// Rust guideline compliant 2026-10-09

fn main() {
    println!("cargo:rerun-if-changed=assets/app-icons/windows/termlet.ico");
    println!("cargo:rerun-if-changed=assets/app-icons/windows/termlet.rc");

    // Embed the icon in the main Windows executable so Explorer, shortcuts,
    // and the taskbar do not have to rely on a separately installed .ico file.
    embed_resource::compile_for(
        "assets/app-icons/windows/termlet.rc",
        ["termlet"],
        embed_resource::NONE,
    )
    .manifest_required()
    .expect("failed to embed the Windows application icon");

    // Bake the build date (UTC, `YYYY-MM-DD`) into the binary so the in-app
    // About screen can show `"<Cargo.toml version> (<build date>)"`. Honor
    // `SOURCE_DATE_EPOCH` (seconds since the Unix epoch) when set so distro and
    // reproducible builds get a stable date; otherwise use the wall clock.
    println!("cargo:rustc-env=TERMLET_BUILD_DATE={}", build_date_utc());
}

/// Today's date in UTC as `YYYY-MM-DD`, or the date derived from
/// `SOURCE_DATE_EPOCH` when that is set. Falls back to the Unix epoch when the
/// system clock is before 1970 (which should never happen in practice).
fn build_date_utc() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64)
        });

    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

/// Convert a day count since the Unix epoch to a civil `(year, month, day)`
/// date (Howard Hinnant's `civil_from_days` algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (y + i64::from(month <= 2), month, day)
}
