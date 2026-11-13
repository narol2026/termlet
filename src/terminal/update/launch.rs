//! PTY command construction and local/SSH session entity creation.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use iced::Task;
use iced::keyboard;
use iced_term::bindings::{Binding, BindingAction, InputKind};
use iced_term::settings::{BackendSettings, FontSettings, Settings, ThemeSettings};
use iced_term::{Command as TerminalCommand, TermMode};
use secrecy::ExposeSecret;

use super::{App, Message, attach_session_tab_at};
use crate::app::SessionId;
use crate::core::data::{AuthType, Host, HostId, LocalShell, LogCategory};
use crate::core::diagnostics::Level;
use crate::core::theme;
use crate::terminal::{SidebarState, SshSessionSecrets, TerminalSession};
use rust_i18n::t;

type ProcessCommand = (String, Vec<String>, HashMap<String, String>);

pub(super) fn ssh_terminal_settings(
    host: &Host,
    secrets: &SshSessionSecrets,
    term_palette: iced_term::ColorPalette,
    font_size: f32,
    health_path: Option<&Path>,
    shell_integration: bool,
) -> Result<Settings, String> {
    let (program, args, env) = pty_command_for_host(host, secrets, health_path, shell_integration)?;
    Ok(terminal_settings(
        program,
        args,
        env,
        None,
        term_palette,
        font_size,
    ))
}

/// Build the PTY child-process invocation that runs the interactive SSH session.
///
/// On Windows we spawn the dedicated console-subsystem `termlet-bridge`
/// executable next to the GUI binary. The GUI binary stays in the Windows GUI
/// subsystem so Explorer never creates a temporary console window for it.
/// Other platforms retain the single-binary launch path.
///
/// `health_path` is the ssh-bridge's health-status file; it is passed to the
/// child via `TERMLET_SSH_HEALTH_FILE` so the bridge can report connection
/// stability back to the GUI.
///
/// `shell_integration` mirrors the global OSC 7/633 setting: when disabled the
/// bridge child gets `TERMLET_SHELL_INTEGRATION=0` so it skips hook injection
/// and starts a plain remote shell.
pub(super) fn pty_command_for_host(
    host: &Host,
    secrets: &SshSessionSecrets,
    health_path: Option<&Path>,
    shell_integration: bool,
) -> Result<ProcessCommand, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("failed to locate the Termlet executable: {error}"))?;
    pty_command_for_host_at(&exe, host, secrets, health_path, shell_integration)
}

fn pty_command_for_host_at(
    exe: &Path,
    host: &Host,
    secrets: &SshSessionSecrets,
    health_path: Option<&Path>,
    shell_integration: bool,
) -> Result<ProcessCommand, String> {
    let program = bridge_program(exe)?;
    let args = bridge_args(host.id);
    let mut env = HashMap::new();
    if let Some(path) = health_path {
        env.insert(
            "TERMLET_SSH_HEALTH_FILE".to_string(),
            path.to_string_lossy().into_owned(),
        );
    }
    if !shell_integration {
        env.insert("TERMLET_SHELL_INTEGRATION".to_string(), "0".to_string());
    }
    if host.auth_type == AuthType::Key
        && let Some(passphrase) = secrets
            .key_passphrase
            .as_ref()
            .or(host.key_passphrase.as_ref())
            .filter(|passphrase| !passphrase.expose_secret().is_empty())
    {
        env.insert(
            "TERMLET_SSH_KEY_PASSPHRASE".to_string(),
            passphrase.expose_secret().to_string(),
        );
    }
    if let Some(fingerprint) = secrets
        .expected_fingerprint
        .as_deref()
        .or(host.expected_fingerprint.as_deref())
        .filter(|fingerprint| !fingerprint.is_empty())
    {
        env.insert(
            "TERMLET_SSH_EXPECTED_FINGERPRINT".to_string(),
            fingerprint.to_string(),
        );
    }
    if let Some(passphrase) = secrets
        .jump_key_passphrase
        .as_ref()
        .filter(|passphrase| !passphrase.expose_secret().is_empty())
    {
        env.insert(
            "TERMLET_SSH_JUMP_KEY_PASSPHRASE".to_string(),
            passphrase.expose_secret().to_string(),
        );
    }
    if let Some(fingerprint) = secrets
        .jump_expected_fingerprint
        .as_deref()
        .filter(|fingerprint| !fingerprint.is_empty())
    {
        env.insert(
            "TERMLET_SSH_JUMP_EXPECTED_FINGERPRINT".to_string(),
            fingerprint.to_string(),
        );
    }
    crate::core::diagnostics::log(
        Level::Debug,
        "TERM",
        format!(
            "pty_command_for_host: spawning ssh-bridge for host_id={} ({}@{}:{}) via \"{program}\" with args {:?}",
            host.id, host.username, host.hostname, host.port, args
        ),
    );
    Ok((program, args, env))
}

