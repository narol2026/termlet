//! Platform-specific local-terminal handling for the SSH bridge.
//!
//! The bridge runs as the child of a terminal tab's PTY. Unix exposes a termios
//! device and `SIGWINCH`; Windows uses the `ConPTY` console handles and has to be
//! polled for size changes. Keeping those details here leaves the SSH and byte
//! forwarding loop in `bridge.rs` shared by every platform.

#[cfg(unix)]
mod unix {
    use std::time::Duration;

    const STDIN_FD: std::os::fd::RawFd = 0;
    const STDOUT_FD: std::os::fd::RawFd = 1;

    /// Query the current terminal window size via `TIOCGWINSZ`.
    ///
    /// Tries the stdout FD first, then falls back to stderr. On macOS PTYs the
    /// window size may not be set instantly at child-process start; the GUI
    /// terminal widget applies it shortly after spawn. If both FDs return a
    /// zero size we fall back to 80x24 with zero pixel dimensions, which is
    /// harmless: the remote shell receives the correct size on the first
    /// `SIGWINCH` resize event.
    pub(crate) fn current_winsize() -> (u16, u16, u16, u16) {
        // Helper: try to read winsize from a single FD.
        fn try_fd(fd: std::os::fd::RawFd) -> Option<(u16, u16, u16, u16)> {
            // SAFETY: `libc::winsize` is a `#[repr(C)]` struct of integer
            // fields, so the all-zero bit pattern is a valid value; every field
            // is overwritten by the `ioctl` below before it is read.
            let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
            // SAFETY: `TIOCGWINSZ` takes a `*mut winsize` out-parameter, and
            // `ws` is a valid, exclusively-borrowed local of exactly that type.
            // `fd` is a descriptor this process owns; an invalid one only makes
            // the call return an error, which is handled below.
            let rc = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &raw mut ws) };
            (rc == 0 && ws.ws_col > 0 && ws.ws_row > 0).then_some((
                ws.ws_col,
                ws.ws_row,
                ws.ws_xpixel,
                ws.ws_ypixel,
            ))
        }

        // Both are connected to the same PTY slave in the child process.
        try_fd(STDOUT_FD)
            .or_else(|| try_fd(libc::STDERR_FILENO))
            .unwrap_or((80, 24, 0, 0))
    }

    /// RAII guard that puts stdin into termios raw mode and restores the
    /// previous settings when dropped.
    ///
    /// Linux and macOS both implement POSIX termios, but glibc's struct carries
    /// a `c_line` field that Darwin's does not; `libc` supplies the right layout
    /// per target. The echo sub-flags `cfmakeraw` leaves behind differ between
    /// the two, which is why `enter()` clears them explicitly.
    pub(crate) struct RawMode {
        fd: std::os::fd::RawFd,
        original: libc::termios,
    }

    impl RawMode {
        pub(crate) fn enter() -> Result<Self, String> {
            let fd = STDIN_FD;
            // Ensure stdin is actually a tty; otherwise tcgetattr will fail and
            // we would silently misbehave (e.g. when launched outside a PTY for
            // tests).
            // SAFETY: `isatty` has no precondition beyond a plausible
            // descriptor; it reports 0 for an invalid one instead of
            // misbehaving.
            if unsafe { libc::isatty(fd) } == 0 {
                return Err("stdin is not a tty".to_string());
            }

            // SAFETY: `libc::termios` is a `#[repr(C)]` struct of integer
            // types; the all-zeros bit pattern is always a valid
            // representation. We immediately overwrite it with `tcgetattr`.
            let mut original: libc::termios = unsafe { std::mem::zeroed() };
            // SAFETY: `original` is a valid local `termios` and `fd` was just
            // confirmed to be a tty, so any failure is a genuine error that is
            // reported to the caller.
            if unsafe { libc::tcgetattr(fd, &raw mut original) } != 0 {
                return Err(format!(
                    "tcgetattr failed on fd {fd}: {}",
                    std::io::Error::last_os_error()
                ));
            }

            let mut raw = original;
            // SAFETY: `raw` is an initialised `termios` local; `cfmakeraw` only
            // writes through the mutable reference it is handed.
            unsafe { libc::cfmakeraw(&raw mut raw) };

            // Defensive flag clearing. `cfmakeraw` is POSIX and clears the
            // major flags, but on BSD-derived systems (macOS included) the echo
            // sub-flags can survive it, so they are zeroed here to get identical
            // raw-mode state on every platform. The critical flags are
            // re-cleared too, in case a future platform changes the set
            // `cfmakeraw` touches.

            // Input: disable all special-character processing.
            raw.c_iflag &= !(libc::IGNBRK
                | libc::BRKINT
                | libc::IGNPAR
                | libc::PARMRK
                | libc::INPCK
                | libc::ISTRIP
                | libc::INLCR
                | libc::IGNCR
                | libc::ICRNL
                | libc::IXON
                | libc::IXOFF
                | libc::IXANY);

            // Output: disable all post-processing (NL -> CRNL translation, tab
            // expansion, etc.) so every byte written to stdout reaches the
            // terminal unchanged.
            raw.c_oflag &= !libc::OPOST;

            // Local: disable echo, canonical (line-buffered) mode,
            // signal-generation (ISIG), and extended processing (IEXTEN).
            raw.c_lflag &= !(libc::ECHO
                | libc::ECHOE
                | libc::ECHOK
                | libc::ECHONL
                | libc::ECHOCTL
                | libc::ECHOKE
                | libc::ICANON
                | libc::ISIG
                | libc::IEXTEN
                | libc::TOSTOP);

            // Control: 8-bit characters, no parity.
            raw.c_cflag &= !(libc::CSIZE | libc::PARENB | libc::CRTSCTS);
            raw.c_cflag |= libc::CS8 | libc::CREAD;

            // Non-canonical mode: return after at least 1 byte, no inter-byte
            // timer.
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;

            // SAFETY: `fd` is a tty and `raw` is a fully initialised `termios`;
            // `TCSANOW` applies the settings immediately and imposes no further
            // preconditions.
            if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw const raw) } != 0 {
                return Err(format!(
                    "tcsetattr (raw mode) failed on fd {fd}: {}",
                    std::io::Error::last_os_error()
                ));
            }

            // Suppress unused-field warning on platforms where the field layout
            // differs; the FD is needed for the destructor.
            let _ = STDOUT_FD;

            Ok(Self { fd, original })
        }
    }

    impl Drop for RawMode {
        fn drop(&mut self) {
            // SAFETY: `self.fd` is the tty this guard switched to raw mode and
            // `self.original` is the `termios` captured before that change, so
            // this restores the settings that were in effect when `enter()`
            // succeeded.
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSANOW, &raw const self.original);
            }
        }
    }

    /// Waits for the next `SIGWINCH` delivered by the parent terminal.
    pub(crate) struct ResizeWatcher {
        signal: tokio::signal::unix::Signal,
    }

    impl ResizeWatcher {
        pub(crate) fn new() -> Result<Self, String> {
            let signal =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())
                    .map_err(|error| error.to_string())?;
            Ok(Self { signal })
        }

        pub(crate) async fn changed(&mut self) {
            loop {
                if self.signal.recv().await.is_some() {
                    return;
                }
                // A closed signal stream means the runtime is shutting down;
                // don't spin the bridge's select loop while it unwinds.
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{
        CONSOLE_MODE, CONSOLE_SCREEN_BUFFER_INFO, ENABLE_ECHO_INPUT, ENABLE_EXTENDED_FLAGS,
        ENABLE_INSERT_MODE, ENABLE_LINE_INPUT, ENABLE_MOUSE_INPUT, ENABLE_PROCESSED_INPUT,
        ENABLE_PROCESSED_OUTPUT, ENABLE_QUICK_EDIT_MODE, ENABLE_VIRTUAL_TERMINAL_INPUT,
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_WINDOW_INPUT, GetConsoleCP, GetConsoleMode,
        GetConsoleOutputCP, GetConsoleScreenBufferInfo, GetStdHandle, STD_ERROR_HANDLE,
        STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleCP, SetConsoleMode, SetConsoleOutputCP,
    };

    const CP_UTF8: u32 = 65001;
    const RESIZE_POLL_INTERVAL: Duration = Duration::from_millis(100);

    fn is_valid_handle(handle: HANDLE) -> bool {
        !handle.is_null() && handle != INVALID_HANDLE_VALUE
    }

    fn dimension(value: i32) -> Option<u16> {
        u16::try_from(value).ok().filter(|value| *value > 0)
    }

    /// Convert the console's viewport into character-cell dimensions.
    ///
    /// `srWindow` is the visible window and matches what the remote shell
    /// expects. `dwSize` is only a fallback because it can describe a larger
    /// screen buffer on a traditional console. Pixel dimensions are unavailable
    /// through this API and are not needed by the SSH pty request.
    fn console_size(handle: HANDLE) -> Option<(u16, u16)> {
        if !is_valid_handle(handle) {
            return None;
        }
        // SAFETY: `CONSOLE_SCREEN_BUFFER_INFO` is a C struct of integers and
        // nested coordinate structs, so all-zeroes is a valid value. The API
        // overwrites it before any field is read.
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = unsafe { std::mem::zeroed() };
        // SAFETY: `handle` is a non-null, non-INVALID console handle and `info`
        // is a valid, exclusively-borrowed out-parameter of the required type.
        let rc = unsafe { GetConsoleScreenBufferInfo(handle, &raw mut info) };
        if rc == 0 {
            return None;
        }

        let width = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
        let height = i32::from(info.srWindow.Bottom) - i32::from(info.srWindow.Top) + 1;
        dimension(width).zip(dimension(height)).or_else(|| {
            dimension(i32::from(info.dwSize.X)).zip(dimension(i32::from(info.dwSize.Y)))
        })
    }

    pub(crate) fn current_winsize() -> (u16, u16, u16, u16) {
        // SAFETY: `GetStdHandle` takes a standard-handle selector and returns a
        // process-owned handle; it has no pointer preconditions.
        let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        // SAFETY: See above; stderr is the fallback when stdout is redirected.
        let error = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        console_size(output)
            .or_else(|| console_size(error))
            .map_or((80, 24, 0, 0), |(cols, rows)| (cols, rows, 0, 0))
    }

    fn raw_input_mode(original: CONSOLE_MODE) -> CONSOLE_MODE {
        (original
            & !(ENABLE_ECHO_INPUT
                | ENABLE_LINE_INPUT
                | ENABLE_PROCESSED_INPUT
                | ENABLE_QUICK_EDIT_MODE
                | ENABLE_MOUSE_INPUT
                | ENABLE_WINDOW_INPUT
                | ENABLE_INSERT_MODE))
            | ENABLE_EXTENDED_FLAGS
            | ENABLE_VIRTUAL_TERMINAL_INPUT
    }

    fn raw_output_mode(original: CONSOLE_MODE) -> CONSOLE_MODE {
        original | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING
    }

    /// RAII guard that switches the `ConPTY` console to raw byte input.
    ///
    /// The parent terminal forwards bytes through the pseudoconsole, so local
    /// line editing, echo and Ctrl+C processing must all be disabled. Virtual
    /// terminal input is enabled so arrows, function keys and paste arrive as
    /// the same escape sequences the Unix bridge forwards.
    pub(crate) struct RawMode {
        input: HANDLE,
        original_input: CONSOLE_MODE,
        output: Option<(HANDLE, CONSOLE_MODE)>,
        input_cp: u32,
        output_cp: u32,
        restore_input_cp: bool,
        restore_output_cp: bool,
    }

    impl RawMode {
        pub(crate) fn enter() -> Result<Self, String> {
            // SAFETY: `GetStdHandle` takes a standard-handle selector and
            // returns a process-owned handle; no pointers are involved.
            let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
            if !is_valid_handle(input) {
                return Err("stdin is not a console handle".to_string());
            }

            let mut original_input = 0;
            // SAFETY: `input` is a non-null, non-INVALID console handle and
            // `original_input` is a valid out-parameter for the console mode.
            let mode_rc = unsafe { GetConsoleMode(input, &raw mut original_input) };
            if mode_rc == 0 {
                return Err(format!(
                    "GetConsoleMode(stdin) failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            // SAFETY: `GetConsoleCP` has no pointer preconditions and returns 0
            // when the process has no attached console.
            let input_cp = unsafe { GetConsoleCP() };
            // SAFETY: `GetConsoleOutputCP` has the same contract as
            // `GetConsoleCP`.
            let output_cp = unsafe { GetConsoleOutputCP() };

            // SAFETY: `input` is a console handle whose mode was just read, and
            // the new mode is built only from documented console-mode flags.
            let raw_rc = unsafe { SetConsoleMode(input, raw_input_mode(original_input)) };
            if raw_rc == 0 {
                return Err(format!(
                    "SetConsoleMode(stdin) failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            let mut guard = Self {
                input,
                original_input,
                output: None,
                input_cp,
                output_cp,
                restore_input_cp: false,
                restore_output_cp: false,
            };

            // SAFETY: See the stdin lookup above; stdout may be redirected,
            // which is handled by the validity check.
            let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
            if is_valid_handle(output) {
                let mut original_output = 0;
                // SAFETY: `output` is a non-null, non-INVALID handle and
                // `original_output` is a valid out-parameter.
                let output_mode_rc = unsafe { GetConsoleMode(output, &raw mut original_output) };
                if output_mode_rc != 0 {
                    // SAFETY: `output` is a console handle whose mode was just
                    // read; the new mode only adds documented output flags.
                    let output_set_rc =
                        unsafe { SetConsoleMode(output, raw_output_mode(original_output)) };
                    if output_set_rc != 0 {
                        guard.output = Some((output, original_output));
                    }
                }
            }

            if input_cp != 0 {
                // SAFETY: `SetConsoleCP` takes a code-page id and has no pointer
                // preconditions. UTF-8 keeps non-ASCII input bytes intact.
                guard.restore_input_cp = unsafe { SetConsoleCP(CP_UTF8) } != 0;
            }
            if output_cp != 0 {
                // SAFETY: `SetConsoleOutputCP` has the same contract as
                // `SetConsoleCP`; restoring the original page happens in Drop.
                guard.restore_output_cp = unsafe { SetConsoleOutputCP(CP_UTF8) } != 0;
            }

            Ok(guard)
        }
    }

    impl Drop for RawMode {
        fn drop(&mut self) {
            if let Some((output, original)) = self.output {
                // SAFETY: `output` is the console handle captured in `enter`
                // and `original` is the mode read before it was changed.
                let _ = unsafe { SetConsoleMode(output, original) };
            }
            // SAFETY: `self.input` is the console handle captured in `enter`
            // and `self.original_input` is its pre-change mode.
            let _ = unsafe { SetConsoleMode(self.input, self.original_input) };
            if self.restore_input_cp {
                // SAFETY: `self.input_cp` is the code page read from the same
                // console in `enter`, before switching it to UTF-8.
                let _ = unsafe { SetConsoleCP(self.input_cp) };
            }
            if self.restore_output_cp {
                // SAFETY: `self.output_cp` is the output code page read in
                // `enter`, before switching it to UTF-8.
                let _ = unsafe { SetConsoleOutputCP(self.output_cp) };
            }
        }
    }

    /// Polls the `ConPTY` console because Windows has no `SIGWINCH`.
    ///
    /// The interval is small enough for interactive resize to feel immediate,
    /// while a single `GetConsoleScreenBufferInfo` call is effectively free.
    pub(crate) struct ResizeWatcher {
        interval: tokio::time::Interval,
        last: (u16, u16, u16, u16),
    }

    impl ResizeWatcher {
        pub(crate) fn new() -> Self {
            let last = current_winsize();
            let mut interval = tokio::time::interval(RESIZE_POLL_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            Self { interval, last }
        }

        pub(crate) async fn changed(&mut self) {
            loop {
                self.interval.tick().await;
                let current = current_winsize();
                if current.0 != self.last.0 || current.1 != self.last.1 {
                    self.last = current;
                    return;
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn raw_mode_disables_local_line_editing() {
            let original = ENABLE_ECHO_INPUT
                | ENABLE_LINE_INPUT
                | ENABLE_PROCESSED_INPUT
                | ENABLE_QUICK_EDIT_MODE
                | ENABLE_MOUSE_INPUT
                | ENABLE_WINDOW_INPUT
                | ENABLE_INSERT_MODE;

            let raw = raw_input_mode(original);

            assert_eq!(
                raw & (ENABLE_ECHO_INPUT
                    | ENABLE_LINE_INPUT
                    | ENABLE_PROCESSED_INPUT
                    | ENABLE_QUICK_EDIT_MODE
                    | ENABLE_MOUSE_INPUT
                    | ENABLE_WINDOW_INPUT
                    | ENABLE_INSERT_MODE),
                0,
                "the console must not echo, buffer or intercept keystrokes"
            );
            assert_ne!(
                raw & ENABLE_VIRTUAL_TERMINAL_INPUT,
                0,
                "special keys have to arrive as VT escape sequences"
            );
            assert_ne!(
                raw & ENABLE_EXTENDED_FLAGS,
                0,
                "quick-edit can only be disabled with extended flags"
            );
        }

        #[test]
        fn raw_mode_keeps_virtual_terminal_output() {
            let raw = raw_output_mode(0);

            assert_ne!(raw & ENABLE_PROCESSED_OUTPUT, 0);
            assert_ne!(raw & ENABLE_VIRTUAL_TERMINAL_PROCESSING, 0);
        }

        #[test]
        fn console_dimensions_reject_non_positive_values() {
            assert_eq!(dimension(120), Some(120));
            assert_eq!(dimension(0), None);
            assert_eq!(dimension(-1), None);
        }
    }
}

#[cfg(unix)]
pub(crate) use unix::{RawMode, ResizeWatcher, current_winsize};
#[cfg(windows)]
pub(crate) use windows::{RawMode, ResizeWatcher, current_winsize};
