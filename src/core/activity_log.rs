//! The user-facing activity log.
//!
//! One file per day at `~/.termlet/logs/termlet-YYYY-MM-DD.log`, holding what
//! the Logs page shows: connects, transfers, snippet edits, terminal lifecycle.
//! The diagnostics log next to it is a different thing — timings, heartbeats
//! and freeze probes for troubleshooting — and it is level-gated, so on a
//! release build it drops exactly the entries the user sees. This log is always
//! written, and the newest entries are read back into the page at startup so a
//! restart does not empty it.
//!
//! Writes go through a thread, the way [`crate::core::diagnostics`] does it:
//! [`record`] only formats a line and hands it to a bounded queue, so a caller
//! on the user-interface thread never waits on the disk.

use std::borrow::Cow;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::data::{LogCategory, LogEntry};
use crate::core::paths;
use crate::core::time::{format_timestamp, parse_timestamp};

/// Prefix every file this module writes, so the reader can ignore anything else
/// the user left in the directory.
const FILE_PREFIX: &str = "termlet-";

/// A day's file is rolled to `.log.1` past this size, the way the diagnostics
/// log is. A quiet week never reaches it; a runaway logger stays bounded.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Files kept on disk: normally one per day, plus the `.log.1` of any day that
/// grew past [`MAX_FILE_BYTES`]. Pruning runs when a new day's file appears.
const MAX_FILES: usize = 7;

/// How much of a day's file the startup read looks at, from its end.
const TAIL_BYTES: u64 = 256 * 1024;

/// Bounded so a flood fills the queue and drops lines instead of growing
/// without limit behind a writer that cannot keep up.
const QUEUE_CAPACITY: usize = 4096;

static WRITER: OnceLock<Option<SyncSender<Pending>>> = OnceLock::new();
static PERMISSIONS_FIXED: OnceLock<PathBuf> = OnceLock::new();
/// Serializes appends because direct writers and the background writer share
/// one day file, and formatted writes are not guaranteed to be one syscall.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// One line waiting for the writer thread.
///
/// The timestamp travels with the text because it decides the file name too:
/// the day boundary is the entry's own clock, not whatever the writer thread
/// sees a moment later.
struct Pending {
    timestamp: i64,
    line: String,
}

impl Pending {
    fn at(category: LogCategory, message: &str, timestamp: i64) -> Self {
        Self {
            timestamp,
            line: format!(
                "[{}][{}] {}",
                format_timestamp(timestamp),
                category.token(),
                one_line(message)
            ),
        }
    }
}

/// Queue one entry for today's file.
///
/// Returns as soon as the line is queued: the disk work happens on the writer
/// thread, and a line is dropped when that thread is too far behind — losing a
/// log line is better than stalling the interface.
pub fn record(category: LogCategory, message: &str) {
    let Some(writer) = writer() else {
        return;
    };
    let _ = writer.try_send(Pending::at(category, message, unix_now()));
}

/// The newest [`crate::core::data::MAX_LOG_ENTRIES`]-worth of entries, oldest
/// first, for the Logs page to start from.
pub fn load_tail(limit: usize) -> Vec<LogEntry> {
    load_tail_from(&paths::logs_dir(), limit)
}

/// The writer thread's queue, started on first use.
///
/// A thread that cannot be spawned leaves the sender in the channel's `Err`
/// state forever after, so every later `record` is a no-op rather than a panic.
fn writer() -> Option<&'static SyncSender<Pending>> {
    WRITER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::sync_channel::<Pending>(QUEUE_CAPACITY);
            let spawned = std::thread::Builder::new()
                .name("termlet-activity-log".into())
                .spawn(move || {
                    while let Ok(entry) = receiver.recv() {
                        append_line(&paths::logs_dir(), &entry);
                    }
                });
            spawned.ok().map(|_| sender)
        })
        .as_ref()
}

/// Fold a message that spans lines onto one, so the file stays line-per-entry
/// and the reader can parse it back.
fn one_line(message: &str) -> Cow<'_, str> {
    if message.contains(['\n', '\r']) {
        Cow::Owned(message.replace(['\n', '\r'], " "))
    } else {
        Cow::Borrowed(message)
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// The file an entry with this timestamp belongs in.
fn file_for(dir: &Path, timestamp: i64) -> PathBuf {
    let stamp = format_timestamp(timestamp);
    // `YYYY-MM-DD` out of the formatted line; a clock the platform cannot
    // render still gets a file of its own rather than sharing one with
    // entries from every other day.
    let day = match stamp.get(..10) {
        Some(day) if !day.is_empty() => day.to_string(),
        _ => timestamp.to_string(),
    };
    dir.join(format!("{FILE_PREFIX}{day}.log"))
}

/// Append one line, creating the directory and rolling the day's file over as
/// needed. Failures are swallowed: a log that cannot be written must not take
/// the app down with it.
fn append_line(dir: &Path, entry: &Pending) {
    let _guard = WRITE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if fs::create_dir_all(dir).is_err() {
        return;
    }

    let path = file_for(dir, entry.timestamp);
    let first_line_of_the_day = !path.exists();
    rotate_if_needed(&path);

    let Ok(mut file) = open_append(&path) else {
        return;
    };
    if writeln!(file, "{}", entry.line).is_err() {
        return;
    }
    restrict_once(&path);

    if first_line_of_the_day {
        prune(dir);
    }
}

fn open_append(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600)
    };
    options.open(path)
}