fn bridge_args(host_id: HostId) -> Vec<String> {
    #[cfg(windows)]
    {
        vec![host_id.to_string()]
    }
    #[cfg(not(windows))]
    {
        vec!["--ssh-bridge".to_string(), host_id.to_string()]
    }
}

fn bridge_executable(exe: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        exe.with_file_name("termlet-bridge.exe")
    }
    #[cfg(not(windows))]
    {
        exe.to_path_buf()
    }
}

/// Return a launchable bridge path or explain why a restart is required.
///
/// Linux reports a deleted binary as `<path> (deleted)`. Other platforms can
/// leave the running executable's path pointing at a replacement or a missing
/// file, so both forms are rejected before the terminal backend is created.
fn bridge_program(exe: &Path) -> Result<String, String> {
    let bridge = bridge_executable(exe);
    let program = bridge.to_string_lossy().into_owned();
    if program.ends_with(" (deleted)") || !bridge.is_file() {
        return Err(format!(
            "the Termlet SSH bridge is no longer available at {program}; restart Termlet"
        ));
    }
    Ok(program)
}

pub fn create_local_session(app: &mut App) -> Task<Message> {
    create_local_session_with_dir(app, None)
}

pub fn create_local_session_with_dir(app: &mut App, working_dir: Option<&str>) -> Task<Message> {
    let (session_id, mut task) = create_local_session_entity_with_dir(app, working_dir);

    if app
        .terminal
        .sessions
        .iter()
        .any(|session| session.id == session_id)
    {
        let (_, attach_task) = attach_session_tab_at(app, session_id, usize::MAX);
        task = Task::batch([task, attach_task]);
    }

    task
}

pub fn create_local_session_entity_with_dir(
    app: &mut App,
    working_dir: Option<&str>,
) -> (SessionId, Task<Message>) {
    let session_id = SessionId(app.terminal.next_session_id as u32);
    app.terminal.next_session_id += 1;

    let requested_working_directory = working_dir.filter(|path| !path.is_empty());
    let (working_directory, invalid_working_directory) =
        resolve_local_working_directory(working_dir);
    let label = if requested_working_directory.is_some() && invalid_working_directory.is_none() {
        working_directory.file_name().map_or_else(
            || t!("terminal.local_label").to_string(),
            |name| name.to_string_lossy().to_string(),
        )
    } else {
        t!("terminal.local_label").to_string()
    };
    if let Some(requested) = invalid_working_directory {
        app.add_log(
            LogCategory::Terminal,
            format!(
                "Local terminal working directory is unavailable: requested={requested:?}, using={}",
                working_directory.display()
            ),
        );
    }

    app.add_log(
        LogCategory::Terminal,
        t!("terminal.local_session_opened", label = label.as_str()),
    );

    let (shell, shell_args) = local_shell_command(app.preferences.local_shell);
    let term_palette = theme::term_palette(app.effective_theme_mode());

    // On Linux/macOS inject the OSC 7 + OSC 633 hook script (bash/zsh/fish) so
    // the local shell reports cwd, prompt boundaries and exit codes, same as
    // SSH remotes.  Windows/`cmd.exe` and unsupported shells get no injection.
    // The global `shell_integration` setting (default on) opts out of this.
    #[cfg(unix)]
    let injected = if app.preferences.shell_integration {
        crate::ssh::shell_integration::local_injection(&shell)
    } else {
        None
    };
    // Local hook injection is Unix-only; Windows starts the plain shell.
    #[cfg(not(unix))]
    let injected: Option<(String, Vec<String>, HashMap<String, String>)> = None;

    let settings = local_terminal_settings(
        shell,
        shell_args,
        injected,
        working_directory,
        term_palette,
        app.preferences.terminal_font_size,
    );

    // Local sessions have no ssh-bridge, so no health file.
    create_session_entity(
        app,
        session_id,
        None,
        label,
        settings,
        None,
        crate::terminal::SshSessionSecrets::default(),
    )
}

