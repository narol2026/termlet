//! Subprocess "ssh-bridge" mode: a small standalone program that connects to a
//! remote host using the in-process russh client and bridges the local PTY
//! (stdin/stdout) to the remote interactive shell.
//!
//! The terminal tab spawns the dedicated bridge executable as its PTY child.
//! On Windows that executable uses the Console subsystem while the GUI stays in
//! the GUI subsystem, so startup never flashes a temporary console window.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use russh::ChannelMsg;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::core::data::{Host, HostId};
use crate::core::diagnostics::Level;
use crate::ssh::local_tty::{RawMode, ResizeWatcher, current_winsize};
use secrecy::SecretString;

// SSH connection-health monitoring
//
// The GUI spawns this bridge with `TERMLET_SSH_HEALTH_FILE` pointing at a
// per-session status file. Every `HEALTH_CHECK_INTERVAL` we probe the SSH link
// with a keepalive ping (a full round trip to the server) and write
// `"stable"/"unstable"` to that file so the topbar can show a yellow dot while
// the link is degraded — even though the connection is still up.
//
// Thresholds are asymmetric on purpose so the dot doesn't flap: a link turns
// unstable on a single bad sample (timeout or RTT ≥ `UNSTABLE_RTT`) but only
// recovers after `GOOD_STREAK` consecutive samples well under the bar.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const HEALTH_PING_TIMEOUT: Duration = Duration::from_secs(2);
const UNSTABLE_RTT: Duration = Duration::from_millis(800);
const RECOVER_RTT: Duration = Duration::from_millis(400);
const GOOD_STREAK: u32 = 3;

/// Write the current SSH health state to the parent's per-session status file.
/// Only called on state transitions (plus once at startup), so file churn is
/// negligible. Failure is best-effort: the GUI keeps its previous state when
/// the file can't be written or read.
fn report_health(path: &Path, unstable: bool) {
    let state = if unstable { "unstable\n" } else { "stable\n" };
    let _ = crate::core::paths::write_private_atomic(path, state.as_bytes());
}

/// Entry point invoked by the dedicated console bridge executable.
///
/// The first positional argument after the flag is the integer host id to
/// connect to (matching `data::Host::id`).
pub fn run(host_id_arg: &str) -> ExitCode {
    let host_id: HostId = match host_id_arg.parse::<u32>() {
        Ok(id) => HostId(id),
        Err(error) => {
            eprintln!("termlet ssh-bridge: invalid host id `{host_id_arg}`: {error}");
            crate::core::diagnostics::log(
                Level::Error,
                "SSH_BRIDGE",
                format!("ssh-bridge start failed: invalid host_id='{host_id_arg}': {error}"),
            );
            return ExitCode::from(2);
        }
    };

    crate::core::diagnostics::log(
        Level::Info,
        "SSH_BRIDGE",
        format!("ssh-bridge starting: host_id={host_id}"),
    );

    let mut host = match load_host(host_id) {
        Ok(host) => host,
        Err(error) => {
            eprintln!("termlet ssh-bridge: {error}");
            crate::core::diagnostics::log(
                Level::Error,
                "SSH_BRIDGE",
                format!("ssh-bridge failed to load host_id={host_id}: {error}"),
            );
            return ExitCode::from(2);
        }
    };

    // The GUI passes a connection-scoped key passphrase through the
    // environment. It is not persisted, so the in-memory host loaded from
    // storage needs it reattached here before opening the session.
    if let Ok(passphrase) = std::env::var("TERMLET_SSH_KEY_PASSPHRASE")
        && !passphrase.is_empty()
    {
        host.key_passphrase = Some(SecretString::from(passphrase));
    }
    // "Continue" trusts a first-seen key for this session without writing
    // known_hosts. The GUI keeps that fingerprint only on the in-memory host,
    // so the bridge has to receive it explicitly before it reloads the host
    // from storage.
    if let Ok(fingerprint) = std::env::var("TERMLET_SSH_EXPECTED_FINGERPRINT")
        && !fingerprint.is_empty()
    {
        host.expected_fingerprint = Some(fingerprint);
    }

    crate::core::diagnostics::log(
        Level::Info,
        "SSH_BRIDGE",
        format!(
            "ssh-bridge loaded host: {}@{}:{} (auth={:?})",
            host.username, host.hostname, host.port, host.auth_type
        ),
    );

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(error) => {
            eprintln!("termlet ssh-bridge: failed to start runtime: {error}");
            crate::core::diagnostics::log(
                Level::Error,
                "SSH_BRIDGE",
                format!("ssh-bridge failed to start tokio runtime: {error}"),
            );
            return ExitCode::from(2);
        }
    };

    crate::core::diagnostics::log(
        Level::Info,
        "SSH_BRIDGE",
        "ssh-bridge runtime started, entering run_bridge",
    );

    match runtime.block_on(run_bridge(host)) {
        Ok(_code) => {
            // Unreachable: run_bridge calls std::process::exit() on success.
            std::process::exit(0);
        }
        Err(error) => {
            eprintln!("termlet ssh-bridge error: {error}\r");
            crate::core::diagnostics::log(
                Level::Error,
                "SSH_BRIDGE",
                format!("ssh-bridge exiting with error: {error}"),
            );
            std::process::exit(1);
        }
    }
}

