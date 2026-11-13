//! Shared date/time formatting helpers.
//!
//! All local-time formatting is centralized here so callers share the same
//! local-offset resolution and error fallbacks instead of re-implementing them
//! (previously duplicated in `core/data.rs`, `core/diagnostics.rs`, and
//! `views/logs.rs`).

use std::time::SystemTime;
use time::{
    OffsetDateTime, PrimitiveDateTime, UtcOffset, format_description::FormatItem,
    macros::format_description,
};

/// `YYYY-MM-DD HH:MM` — minute precision, used for file modification times.
const FMT_MINUTE: &[FormatItem<'_>] = format_description!("[year]-[month]-[day] [hour]:[minute]");

/// `YYYY-MM-DD HH:MM:SS` — second precision, used for logs and history entries.
const FMT_SECOND: &[FormatItem<'_>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

/// `YYYY-MM-DD HH:MM:SS.mmm` — millisecond precision, used for diagnostic logs.
const FMT_MILLIS: &[FormatItem<'_>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]");

/// Resolve the local offset, falling back to UTC when the platform can't tell us.
fn local_offset() -> UtcOffset {
    UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC)
}

/// Format a Unix timestamp (seconds) in `offset` local time using `fmt`.
/// Returns an empty string if the timestamp or formatting fails.
fn format_with_offset(ts: i64, offset: UtcOffset, fmt: &[FormatItem<'_>]) -> String {
    OffsetDateTime::from_unix_timestamp(ts)
        .ok()
        .map(|utc| utc.to_offset(offset))
        .and_then(|dt| dt.format(fmt).ok())
        .unwrap_or_default()
}

/// Format a Unix timestamp (seconds) as a local `YYYY-MM-DD HH:MM:SS` string.
pub(crate) fn format_timestamp(ts: i64) -> String {
    format_with_offset(ts, local_offset(), FMT_SECOND)
}

/// Parse a `YYYY-MM-DD HH:MM:SS` local timestamp back into Unix seconds.
///
/// The inverse of [`format_timestamp`], used by the activity log to date the
/// entries of a previous run again. Anything else parses to `None`, so a
/// hand-edited line is skipped instead of being dated wrongly.
pub(crate) fn parse_timestamp(s: &str) -> Option<i64> {
    parse_with_offset(s, local_offset())
}

/// Parse `s` as a wall-clock time in `offset`. Split out of [`parse_timestamp`]
/// so tests can pin the offset instead of reading whatever the machine's is.
fn parse_with_offset(s: &str, offset: UtcOffset) -> Option<i64> {
    PrimitiveDateTime::parse(s, FMT_SECOND)
        .ok()
        .map(|dt| dt.assume_offset(offset).unix_timestamp())
}

/// Format a `SystemTime` as a local `YYYY-MM-DD HH:MM` string (minute precision),
/// used for file modification times.
pub(crate) fn format_system_time(st: SystemTime) -> String {
    let secs = st
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    format_with_offset(secs, local_offset(), FMT_MINUTE)
}

/// Format a `SystemTime` as a local `YYYY-MM-DD HH:MM:SS.mmm` string, falling
/// back to the raw millisecond value if formatting fails (e.g. no local offset).
pub(crate) fn format_log_time(ts: SystemTime) -> String {
    format_log_time_with_offset(ts, local_offset())
}

fn format_log_time_with_offset(ts: SystemTime, offset: UtcOffset) -> String {
    OffsetDateTime::from(ts)
        .to_offset(offset)
        .format(FMT_MILLIS)
        .unwrap_or_else(|_| {
            ts.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_second_precision() {
        assert_eq!(
            format_with_offset(0, UtcOffset::UTC, FMT_SECOND),
            "1970-01-01 00:00:00"
        );
    }

    #[test]
    fn formats_minute_precision() {
        assert_eq!(
            format_with_offset(0, UtcOffset::UTC, FMT_MINUTE),
            "1970-01-01 00:00"
        );
    }

    #[test]
    fn formats_millisecond_precision() {
        let st = std::time::UNIX_EPOCH + std::time::Duration::from_millis(123);
        assert_eq!(
            format_log_time_with_offset(st, UtcOffset::UTC),
            "1970-01-01 00:00:00.123"
        );
    }

    #[test]
    fn respects_offset() {
        let offset = UtcOffset::from_hms(5, 30, 0).unwrap();
        assert_eq!(
            format_with_offset(0, offset, FMT_SECOND),
            "1970-01-01 05:30:00"
        );
    }

    #[test]
    fn parsing_undoes_the_formatting_in_the_same_offset() {
        let offset = UtcOffset::from_hms(-4, 0, 0).unwrap();
        let timestamp = 1_760_689_647;

        let formatted = format_with_offset(timestamp, offset, FMT_SECOND);
        assert_eq!(parse_with_offset(&formatted, offset), Some(timestamp));
    }

    #[test]
    fn a_line_that_is_not_a_timestamp_is_not_dated() {
        assert_eq!(parse_with_offset("", UtcOffset::UTC), None);
        assert_eq!(parse_with_offset("yesterday", UtcOffset::UTC), None);
        // A diagnostic-shaped line carries milliseconds, which this format
        // does not accept.
        assert_eq!(
            parse_with_offset("1970-01-01 00:00:00.123", UtcOffset::UTC),
            None
        );
    }
}