/// Resolve a local terminal's starting directory on every platform.
///
/// A valid requested directory wins. Missing paths and the absence of a request
/// both fall back to the user's home directory, keeping Unix and Windows spawn
/// behavior consistent. The second return value identifies a rejected request
/// for diagnostics.
fn resolve_local_working_directory(requested: Option<&str>) -> (PathBuf, Option<String>) {
    let home = crate::core::paths::home_dir();
    let Some(requested) = requested.filter(|path| !path.is_empty()) else {
        return (home, None);
    };
    let candidate = PathBuf::from(crate::core::paths::expand_home(requested));
    if candidate.is_dir() {
        (candidate, None)
    } else {
        (home, Some(requested.to_string()))
    }
}

/// Build settings that launch a local shell directly in the requested directory.
fn local_terminal_settings(
    shell: String,
    shell_args: Vec<String>,
    injected: Option<ProcessCommand>,
    working_directory: PathBuf,
    term_palette: iced_term::ColorPalette,
    font_size: f32,
) -> Settings {
    let (program, args, env) = injected.unwrap_or_else(|| (shell, shell_args, HashMap::new()));
    terminal_settings(
        program,
        args,
        env,
        Some(working_directory),
        term_palette,
        font_size,
    )
}

/// Resolve the configured local shell immediately before each new session.
///
/// Windows has no standard user-shell environment variable equivalent to Unix
/// `$SHELL`, so the Profiles preference selects one of a small set of stable
/// executable names. On other platforms the existing `$SHELL` behavior is
/// retained even if a preference was copied from a Windows profile.
fn local_shell_command(preference: LocalShell) -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        match preference {
            LocalShell::CommandPrompt => (default_local_shell(), Vec::new()),
            LocalShell::WindowsPowerShell => {
                ("powershell.exe".to_string(), vec!["-NoLogo".to_string()])
            }
            LocalShell::PowerShell7 => ("pwsh.exe".to_string(), vec!["-NoLogo".to_string()]),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = preference;
        (default_local_shell(), Vec::new())
    }
}

fn default_local_shell() -> String {
    #[cfg(unix)]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
    #[cfg(windows)]
    {
        std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string())
    }
    #[cfg(not(any(unix, windows)))]
    {
        "sh".to_string()
    }
}

