//! Local crash reporter.
//!
//! Installs a custom panic hook that writes crash reports to
//! `~/.termlet/crashes/` so users can inspect them after a restart.
//! No data is ever sent over the network.

use std::fmt::Write;
use std::fs;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::paths;

const MAX_CRASH_FILES: usize = 20;

static CRASH_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Install the panic hook. Call once at the very start of `main`.
pub fn init() {
    let crash_dir = paths::config_dir().join("crashes");
    let _ = fs::create_dir_all(&crash_dir);

    if let Ok(mut guard) = CRASH_DIR.lock() {
        *guard = Some(crash_dir);
    }

    std::panic::set_hook(Box::new(panic_handler));
}

fn crash_dir() -> Option<PathBuf> {
    CRASH_DIR.lock().ok().and_then(|g| g.clone())
}

fn panic_handler(info: &PanicHookInfo<'_>) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let msg = info
        .payload()
        .downcast_ref::<&str>()
        .map(std::string::ToString::to_string)
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<unknown>".to_string());

    let location = info.location().map_or_else(
        || "<unknown location>".to_string(),
        |loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()),
    );

    let backtrace = std::backtrace::Backtrace::capture();

    let report = format_report(timestamp, &msg, &location, &backtrace);

    eprintln!("\n{report}");

    if let Some(dir) = crash_dir() {
        let path = dir.join(format!("crash_{timestamp}.log"));
        let _ = paths::write_private_atomic(&path, report.as_bytes());
        prune_old_reports(&dir);
    }

    std::process::abort();
}

/// Render the text of a crash report.
///
/// Split out of [`panic_handler`], which ends in `process::abort()` and so can
/// never be called from a test. The layout is what the user reads after a
/// crash, and what a bug report quotes, so it is worth pinning down here.
fn format_report(
    timestamp: u64,
    msg: &str,
    location: &str,
    backtrace: &std::backtrace::Backtrace,
) -> String {
    let hostname = hostname().unwrap_or_else(|| "unknown".to_string());
    let os = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);

    let mut report = String::new();
    let _ = writeln!(report, "termlet crash report");
    let _ = writeln!(report, "====================");
    let _ = writeln!(report, "version: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(report, "timestamp: {timestamp}");
    let _ = writeln!(report, "hostname: {hostname}");
    let _ = writeln!(report, "os: {os}");
    let _ = writeln!(report, "thread: {}", thread_name());
    let _ = writeln!(report);
    let _ = writeln!(report, "panic: {msg}");
    let _ = writeln!(report, "location: {location}");
    let _ = writeln!(report);
    let _ = writeln!(report, "backtrace:");
    let _ = writeln!(report, "{backtrace}");

    report
}

fn prune_old_reports(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<_> = entries
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
        .filter_map(|e| {
            let path = e.path();
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((path, modified))
        })
        .collect();

    if files.len() <= MAX_CRASH_FILES {
        return;
    }

    files.sort_by_key(|(_, m)| *m);
    let to_remove = files.len() - MAX_CRASH_FILES;
    for (path, _) in files.iter().take(to_remove) {
        let _ = fs::remove_file(path);
    }
}

/// Check whether a crash has occurred since the last time this was called.
/// Returns the newest crash report content and marks it as seen.
pub fn check_new_crashes() -> Vec<(u64, String)> {
    match crash_dir() {
        Some(dir) => check_new_crashes_in(&dir),
        // The hook was never installed (or failed to be), so nothing writes
        // reports and there is nothing to report.
        None => vec![],
    }
}

/// The body of [`check_new_crashes`], against an explicit directory.
///
/// `crash_dir` is a process-wide slot that only a live panic hook fills, so
/// that lookup is the one part of this a test cannot set up for itself.
fn check_new_crashes_in(dir: &Path) -> Vec<(u64, String)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return vec![];
    };

    let last_seen = read_last_seen(dir);

    let mut results: Vec<(u64, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "log") {
            continue;
        }
        let ts = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix("crash_"))
            .and_then(|s| s.parse::<u64>().ok());
        let Some(ts) = ts else { continue };
        if ts <= last_seen {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&path) {
            results.push((ts, content));
        }
    }

    results.sort_by_key(|(ts, _)| *ts);

    if let Some((ts, _)) = results.last() {
        write_last_seen(dir, *ts);
    }

    results
}

fn last_seen_path(dir: &std::path::Path) -> PathBuf {
    dir.join(".last_seen")
}

fn read_last_seen(dir: &std::path::Path) -> u64 {
    fs::read_to_string(last_seen_path(dir))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn write_last_seen(dir: &std::path::Path, ts: u64) {
    let _ = paths::write_private_atomic(&last_seen_path(dir), ts.to_string().as_bytes());
}

#[cfg(unix)]
fn hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is a valid, writable 256-byte array and `gethostname`
    // writes at most `buf.len()` bytes into it, truncating a longer name
    // rather than overflowing.
    let ret = unsafe { libc::gethostname(buf.as_mut_ptr().cast::<libc::c_char>(), buf.len()) };
    if ret != 0 {
        return None;
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..len].to_vec()).ok()
}

#[cfg(not(unix))]
fn hostname() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
}