/// Restrict a day's file to `0600` once per path. The directory itself is
/// protected by `~/.termlet/`, which the app keeps at `0700`.
fn restrict_once(path: &Path) {
    if PERMISSIONS_FIXED.get().is_some_and(|fixed| fixed == path) {
        return;
    }
    if paths::restrict_file_permissions(path).is_ok() {
        let _ = PERMISSIONS_FIXED.set(path.to_path_buf());
    }
}

fn rotate_if_needed(path: &Path) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() <= MAX_FILE_BYTES {
        return;
    }
    let rotated = path.with_extension("log.1");
    let _ = fs::remove_file(&rotated);
    let _ = fs::rename(path, rotated);
}

/// Delete the oldest files beyond [`MAX_FILES`].
fn prune(dir: &Path) {
    let mut files = files_by_age(dir);
    while files.len() > MAX_FILES {
        let (oldest, _) = files.remove(0);
        let _ = fs::remove_file(oldest);
    }
}

/// The activity log files in `dir`, oldest first.
///
/// Ordered by modification time rather than by name: a rolled-over file keeps
/// the day in its name while holding the older half of that day's entries.
fn files_by_age(dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(PathBuf, SystemTime)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(FILE_PREFIX))
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((entry.path(), modified))
        })
        .collect();
    files.sort_by_key(|(_, modified)| *modified);
    files
}

/// The newest `limit` entries in `dir`, oldest first.
fn load_tail_from(dir: &Path, limit: usize) -> Vec<LogEntry> {
    let mut entries: Vec<LogEntry> = files_by_age(dir)
        .iter()
        .rev()
        .flat_map(|(path, _)| {
            tail_lines(path)
                .into_iter()
                .rev()
                .filter_map(|line| parse_line(&line))
                .collect::<Vec<_>>()
        })
        .collect();

    // Each file is in order, but two of them can share a modification time —
    // same-second writes, or a filesystem whose clock is coarse — so the
    // entries are ordered by their own timestamp instead. The sort is stable,
    // which leaves entries written within the same second newest-first.
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.timestamp));
    entries.truncate(limit);
    entries.reverse();
    entries
}

/// The lines at the end of `path`.
///
/// Reads at most [`TAIL_BYTES`], dropping the first line when the read started
/// mid-line. Invalid UTF-8 is replaced rather than refused, so one bad byte
/// cannot hide the rest of the file.
fn tail_lines(path: &Path) -> Vec<String> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let length = file.metadata().map_or(0, |metadata| metadata.len());
    let start = length.saturating_sub(TAIL_BYTES);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }

    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let mut text = String::from_utf8_lossy(&bytes);
    if start > 0
        && let Some(end_of_first_line) = text.find('\n')
    {
        text = Cow::Owned(text[end_of_first_line + 1..].to_string());
    }

    text.lines().map(str::to_string).collect()
}