pub(super) fn app_terminal_bindings() -> Vec<(Binding<InputKind>, BindingAction)> {
    let shortcut_mod = keyboard::Modifiers::COMMAND;

    vec![
        // Replaces iced_term's default `F1 → ESC OP`: readline would stall on
        // the escape's next byte once the spotlight took the keyboard. The
        // global subscription is F1's only writer, forwarding ESC OP itself in
        // alt-screen mode so vim and htop keep their F1.
        (
            Binding {
                target: InputKind::KeyCode(keyboard::key::Named::F1),
                modifiers: keyboard::Modifiers::empty(),
                terminal_mode_include: TermMode::empty(),
                terminal_mode_exclude: TermMode::empty(),
            },
            BindingAction::Block,
        ),
        // The fullscreen shortcut is handled globally. On Windows and Linux,
        // blocking F11 also prevents its default ESC [23~ sequence from
        // reaching the shell. macOS leaves F11 to Show Desktop.
        #[cfg(target_os = "macos")]
        (
            Binding {
                target: InputKind::Char("f".into()),
                modifiers: keyboard::Modifiers::CTRL | keyboard::Modifiers::COMMAND,
                terminal_mode_include: TermMode::empty(),
                terminal_mode_exclude: TermMode::empty(),
            },
            BindingAction::Block,
        ),
        #[cfg(not(target_os = "macos"))]
        (
            Binding {
                target: InputKind::KeyCode(keyboard::key::Named::F11),
                modifiers: keyboard::Modifiers::empty(),
                terminal_mode_include: TermMode::empty(),
                terminal_mode_exclude: TermMode::empty(),
            },
            BindingAction::Block,
        ),
        // Font size (zoom) is handled globally.
        (
            Binding {
                target: InputKind::Char("=".into()),
                modifiers: shortcut_mod,
                terminal_mode_include: TermMode::empty(),
                terminal_mode_exclude: TermMode::empty(),
            },
            BindingAction::Block,
        ),
        (
            Binding {
                target: InputKind::Char("+".into()),
                modifiers: shortcut_mod,
                terminal_mode_include: TermMode::empty(),
                terminal_mode_exclude: TermMode::empty(),
            },
            BindingAction::Block,
        ),
        (
            Binding {
                target: InputKind::Char("-".into()),
                modifiers: shortcut_mod,
                terminal_mode_include: TermMode::empty(),
                terminal_mode_exclude: TermMode::empty(),
            },
            BindingAction::Block,
        ),
        // App shortcuts must not leak into the PTY.
        (
            Binding {
                target: InputKind::Char("l".into()),
                modifiers: shortcut_mod,
                terminal_mode_include: TermMode::empty(),
                terminal_mode_exclude: TermMode::empty(),
            },
            BindingAction::Block,
        ),
        (
            Binding {
                target: InputKind::Char("n".into()),
                modifiers: shortcut_mod,
                terminal_mode_include: TermMode::empty(),
                terminal_mode_exclude: TermMode::empty(),
            },
            BindingAction::Block,
        ),
        (
            Binding {
                target: InputKind::Char("f".into()),
                modifiers: shortcut_mod,
                terminal_mode_include: TermMode::empty(),
                terminal_mode_exclude: TermMode::empty(),
            },
            BindingAction::Block,
        ),
        (
            Binding {
                target: InputKind::Char("w".into()),
                modifiers: shortcut_mod,
                terminal_mode_include: TermMode::empty(),
                terminal_mode_exclude: TermMode::empty(),
            },
            BindingAction::Block,
        ),
        (
            Binding {
                target: InputKind::Char("d".into()),
                modifiers: shortcut_mod,
                terminal_mode_include: TermMode::empty(),
                terminal_mode_exclude: TermMode::empty(),
            },
            BindingAction::Block,
        ),
    ]
}