fn load_host(host_id: HostId) -> Result<Host, String> {
    let storage = crate::core::storage::Storage::open()?;
    storage
        .load_host(host_id)?
        .ok_or_else(|| format!("host id {host_id} not found in local store"))
}

async fn run_bridge(host: Host) -> Result<i32, String> {
    let host_label = format!("{}@{}:{}", host.username, host.hostname, host.port);
    crate::core::diagnostics::log(
        Level::Info,
        "SSH_BRIDGE",
        format!("bridge opening SSH session to {host_label}"),
    );

    let jump_key_passphrase = std::env::var("TERMLET_SSH_JUMP_KEY_PASSPHRASE")
        .ok()
        .filter(|passphrase| !passphrase.is_empty())
        .map(SecretString::from);
    let jump_expected_fingerprint = std::env::var("TERMLET_SSH_JUMP_EXPECTED_FINGERPRINT")
        .ok()
        .filter(|fingerprint| !fingerprint.is_empty());
    let session = crate::ssh::open_authenticated_session_with_jump(
        &host,
        jump_key_passphrase,
        jump_expected_fingerprint,
    )
    .await
    .map_err(|e| {
        crate::core::diagnostics::log(
            Level::Error,
            "SSH_BRIDGE",
            format!("bridge failed to open SSH session to {host_label}: {e}"),
        );
        format!("failed to open SSH session to {host_label}: {e}")
    })?;

    crate::core::diagnostics::log(
        Level::Info,
        "SSH_BRIDGE",
        format!("bridge SSH session opened to {host_label}"),
    );

    let channel = session.channel_open_session().await.map_err(|error| {
        crate::core::diagnostics::log(
            Level::Error,
            "SSH_BRIDGE",
            format!("bridge failed to open channel to {host_label}: {error}"),
        );
        format!("failed to open ssh channel: {error}")
    })?;

    crate::core::diagnostics::log(
        Level::Info,
        "SSH_BRIDGE",
        format!("bridge channel opened to {host_label}"),
    );

    let term = std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string());

    // The GUI sets `TERMLET_SHELL_INTEGRATION=0` when the global "Shell
    // integration" setting is off. Absent (or anything but "0") means enabled.
    let shell_integration = std::env::var("TERMLET_SHELL_INTEGRATION").map_or(true, |v| v != "0");
    if !shell_integration {
        crate::core::diagnostics::log(
            Level::Info,
            "SSH_BRIDGE",
            "shell integration disabled by user preference",
        );
    }

    let (cols, rows, px_w, px_h) = current_winsize();

    channel
        .request_pty(
            false,
            &term,
            u32::from(cols),
            u32::from(rows),
            u32::from(px_w),
            u32::from(px_h),
            &[],
        )
        .await
        .map_err(|error| {
            crate::core::diagnostics::log(
                Level::Error,
                "SSH_BRIDGE",
                format!("bridge failed to request pty ({term}, {cols}x{rows}): {error}"),
            );
            format!("failed to request remote pty ({term}, {cols}x{rows}): {error}")
        })?;

    crate::core::diagnostics::log(
        Level::Info,
        "SSH_BRIDGE",
        format!("bridge pty allocated ({term}, {cols}x{rows})"),
    );

    // Without the locale the remote login can warn about an unknown locale and
    // fail to initialise line editing, so keystrokes stop echoing. Best effort:
    // a rejected variable is logged, not fatal.
    for (key, value) in std::env::vars() {
        if key == "LANG"
            || key == "LC_ALL"
            || key == "LC_CTYPE"
            || key == "LC_MESSAGES"
            || key == "LC_NUMERIC"
            || key == "LC_TIME"
            || key == "LC_COLLATE"
            || key == "LC_MONETARY"
        {
            match channel.set_env(false, &key, &value).await {
                Ok(()) => {
                    crate::core::diagnostics::log(
                        Level::Debug,
                        "SSH_BRIDGE",
                        format!("bridge forwarded env {key}={value}"),
                    );
                }
                Err(error) => {
                    crate::core::diagnostics::log(
                        Level::Warn,
                        "SSH_BRIDGE",
                        format!("bridge failed to forward env {key}={value} (continuing): {error}"),
                    );
                }
            }
        }
    }

    // Best-effort shell integration (OSC 7 / OSC 633;D/A).  NEVER fatal: any
    // failure — unsupported shell, exec error, timeout, unsafe path — falls
    // through to a plain `request_shell` below.  Prep runs on its own channels
    // so the main pty channel is only ever used for one exec/shell request.
    let injected = if shell_integration {
        match crate::ssh::shell_integration::prepare_shell_integration(&session).await {
            Ok(Some(si)) => {
                crate::core::diagnostics::log(
                    Level::Debug,
                    "SSH_BRIDGE",
                    format!(
                        "shell integration: kind={:?} script_path={} launch={:?}",
                        si.kind, si.path, si.launch
                    ),
                );
                Some(si)
            }
            Ok(None) => {
                crate::core::diagnostics::log(
                    Level::Warn,
                    "SSH_BRIDGE",
                    "shell integration skipped (no supported shell)",
                );
                None
            }
            Err(error) => {
                crate::core::diagnostics::log(
                    Level::Warn,
                    "SSH_BRIDGE",
                    format!("shell integration prep failed, falling back to plain shell: {error}"),
                );
                None
            }
        }
    } else {
        None
    };

    if let Some(si) = injected {
        channel
            .exec(false, si.launch.as_bytes())
            .await
            .map_err(|error| {
                crate::core::diagnostics::log(
                    Level::Error,
                    "SSH_BRIDGE",
                    format!("bridge failed to exec shell-integration wrapper: {error}"),
                );
                format!("failed to exec remote shell wrapper: {error}")
            })?;
        crate::core::diagnostics::log(
            Level::Info,
            "SSH_BRIDGE",
            format!(
                "bridge remote shell started via exec wrapper (kind={:?})",
                si.kind
            ),
        );
    } else {
        channel.request_shell(false).await.map_err(|error| {
            crate::core::diagnostics::log(
                Level::Error,
                "SSH_BRIDGE",
                format!("bridge failed to request shell: {error}"),
            );
            format!("failed to request remote shell: {error}")
        })?;
        crate::core::diagnostics::log(
            Level::Info,
            "SSH_BRIDGE",
            "bridge remote shell started (plain request_shell)",
        );
    }

    // Raw mode passes keystrokes through unmodified; restored on drop. A
    // failure is ignored on purpose: without a TTY (tests) the session still
    // runs, just degraded.
    let raw = match RawMode::enter() {
        Ok(raw) => Some(raw),
        Err(error) => {
            crate::core::diagnostics::log(
                Level::Warn,
                "SSH_BRIDGE",
                format!("bridge raw mode entry failed ({error}), continuing degraded"),
            );
            None
        }
    };

    #[cfg(unix)]
    let mut resize_watcher = ResizeWatcher::new().map_err(|error| {
        crate::core::diagnostics::log(
            Level::Error,
            "SSH_BRIDGE",
            format!("bridge failed to install SIGWINCH handler: {error}"),
        );
        format!(
            "failed to install SIGWINCH handler on platform {}: {error}",
            std::env::consts::OS
        )
    })?;
    #[cfg(windows)]
    let mut resize_watcher = ResizeWatcher::new();

    crate::core::diagnostics::log(Level::Info, "SSH_BRIDGE", "bridge entering I/O loop");

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut buf = vec![0u8; 8192];
    let mut exit_code: i32 = 0;
    let mut bytes_remote_to_local: u64 = 0;
    let mut bytes_local_to_remote: u64 = 0;

    let mut channel = channel;

    // `TERMLET_SSH_HEALTH_FILE` is set by the GUI for an SSH tab; without it
    // (external-ssh fallback) the probe is skipped. A persistent `interval`
    // rather than a fresh `sleep` keeps the probe on wall-clock time even when
    // I/O keeps the biased select busy, which is exactly when lag shows.
    let health_file = std::env::var_os("TERMLET_SSH_HEALTH_FILE").map(PathBuf::from);
    let mut health_interval = tokio::time::interval(HEALTH_CHECK_INTERVAL);
    health_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut health_unstable = false; // last state reported to the GUI
    let mut health_good_streak = 0u32; // consecutive good samples toward recovery
    if let Some(path) = health_file.as_deref() {
        // Emit the initial "stable" state so the file exists immediately and
        // overwrites any stale state left by a previous connection to the same
        // session id (e.g. after a reconnect).
        report_health(path, false);
    }

    loop {
        tokio::select! {
            biased;

            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        let len = data.len();
                        bytes_remote_to_local += len as u64;

                        stdout.write_all(&data).await
                            .map_err(|error| {
                                crate::core::diagnostics::log(
                                    Level::Error,
                                    "SSH_BRIDGE",
                                    format!(
                                        "bridge write to stdout failed (r→l: {bytes_remote_to_local}): {error}"
                                    ),
                                );
                                format!("write to stdout failed: {error}")
                            })?;
                        let _ = stdout.flush().await;
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        let _ = stdout.write_all(&data).await;
                        let _ = stdout.flush().await;
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        crate::core::diagnostics::log(
                            Level::Info,
                            "SSH_BRIDGE",
                            format!(
                                "bridge received exit status: {exit_status} (r→l: {bytes_remote_to_local}, l→r: {bytes_local_to_remote})"
                            ),
                        );
                        exit_code = exit_status as i32;
                    }
                    Some(ChannelMsg::ExitSignal { signal_name, .. }) => {
                        crate::core::diagnostics::log(
                            Level::Info,
                            "SSH_BRIDGE",
                            format!(
                                "bridge remote killed by signal: {signal_name:?} (r→l: {bytes_remote_to_local}, l→r: {bytes_local_to_remote})"
                            ),
                        );
                        eprintln!("\r\nremote process killed by signal: {signal_name:?}\r");
                        exit_code = 130;
                    }
                    Some(ChannelMsg::Eof) => {
                        crate::core::diagnostics::log(
                            Level::Info,
                            "SSH_BRIDGE",
                            format!(
                                "bridge received EOF (r→l: {bytes_remote_to_local}, l→r: {bytes_local_to_remote})"
                            ),
                        );
                        break;
                    }
                    Some(ChannelMsg::Close) => {
                        crate::core::diagnostics::log(
                            Level::Info,
                            "SSH_BRIDGE",
                            format!(
                                "bridge received Close (r→l: {bytes_remote_to_local}, l→r: {bytes_local_to_remote})"
                            ),
                        );
                        break;
                    }
                    None => {
                        crate::core::diagnostics::log(
                            Level::Warn,
                            "SSH_BRIDGE",
                            format!(
                                "bridge channel.wait() returned None (r→l: {bytes_remote_to_local}, l→r: {bytes_local_to_remote})"
                            ),
                        );
                        break;
                    }
                    Some(_) => {}
                }
            }

            read = stdin.read(&mut buf) => {
                match read {
                    Ok(0) => {
                        crate::core::diagnostics::log(
                            Level::Info,
                            "SSH_BRIDGE",
                            format!("bridge stdin EOF (l→r: {bytes_local_to_remote})"),
                        );
                        let send_result = channel.eof().await;
                        if let Err(e) = &send_result {
                            crate::core::diagnostics::log(
                                Level::Error,
                                "SSH_BRIDGE",
                                format!("bridge channel.eof() failed: {e}"),
                            );
                        }
                    }
                    Ok(n) => {
                        bytes_local_to_remote += n as u64;
                        let send_result = tokio::time::timeout(
                            std::time::Duration::from_secs(10),
                            channel.data(&buf[..n]),
                        )
                        .await;
                        match send_result {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                crate::core::diagnostics::log(
                                    Level::Error,
                                    "SSH_BRIDGE",
                                    format!(
                                        "bridge send to remote failed (wrote {n}B, l→r: {bytes_local_to_remote}): {error}"
                                    ),
                                );
                                return Err(format!(
                                    "send to remote failed (wrote {n}B): {error}"
                                ));
                            }
                            Err(_elapsed) => {
                                crate::core::diagnostics::log(
                                    Level::Warn,
                                    "SSH_BRIDGE",
                                    format!(
                                        "bridge send to remote timed out after 10s (wrote {n}B, l→r: {bytes_local_to_remote})"
                                    ),
                                );
                                return Err(format!(
                                    "send to remote timed out after 10s (wrote {n}B)"
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        crate::core::diagnostics::log(
                            Level::Error,
                            "SSH_BRIDGE",
                            format!(
                                "bridge stdin read failed (l→r: {bytes_local_to_remote}): {error}"
                            ),
                        );
                        return Err(format!(
                            "read from stdin failed: {error}"
                        ));
                    }
                }
            }

            () = resize_watcher.changed() => {
                let (cols, rows, px_w, px_h) = current_winsize();
                crate::core::diagnostics::log(
                    Level::Debug,
                    "SSH_BRIDGE",
                    format!("bridge SIGWINCH: resize to {cols}x{rows} (px: {px_w}x{px_h})"),
                );
                let _ = channel
                    .window_change(u32::from(cols), u32::from(rows), u32::from(px_w), u32::from(px_h))
                    .await;
            }

            // Health probe: a keepalive round trip. Healthy links pong in
            // milliseconds, so data forwarding barely pauses; a degraded link
            // stalls no longer than the send path's own `HEALTH_PING_TIMEOUT`.
            _ = health_interval.tick() => {
                if health_file.is_some() {
                    let started = Instant::now();
                    let pong = tokio::time::timeout(
                        HEALTH_PING_TIMEOUT,
                        session.send_ping(),
                    )
                    .await;
                    let rtt = started.elapsed();
                    // `Ok(Ok(()))` is the only healthy outcome — `Err` covers
                    // both a timeout (session not responding) and a send
                    // failure (session task already dead).
                    let bad = !matches!(pong, Ok(Ok(()))) || rtt >= UNSTABLE_RTT;
                    if bad {
                        health_good_streak = 0;
                        if !health_unstable {
                            health_unstable = true;
                            if let Some(path) = health_file.as_deref() {
                                report_health(path, true);
                            }
                        }
                    } else if rtt < RECOVER_RTT {
                        health_good_streak += 1;
                        if health_unstable && health_good_streak >= GOOD_STREAK {
                            health_unstable = false;
                            health_good_streak = 0;
                            if let Some(path) = health_file.as_deref() {
                                report_health(path, false);
                            }
                        }
                    }
                }
            }

            // Safety net so the loop never parks indefinitely; dead-connection
            // detection belongs to russh's `keepalive_interval`.
            () = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                if session.is_closed() {
                    crate::core::diagnostics::log(
                        Level::Warn,
                        "SSH_BRIDGE",
                        "bridge session closed (detected via select sleep)",
                    );
                    return Err("session closed — connection lost".to_string());
                }
            }
        }
    }

    crate::core::diagnostics::log(
        Level::Info,
        "SSH_BRIDGE",
        format!(
            "bridge I/O loop ended: exit_code={exit_code}, remote->local={bytes_remote_to_local}, local->remote={bytes_local_to_remote}"
        ),
    );

    // Flush remaining output and restore the terminal to cooked mode
    // before we exit.  We deliberately skip `session.disconnect()` because
    // on a dead TCP connection it may hang waiting for an acknowledgement
    // that will never arrive.  The OS will send RST on all open fds when
    // the process terminates anyway.
    let _ = stdout.flush().await;
    drop(raw);
    crate::core::diagnostics::log(
        Level::Info,
        "SSH_BRIDGE",
        "bridge raw mode restored, exiting process",
    );

    // Use `std::process::exit()` rather than returning Ok(exit_code) so that
    // the process terminates immediately without waiting for tokio runtime
    // cleanup or russh session teardown.  This is essential for the parent
    // PTY to receive SIGCHLD promptly and trigger the "SSH disconnected"
    // toast in the GUI.
    std::process::exit(exit_code);
}