/// Read one line back as the entry it was written from.
///
/// `None` for anything that is not shaped like a line this module wrote, so a
/// half line at the edge of the read window, a file the user dropped in, or an
/// older format does not surface as a bogus entry.
fn parse_line(line: &str) -> Option<LogEntry> {
    let rest = line.strip_prefix('[')?;
    let (stamp, rest) = rest.split_once(']')?;
    let (token, message) = rest.strip_prefix('[')?.split_once("] ")?;

    Some(LogEntry {
        timestamp: parse_timestamp(stamp)?,
        // A token from a future version still carries its message; only the
        // badge is wrong, where dropping the line would lose the event.
        category: LogCategory::from_token(token).unwrap_or(LogCategory::System),
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_support::scratch_dir;
    use std::time::Duration;

    fn written(dir: &Path, label: &str, timestamp: i64) -> LogEntry {
        append_line(dir, &Pending::at(LogCategory::Terminal, label, timestamp));
        LogEntry {
            timestamp,
            category: LogCategory::Terminal,
            message: label.to_string(),
        }
    }

    #[test]
    fn an_entry_reads_back_from_the_file_it_was_written_to() {
        let dir = scratch_dir("termlet-activity-round-trip");
        let entry = written(&dir, "session 1: command \"ls\"", 1_760_689_647);

        assert_eq!(load_tail_from(&dir, 10), vec![entry]);
    }

    #[test]
    fn the_read_returns_the_newest_entries_in_order() {
        let dir = scratch_dir("termlet-activity-newest");
        for (index, timestamp) in [10_i64, 20, 30, 40].into_iter().enumerate() {
            written(&dir, &format!("line {index}"), timestamp);
        }

        let tail = load_tail_from(&dir, 2);

        assert_eq!(
            tail.iter()
                .map(|entry| entry.message.as_str())
                .collect::<Vec<_>>(),
            vec!["line 2", "line 3"],
            "the page starts on the newest entries, in the order they happened"
        );
    }

    #[test]
    fn a_day_is_a_file_of_its_own() {
        let dir = scratch_dir("termlet-activity-days");
        written(&dir, "first", 1_760_689_647);
        written(&dir, "second", 1_760_689_647 + 7 * 86_400);

        let tail = load_tail_from(&dir, 10);

        assert_eq!(tail.len(), 2, "both days are read back");
        assert_eq!(tail[0].message, "first");
        assert_eq!(tail[1].message, "second");
        assert_eq!(files_by_age(&dir).len(), 2, "each day gets its own file");
    }

    #[test]
    fn a_line_this_module_did_not_write_is_skipped() {
        let dir = scratch_dir("termlet-activity-foreign");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("termlet-2026-01-01.log"),
            "not a log line\n[not a date][terminal] half a line\n",
        )
        .unwrap();
        let entry = written(&dir, "a real entry", 1_760_689_647);

        assert_eq!(
            load_tail_from(&dir, 10),
            vec![entry],
            "only the lines this module wrote come back"
        );
    }

    #[test]
    fn a_message_that_spans_lines_is_folded_into_one() {
        assert_eq!(one_line("one\ntwo"), "one two");
        assert_eq!(one_line("one\rtwo"), "one two");
        assert!(matches!(one_line("single"), Cow::Borrowed(_)));
    }

    #[test]
    fn pruning_keeps_only_the_newest_files() {
        let dir = scratch_dir("termlet-activity-prune");
        fs::create_dir_all(&dir).unwrap();
        for day in 1..=9_u64 {
            let path = dir.join(format!("termlet-2026-01-0{day}.log"));
            fs::write(&path, "[2026-01-01 00:00:00][system] something\n").unwrap();
            File::options()
                .append(true)
                .open(&path)
                .unwrap()
                .set_modified(UNIX_EPOCH + Duration::from_secs(day))
                .unwrap();
        }

        prune(&dir);

        let left: Vec<String> = files_by_age(&dir)
            .iter()
            .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left.len(), MAX_FILES);
        assert!(
            left.iter()
                .all(|name| !name.ends_with("01.log") && !name.ends_with("02.log")),
            "the two oldest days are the ones that go: {left:?}"
        );
    }

    /// The two tests below are the seam with the app: everything above drives
    /// this module on its own, these go through [`crate::app::App`] and its
    /// `add_log`, which is what the UI actually calls.
    #[test]
    fn an_entry_the_app_logs_reaches_the_file() {
        let mut app = crate::core::test_support::app();
        let message = "snippet \"deploy\" saved";
        app.add_log(LogCategory::Snippet, message);

        // `add_log` hands the line to the writer thread and returns, so the
        // file appearing is the thing to wait for.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let file_has_it = loop {
            if load_tail_from(&paths::logs_dir(), crate::core::data::MAX_LOG_ENTRIES)
                .iter()
                .any(|entry| entry.message == message && entry.category == LogCategory::Snippet)
            {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(10));
        };

        assert!(
            file_has_it,
            "the entry never reached {:?}",
            paths::logs_dir()
        );
    }

    #[test]
    fn the_logs_page_starts_on_what_the_last_run_wrote() {
        // Point the process's data directory at the scratch directory before
        // asking where the logs live: without this the paths below would be the
        // developer's own `~/.termlet/logs/`.
        crate::core::test_support::prepare_data_dir();
        append_line(
            &paths::logs_dir(),
            &Pending::at(LogCategory::Host, "Updated host \"web\"", unix_now()),
        );

        let app = crate::core::test_support::app();

        assert!(
            app.logs
                .iter()
                .any(|entry| entry.message == "Updated host \"web\""
                    && entry.category == LogCategory::Host),
            "a restart should show the previous run's entries: {:?}",
            app.logs
                .iter()
                .map(|entry| &entry.message)
                .collect::<Vec<_>>()
        );
    }
}