pub(super) fn create_session_entity(
    app: &mut App,
    session_id: SessionId,
    host_id: Option<HostId>,
    label: String,
    settings: Settings,
    ssh_health_path: Option<PathBuf>,
    ssh_secrets: crate::terminal::SshSessionSecrets,
) -> (SessionId, Task<Message>) {
    app.add_log(
        LogCategory::Terminal,
        format!(
            "Creating terminal session: id={session_id}, host_id={host_id:?}, label=\"{label}\", program=\"{}\", args={args:?}",
            settings.backend.program,
            args = settings.backend.args,
        ),
    );

    let mut terminal = match iced_term::Terminal::new(u64::from(session_id.0), settings) {
        Ok(terminal) => {
            app.add_log(
                LogCategory::Terminal,
                format!("PTY created successfully: session_id={session_id}, label=\"{label}\""),
            );
            terminal
        }
        Err(e) => {
            app.add_log(LogCategory::Terminal, format!("Failed to create PTY: {e}"));
            return (
                session_id,
                Task::done(Message::ShowErrorToast(format!(
                    "Failed to create terminal: {e}"
                ))),
            );
        }
    };

    terminal.handle(TerminalCommand::AddBindings(app_terminal_bindings()));

    app.terminal.sessions.push(TerminalSession {
        id: session_id,
        host_id,
        ssh_secrets,
        label,
        terminal,
        sidebar: SidebarState::default(),
        has_new_output: false,
        spotlight: None,
        working_directory: None,
        working_directory_synced: false,
        osc_integration_detected: false,
        // Restore the cached command history for this host scope
        // (local terminals share one; each SSH host keeps its own), trimmed to
        // the configured ring size: the file was written under whatever limit
        // was in force at the time.
        recent_commands: {
            let mut history = crate::core::cache::load_command_history(host_id);
            crate::terminal::history::trim_to_limit(
                &mut history,
                app.preferences.command_history_limit,
            );
            history
        },
        disconnected: false,
        disconnect_dismissed: false,
        disconnect_anim: 0.0,
        auto_reconnect: crate::terminal::state::AutoReconnectState::default(),
        connection_unstable: false,
        ssh_health_path,
        term_scrollbar_generation: 0,
        search_query: String::new(),
        search_matches_count: 0,
        search_index: 0,
        search_case_sensitive: false,
        search_whole_word: false,
        search_regex: false,
        show_search: false,
        search_anim: 0.0,
        search_closing: false,
    });

    (session_id, Task::none())
}