fn thread_name() -> String {
    std::thread::current()
        .name()
        .unwrap_or("<unnamed>")
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::core::test_support::scratch_dir;

    /// Write a report under the name `crash_<ts>.log` — the shape the panic
    /// hook and the startup check have to agree on.
    fn report(dir: &Path, ts: u64, content: &str) {
        fs::write(dir.join(format!("crash_{ts}.log")), content).unwrap();
    }

    fn count_reports(dir: &Path) -> usize {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "log"))
            .count()
    }

    #[test]
    fn pruning_keeps_the_newest_reports_and_drops_the_oldest() {
        let dir = scratch_dir("termlet-crash-prune");
        let total = MAX_CRASH_FILES as u64 + 5;
        for i in 0..total {
            let path = dir.join(format!("crash_{i}.log"));
            fs::write(&path, format!("report {i}")).unwrap();
            // Pruning orders by modification time, so give every report a
            // distinct one, oldest first.
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(i + 1))
                .unwrap();
        }
        write_last_seen(&dir, 7);

        prune_old_reports(&dir);

        assert_eq!(count_reports(&dir), MAX_CRASH_FILES);
        for i in 0..5 {
            assert!(
                !dir.join(format!("crash_{i}.log")).exists(),
                "crash_{i}.log is one of the 5 oldest of {total} reports and should be pruned"
            );
        }
        for i in 5..total {
            assert!(dir.join(format!("crash_{i}.log")).exists());
        }
        // The marker is not a report: it is not counted towards the limit, and
        // it is what stops an already-read crash from being shown again.
        assert_eq!(read_last_seen(&dir), 7);
    }

    #[test]
    fn pruning_at_or_below_the_limit_removes_nothing() {
        let dir = scratch_dir("termlet-crash-limit");
        for i in 0..MAX_CRASH_FILES as u64 {
            report(&dir, i, "x");
        }
        prune_old_reports(&dir);
        assert_eq!(count_reports(&dir), MAX_CRASH_FILES);
    }

    #[test]
    fn pruning_a_directory_that_does_not_exist_is_harmless() {
        // This runs on the way out of the panic hook: failing here would
        // swallow the crash report that was just written.
        prune_old_reports(&scratch_dir("termlet-crash-no-dir").join("missing"));
    }

    #[test]
    fn only_reports_newer_than_the_marker_come_back_and_oldest_first() {
        let dir = scratch_dir("termlet-crash-check");
        report(&dir, 50, "already seen");
        report(&dir, 200, "newer");
        report(&dir, 100, "oldest unseen");
        // The marker is how a crash that was already shown once stays shown.
        write_last_seen(&dir, 50);

        let fresh = check_new_crashes_in(&dir);
        assert_eq!(
            fresh.iter().map(|(ts, _)| *ts).collect::<Vec<_>>(),
            vec![100, 200],
            "reports come back in the order they happened"
        );
        assert_eq!(fresh[0].1, "oldest unseen");

        // Reading them consumes them, as far as the newest one.
        assert_eq!(read_last_seen(&dir), 200);
        assert!(check_new_crashes_in(&dir).is_empty());
    }

    #[test]
    fn files_that_are_not_crash_reports_are_left_alone() {
        let dir = scratch_dir("termlet-crash-noise");
        report(&dir, 10, "real");
        fs::write(dir.join("notes.txt"), "not a report").unwrap();
        fs::write(dir.join("crash_abc.log"), "unparsable timestamp").unwrap();
        fs::write(dir.join("crash_11"), "no extension").unwrap();
        // A directory can carry a `.log` name too, and reading it will fail.
        fs::create_dir(dir.join("crash_12.log")).unwrap();

        let fresh = check_new_crashes_in(&dir);
        assert_eq!(fresh, vec![(10, "real".to_string())]);
        assert!(dir.join("notes.txt").exists());
        assert!(dir.join("crash_abc.log").exists());
        assert!(
            dir.join("crash_12.log").exists(),
            "an entry that cannot be read is skipped, not deleted"
        );
        // Only a report that was actually read moves the marker, so the newer
        // unreadable entry above is still pending.
        assert_eq!(read_last_seen(&dir), 10);
    }

    #[test]
    fn a_directory_without_reports_yields_nothing() {
        let dir = scratch_dir("termlet-crash-empty");
        assert!(check_new_crashes_in(&dir).is_empty());
        assert!(check_new_crashes_in(&dir.join("missing")).is_empty());
        assert_eq!(read_last_seen(&dir), 0, "nothing was read to mark as seen");
    }

    #[test]
    fn an_unreadable_last_seen_marker_restarts_from_zero() {
        let dir = scratch_dir("termlet-crash-marker");
        write_last_seen(&dir, 1_700_000_000);
        assert_eq!(read_last_seen(&dir), 1_700_000_000);

        fs::write(last_seen_path(&dir), "not a timestamp").unwrap();
        assert_eq!(read_last_seen(&dir), 0);

        fs::remove_file(last_seen_path(&dir)).unwrap();
        assert_eq!(read_last_seen(&dir), 0);
    }

    #[test]
    fn the_report_text_carries_the_version_the_panic_and_its_location() {
        let report = format_report(
            1_700_000_000,
            "index out of bounds",
            "src/main.rs:12:5",
            &std::backtrace::Backtrace::capture(),
        );

        assert!(report.starts_with("termlet crash report\n"), "{report}");

        let version_line = format!("version: {}", env!("CARGO_PKG_VERSION"));
        for expected in [
            version_line.as_str(),
            "timestamp: 1700000000",
            "panic: index out of bounds",
            "location: src/main.rs:12:5",
            "backtrace:",
        ] {
            assert!(
                report.contains(expected),
                "missing {expected:?} in:\n{report}"
            );
        }

        // The machine lines are filled in rather than left blank, so a report
        // pasted into a bug report says where it came from.
        for prefix in ["hostname: ", "os: ", "thread: "] {
            let line = report
                .lines()
                .find(|line| line.starts_with(prefix))
                .unwrap_or_else(|| panic!("no {prefix:?} line in:\n{report}"));
            assert!(
                line.len() > prefix.len(),
                "an empty {prefix:?} line says nothing: {line:?}"
            );
        }
    }
}
