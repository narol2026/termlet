//! Spawning additional termlet windows.
//!
//! Termlet is one window per process, so "new window" actions launch another
//! instance of the current executable. Keeping that here (instead of copying
//! the `current_exe` checks around) makes the shared failure handling
//! consistent for the Dock menu and the in-app "Open in new window" action.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Keep a background child process from allocating a visible Windows console.
///
/// Termlet's release build is a GUI-subsystem process with no console of its
/// own. A console child such as `reg.exe` or `cmd.exe` would otherwise receive
/// a new console window, which appears for the lifetime of the child and looks
/// like a window flashing on startup. The helper is only needed on Windows,
/// so it is not compiled on other platforms.
#[cfg(windows)]
pub fn suppress_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    command.creation_flags(CREATE_NO_WINDOW);
}

/// Why a secondary termlet window could not be launched.
#[derive(Debug)]
pub enum SpawnWindowError {
    /// The current executable path could not be determined.
    CurrentExe(io::Error),
    /// The binary backing this process is gone (for example, it was replaced
    /// while running, which makes `/proc/self/exe` report " (deleted)").
    BinaryGone(PathBuf),
    /// The child process failed to start.
    Spawn(io::Error),
}

impl std::fmt::Display for SpawnWindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CurrentExe(error) => write!(f, "current_exe failed: {error}"),
            Self::BinaryGone(path) => write!(
                f,
                "current_exe={} is gone (binary was replaced); please restart termlet",
                path.display()
            ),
            Self::Spawn(error) => error.fmt(f),
        }
    }
}

/// Launch another termlet window process with `args`.
pub fn spawn_window<I, S>(args: I) -> Result<(), SpawnWindowError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let exe = std::env::current_exe().map_err(SpawnWindowError::CurrentExe)?;
    spawn_window_from(&exe, args)
}

/// Launch `exe` with `args`, refusing a binary that is obviously unusable.
///
/// Split out of [`spawn_window`] so the guards below can be exercised without
/// depending on how the running process was started: nothing a test can do
/// makes its own `current_exe` point at a deleted file.
fn spawn_window_from<I, S>(exe: &Path, args: I) -> Result<(), SpawnWindowError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let exe_str = exe.to_string_lossy();
    if exe_str.ends_with(" (deleted)") || !exe.exists() {
        return Err(SpawnWindowError::BinaryGone(exe.to_path_buf()));
    }

    Command::new(exe)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(SpawnWindowError::Spawn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_support::scratch_dir;

    #[cfg(windows)]
    #[test]
    fn a_suppressed_console_child_still_runs() {
        let mut command = Command::new("cmd.exe");
        command.args(["/C", "exit", "0"]);
        suppress_console_window(&mut command);

        assert!(
            command.status().unwrap().success(),
            "suppressing the console must not prevent the child from running"
        );
    }

    #[test]
    fn a_binary_that_is_not_there_is_reported_as_gone() {
        let missing = PathBuf::from("/nonexistent/termlet");
        assert!(matches!(
            spawn_window_from(&missing, ["--new-window"]),
            Err(SpawnWindowError::BinaryGone(path)) if path == missing
        ));
    }

    #[test]
    fn a_replaced_binary_is_reported_as_gone_even_though_the_path_exists() {
        // When the running binary is replaced, Linux reports its path with
        // " (deleted)" appended. The path itself still exists, so this check
        // has to come before the existence test — otherwise the app would try
        // to exec a file that is not the program the user is running.
        let exe = scratch_dir("termlet-spawn-deleted").join("termlet (deleted)");
        std::fs::write(&exe, b"#!/bin/sh\n").unwrap();

        assert!(matches!(
            spawn_window_from(&exe, ["--new-window"]),
            Err(SpawnWindowError::BinaryGone(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_usable_binary_is_spawned() {
        spawn_window_from(Path::new("/bin/true"), ["--new-window"]).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_cannot_be_executed_reports_the_spawn_error() {
        // A plain file with no execute bit and no shebang: the failure comes
        // from the OS, not from the guards above, so it must surface as
        // `Spawn` rather than as `BinaryGone`.
        let exe = scratch_dir("termlet-spawn-not-executable").join("not-a-program");
        std::fs::write(&exe, b"just some text\n").unwrap();

        let error = spawn_window_from(&exe, ["--new-window"]).unwrap_err();
        assert!(matches!(error, SpawnWindowError::Spawn(_)), "{error:?}");
    }

    #[test]
    fn every_error_says_what_went_wrong() {
        let gone = SpawnWindowError::BinaryGone(PathBuf::from("/opt/termlet/termlet"));
        let text = gone.to_string();
        assert!(text.contains("/opt/termlet/termlet"), "{text}");
        assert!(text.contains("gone"), "{text}");
        assert!(
            text.contains("restart"),
            "the message tells the user what to do: {text}"
        );

        let unreadable = SpawnWindowError::CurrentExe(io::Error::new(
            io::ErrorKind::NotFound,
            "no such process",
        ));
        assert!(unreadable.to_string().contains("current_exe"));

        let denied = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let expected = denied.to_string();
        assert_eq!(SpawnWindowError::Spawn(denied).to_string(), expected);
    }
}