pub(crate) fn terminal_settings(
    program: String,
    args: Vec<String>,
    mut env: HashMap<String, String>,
    working_directory: Option<PathBuf>,
    term_palette: iced_term::ColorPalette,
    font_size: f32,
) -> Settings {
    env.insert("TERM".to_string(), "xterm-256color".to_string());
    env.insert(
        "TERMLET_DIAGNOSTIC_LOG".to_string(),
        crate::core::diagnostics::log_path()
            .to_string_lossy()
            .to_string(),
    );

    Settings {
        font: FontSettings {
            size: font_size,
            ..Default::default()
        },
        theme: ThemeSettings::new(Box::new(term_palette)),
        backend: BackendSettings {
            program,
            args,
            env,
            working_directory,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data::{OsType, Protocol};

    fn host_with(auth: AuthType, key_path: Option<&str>) -> Host {
        Host {
            id: HostId(1),
            name: "web".into(),
            hostname: "example.com".into(),
            port: 2222,
            protocol: Protocol::Ssh,
            username: "root".into(),
            auth_type: auth,
            jump_host_id: None,
            identity_id: None,
            key_path: key_path.map(str::to_string),
            key_passphrase: None,
            password: None,
            os_type: OsType::Unknown,
            tags: Vec::new(),
            expected_fingerprint: None,
        }
    }

    // ---- how a session is launched ----

    /// A host that carries a key passphrase, so the launch paths that hand
    /// credentials to the child process have something to hand over.
    fn key_host_with_passphrase() -> Host {
        Host {
            key_passphrase: Some(secrecy::SecretString::from("unlocked".to_string())),
            password: None,
            ..host_with(AuthType::Key, Some("~/.ssh/id_ed25519"))
        }
    }

    fn launchable_bridge_app() -> PathBuf {
        static NEXT_FIXTURE: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);
        let sequence = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = crate::core::test_support::scratch_dir(&format!("bridge-sidecar-{sequence}"));
        let app = dir.join("termlet.exe");
        std::fs::write(&app, b"test app").expect("create the test application");
        std::fs::write(bridge_executable(&app), b"test bridge")
            .expect("create the test bridge executable");
        app
    }

    #[test]
    fn an_ssh_session_launches_the_bridge_from_the_app_directory() {
        let host = host_with(AuthType::Password, None);
        let health = std::env::temp_dir().join("termlet-health-1");
        let health_text = health.to_string_lossy().into_owned();
        let app = launchable_bridge_app();

        let (program, args, env) = pty_command_for_host_at(
            &app,
            &host,
            &SshSessionSecrets::default(),
            Some(&health),
            true,
        )
        .expect("the test bridge is launchable");

        assert_eq!(
            program,
            bridge_program(&app).expect("the test bridge exists"),
            "the bridge is the sidecar that preserves ConPTY handles"
        );
        assert_eq!(args, bridge_args(host.id));
        assert_eq!(
            env.get("TERMLET_SSH_HEALTH_FILE").map(String::as_str),
            Some(health_text.as_str()),
            "the child reports connection health through this file"
        );
        assert!(
            !env.contains_key("TERMLET_SHELL_INTEGRATION"),
            "shell integration is on, and only the off switch is passed along"
        );
        assert!(
            !env.contains_key("TERMLET_SSH_KEY_PASSPHRASE"),
            "a password host has no key passphrase to forward"
        );
    }

    #[test]
    fn the_bridge_is_told_which_credentials_the_host_carries() {
        let host = key_host_with_passphrase();
        let app = launchable_bridge_app();

        let (_, _, env) =
            pty_command_for_host_at(&app, &host, &SshSessionSecrets::default(), None, false)
                .expect("the test bridge is launchable");

        assert_eq!(
            env.get("TERMLET_SHELL_INTEGRATION"),
            Some(&"0".to_string()),
            "the setting reaches the bridge as an off switch"
        );
        assert_eq!(
            env.get("TERMLET_SSH_KEY_PASSPHRASE"),
            Some(&"unlocked".to_string()),
            "the passphrase is forwarded so the connection is never left waiting on a prompt"
        );
        assert!(
            !env.contains_key("TERMLET_SSH_HEALTH_FILE"),
            "a session without a health file says nothing rather than pointing at nothing"
        );
        assert!(
            !env.contains_key("TERMLET_SSH_EXPECTED_FINGERPRINT"),
            "an ordinary known-host connection has no session-only pin to forward"
        );
        assert!(
            !env.contains_key("TERMLET_SSH_JUMP_KEY_PASSPHRASE"),
            "a direct connection has no jump-host passphrase to forward"
        );
    }

    #[test]
    fn the_bridge_receives_a_session_only_trusted_fingerprint() {
        let host = Host {
            expected_fingerprint: Some("SHA256:session-only".to_string()),
            ..host_with(AuthType::Password, None)
        };
        let app = launchable_bridge_app();

        let (_, _, env) =
            pty_command_for_host_at(&app, &host, &SshSessionSecrets::default(), None, true)
                .expect("the test bridge is launchable");

        assert_eq!(
            env.get("TERMLET_SSH_EXPECTED_FINGERPRINT"),
            Some(&"SHA256:session-only".to_string()),
            "the bridge must not fall back to accepting an unknown key"
        );
    }

    #[test]
    fn the_bridge_receives_the_jump_host_passphrase_and_fingerprint() {
        let secrets = SshSessionSecrets {
            jump_key_passphrase: Some(secrecy::SecretString::from("jump-unlocked".to_string())),
            jump_expected_fingerprint: Some("SHA256:jump-session-only".to_string()),
            ..Default::default()
        };
        let app = launchable_bridge_app();

        let (_, _, env) = pty_command_for_host_at(
            &app,
            &host_with(AuthType::Password, None),
            &secrets,
            None,
            true,
        )
        .expect("the test bridge is launchable");

        assert_eq!(
            env.get("TERMLET_SSH_JUMP_KEY_PASSPHRASE"),
            Some(&"jump-unlocked".to_string()),
            "the bridge must be able to unlock the jump host's private key"
        );
        assert_eq!(
            env.get("TERMLET_SSH_JUMP_EXPECTED_FINGERPRINT"),
            Some(&"SHA256:jump-session-only".to_string()),
            "a session-only jump-host trust decision must survive the bridge reload"
        );
    }

    #[test]
    fn a_reconnect_reuses_the_target_key_passphrase_from_the_session() {
        let host = host_with(AuthType::Key, Some("~/.ssh/id_ed25519"));
        assert!(
            host.key_passphrase.is_none(),
            "the host store never persists the passphrase"
        );
        let secrets = SshSessionSecrets {
            key_passphrase: Some(secrecy::SecretString::from("reconnect-secret".to_string())),
            ..Default::default()
        };
        let app = launchable_bridge_app();

        let (_, _, env) = pty_command_for_host_at(&app, &host, &secrets, None, true)
            .expect("the test bridge is launchable");

        assert_eq!(
            env.get("TERMLET_SSH_KEY_PASSPHRASE"),
            Some(&"reconnect-secret".to_string()),
            "an in-place reconnect must not lose the in-memory passphrase"
        );
    }

    #[test]
    fn a_missing_bridge_executable_requires_a_restart() {
        let missing =
            crate::core::test_support::scratch_dir("missing-bridge").join("deleted-termlet");

        let error = bridge_program(&missing).expect_err("a missing binary cannot host the bridge");

        assert!(
            error.contains("restart Termlet"),
            "the diagnostic should tell the user how to recover: {error}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_bridge_program_is_the_console_binary_beside_the_gui() {
        let exe = std::env::current_exe().expect("the test process has an executable path");
        let bridge = exe.with_file_name("termlet-bridge.exe");

        assert_eq!(
            bridge_executable(&exe),
            bridge,
            "the GUI binary must stay in the Windows GUI subsystem"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn the_bridge_program_is_the_running_executable() {
        let exe = std::env::current_exe().expect("the test process has an executable path");
        assert_eq!(bridge_executable(&exe), exe);
    }

    #[test]
    fn terminal_settings_carry_the_theme_font_and_the_diagnostics_log() {
        crate::core::test_support::prepare_data_dir();
        let palette = crate::core::theme::term_palette(crate::core::theme::ThemeMode::Dark);

        let settings = terminal_settings(
            "termlet".to_string(),
            vec!["-p".to_string(), "22".to_string()],
            HashMap::new(),
            None,
            palette,
            17.5,
        );

        assert_eq!(
            settings.backend.env.get("TERM").map(String::as_str),
            Some("xterm-256color"),
            "the child is told which terminal it is talking to"
        );
        assert_eq!(
            settings.backend.env.get("TERMLET_DIAGNOSTIC_LOG"),
            Some(
                &crate::core::diagnostics::log_path()
                    .to_string_lossy()
                    .to_string()
            ),
            "what the PTY reports about itself lands in the app's own log file"
        );
        assert_eq!(settings.font.size, 17.5);
        assert_eq!(settings.backend.program, "termlet");
        assert_eq!(settings.backend.args, vec!["-p", "22"]);
        assert_eq!(settings.backend.working_directory, None);
    }

    #[test]
    fn an_existing_local_working_directory_is_preserved() {
        let directory = crate::core::test_support::scratch_dir("local working directory")
            .join("项目 with spaces");
        std::fs::create_dir_all(&directory).expect("create the requested directory");
        let requested = directory.to_string_lossy().into_owned();

        let (resolved, rejected) = resolve_local_working_directory(Some(&requested));

        assert_eq!(resolved, directory);
        assert_eq!(rejected, None);
    }

    #[test]
    fn a_missing_local_working_directory_falls_back_to_home() {
        let missing =
            crate::core::test_support::scratch_dir("missing-local-working-directory").join("gone");
        let requested = missing.to_string_lossy().into_owned();

        let (resolved, rejected) = resolve_local_working_directory(Some(&requested));

        assert_eq!(resolved, crate::core::paths::home_dir());
        assert_eq!(rejected.as_deref(), Some(requested.as_str()));
    }

    #[test]
    fn an_absent_local_working_directory_uses_home() {
        let (resolved, rejected) = resolve_local_working_directory(None);

        assert_eq!(resolved, crate::core::paths::home_dir());
        assert_eq!(rejected, None);
    }

    #[test]
    fn local_shell_settings_pass_the_directory_to_the_pty() {
        crate::core::test_support::prepare_data_dir();
        let directory = crate::core::test_support::scratch_dir("local-pty-working-directory");
        let palette = crate::core::theme::term_palette(crate::core::theme::ThemeMode::Dark);
        let program = "bash".to_string();
        let args = vec!["--rcfile".to_string(), "/tmp/termlet-hook".to_string()];
        let injected = Some((program.clone(), args.clone(), HashMap::new()));

        let settings = local_terminal_settings(
            "sh".to_string(),
            Vec::new(),
            injected,
            directory.clone(),
            palette,
            15.0,
        );

        assert_eq!(settings.backend.program, program);
        assert_eq!(settings.backend.args, args);
        assert_eq!(settings.backend.working_directory, Some(directory));
    }

    #[test]
    fn a_local_shell_without_injection_runs_directly_in_the_directory() {
        crate::core::test_support::prepare_data_dir();
        let directory = crate::core::test_support::scratch_dir("local-direct-shell-directory");
        let palette = crate::core::theme::term_palette(crate::core::theme::ThemeMode::Dark);

        let settings = local_terminal_settings(
            "cmd.exe".to_string(),
            Vec::new(),
            None,
            directory.clone(),
            palette,
            15.0,
        );

        assert_eq!(settings.backend.program, "cmd.exe");
        assert!(settings.backend.args.is_empty());
        assert_eq!(settings.backend.working_directory, Some(directory));
    }

    #[cfg(windows)]
    #[test]
    fn powershell_choices_launch_without_a_startup_banner() {
        let (program, args) = local_shell_command(LocalShell::WindowsPowerShell);
        assert_eq!(program, "powershell.exe");
        assert_eq!(args, ["-NoLogo"]);

        let (program, args) = local_shell_command(LocalShell::PowerShell7);
        assert_eq!(program, "pwsh.exe");
        assert_eq!(args, ["-NoLogo"]);
    }

    #[test]
    fn the_app_shortcuts_are_blocked_before_they_reach_the_pty() {
        let bindings = app_terminal_bindings();
        let shortcut_mod = keyboard::Modifiers::COMMAND;
        let blocked = |key: InputKind, modifiers| {
            bindings.iter().any(|(binding, action)| {
                binding.target == key
                    && binding.modifiers == modifiers
                    && *action == BindingAction::Block
            })
        };

        // F1 opens the spotlight; if it reached the PTY as well, readline would
        // stall waiting for the rest of the escape sequence.
        assert!(blocked(
            InputKind::KeyCode(keyboard::key::Named::F1),
            keyboard::Modifiers::empty()
        ));

        #[cfg(target_os = "macos")]
        {
            assert!(blocked(
                InputKind::Char("f".into()),
                keyboard::Modifiers::CTRL | keyboard::Modifiers::COMMAND
            ));
            assert!(!blocked(
                InputKind::KeyCode(keyboard::key::Named::F11),
                keyboard::Modifiers::empty()
            ));
        }
        #[cfg(not(target_os = "macos"))]
        assert!(blocked(
            InputKind::KeyCode(keyboard::key::Named::F11),
            keyboard::Modifiers::empty()
        ));

        for shortcut in ["l", "n", "f", "w", "d", "=", "+", "-"] {
            assert!(
                blocked(InputKind::Char(shortcut.into()), shortcut_mod),
                "Ctrl+{shortcut} must not reach the shell"
            );
        }

        assert!(
            !blocked(InputKind::Char("c".into()), shortcut_mod),
            "Ctrl+C belongs to the shell, so it is deliberately left alone"
        );
    }
}
