//! Opening external URLs in the OS default handler.
//!
//! Used by the in-app feedback entry, which hands a prefilled GitHub issue form
//! to the OS so it opens in whatever browser the user already has configured.

/// Repository that hosts the source and the issue tracker.
const REPOSITORY: &str = "https://github.com/narol2026/termlet";

/// The bug-report issue form, prefilled with the running build's version and
/// platform so a reporter does not have to look either of them up.
///
/// GitHub prefills issue-form fields from query parameters named after each
/// field's `id` in `.github/ISSUE_TEMPLATE/bug_report.yml`.
pub fn bug_report_url(version: &str) -> String {
    format!(
        "{REPOSITORY}/issues/new?template=bug_report.yml&version={}&platform={}",
        encode_component(version),
        encode_component(platform_label(std::env::consts::OS, std::env::consts::ARCH)),
    )
}

/// Label of the `platform` option in the bug-report form that matches the
/// running build.
///
/// The string has to match one of the dropdown's options exactly, so anything
/// the form does not list falls back to its catch-all option.
fn platform_label(os: &str, arch: &str) -> &'static str {
    match os {
        "linux" => match arch {
            "x86_64" => "Linux (x86_64)",
            "aarch64" => "Linux (aarch64)",
            _ => "Other",
        },
        "macos" => match arch {
            "aarch64" => "macOS (Apple Silicon)",
            "x86_64" => "macOS (Intel)",
            _ => "Other",
        },
        "windows" => "Windows",
        _ => "Other",
    }
}

/// Open `url` in the OS default handler for its scheme (usually a browser).
pub fn open_url(url: &str) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();

    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();

    #[cfg(target_os = "windows")]
    {
        let mut command = std::process::Command::new("cmd");
        // The empty argument after `start` is the window title; without it
        // `cmd` treats a quoted URL as the title and opens nothing.
        command.args(["/c", "start", "", url]);
        crate::core::process::suppress_console_window(&mut command);
        let _ = command.spawn();
    }
}

/// Percent-encode a string for use in a query-string value.
///
/// Keeps the RFC 3986 unreserved set (`A–Z a–z 0–9 - . _ ~`) literal and
/// encodes every other byte of the UTF-8 encoding, so spaces, parentheses and
/// non-ASCII text survive the round trip.
fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &byte in s.as_bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(hex_digit(byte >> 4));
            out.push(hex_digit(byte & 0x0f));
        }
    }
    out
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'A' + (nibble - 10)) as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bug_report_url_prefills_the_version_field() {
        let url = bug_report_url("1.0.0 (2026-10-13)");
        assert!(
            url.starts_with(
                "https://github.com/narol2026/termlet/issues/new?template=bug_report.yml"
            ),
            "got {url}"
        );
        assert!(
            url.contains("&version=1.0.0%20%282026-10-13%29"),
            "got {url}"
        );
        assert!(
            url.contains(&format!(
                "&platform={}",
                encode_component(platform_label(std::env::consts::OS, std::env::consts::ARCH))
            )),
            "got {url}"
        );
    }

    #[test]
    fn platform_labels_match_the_issue_form_options() {
        assert_eq!(platform_label("linux", "x86_64"), "Linux (x86_64)");
        assert_eq!(platform_label("linux", "aarch64"), "Linux (aarch64)");
        assert_eq!(platform_label("macos", "aarch64"), "macOS (Apple Silicon)");
        assert_eq!(platform_label("macos", "x86_64"), "macOS (Intel)");
        assert_eq!(platform_label("windows", "x86_64"), "Windows");
        // Anything the form does not list lands on its catch-all option.
        assert_eq!(platform_label("freebsd", "x86_64"), "Other");
        assert_eq!(platform_label("linux", "riscv64"), "Other");
    }

    #[test]
    fn encodes_non_ascii_as_utf8() {
        // "中" is E4 B8 AD in UTF-8.
        assert_eq!(encode_component("中"), "%E4%B8%AD");
    }

    #[test]
    fn keeps_unreserved_characters_literal() {
        assert_eq!(encode_component("aZ0-._~"), "aZ0-._~");
    }
}
