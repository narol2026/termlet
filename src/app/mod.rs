use std::collections::HashMap;
use std::sync::Arc;

use iced::widget::text_editor;
use iced::window;
use iced::{Point, Subscription, mouse};
use secrecy::SecretString;

pub mod command;
pub mod keybindings;

#[cfg(test)]
use crate::core::data::KeyType;
use crate::core::data::{
    AuthType, COMMAND_HISTORY_DEFAULT_LIMIT, COMMAND_HISTORY_MAX_LIMIT, Host, HostId,
    HostTagFilter, Identity, KeychainEntry, LocalShell, LogCategory, LogEntry, NavItem,
    PortForward, PortForwardKind, Snippet, SnippetPackage,
};
use crate::core::diagnostics::Level;
use crate::core::dropdown::{DropdownState, DropdownTarget};
use crate::core::grid_cursor::GridCursor;
use crate::core::locale::Locale;
use crate::core::message::Message;
use crate::core::storage::Storage;
use crate::core::storage_service::StorageService;
use crate::core::theme::ThemeMode;
use crate::hosts::{FormState, HostsState};
use crate::keychain::{IdentityFormState, KeychainFormState, KeychainState};
use crate::port_forward::password::PasswordState;
use crate::port_forward::trust::TrustState;
use crate::port_forward::{PortForwardFormState, PortForwardState};
use crate::ssh::known_hosts_file;
use keybindings::KeybindingManager;

use crate::known_hosts::KnownHostsDomain;
use crate::sftp::{SftpDomain, SftpPreviewState};
use crate::snippets::{SnippetFormState, SnippetPackageFormState, SnippetsState};
use crate::ssh_connect::SshConnectionState;
use crate::terminal::{
    PendingLockedClose, SidebarState, Tab, TabKind, TerminalDomain, TerminalSession,
    state::disconnect_overlay_visible,
};
use crate::workspace::{PendingWorkspaceApply, WorkspaceManagerState};

#[derive(Debug, Clone, Copy)]
pub struct ScrollbarState {
    pub visible: bool,
    pub last_active: std::time::Instant,
}

impl ScrollbarState {
    pub fn new() -> Self {
        Self {
            visible: false,
            last_active: std::time::Instant::now(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InitialAction {
    None,
    /// A fresh, empty window with no initial terminal session. Unlike
    /// [`InitialAction::None`], it opts out of workspace recovery so a
    /// secondary window never clobbers the primary window's autosave state.
    NewWindow,
    QuickSsh(HostId),
    QuickLocal,
}

// Typed Identifiers
//
// Defined in [`crate::core::ids`]; re-exported here so the existing
// `use crate::app::{SessionId, ...}` import paths keep working.

pub use crate::core::ids::{DraftId, SessionId, SftpSessionId, SplitId, TabId};

/// Static workspace tabs, fixed at the far left of the topbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Vaults,
    Sftp,
}

/// The currently active location: a static workspace or a dynamic tab.
///
/// This is the *tab-level* focus — which topbar entry is selected. It's one of
/// three focus layers: tab level ([`ActiveTab`]), pane level ([`Split::focus`]),
/// and keyboard level (`App::focus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Workspace(Workspace),
    Tab(TabId),
}

/// Domain-owned UI preferences and theme/locale state.
pub struct PreferencesDomain {
    pub locale: Locale,
    pub locale_picker: DropdownState<Locale>,
    pub theme_mode: ThemeMode,
    pub theme_picker: DropdownState<ThemeMode>,
    pub shell_integration_picker: DropdownState<bool>,
    pub local_shell: LocalShell,
    pub local_shell_picker: DropdownState<LocalShell>,
    pub diagnostics_level: crate::core::diagnostics::Level,
    pub diagnostics_picker: DropdownState<crate::core::diagnostics::Level>,
    pub system_dark_mode: bool,
    pub system_theme_detecting: bool,
    pub terminal_font_size: f32,
    pub shell_integration: bool,
    pub osc_watermark_enabled: bool,
    /// How many commands each terminal session keeps in its MRU history. Set in
    /// the Profiles dropdown; clamped here, so a hand-edited database cannot
    /// hand the sidebar a ring it would have to render forever.
    pub command_history_limit: usize,
    pub command_history_limit_picker: DropdownState<usize>,
}

impl PreferencesDomain {
    pub fn from_prefs(prefs: &HashMap<String, String>) -> Self {
        let command_history_limit = prefs
            .get("command_history_limit")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(COMMAND_HISTORY_DEFAULT_LIMIT)
            .clamp(1, COMMAND_HISTORY_MAX_LIMIT);
        Self {
            locale: prefs
                .get("locale")
                .and_then(|v| Locale::from_str(v))
                .unwrap_or(Locale::En),
            locale_picker: DropdownState::new(),
            theme_mode: prefs
                .get("theme_mode")
                .and_then(|v| ThemeMode::from_str(v))
                .unwrap_or(ThemeMode::Dark),
            theme_picker: DropdownState::new(),
            shell_integration_picker: DropdownState::new(),
            local_shell: prefs
                .get("local_shell")
                .and_then(|v| LocalShell::from_str(v))
                .unwrap_or_default(),
            local_shell_picker: DropdownState::new(),
            diagnostics_level: prefs
                .get("diagnostics_level")
                .and_then(|v| crate::core::diagnostics::Level::parse(v))
                .unwrap_or_else(crate::core::diagnostics::default_level),
            diagnostics_picker: DropdownState::new(),
            system_dark_mode: crate::core::theme::detect_system_dark_mode_fast(),
            system_theme_detecting: false,
            terminal_font_size: prefs
                .get("terminal_font_size")
                .and_then(|v| v.parse().ok())
                .unwrap_or(14.0),
            shell_integration: prefs.get("shell_integration").is_none_or(|v| v == "true"),
            osc_watermark_enabled: std::env::var("TERMLET_SHOW_OSC_WATERMARK")
                .is_ok_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")),
            command_history_limit,
            command_history_limit_picker: DropdownState::new(),
        }
    }
}

/// Domain-owned window geometry and shell state.
#[derive(Debug, Clone)]
pub struct WindowState {
    pub cursor_pos: Point,
    pub position: Option<Point>,
    pub scale_factor: f32,
    pub is_maximized: bool,
    /// Windows has made the main window visible behind a DWM cloak.
    #[cfg(target_os = "windows")]
    pub window_prepared: bool,
    /// iced has completed a redraw for the main window.
    #[cfg(target_os = "windows")]
    pub first_frame_presented: bool,
}

impl WindowState {
    pub fn new() -> Self {
        Self {
            cursor_pos: Point::ORIGIN,
            position: None,
            scale_factor: 1.0,
            is_maximized: false,
            #[cfg(target_os = "windows")]
            window_prepared: false,
            #[cfg(target_os = "windows")]
            first_frame_presented: false,
        }
    }
}

pub struct App {
    pub active_nav: NavItem,
    pub active_tab: ActiveTab,
    /// The dropdown that currently owns keyboard focus and its popup.
    pub active_dropdown: Option<DropdownTarget>,
    /// Nested keyboard-focus owners, with the last one receiving input.
    pub focus: crate::focus::FocusStack,
    /// Deferred startup action (e.g. `--quick-ssh`), fired after the bulk
    /// collections have finished loading in the background.
    pub pending_initial_action: InitialAction,
    /// True while the host list is still being read from `SQLite` at startup;
    /// drives a loading indicator instead of the empty state.
    pub hosts_loading: bool,
    pub scroll_offsets: HashMap<&'static str, f32>,
    /// Cached viewport heights keyed by scrollable id, for keep-in-view logic.
    pub scroll_viewport_heights: HashMap<&'static str, f32>,
    pub scrollbar_states: HashMap<&'static str, ScrollbarState>,
    /// Set to true when a form panel closes; triggers scroll position restoration.
    pub needs_scroll_restore: bool,
    /// Relative horizontal scroll offset of the topbar tab strip (0.0 = start, 1.0 = end).
    pub top_tabs_scroll_offset: f32,
    /// Whether the topbar tab strip content overflows the viewport horizontally.
    pub top_tabs_can_scroll: bool,
    /// Absolute horizontal scroll offset of the topbar tab strip (pixels from left).
    /// Cached from the last Scrolled event; used for visibility checks that must
    /// not depend on the relative offset (which breaks when content width changes).
    pub top_tabs_scroll_absolute_x: f32,
    /// Cached viewport width of the topbar tab strip scrollable (from last Scrolled event).
    pub top_tabs_viewport_width: f32,
    /// Armed when `scroll_to_active_tab` is requested while the strip's
    /// viewport width is unknown (the strip has never overflowed). Each
    /// Scrolled event for the strip re-attempts the scroll while armed; the
    /// flag clears once the active tab is visible or no longer scrollable.
    pub top_tabs_pending_scroll: bool,
    pub terminal: TerminalDomain,
    pub hosts: HostsState,
    pub sftp: SftpDomain,
    pub confirm_dialog_anim: f32,
    pub port_forwards: PortForwardState,
    pub port_forward_manager: crate::port_forward::PortForwardManager,
    pub snippets: SnippetsState,
    pub keybindings: KeybindingManager,
    pub show_quit_confirm: bool,
    pub workspace_manager: WorkspaceManagerState,
    pub show_apply_workspace_confirm: bool,
    pub pending_apply_workspace: Option<PendingWorkspaceApply>,
    /// Crash-recovery state for the terminal workspace (transient autosave and
    /// unclean-exit restore prompt).
    pub workspace_recovery: crate::workspace::recovery::State,
    pub pending_locked_close: Option<PendingLockedClose>,
    pub crash_reports: Vec<(u64, String)>,
    pub show_crash_dialog: bool,
    /// Locale, theme, diagnostics, shell and privacy preferences. See
    /// [`PreferencesDomain`] for the domain-owned fields.
    pub preferences: PreferencesDomain,
    pub known_hosts: KnownHostsDomain,
    pub keychain: KeychainState,
    pub toast: Option<Toast>,
    pub toast_seq: u64,
    pub toast_anim: f32,
    pub toast_closing: bool,
    pub command_frequency_prompt: Option<CommandFrequencyPrompt>,
    pub command_frequency_prompt_anim: f32,
    pub command_frequency_prompt_closing: bool,
    /// Generation guard for the auto-dismiss timer. Incremented every time a
    /// new prompt is shown so a stale timer can't dismiss a newer prompt.
    pub command_frequency_prompt_seq: u64,
    pub window: WindowState,
    pub logs: Vec<LogEntry>,
    pub storage_service: Arc<StorageService>,
}

/// Bulk data loaded off the UI thread at startup. `Host` does not derive
/// `Debug` (it can carry secrets), so the `Debug` impl reports counts only.
#[derive(Clone)]
pub struct StartupData {
    pub hosts: Vec<Host>,
    pub port_forwards: Vec<PortForward>,
    pub snippets: Vec<Snippet>,
    pub snippet_packages: Vec<SnippetPackage>,
    pub recent_ids: Vec<HostId>,
    pub keychain_entries: Vec<KeychainEntry>,
    pub identities: Vec<Identity>,
    /// Non-fatal per-load errors, surfaced in the in-app log after applying.
    pub errors: Vec<String>,
}

impl std::fmt::Debug for StartupData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartupData")
            .field("hosts", &self.hosts.len())
            .field("port_forwards", &self.port_forwards.len())
            .field("snippets", &self.snippets.len())
            .field("snippet_packages", &self.snippet_packages.len())
            .field("recent_ids", &self.recent_ids.len())
            .field("keychain_entries", &self.keychain_entries.len())
            .field("identities", &self.identities.len())
            .field("errors", &self.errors.len())
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub kind: ToastKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToastKind {
    Info,
    Warning,
    Error,
}

/// Bottom-right prompt suggesting the user save a frequently-executed command
/// as a snippet.
#[derive(Debug, Clone)]
pub struct CommandFrequencyPrompt {
    /// The exact command text captured from `OSC 633 ; E`.
    pub command: String,
    /// How many times the command has been executed (persisted).
    pub count: u32,
}

/// Read every bulk collection once, off the UI thread. Individual failures are
/// collected (and logged) rather than aborting startup, matching the previous
/// synchronous startup behaviour.
fn load_startup_data(storage: &Arc<Storage>) -> StartupData {
    let mut errors = Vec::new();

    macro_rules! load_or_default {
        ($expr:expr, $label:literal) => {
            match $expr {
                Ok(v) => v,
                Err(e) => {
                    let msg = format!("{}: {e}", $label);
                    crate::core::diagnostics::log(Level::Error, "APP-START", &msg);
                    errors.push(msg);
                    Default::default()
                }
            }
        };
    }

    StartupData {
        hosts: load_or_default!(storage.load_hosts(), "load_hosts"),
        port_forwards: load_or_default!(storage.load_port_forwards(), "load_port_forwards"),
        snippets: load_or_default!(storage.load_snippets(), "load_snippets"),
        snippet_packages: load_or_default!(
            storage.load_snippet_packages(),
            "load_snippet_packages"
        ),
        recent_ids: load_or_default!(storage.load_recent_ids(), "load_recent_ids"),
        keychain_entries: load_or_default!(
            storage.load_keychain_entries(),
            "load_keychain_entries"
        ),
        identities: load_or_default!(storage.load_identities(), "load_identities"),
        errors,
    }
}

impl App {
    pub fn new(initial_action: InitialAction) -> (Self, iced::Task<Message>) {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STARTUP",
            "App::new",
            std::time::Duration::from_millis(500),
        );
        // Storage init & data loading
        //
        // Every load is fallible (corrupted DB, missing file, disk full, …).
        // We keep the app alive by falling back to empty defaults, but
        // accumulate every error and surface them in the in-app log so the
        // user can see what went wrong instead of silently losing data.
        let mut startup_errors: Vec<String> = Vec::new();

        let storage = Arc::new(match Storage::open() {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("storage::open: {e}");
                crate::core::diagnostics::log(Level::Error, "APP-START", &msg);
                startup_errors.push(msg);
                // Degraded mode: keep the app alive on a throwaway in-memory DB.
                // Loads return empty defaults (matching prior behavior).
                Storage::open_in_memory().expect("in-memory storage cannot fail")
            }
        });

        Self::with_storage(&storage, initial_action, startup_errors)
    }

    /// A real app on an in-memory database for the test suite, together with
    /// its startup task — the bulk data load, the known-hosts parse and the
    /// first local SFTP listing.
    ///
    /// Message-driven tests drop the task; the headless runtime tests hand it
    /// to the emulator so the real boot pipeline runs. See
    /// [`crate::core::test_support`] for the scratch data directory this
    /// relies on.
    #[cfg(test)]
    pub(crate) fn for_test_with_boot() -> (Self, iced::Task<Message>) {
        Self::for_test_with_saved_hosts(&Vec::new())
    }

    /// The same, on a database that already holds `hosts`: the vault then
    /// arrives through the real startup load instead of being handed to the
    /// app, which is what a second launch looks like.
    #[cfg(test)]
    pub(crate) fn for_test_with_saved_hosts(hosts: &[Host]) -> (Self, iced::Task<Message>) {
        let storage = Arc::new(Storage::open_in_memory().expect("in-memory storage cannot fail"));
        for host in hosts {
            storage
                .upsert_host(host)
                .expect("seeding an in-memory database cannot fail");
        }
        Self::with_storage(&storage, InitialAction::None, Vec::new())
    }

    /// Build the app on an already-open database.
    ///
    /// Split out of [`App::new`] so tests can drive the real app —
    /// message handlers, views and all — against an in-memory database instead
    /// of the user's `~/.termlet/hosts.sqlite`.
    fn with_storage(
        storage: &Arc<Storage>,
        initial_action: InitialAction,
        mut startup_errors: Vec<String>,
    ) -> (Self, iced::Task<Message>) {
        macro_rules! load_or_default {
            ($expr:expr, $label:literal) => {
                match $expr {
                    Ok(v) => v,
                    Err(e) => {
                        let msg = format!("{}: {e}", $label);
                        crate::core::diagnostics::log(Level::Error, "APP-START", &msg);
                        startup_errors.push(msg);
                        Default::default()
                    }
                }
            };
        }

        let storage_service = StorageService::start(Arc::clone(storage));

        // Preferences stay synchronous: they are tiny and drive theme, locale
        // and diagnostics before the first frame renders. Bulk collections are
        // loaded in the background below.
        let prefs = load_or_default!(storage.load_preferences(), "load_preferences");
        crate::core::diagnostics::configure(
            prefs
                .get("diagnostics_enabled")
                .is_none_or(|v| v != "false"),
            prefs
                .get("diagnostics_level")
                .and_then(|v| crate::core::diagnostics::Level::parse(v))
                .unwrap_or_else(crate::core::diagnostics::default_level),
            prefs.get("diagnostics_path").map(std::path::PathBuf::from),
        );

        let storage_for_load = Arc::clone(storage);
        let preferences = PreferencesDomain::from_prefs(&prefs);
        rust_i18n::set_locale(preferences.locale.to_str());

        let mut app = Self {
            active_nav: NavItem::Hosts,
            active_tab: ActiveTab::Workspace(Workspace::Vaults),
            active_dropdown: None,
            focus: crate::focus::FocusStack::new(),
            pending_initial_action: initial_action,
            hosts_loading: true,
            terminal: TerminalDomain::new(),
            scroll_offsets: HashMap::new(),
            scroll_viewport_heights: HashMap::new(),
            scrollbar_states: HashMap::new(),
            needs_scroll_restore: false,
            top_tabs_scroll_offset: 0.0,
            top_tabs_can_scroll: false,
            top_tabs_scroll_absolute_x: 0.0,
            top_tabs_viewport_width: 0.0,
            top_tabs_pending_scroll: false,
            hosts: HostsState {
                list: Vec::new(),
                form: FormState {
                    visible: false,
                    editing_id: None,
                    name: String::new(),
                    hostname: String::new(),
                    port: "22".into(),
                    username: String::new(),
                    auth: AuthType::Password,
                    jump_host_id: None,
                    jump_picker: DropdownState::new(),
                    key_path: String::new(),
                    password: SecretString::from(String::new()),
                    password_visible: false,
                    identity_picker: DropdownState::new(),
                    selected_identity_id: None,
                    keychain_picker: DropdownState::new(),
                    tags: Vec::new(),
                    tag_picker: DropdownState::new(),
                    original: None,
                },
                form_anim: 1.0,
                form_closing: false,
                hovered_id: None,
                cursor: GridCursor::new(),
                grid_columns: 1,
                search_query: String::new(),
                recent_ids: Vec::new(),
                grid_view: prefs.get("hosts_grid_view").is_none_or(|v| v == "true"),
                delete_pending: None,
                context_menu: None,
                cursor_pos: Point::ORIGIN,
                transfer_dialog: None,
                transfer_menu: DropdownState::new(),
                tag_filter: HostTagFilter::All,
                tag_filter_picker: DropdownState::new(),
            },
            sftp: SftpDomain::new(),
            preferences,
            confirm_dialog_anim: 0.0,
            port_forwards: PortForwardState {
                trust: TrustState::default(),
                password: PasswordState::default(),
                list: Vec::new(),
                runtime: HashMap::new(),
                cursor: GridCursor::new(),
                grid_columns: 1,
                form: PortForwardFormState {
                    visible: false,
                    editing_id: None,
                    name: String::new(),
                    kind: PortForwardKind::Local,
                    host_id: None,
                    bind_address: "127.0.0.1".into(),
                    listen_port: "8080".into(),
                    dest_host: "127.0.0.1".into(),
                    dest_port: "80".into(),
                    host_picker: DropdownState::new(),
                    original: None,
                },
                logs_open: false,
                delete_pending: None,
                grid_view: prefs
                    .get("port_forwards_grid_view")
                    .is_some_and(|v| v == "true"),
                hovered_id: None,
                context_menu: None,
                cursor_pos: Point::ORIGIN,
                search_query: String::new(),
                form_anim: 1.0,
                form_closing: false,
            },
            port_forward_manager: crate::port_forward::PortForwardManager::default(),
            snippets: SnippetsState {
                list: Vec::new(),
                form: SnippetFormState {
                    visible: false,
                    editing_id: None,
                    description: String::new(),
                    script: text_editor::Content::new(),
                    package_id: None,
                    package_picker: DropdownState::new(),
                    original: None,
                },
                delete_pending: None,
                right_show_form: false,
                right_editing_id: None,
                right_description: String::new(),
                right_script: text_editor::Content::new(),
                right_package_id: None,
                right_package_picker: DropdownState::new(),
                packages: Vec::new(),
                package_form: SnippetPackageFormState {
                    visible: false,
                    editing_id: None,
                    name: String::new(),
                    description: String::new(),
                    original: None,
                },
                package_delete_pending: None,
                show_new_menu: false,
                grid_view: prefs.get("snippets_grid_view").is_some_and(|v| v == "true"),
                hovered_package_id: None,
                hovered_snippet_id: None,
                cursor: GridCursor::new(),
                grid_columns: 1,
                context_menu: None,
                cursor_pos: Point::ORIGIN,
                form_anim: 1.0,
                form_closing: false,
            },
            keybindings: KeybindingManager::new(),
            show_quit_confirm: false,
            workspace_manager: WorkspaceManagerState::default(),
            show_apply_workspace_confirm: false,
            pending_apply_workspace: None,
            workspace_recovery: crate::workspace::recovery::State::new(matches!(
                initial_action,
                InitialAction::None
            )),
            pending_locked_close: None,
            crash_reports: Vec::new(),
            show_crash_dialog: false,
            known_hosts: KnownHostsDomain::new(
                prefs
                    .get("known_hosts_grid_view")
                    .is_none_or(|v| v == "true"),
            ),
            keychain: KeychainState {
                list: Vec::new(),
                form: KeychainFormState {
                    visible: false,
                    editing_id: None,
                    name: String::new(),
                    key_type: String::new(),
                    selected_key_type: None,
                    passphrase: SecretString::from(String::new()),
                    passphrase_visible: false,
                    fingerprint: String::new(),
                    public_key: String::new(),
                    path: String::new(),
                    path_manual: false,
                    in_progress: false,
                    original: None,
                },
                delete_pending: None,
                grid_view: prefs.get("keychain_grid_view").is_none_or(|v| v == "true"),
                hovered_id: None,
                cursor: GridCursor::new(),
                grid_columns: 1,
                search_query: String::new(),
                context_menu: None,
                cursor_pos: Point::ORIGIN,
                form_anim: 1.0,
                form_closing: false,
                show_new_menu: false,
                identities: Vec::new(),
                identity_form: IdentityFormState {
                    visible: false,
                    editing_id: None,
                    label: String::new(),
                    username: String::new(),
                    password: SecretString::from(String::new()),
                    password_visible: false,
                    original: None,
                },
                identity_delete_pending: None,
                hovered_identity_id: None,
            },
            toast: None,
            toast_seq: 0,
            toast_anim: 0.0,
            toast_closing: false,
            command_frequency_prompt: None,
            command_frequency_prompt_anim: 0.0,
            command_frequency_prompt_closing: false,
            command_frequency_prompt_seq: 0,
            window: WindowState::new(),
            // The page starts where the last run left off, so the entries a
            // user was looking at are still there after a restart.
            logs: crate::core::activity_log::load_tail(crate::core::data::MAX_LOG_ENTRIES),
            storage_service,
        };

        let crashes = crate::core::crash_reporter::check_new_crashes();
        if !crashes.is_empty() {
            app.crash_reports = crashes;
            app.show_crash_dialog = true;
        }

        // Detect an unclean previous exit, load any recoverable snapshot, and
        // mark this run as running. The prompt is surfaced later, once the host
        // list has finished loading.
        crate::workspace::recovery::start(&mut app);

        crate::core::diagnostics::log(
            Level::Debug,
            "APP-START",
            format!(
                "version={} exe={} cwd={} args={:?} PATH={} SHELL={} TERM={} XDG_CONFIG_HOME={} diagnostic_log={}",
                env!("CARGO_PKG_VERSION"),
                std::env::current_exe()
                    .map_or_else(|e| format!("<error: {e}>"), |p| p.display().to_string()),
                std::env::current_dir()
                    .map_or_else(|e| format!("<error: {e}>"), |p| p.display().to_string()),
                std::env::args().collect::<Vec<_>>(),
                std::env::var("PATH").unwrap_or_else(|_| "<unset>".to_string()),
                std::env::var("SHELL").unwrap_or_else(|_| "<unset>".to_string()),
                std::env::var("TERM").unwrap_or_else(|_| "<unset>".to_string()),
                std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| "<unset>".to_string()),
                crate::core::diagnostics::log_path().display()
            ),
        );

        app.add_log(LogCategory::System, "Termlet started");

        // Surface any storage errors collected during construction so the user
        // sees them in the in-app log instead of silently losing data.
        for err in startup_errors {
            app.add_log(LogCategory::System, format!("Startup storage error: {err}"));
        }

        // Load bulk collections off the UI thread so the first frame paints
        // immediately (empty lists) and data appears as soon as the reads finish.
        let load_task = iced::Task::perform(
            async move {
                tokio::task::spawn_blocking(move || load_startup_data(&storage_for_load))
                    .await
                    .unwrap_or_else(|join_err| {
                        let msg = format!("startup load thread panicked: {join_err}");
                        crate::core::diagnostics::log(Level::Error, "APP-START", &msg);
                        StartupData {
                            hosts: Vec::new(),
                            port_forwards: Vec::new(),
                            snippets: Vec::new(),
                            snippet_packages: Vec::new(),
                            recent_ids: Vec::new(),
                            keychain_entries: Vec::new(),
                            identities: Vec::new(),
                            errors: vec![msg],
                        }
                    })
            },
            Message::StartupDataLoaded,
        );

        // Parse ~/.ssh/known_hosts off the UI thread. On systems with large or
        // heavily-hashed files this keeps the first frame from stalling.
        let known_hosts_task = iced::Task::perform(
            async move {
                tokio::task::spawn_blocking(known_hosts_file::load_all)
                    .await
                    .unwrap_or_else(|join_err| {
                        Err(format!("known_hosts load thread panicked: {join_err}"))
                    })
            },
            |v0| Message::KnownHosts(crate::known_hosts::message::Message::KnownHostsLoaded(v0)),
        );

        // Query the OS colour scheme in the background. The first frame uses
        // `detect_system_dark_mode_fast()` as a non-spawning approximation.
        app.preferences.system_theme_detecting = true;
        let system_theme_task = iced::Task::perform(
            async move {
                tokio::task::spawn_blocking(crate::core::theme::detect_system_dark_mode)
                    .await
                    .unwrap_or(false)
            },
            Message::SystemThemeDetected,
        );

        // The default SFTP session starts with an empty local listing; read
        // the home directory off the UI thread after the first frame.
        let sftp_initial_local_load = iced::Task::done(Message::Sftp(
            crate::sftp::message::Message::SftpRefreshLocal,
        ));

        let task = iced::Task::batch(vec![
            load_task,
            known_hosts_task,
            system_theme_task,
            sftp_initial_local_load,
        ]);

        // AppKit-only: make "New Window" available in the Dock's right-click
        // menu. The winit application delegate is registered by the time the
        // event loop reaches `App::new`, so this installs on the first window.
        #[cfg(target_os = "macos")]
        crate::core::dock_menu::install();

        (app, task)
    }

    pub fn add_log(&mut self, category: LogCategory, message: impl Into<String>) {
        let message = message.into();
        // Two sinks on purpose: the Logs page shows the entry now, and the
        // activity log file keeps it for the next run — the diagnostics log
        // beside it is level-gated, so on a release build it drops this line.
        crate::core::activity_log::record(category, &message);
        crate::core::diagnostics::log(Level::Info, category.label(), &message);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.logs.push(LogEntry {
            timestamp: now,
            category,
            message: message.clone(),
        });
        if self.logs.len() > crate::core::data::MAX_LOG_ENTRIES {
            let excess = self.logs.len() - crate::core::data::MAX_LOG_ENTRIES;
            self.logs.drain(0..excess);
        }
    }

    #[allow(
        dead_code,
        reason = "unused accessor; kept beside the other session queries"
    )]
    pub fn sidebar(&self) -> Option<&SidebarState> {
        let id = self.active_terminal_session_id()?;
        self.terminal
            .sessions
            .iter()
            .find(|s| s.id == id)
            .map(|s| &s.sidebar)
    }

    pub fn sidebar_mut(&mut self) -> Option<&mut SidebarState> {
        let id = self.active_terminal_session_id()?;
        self.terminal
            .sessions
            .iter_mut()
            .find(|s| s.id == id)
            .map(|s| &mut s.sidebar)
    }

    /// Save the active SFTP session's state back into the session store.
    pub fn save_active_sftp_state(&mut self) {
        if let Some(session) = self
            .sftp
            .sessions
            .iter_mut()
            .find(|s| s.id == self.sftp.active_session)
        {
            session.state = self.sftp.state.clone();
        }
    }

    /// Load the given SFTP session's state into the active `sftp` field.
    pub fn load_sftp_state(&mut self, session_id: SftpSessionId) {
        self.save_active_sftp_state();
        if let Some(session) = self.sftp.sessions.iter().find(|s| s.id == session_id) {
            self.sftp.state = session.state.clone();
            self.sftp.active_session = session_id;
        }
    }

    /// Returns true when the active tab is a split pane grid.  When false,
    /// sidebar operations should target the active session directly rather
    /// than iterating over split panes.
    pub fn is_active_session_in_split(&self) -> bool {
        match self.active_tab {
            ActiveTab::Tab(tid) => matches!(
                self.terminal.tab_registry.get(&tid).map(|tab| tab.kind),
                Some(TabKind::Split(_))
            ),
            ActiveTab::Workspace(_) => false,
        }
    }

    /// Allocate a new dynamic tab id, register its kind, append it to the
    /// end of [`TerminalDomain::tabs`], and return the id.
    pub fn allocate_tab_id(&mut self, kind: TabKind) -> TabId {
        let id = TabId(self.terminal.next_tab_id as u32);
        self.terminal.next_tab_id += 1;
        self.terminal.tab_registry.insert(
            id,
            Tab {
                id,
                kind,
                locked: false,
            },
        );
        self.terminal.tabs.push(id);
        id
    }

    /// Remove a dynamic tab from the order list and registry. Does not touch
    /// the underlying entity (session / draft) — callers handle that.
    pub fn remove_tab(&mut self, tab_id: TabId) {
        self.terminal.tabs.retain(|&t| t != tab_id);
        self.terminal.tab_registry.remove(&tab_id);
        self.terminal.tab_width_anims.remove(&tab_id);
    }

    pub fn tab_id_for_session(&self, session_id: SessionId) -> Option<TabId> {
        self.terminal
            .tab_registry
            .iter()
            .find_map(|(tid, tab)| (tab.kind == TabKind::Session(session_id)).then_some(*tid))
    }

    pub fn tab_id_for_draft(&self, draft_id: DraftId) -> Option<TabId> {
        self.terminal
            .tab_registry
            .iter()
            .find_map(|(tid, tab)| (tab.kind == TabKind::Draft(draft_id)).then_some(*tid))
    }

    pub fn tab_id_for_split(&self, split_id: SplitId) -> Option<TabId> {
        self.terminal
            .tab_registry
            .iter()
            .find_map(|(tid, tab)| (tab.kind == TabKind::Split(split_id)).then_some(*tid))
    }

    /// Resolve the tab hosting a terminal session, whether the session is a
    /// standalone tab or a pane inside a split group.
    pub fn tab_id_for_terminal_session(&self, session_id: SessionId) -> Option<TabId> {
        self.tab_id_for_session(session_id).or_else(|| {
            self.split_for_session(session_id)
                .and_then(|sid| self.tab_id_for_split(sid))
        })
    }

    /// Whether the tab with the given id is locked. Keyed by `TabId` so the
    /// close-others / close-right paths (which already hold a `TabId`) can use
    /// it without re-deriving the hosting tab from a session id.
    pub fn tab_is_locked_by_id(&self, tid: TabId) -> bool {
        self.terminal
            .tab_registry
            .get(&tid)
            .is_some_and(|t| t.locked)
    }

    /// Whether the tab hosting `session_id` is locked.
    pub fn tab_is_locked(&self, session_id: SessionId) -> bool {
        self.tab_id_for_terminal_session(session_id)
            .is_some_and(|tid| self.tab_is_locked_by_id(tid))
    }

    /// Set the locked flag on the tab with the given id. No-op when the id is
    /// not registered.
    pub fn set_tab_locked_by_id(&mut self, tid: TabId, locked: bool) {
        if let Some(tab) = self.terminal.tab_registry.get_mut(&tid) {
            tab.locked = locked;
        }
    }

    /// Set the locked flag on the tab hosting `session_id`. No-op when the
    /// session is not found in any terminal tab.
    pub fn set_tab_locked(&mut self, session_id: SessionId, locked: bool) {
        if let Some(tid) = self.tab_id_for_terminal_session(session_id) {
            self.set_tab_locked_by_id(tid, locked);
        }
    }

    /// Number of locked terminal tabs (sessions and split groups). Used by the
    /// quit-confirm dialog to warn before closing locked terminals.
    pub fn locked_terminal_tab_count(&self) -> usize {
        self.terminal
            .tab_registry
            .values()
            .filter(|t| t.locked && matches!(t.kind, TabKind::Session(_) | TabKind::Split(_)))
            .count()
    }

    /// The session id of the active tab, when the active tab is a session or
    /// the focused pane of a split.
    pub fn active_terminal_session_id(&self) -> Option<SessionId> {
        match self.active_tab {
            ActiveTab::Tab(tid) => match self.terminal.tab_registry.get(&tid)?.kind {
                TabKind::Session(sid) => Some(sid),
                TabKind::Split(split_id) => {
                    self.terminal.splits.get(&split_id).and_then(|split| {
                        split.focus.and_then(|pane| split.panes.get(pane).copied())
                    })
                }
                TabKind::Draft(_) => None,
            },
            ActiveTab::Workspace(_) => None,
        }
    }

    /// The disconnected session whose "connection lost" overlay is on top for
    /// the active tab — i.e. the session Enter/Esc should act on. For a split
    /// this mirrors `split_disconnect_overlay`: the focused pane's dialog is
    /// drawn last (top-most), otherwise the last disconnected pane in order.
    /// A dismissed dialog no longer counts: the terminal is interactive again
    /// for selecting/copying, so Enter must not silently reconnect it.
    pub fn active_disconnected_session_id(&self) -> Option<SessionId> {
        let tab_id = match self.active_tab {
            ActiveTab::Tab(tid) => tid,
            ActiveTab::Workspace(_) => return None,
        };

        match self.terminal.tab_registry.get(&tab_id)?.kind {
            TabKind::Session(sid) => self
                .terminal
                .sessions
                .iter()
                .find(|s| s.id == sid && disconnect_overlay_visible(s))
                .map(|s| s.id),
            TabKind::Split(split_id) => {
                let split = self.terminal.splits.get(&split_id)?;
                let mut disconnected: Vec<SessionId> = split
                    .panes
                    .iter()
                    .filter_map(|(_, sid)| {
                        self.terminal
                            .sessions
                            .iter()
                            .find(|s| s.id == *sid && disconnect_overlay_visible(s))
                            .map(|_| *sid)
                    })
                    .collect();

                if disconnected.is_empty() {
                    return None;
                }

                if let Some(focused) = split.focus.and_then(|pane| split.panes.get(pane).copied())
                    && let Some(pos) = disconnected.iter().position(|sid| *sid == focused)
                {
                    let focused = disconnected.remove(pos);
                    disconnected.push(focused);
                }

                disconnected.last().copied()
            }
            TabKind::Draft(_) => None,
        }
    }

    /// The draft id of the active tab, when the active tab is a draft.
    pub fn active_draft_id(&self) -> Option<DraftId> {
        match self.active_tab {
            ActiveTab::Tab(tid) => match self.terminal.tab_registry.get(&tid)?.kind {
                TabKind::Draft(did) => Some(did),
                TabKind::Session(_) | TabKind::Split(_) => None,
            },
            ActiveTab::Workspace(_) => None,
        }
    }

    pub fn activate_tab(&mut self, tab_id: TabId) {
        crate::cursor::close_active_dropdown(self);
        self.close_page_panels();
        self.active_tab = ActiveTab::Tab(tab_id);
    }

    /// Clears panels tied to the page or session being left.
    pub(crate) fn close_page_panels(&mut self) {
        self.hosts.form.visible = false;
        self.hosts.form_closing = false;
        self.keychain.form.visible = false;
        self.keychain.identity_form.visible = false;
        self.keychain.form_closing = false;
        self.port_forwards.form.visible = false;
        self.port_forwards.form_closing = false;
        self.snippets.form.visible = false;
        self.snippets.package_form.visible = false;
        self.snippets.form_closing = false;
        self.known_hosts.detail_open = false;
        self.known_hosts.detail_closing = false;
        self.terminal.command_info = None;
    }

    /// After the active tab `tab_id` is closed, activate the tab immediately to
    /// its left (else the first remaining tab, else Vaults). Call this *before*
    /// removing `tab_id` from [`TerminalDomain::tabs`].
    pub fn activate_neighbor_after_close(&mut self, tab_id: TabId) {
        if self.active_tab != ActiveTab::Tab(tab_id) {
            return;
        }
        crate::cursor::close_active_dropdown(self);
        let pos = self.terminal.tabs.iter().position(|&t| t == tab_id);
        let next = match pos {
            Some(p) if p > 0 => self.terminal.tabs.get(p - 1).copied(),
            _ => self.terminal.tabs.iter().copied().find(|&t| t != tab_id),
        };
        self.active_tab = match next {
            Some(tid) => ActiveTab::Tab(tid),
            None => ActiveTab::Workspace(Workspace::Vaults),
        };
    }

    /// True when any draft's SSH connection is mid-handshake (drives the
    /// per-frame animation and the 800 ms `SshTick` poll).
    pub fn any_ssh_connecting(&self) -> bool {
        self.terminal
            .drafts
            .values()
            .any(|d| d.ssh.attempt.state == SshConnectionState::Connecting)
    }

    pub fn split_for_session(&self, session_id: SessionId) -> Option<SplitId> {
        self.terminal.splits.iter().find_map(|(split_id, split)| {
            split
                .panes
                .iter()
                .any(|(_, sid)| *sid == session_id)
                .then_some(*split_id)
        })
    }

    /// The session ids of every pane in the active split, when the active tab is
    /// a split. Empty otherwise. Sidebar / command-history state is mirrored to
    /// each of these sessions, so callers collect the ids once and mutate
    /// `terminal_sessions` without holding a borrow of `splits`.
    pub fn active_split_pane_sessions(&self) -> Vec<SessionId> {
        let ActiveTab::Tab(tid) = self.active_tab else {
            return Vec::new();
        };
        let Some(TabKind::Split(split_id)) = self.terminal.tab_registry.get(&tid).map(|t| t.kind)
        else {
            return Vec::new();
        };
        self.terminal
            .splits
            .get(&split_id)
            .map(|split| split.panes.iter().map(|(_, sid)| *sid).collect())
            .unwrap_or_default()
    }

    /// Allocate and return a unique split identifier.
    pub fn allocate_split_id(&mut self) -> SplitId {
        let id = SplitId(self.terminal.next_split_id as u32);
        self.terminal.next_split_id += 1;
        id
    }

    /// Return the effective theme mode, resolving [`ThemeMode::Auto`] to
    /// `Dark` or `Light` based on the cached OS preference.
    pub fn effective_theme_mode(&self) -> ThemeMode {
        self.preferences
            .theme_mode
            .resolve(self.preferences.system_dark_mode)
    }

    /// Queue a storage operation on the background IO worker and turn it into
    /// an `iced::Task`, so an `update` function never writes storage directly.
    pub fn storage_task<R, F, M>(&self, f: F, mapper: M) -> iced::Task<Message>
    where
        R: Send + 'static,
        F: FnOnce(&Storage) -> Result<R, String> + Send + 'static,
        M: FnOnce(Result<R, String>) -> Message + Send + 'static,
    {
        iced::Task::perform(self.storage_service.submit(f), mapper)
    }

    /// Queue a unit-result storage write and surface a storage error as an
    /// error toast.  Success is deliberately silent.
    pub fn storage_write_task<F>(&self, f: F) -> iced::Task<Message>
    where
        F: FnOnce(&Storage) -> Result<(), String> + Send + 'static,
    {
        self.storage_task(f, |result| match result {
            Ok(()) => Message::Noop,
            Err(err) => Message::ShowErrorToast(err),
        })
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // `subscription()` runs every frame, so a slow rebuild is worth a line.
        let _sub_guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "SUB",
            "subscription",
            std::time::Duration::from_millis(4),
        );

        let mut subs: Vec<Subscription<Message>> = Vec::new();

        subs.push(iced::event::listen_with(|event, status, _window| {
            if !forwards_to_global_handler(&event, status) {
                return None;
            }

            match &event {
                iced::Event::Keyboard(_) => Some(Message::TermEvent(event)),
                _ if status == iced::event::Status::Ignored => Some(Message::TermEvent(event)),
                _ => None,
            }
        }));

        subs.push(window::resize_events().map(|(_id, size)| Message::WindowResized(size)));

        subs.push(iced::event::listen_with(
            |event, _status, _window| match event {
                iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Message::CursorPositionChanged(position))
                }
                iced::Event::Window(window::Event::Opened { position, .. }) => {
                    Some(Message::WindowPositionChanged(position))
                }
                iced::Event::Window(window::Event::Moved(position)) => {
                    Some(Message::WindowPositionChanged(Some(position)))
                }
                iced::Event::Window(window::Event::Rescaled(scale_factor)) => {
                    Some(Message::WindowScaleFactorChanged(scale_factor))
                }
                iced::Event::Window(window::Event::FileDropped(path)) => Some(Message::Sftp(
                    crate::sftp::message::Message::SftpExternalFilesDropped(vec![path]),
                )),
                iced::Event::Window(window::Event::FileHovered(path)) => Some(Message::Sftp(
                    crate::sftp::message::Message::SftpExternalFilesHovered(vec![path]),
                )),
                iced::Event::Window(window::Event::FilesHoveredLeft) => Some(Message::Sftp(
                    crate::sftp::message::Message::SftpExternalFilesHoverLeft,
                )),
                iced::Event::Window(window::Event::CloseRequested) => Some(Message::WindowClose),
                _ => None,
            },
        ));

        #[cfg(target_os = "windows")]
        subs.push(iced::event::listen_with(
            |event, _status, _window| match event {
                iced::Event::Window(window::Event::Opened { .. }) => {
                    Some(Message::SyncWindowChrome)
                }
                _ => None,
            },
        ));

        #[cfg(target_os = "windows")]
        if !self.window.first_frame_presented {
            subs.push(iced::event::listen_raw(
                |event, _status, _window| match event {
                    iced::Event::Window(window::Event::RedrawRequested(_)) => {
                        Some(Message::RevealWindowFirstFrame)
                    }
                    _ => None,
                },
            ));
        }

        // ButtonReleased rather than ButtonPressed, so the path bar's own
        // buttons fire their on_press handlers first and cancel edit mode; a
        // premature cancel would let the breadcrumb button re-enter it.
        subs.push(iced::event::listen_with(
            |event, _status, _window| match event {
                iced::Event::Mouse(mouse::Event::ButtonReleased(_)) => Some(Message::Sftp(
                    crate::sftp::message::Message::SftpPathEditBlurCheck,
                )),
                _ => None,
            },
        ));

        subs.push(iced::event::listen_with(
            |event, _status, _window| match event {
                iced::Event::Mouse(mouse::Event::ButtonReleased(_)) => Some(Message::ObserveFocus),
                _ => None,
            },
        ));

        // The tab rename input submits on focus loss, same pattern as above.
        subs.push(iced::event::listen_with(
            |event, _status, _window| match event {
                iced::Event::Mouse(mouse::Event::ButtonReleased(_)) => Some(Message::Terminal(
                    crate::terminal::message::Message::TermRenameBlurCheck,
                )),
                _ => None,
            },
        ));

        for session in &self.terminal.sessions {
            subs.push(
                session.terminal.subscription().map(|v0| {
                    Message::Terminal(crate::terminal::message::Message::IcedTermEvent(v0))
                }),
            );
        }

        // A thread-local set deduplicates the per-session log line, so only the
        // first appearance of a session in each subscription cycle is logged.
        {
            use std::cell::RefCell;
            use std::collections::HashSet;
            thread_local! {
                static SEEN_SESSIONS: RefCell<HashSet<SessionId>> = RefCell::new(HashSet::new());
            }
            SEEN_SESSIONS.with(|seen| {
                let mut seen = seen.borrow_mut();
                for s in &self.terminal.sessions {
                    if seen.insert(s.id) {
                        eprintln!(
                            "[APP-SUB {:?}] subscription() wired terminal session_id={}, label=\"{}\"",
                            std::time::Instant::now(),
                            s.id,
                            s.label
                        );
                        crate::core::diagnostics::log(
                            Level::Debug,
                            "APP-SUB",
                            format!(
                                "subscription() wired terminal session_id={}, label=\"{}\"",
                                s.id, s.label
                            ),
                        );
                    }
                }
                // Pruned each cycle, so the set cannot grow without bound.
                let active: Vec<SessionId> =
                    self.terminal.sessions.iter().map(|s| s.id).collect();
                seen.retain(|id| active.contains(id));
            });
        };

        if self.hosts.form.visible
            && (self.hosts.form_anim < 1.0 || self.hosts.form_closing)
            || self
                .terminal
                    .sessions
                .iter()
                .any(|s| s.sidebar.show && s.sidebar.anim < 1.0)
            || self.terminal.sessions.iter().any(|s| s.sidebar.closing)
            || (self.keychain.form.visible || self.keychain.identity_form.visible)
                && (self.keychain.form_anim < 1.0 || self.keychain.form_closing)
            || self.port_forwards.form.visible
                && (self.port_forwards.form_anim < 1.0 || self.port_forwards.form_closing)
            || (self.snippets.form.visible || self.snippets.package_form.visible)
                && (self.snippets.form_anim < 1.0 || self.snippets.form_closing)
            || self.known_hosts.detail_open
                && (self.known_hosts.detail_anim < 1.0 || self.known_hosts.detail_closing)
            || self.any_ssh_connecting()
            || self.sftp.state.left.pane.loading
            || self.sftp.state.right.pane.loading
            || self
                .sftp
                .state.preview
                .as_ref()
                .is_some_and(|p| matches!(p.state, SftpPreviewState::Loading))
            || self.sftp.state.transfer_progress.is_some()
            || !self.sftp.state.external_drag_paths.is_empty()
            || self.sftp.state.left.pane.filter_anim
                != if self.sftp.state.left.pane.filter_open {
                    1.0
                } else {
                    0.0
                }
            || self.sftp.state.right.pane.filter_anim
                != if self.sftp.state.right.pane.filter_open {
                    1.0
                } else {
                    0.0
                }
            || self.toast.is_some()
                && (self.toast_anim < 1.0 || self.toast_closing)
            || self.command_frequency_prompt.is_some()
                && (self.command_frequency_prompt_anim < 1.0
                    || self.command_frequency_prompt_closing)
            || (self.sftp.sidebar_show && self.sftp.sidebar_anim < 1.0)
            || self.sftp.sidebar_closing
            || (self.has_any_dialog_open() && self.confirm_dialog_anim < 1.0)
            || (!self.has_any_dialog_open() && self.confirm_dialog_anim > 0.0)
            // Keep the shimmer animating while an age encrypt/decrypt runs
            // (scrypt KDF ~1s); the dialog is already fully open by then.
            || self
                .hosts
                .transfer_dialog
                .as_ref()
                .is_some_and(|d| d.busy)
            || self.tab_width_anims_active()
            || self
                .terminal
                    .sessions
                .iter()
                .any(|s| s.spotlight.as_ref().is_some_and(|p| p.anim < 1.0))
            || self
                .terminal
                    .sessions
                .iter()
                .any(|s| disconnect_overlay_visible(s) && s.disconnect_anim < 1.0)
            || self
                .terminal
                    .sessions
                .iter()
                .any(|s| s.show_search && s.search_anim < 1.0)
            || self.terminal.sessions.iter().any(|s| s.search_closing)
        {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::AnimTick),
            );
        }

        if self.any_ssh_connecting() {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(800))
                    .map(|_| Message::SshConnect(crate::ssh_connect::message::Message::SshTick)),
            );
        }

        // While any SSH terminal session is live, poll its ssh-bridge
        // health-status file so the topbar can show a yellow dot when the
        // connection degrades (see `TerminalSession::connection_unstable`).
        if self
            .terminal
            .sessions
            .iter()
            .any(|s| s.ssh_health_path.is_some())
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(1))
                    .map(|_| Message::Terminal(crate::terminal::message::Message::SshHealthTick)),
            );
        }

        // When Auto theme is active, poll for OS colour-scheme changes so the
        // UI switches between dark / light without a restart.
        if self.preferences.theme_mode == ThemeMode::Auto {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(3))
                    .map(|_| Message::SystemThemeCheck),
            );
        }

        // New windows are separate processes that share the same SQLite
        // database. Poll only the theme preference so a change made in one
        // window is reflected in every other already-open window.
        subs.push(
            iced::time::every(std::time::Duration::from_secs(2))
                .map(|_| Message::ThemePreferenceSyncCheck),
        );

        // Primary windows periodically persist their terminal layout to the
        // transient crash-recovery snapshot. The handler serializes and
        // compares bytes, so unchanged layouts are not rewritten.
        if self.workspace_recovery.enabled {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(10)).map(|_| {
                    Message::WorkspaceRecovery(crate::workspace::recovery::Message::AutoSaveTick)
                }),
            );
        }

        if self.scrollbar_states.values().any(|s| s.visible) {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(200))
                    .map(|_| Message::ScrollbarVisibilityTimer),
            );
        }

        Subscription::batch(subs)
    }

    #[allow(
        dead_code,
        reason = "unused accessor; kept beside the other session queries"
    )]
    pub fn active_session(&self) -> Option<&TerminalSession> {
        let id = self.active_terminal_session_id()?;
        self.terminal.sessions.iter().find(|s| s.id == id)
    }

    /// Returns the command metadata entry shown for the active session.
    pub(crate) fn active_command_info(&self) -> Option<&crate::core::cache::CommandHistoryEntry> {
        let info = self.terminal.command_info?;
        if Some(info.session_id) != self.active_terminal_session_id() {
            return None;
        }
        self.terminal
            .sessions
            .iter()
            .find(|session| session.id == info.session_id)?
            .recent_commands
            .get(info.index)
    }

    /// Returns `true` when any modal / confirm dialog is currently open.
    /// Used to prevent stacking multiple dialogs (e.g. quit confirm on top
    /// of a host-delete confirm).
    pub fn has_any_dialog_open(&self) -> bool {
        crate::overlay::active(self).is_some()
    }

    pub fn is_scrollbar_visible(&self, id: &str) -> bool {
        self.scrollbar_states.get(id).is_some_and(|s| s.visible)
    }

    /// Visible dynamic tab ids in left-to-right order. Every tab in
    /// [`TerminalDomain::tabs`] maps one-to-one to an entity (session, split, or
    /// draft), so nothing is hidden — this filters out ids missing from the
    /// registry.
    pub fn visible_tab_ids(&self) -> Vec<TabId> {
        self.terminal
            .tabs
            .iter()
            .copied()
            .filter(|tid| self.terminal.tab_registry.contains_key(tid))
            .collect()
    }

    /// Seed the tab width animation map with current displayed widths.
    /// Call this BEFORE changing `active_tab` so the map captures the
    /// "before" state; the `AnimTick` handler will then lerp toward new targets.
    ///
    /// Also resets `top_tabs_can_scroll` — the iced scrollable only fires
    /// `Scrolled` when content *overflows*, so we conservatively clear the
    /// flag here.  If tabs still overflow after the mutation the next
    /// `Scrolled` event will set it back to `true`.
    pub fn seed_tab_width_anims(&mut self) {
        self.top_tabs_can_scroll = false;
        for tid in self.visible_tab_ids() {
            let is_active = self.active_tab == ActiveTab::Tab(tid);
            let width = if is_active {
                (crate::views::TAB_MIN_WIDTH + 40.0).min(crate::views::MAX_TAB_WIDTH)
            } else {
                crate::views::TAB_MIN_WIDTH
            };
            self.terminal.tab_width_anims.insert(tid, width);
        }
    }

    /// Returns `true` when any tab's animated width hasn't converged to its
    /// target. Used by `subscription()` to request `AnimTick` frames.
    fn tab_width_anims_active(&self) -> bool {
        self.visible_tab_ids().into_iter().any(|tid| {
            let is_active = self.active_tab == ActiveTab::Tab(tid);
            let target = if is_active {
                (crate::views::TAB_MIN_WIDTH + 40.0).min(crate::views::MAX_TAB_WIDTH)
            } else {
                crate::views::TAB_MIN_WIDTH
            };
            let current = self
                .terminal
                .tab_width_anims
                .get(&tid)
                .copied()
                .unwrap_or(target);
            (target - current).abs() > 0.5
        })
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if self.workspace_recovery.enabled
            && let Err(error) = crate::workspace::recovery::mark_clean_shutdown()
        {
            crate::core::diagnostics::log(
                Level::Warn,
                "RECOVERY",
                format!("mark clean shutdown failed: {error}"),
            );
        }
        self.port_forward_manager.shutdown_all();
    }
}

/// Route vertical navigation to the global handler only when no widget owns it.
///
/// Collection cursors capture arrow keys after receiving focus. Text inputs
/// ignore Up and Down, so their focused search fields still fall through to
/// the app-level host picker routing without applying a capture twice.
fn forwards_to_global_handler(event: &iced::Event, status: iced::event::Status) -> bool {
    let is_vertical_navigation = matches!(
        event,
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(
                iced::keyboard::key::Named::ArrowUp | iced::keyboard::key::Named::ArrowDown
            ),
            ..
        })
    );

    !is_vertical_navigation || status == iced::event::Status::Ignored
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data::{OsType, Protocol, RightSidebarTab};
    use crate::core::forms::FormDraft;
    use crate::core::grid_cursor::GridDirection;
    use crate::core::ids::{
        IdentityId, KeychainEntryId, PortForwardId, SnippetId, SnippetPackageId,
    };
    use crate::port_forward::PortForwardStatus;
    use crate::port_forward::state::PortForwardRuntime;
    use crate::sftp::{SftpSession, SftpState};
    use crate::terminal::state::{SidebarCursors, disconnect_notice_visible};

    #[test]
    fn captured_vertical_navigation_is_not_routed_to_the_global_handler_twice() {
        let down = iced_test::simulator::press_key(
            iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown),
            None,
        );
        assert!(!forwards_to_global_handler(
            &down,
            iced::event::Status::Captured
        ));
        assert!(forwards_to_global_handler(
            &down,
            iced::event::Status::Ignored
        ));

        let enter = iced_test::simulator::press_key(
            iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
            None,
        );
        assert!(forwards_to_global_handler(
            &enter,
            iced::event::Status::Captured
        ));
    }

    fn host(id: u32, name: &str, hostname: &str, username: &str, tags: &[&str]) -> Host {
        Host {
            id: HostId(id),
            name: name.to_string(),
            hostname: hostname.to_string(),
            port: 22,
            protocol: Protocol::Ssh,
            username: username.to_string(),
            auth_type: AuthType::Password,
            jump_host_id: None,
            identity_id: None,
            key_path: None,
            key_passphrase: None,
            password: Some(SecretString::from("pw")),
            os_type: OsType::Unknown,
            tags: tags.iter().map(std::string::ToString::to_string).collect(),
            expected_fingerprint: None,
        }
    }

    /// A vault list with nothing filtered or selected, so each test only has to
    /// state the fields it cares about.
    fn hosts_state(list: Vec<Host>) -> HostsState {
        HostsState {
            list,
            hovered_id: None,
            cursor: GridCursor::new(),
            grid_columns: 1,
            search_query: String::new(),
            recent_ids: Vec::new(),
            grid_view: true,
            delete_pending: None,
            context_menu: None,
            cursor_pos: Point::ORIGIN,
            transfer_dialog: None,
            transfer_menu: DropdownState::new(),
            tag_filter: HostTagFilter::All,
            tag_filter_picker: DropdownState::new(),
            ..HostsState::default()
        }
    }

    /// The two-host fixture the filtering tests search over.
    fn sample_vault() -> HostsState {
        hosts_state(vec![
            host(1, "Web", "10.0.0.1", "root", &["prod", "eu-west"]),
            host(2, "db", "ten.example.com", "deploy", &["prod"]),
            host(3, "scratch", "127.0.0.1", "dev", &[]),
        ])
    }

    #[test]
    fn dismissing_the_connection_lost_dialog_keeps_the_session_disconnected() {
        // Fresh drop: the blocking dialog is visible.
        assert!(disconnect_notice_visible(true, false));
        // Dismissed: the terminal is readable again, but the session stays
        // disconnected so the tab status and Reconnect action remain.
        assert!(!disconnect_notice_visible(true, true));
        // A live session never shows the disconnect notice.
        assert!(!disconnect_notice_visible(false, false));
    }

    #[test]
    fn host_search_matches_name_host_and_user_case_insensitively() {
        let mut state = sample_vault();

        assert_eq!(state.filtered().len(), 3, "an empty query filters nothing");

        state.search_query = "web".to_string();
        assert_eq!(names(state.filtered()), ["Web"], "name match is folded");

        state.search_query = "TEN.EXAMPLE".to_string();
        assert_eq!(names(state.filtered()), ["db"], "hostname match is folded");

        state.search_query = "DEV".to_string();
        assert_eq!(
            names(state.filtered()),
            ["scratch"],
            "username match is folded"
        );

        state.search_query = "nothing-here".to_string();
        assert!(state.filtered().is_empty());
    }

    /// The list order a search returns, so assertions read as expectations
    /// rather than as index arithmetic.
    fn names(hosts: Vec<&Host>) -> Vec<&str> {
        hosts.into_iter().map(|host| host.name.as_str()).collect()
    }

    #[test]
    fn tag_filter_narrows_the_list_and_composes_with_the_search_box() {
        let mut state = sample_vault();

        state.tag_filter = HostTagFilter::Tag("prod".to_string());
        assert_eq!(names(state.filtered()), ["Web", "db"]);

        // Untagged hosts are reachable only through "All".
        state.tag_filter = HostTagFilter::Tag(String::new());
        assert!(state.filtered().is_empty());

        state.tag_filter = HostTagFilter::Tag("prod".to_string());
        state.search_query = "deploy".to_string();
        assert_eq!(
            names(state.filtered()),
            ["db"],
            "filter and query both apply"
        );
    }

    #[test]
    fn recently_used_hosts_sort_to_the_top_of_the_list() {
        let mut state = sample_vault();
        assert_eq!(names(state.filtered()), ["Web", "db", "scratch"]);

        state.recent_ids = vec![HostId(3), HostId(1)];
        assert_eq!(names(state.filtered()), ["scratch", "Web", "db"]);

        // `sorted_by_recency` keeps the same order without a search box.
        assert_eq!(names(state.sorted_by_recency()), ["scratch", "Web", "db"]);
        // `recent()` follows the recency list itself, not the host list order.
        assert_eq!(names(state.recent()), ["scratch", "Web"]);
    }

    #[test]
    fn recording_a_recent_host_moves_it_to_the_front_and_caps_the_list() {
        let mut state = sample_vault();

        state.record_recent(HostId(2));
        state.record_recent(HostId(3));
        assert_eq!(state.recent_ids, vec![HostId(3), HostId(2)]);

        // Re-connecting to an already-listed host refreshes its position
        // instead of adding a duplicate.
        state.record_recent(HostId(2));
        assert_eq!(state.recent_ids, vec![HostId(2), HostId(3)]);

        for id in 10..20 {
            state.record_recent(HostId(id));
        }
        assert_eq!(state.recent_ids.len(), 8, "the recency list stays bounded");
        assert_eq!(state.recent_ids[0], HostId(19));
    }

    #[test]
    fn recent_hosts_skip_ids_that_are_no_longer_saved() {
        let mut state = sample_vault();
        state.recent_ids = vec![HostId(1), HostId(404), HostId(3)];

        assert_eq!(names(state.recent()), ["Web", "scratch"]);
    }

    #[test]
    fn all_tags_are_deduplicated_sorted_and_free_of_blanks() {
        let mut state = hosts_state(vec![
            host(1, "a", "a", "a", &["prod", "EU-west", ""]),
            host(2, "b", "b", "b", &["prod", "eu-west", "dev"]),
            host(3, "c", "c", "c", &[]),
        ]);

        // Deduplication is exact while the ordering is case-insensitive, so two
        // spellings of one tag stay two entries instead of silently merging
        // into a single filter.
        assert_eq!(state.all_tags(), ["dev", "EU-west", "eu-west", "prod"]);

        state.list.clear();
        assert!(state.all_tags().is_empty());
    }

    /// The host form, filled in far enough to save, before the test changes the
    /// one field it is about.
    fn complete_host_form() -> FormState {
        let mut form = FormState {
            visible: true,
            editing_id: None,
            name: "web".into(),
            hostname: "10.0.0.1".into(),
            port: "22".into(),
            username: "root".into(),
            auth: AuthType::Password,
            jump_host_id: None,
            jump_picker: DropdownState::new(),
            key_path: String::new(),
            password: SecretString::from("hunter2"),
            password_visible: false,
            identity_picker: DropdownState::new(),
            selected_identity_id: None,
            keychain_picker: DropdownState::new(),
            tags: Vec::new(),
            tag_picker: DropdownState::new(),
            original: None,
        };
        form.reset();
        form.name = "web".into();
        form.hostname = "10.0.0.1".into();
        form.port = "22".into();
        form.username = "root".into();
        form.auth = AuthType::Password;
        form.password = SecretString::from("hunter2");
        form
    }

    #[test]
    fn host_form_requires_target_user_and_a_usable_port() {
        let mut form = complete_host_form();
        assert!(form.is_complete());
        assert!(form.save_enabled());

        form.hostname = "   ".into();
        assert!(!form.is_complete(), "whitespace is not a hostname");

        form.hostname = "10.0.0.1".into();
        form.username = " ".into();
        assert!(!form.is_complete());

        form.username = "root".into();
        form.port = "0".into();
        assert!(!form.is_complete(), "port 0 is not connectable");

        form.port = "65536".into();
        assert!(!form.is_complete(), "anything above 65535 overflows u16");

        form.port = String::new();
        assert!(!form.is_complete());

        form.port = "not-a-port".into();
        assert!(!form.is_complete());

        form.port = "2222".into();
        assert!(form.is_complete());
    }

    #[test]
    fn a_new_host_form_demands_the_credential_its_auth_type_needs() {
        let mut form = complete_host_form();

        form.auth = AuthType::Key;
        assert!(!form.is_complete(), "key auth needs a key");
        form.key_path = "~/.ssh/id_ed25519".into();
        assert!(form.is_complete());

        form.auth = AuthType::Password;
        form.password = SecretString::from(String::new());
        assert!(!form.is_complete(), "password auth needs a password");
        form.password = SecretString::from("hunter2");
        assert!(form.is_complete());
    }

    #[test]
    fn editing_a_saved_host_does_not_re_demand_its_stored_credential() {
        let mut form = complete_host_form();
        form.editing_id = Some(HostId(1));
        form.auth = AuthType::Password;
        form.password = SecretString::from(String::new());
        assert!(
            form.is_complete(),
            "a saved password is offered from the vault"
        );

        form.auth = AuthType::Key;
        form.key_path = String::new();
        assert!(
            form.is_complete(),
            "a saved key path is offered from the vault"
        );
    }

    fn saved_host_for_edit() -> Host {
        let mut saved = host(1, "web", "10.0.0.1", "root", &["prod"]);
        saved.auth_type = AuthType::Password;
        saved.key_path = None;
        saved
    }

    /// Load the edit form the way the app does when a saved host is opened:
    /// every persisted field, including the credential the host stores.
    fn host_form_editing(saved: &Host) -> FormState {
        let mut form = complete_host_form();
        form.editing_id = Some(saved.id);
        form.name = saved.name.clone();
        form.hostname = saved.hostname.clone();
        form.username = saved.username.clone();
        form.port = saved.port.to_string();
        form.auth = saved.auth_type;
        form.tags = saved.tags.clone();
        form.key_path = saved.key_path.clone().unwrap_or_default();
        form.password = SecretString::from(saved.password_for_edit());
        form.jump_host_id = saved.jump_host_id;
        form.selected_identity_id = saved.identity_id;
        form.original = Some(saved.clone());
        form
    }

    #[test]
    fn an_untouched_form_is_not_dirty_and_a_changed_one_is() {
        let saved = saved_host_for_edit();
        let mut form = host_form_editing(&saved);

        assert!(!form.is_dirty(), "an untouched edit form is not a change");
        // The save button stays disabled until something actually changed.
        assert!(!form.save_enabled());

        form.hostname = "10.0.0.2".into();
        assert!(form.is_dirty());
        assert!(form.save_enabled());
    }

    #[test]
    fn a_password_typed_while_editing_a_key_host_is_ignored() {
        let mut saved = saved_host_for_edit();
        saved.auth_type = AuthType::Key;
        saved.key_path = Some("~/.ssh/id_ed25519".into());
        saved.password = None;
        let mut form = host_form_editing(&saved);
        assert!(!form.is_dirty(), "the loaded key host is unchanged");

        // The password field is on screen but nothing under key auth persists
        // it, so typing there is not an edit.
        form.password = SecretString::from("typed-by-accident");
        assert!(!form.is_dirty());

        form.key_path = "~/.ssh/other_key".into();
        assert!(form.is_dirty(), "the key path is what key auth persists");
    }

    #[test]
    fn switching_a_saved_host_to_the_other_auth_type_is_an_edit() {
        // `is_dirty` compares what saving would persist against what is stored,
        // so dropping a stored credential counts even though the user typed
        // nothing into the form.
        let password_host = saved_host_for_edit();
        let mut form = host_form_editing(&password_host);
        assert!(!form.is_dirty());

        form.auth = AuthType::Key;
        form.key_path = "~/.ssh/id_ed25519".into();
        assert!(
            form.is_dirty(),
            "saving under key auth drops the stored password"
        );

        let mut key_host = saved_host_for_edit();
        key_host.auth_type = AuthType::Key;
        key_host.key_path = Some("~/.ssh/id_ed25519".into());
        key_host.password = None;
        let mut form = host_form_editing(&key_host);
        assert!(!form.is_dirty());

        form.auth = AuthType::Password;
        form.password = SecretString::from("hunter2");
        assert!(
            form.is_dirty(),
            "saving under password auth drops the stored key path"
        );
    }

    #[test]
    fn a_typed_port_that_matches_the_saved_one_is_not_a_change() {
        let mut saved = saved_host_for_edit();
        saved.auth_type = AuthType::Key;
        saved.key_path = Some("~/.ssh/id_ed25519".into());
        saved.password = None;
        let mut form = host_form_editing(&saved);

        // Ports are compared as numbers, so whitespace and still-valid padding
        // must not register as edits.
        form.port = format!(" {} ", saved.port);
        assert!(!form.is_dirty());

        form.port = (saved.port + 1).to_string();
        assert!(form.is_dirty());
    }

    fn port_forward(id: u32, name: &str, kind: PortForwardKind) -> PortForward {
        PortForward {
            id: PortForwardId(id),
            name: name.to_string(),
            kind,
            host_id: HostId(1),
            bind_address: "127.0.0.1".into(),
            listen_port: 8080,
            dest_host: "10.0.0.9".into(),
            dest_port: 80,
        }
    }

    #[test]
    fn port_forward_search_covers_every_field_the_row_shows() {
        let mut state = PortForwardState {
            trust: TrustState::default(),
            password: PasswordState::default(),
            list: vec![
                port_forward(1, "web tunnel", PortForwardKind::Local),
                port_forward(2, "db tunnel", PortForwardKind::Remote),
            ],
            runtime: HashMap::new(),
            cursor: GridCursor::new(),
            grid_columns: 1,
            form: PortForwardFormState {
                visible: false,
                editing_id: None,
                name: String::new(),
                kind: PortForwardKind::Local,
                host_id: None,
                bind_address: String::new(),
                listen_port: String::new(),
                dest_host: String::new(),
                dest_port: String::new(),
                host_picker: DropdownState::new(),
                original: None,
            },
            form_anim: 1.0,
            form_closing: false,
            logs_open: false,
            delete_pending: None,
            grid_view: true,
            hovered_id: None,
            context_menu: None,
            cursor_pos: Point::ORIGIN,
            search_query: String::new(),
        };

        assert_eq!(state.filtered().len(), 2);

        state.search_query = "WEB".to_string();
        assert_eq!(
            state
                .filtered()
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            ["web tunnel"]
        );

        // "10.0.0.9" only appears in the destination, which the row renders as
        // part of its summary — so it has to be searchable.
        state.search_query = "10.0.0.9".to_string();
        assert_eq!(state.filtered().len(), 2);

        state.search_query = "8080".to_string();
        assert_eq!(state.filtered().len(), 2, "the listen port is searchable");

        state.search_query = "nope".to_string();
        assert!(state.filtered().is_empty());
    }

    #[test]
    fn a_stopped_forward_reports_stopped_and_running_ones_are_counted() {
        let mut state = PortForwardState {
            trust: TrustState::default(),
            password: PasswordState::default(),
            list: vec![port_forward(1, "one", PortForwardKind::Local)],
            runtime: HashMap::new(),
            cursor: GridCursor::new(),
            grid_columns: 1,
            form: PortForwardFormState {
                visible: false,
                editing_id: None,
                name: String::new(),
                kind: PortForwardKind::Local,
                host_id: None,
                bind_address: String::new(),
                listen_port: String::new(),
                dest_host: String::new(),
                dest_port: String::new(),
                host_picker: DropdownState::new(),
                original: None,
            },
            form_anim: 1.0,
            form_closing: false,
            logs_open: false,
            delete_pending: None,
            grid_view: true,
            hovered_id: None,
            context_menu: None,
            cursor_pos: Point::ORIGIN,
            search_query: String::new(),
        };

        // A forward with no runtime entry has never started.
        assert_eq!(
            state.status_for(PortForwardId(1)),
            PortForwardStatus::Stopped
        );
        assert!(state.runtime_for(PortForwardId(1)).is_none());
        assert!(!state.has_running());

        for status in [PortForwardStatus::Starting, PortForwardStatus::Running] {
            state.runtime.insert(
                PortForwardId(1),
                PortForwardRuntime {
                    status,
                    logs: Vec::new(),
                },
            );
            assert!(state.has_running(), "{status:?} counts as live");
            assert_eq!(state.status_for(PortForwardId(1)), status);
        }

        for status in [
            PortForwardStatus::Stopping,
            PortForwardStatus::Stopped,
            PortForwardStatus::Failed,
        ] {
            state.runtime.insert(
                PortForwardId(1),
                PortForwardRuntime {
                    status,
                    logs: Vec::new(),
                },
            );
            assert!(!state.has_running(), "{status:?} is not live");
        }
    }

    /// The forward form filled in far enough to save.
    fn complete_forward_form(kind: PortForwardKind) -> PortForwardFormState {
        let mut form = PortForwardFormState {
            visible: true,
            editing_id: None,
            name: String::new(),
            kind,
            host_id: None,
            bind_address: String::new(),
            listen_port: String::new(),
            dest_host: String::new(),
            dest_port: String::new(),
            host_picker: DropdownState::new(),
            original: None,
        };
        form.reset();
        form.kind = kind;
        form.host_id = Some(HostId(1));
        form.listen_port = "8080".into();
        form.dest_host = "127.0.0.1".into();
        form.dest_port = "80".into();
        form
    }

    #[test]
    fn forward_form_reset_restores_the_documented_defaults() {
        let mut form = complete_forward_form(PortForwardKind::Remote);
        form.name = "half-typed".into();
        form.original = Some(port_forward(1, "one", PortForwardKind::Local));

        form.reset();

        assert_eq!(form.editing_id, None);
        assert!(form.name.is_empty());
        assert_eq!(form.kind, PortForwardKind::Local);
        assert_eq!(form.host_id, None);
        assert_eq!(form.bind_address, "127.0.0.1");
        assert_eq!(form.listen_port, "8080");
        assert_eq!(form.dest_host, "127.0.0.1");
        assert_eq!(form.dest_port, "80");
        assert!(form.original.is_none());
    }

    #[test]
    fn local_and_remote_forwards_need_a_destination_but_dynamic_ones_do_not() {
        for kind in [PortForwardKind::Local, PortForwardKind::Remote] {
            let mut form = complete_forward_form(kind);
            assert!(form.is_complete(), "{kind:?} with a destination");

            form.dest_host = String::new();
            assert!(!form.is_complete(), "{kind:?} without a destination host");
            form.dest_host = "127.0.0.1".into();

            form.dest_port = String::new();
            assert!(!form.is_complete(), "{kind:?} without a destination port");
        }

        // A SOCKS proxy has no destination at all.
        let mut socks = complete_forward_form(PortForwardKind::Dynamic);
        socks.dest_host = String::new();
        socks.dest_port = String::new();
        assert!(socks.is_complete());

        socks.listen_port = "70000".into();
        assert!(!socks.is_complete(), "the listen port must still be a u16");

        socks.listen_port = "1080".into();
        socks.host_id = None;
        assert!(!socks.is_complete(), "a forward always belongs to a host");
    }

    #[test]
    fn editing_a_forward_only_reports_edits_that_would_change_it() {
        let saved = port_forward(1, "web tunnel", PortForwardKind::Local);
        let mut form = complete_forward_form(PortForwardKind::Local);
        form.editing_id = Some(saved.id);
        form.name = saved.name.clone();
        form.host_id = Some(saved.host_id);
        form.bind_address = saved.bind_address.clone();
        form.listen_port = saved.listen_port.to_string();
        form.dest_host = saved.dest_host.clone();
        form.dest_port = saved.dest_port.to_string();
        form.original = Some(saved.clone());

        assert!(!form.is_dirty());
        assert!(!form.save_enabled());

        // Ports are compared numerically, so an untrimmed-but-equal value is
        // not an edit.
        form.listen_port = format!(" {} ", saved.listen_port);
        assert!(!form.is_dirty());

        form.listen_port = (saved.listen_port + 1).to_string();
        assert!(form.is_dirty());
    }

    #[test]
    fn a_new_form_is_always_dirty_because_there_is_nothing_to_compare_against() {
        let forward = complete_forward_form(PortForwardKind::Local);
        assert!(forward.is_dirty(), "creating a forward is always a change");

        let snippet = SnippetFormState {
            visible: true,
            editing_id: None,
            description: "deploy".into(),
            script: text_editor::Content::with_text("git push"),
            package_id: None,
            package_picker: DropdownState::new(),
            original: None,
        };
        assert!(snippet.is_dirty());
        assert!(snippet.is_complete());
        assert!(snippet.save_enabled());
    }

    #[test]
    fn snippet_forms_need_both_a_description_and_a_script() {
        let mut snippet = SnippetFormState {
            visible: true,
            editing_id: None,
            description: "deploy".into(),
            script: text_editor::Content::with_text("git push"),
            package_id: None,
            package_picker: DropdownState::new(),
            original: None,
        };
        assert!(snippet.is_complete());

        snippet.description = "   ".into();
        assert!(!snippet.is_complete(), "whitespace is not a description");

        snippet.description = "deploy".into();
        snippet.script = text_editor::Content::with_text(" \n\t ");
        assert!(!snippet.is_complete(), "whitespace is not a script");

        snippet.script = text_editor::Content::with_text("git push");
        assert!(snippet.is_complete());
    }

    #[test]
    fn an_edited_snippet_ignores_surrounding_whitespace() {
        let saved = Snippet {
            id: SnippetId(1),
            description: "deploy".into(),
            script: "git push".into(),
            package_id: Some(SnippetPackageId(4)),
            last_used_at: String::new(),
        };
        let mut form = SnippetFormState {
            visible: true,
            editing_id: Some(saved.id),
            description: saved.description.clone(),
            script: text_editor::Content::with_text(&saved.script),
            package_id: saved.package_id,
            package_picker: DropdownState::new(),
            original: Some(saved.clone()),
        };

        assert!(!form.is_dirty());

        form.description = "  deploy  ".into();
        form.script = text_editor::Content::with_text("  git push\n");
        assert!(!form.is_dirty(), "padding is not an edit");

        form.package_id = None;
        assert!(
            form.is_dirty(),
            "moving a snippet between packages is an edit"
        );

        form.package_id = saved.package_id;
        form.script = text_editor::Content::with_text("git push --force");
        assert!(form.is_dirty());
    }

    #[test]
    fn snippet_package_forms_only_require_a_name() {
        let mut package = SnippetPackageFormState {
            visible: true,
            editing_id: None,
            name: "ops".into(),
            description: String::new(),
            original: None,
        };
        assert!(package.is_complete(), "a description is optional");
        assert!(package.is_dirty());

        package.name = "  ".into();
        assert!(!package.is_complete());

        let saved = SnippetPackage {
            id: SnippetPackageId(1),
            name: "ops".into(),
            description: "runbooks".into(),
        };
        package.name = saved.name.clone();
        package.description = saved.description.clone();
        package.editing_id = Some(saved.id);
        package.original = Some(saved);
        assert!(!package.is_dirty());
        assert!(!package.save_enabled());

        package.description = "runbooks and scripts".into();
        assert!(package.is_dirty());
    }

    #[test]
    fn generating_a_key_needs_a_name_a_path_and_a_type_but_editing_does_not() {
        let mut form = KeychainFormState {
            visible: true,
            editing_id: None,
            name: String::new(),
            key_type: String::new(),
            selected_key_type: None,
            passphrase: SecretString::from(String::new()),
            passphrase_visible: false,
            fingerprint: String::new(),
            public_key: String::new(),
            path: String::new(),
            path_manual: false,
            in_progress: false,
            original: None,
        };

        assert!(!form.is_complete(), "an empty form cannot generate a key");

        form.name = "github".into();
        assert!(!form.is_complete(), "a key needs a type");
        form.selected_key_type = Some(KeyType::ED25519);
        assert!(!form.is_complete(), "a key needs somewhere to be written");

        form.path = "~/.ssh/github".into();
        assert!(form.is_complete());

        // An existing key is edited, not regenerated: it already has a path on
        // disk, so only the label and type are required.
        form.editing_id = Some(KeychainEntryId(1));
        form.path = String::new();
        form.selected_key_type = None;
        form.key_type = "ED25519".into();
        assert!(form.is_complete());

        form.key_type = String::new();
        assert!(!form.is_complete());
    }

    #[test]
    fn editing_a_key_only_cares_about_the_fields_that_are_persisted() {
        let saved = KeychainEntry {
            id: KeychainEntryId(1),
            name: "github".into(),
            key_type: "ED25519".into(),
            fingerprint: "SHA256:abc".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            path: "~/.ssh/github".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let mut form = KeychainFormState {
            visible: true,
            editing_id: Some(saved.id),
            name: saved.name.clone(),
            key_type: saved.key_type.clone(),
            selected_key_type: Some(KeyType::ED25519),
            passphrase: SecretString::from(String::new()),
            passphrase_visible: false,
            fingerprint: saved.fingerprint.clone(),
            public_key: saved.public_key.clone(),
            path: saved.path.clone(),
            path_manual: false,
            in_progress: false,
            original: Some(saved),
        };

        assert!(!form.is_dirty());

        // The fingerprint and public key are read back from the file, so
        // touching them is not an edit.
        form.fingerprint = "SHA256:zzz".into();
        form.public_key = "ssh-ed25519 BBBB".into();
        assert!(!form.is_dirty());

        form.name = "github-work".into();
        assert!(form.is_dirty());

        form.name = "github".into();
        form.key_type = "RSA".into();
        assert!(form.is_dirty(), "the key type is persisted");
    }

    #[test]
    fn identity_forms_need_a_label_and_a_password() {
        let mut form = IdentityFormState {
            visible: true,
            editing_id: None,
            label: "deploy".into(),
            username: "deploy".into(),
            password: SecretString::from(String::new()),
            password_visible: false,
            original: None,
        };

        assert!(
            !form.is_complete(),
            "an identity without a password is useless"
        );

        form.password = SecretString::from("hunter2");
        assert!(form.is_complete(), "the username may be filled in later");

        form.label = "  ".into();
        assert!(!form.is_complete());

        form.label = "deploy".into();
        let saved = Identity {
            id: IdentityId(1),
            label: "deploy".into(),
            username: "deploy".into(),
            password: SecretString::from("hunter2"),
            created_at: String::new(),
        };
        form.editing_id = Some(saved.id);
        form.username = saved.username.clone();
        form.password = SecretString::from(saved.password_for_edit());
        form.original = Some(saved.clone());
        assert!(!form.is_dirty());

        form.username = "root".into();
        assert!(form.is_dirty());
    }

    #[test]
    fn preferences_fall_back_to_defaults_and_accept_stored_values() {
        let defaults = PreferencesDomain::from_prefs(&HashMap::new());
        assert_eq!(defaults.locale, crate::core::locale::Locale::En);
        assert_eq!(defaults.theme_mode, crate::core::theme::ThemeMode::Dark);
        assert_eq!(defaults.terminal_font_size, 14.0);
        assert!(
            defaults.shell_integration,
            "shell integration is on unless switched off"
        );
        assert_eq!(defaults.local_shell, LocalShell::CommandPrompt);
        assert_eq!(
            defaults.command_history_limit,
            crate::core::data::COMMAND_HISTORY_DEFAULT_LIMIT
        );
        assert_eq!(defaults.command_history_limit_picker, DropdownState::new());

        let stored = PreferencesDomain::from_prefs(&HashMap::from([
            ("locale".to_string(), "zh-CN".to_string()),
            ("theme_mode".to_string(), "Dracula".to_string()),
            ("terminal_font_size".to_string(), "16.5".to_string()),
            ("shell_integration".to_string(), "false".to_string()),
            ("local_shell".to_string(), "pwsh".to_string()),
            ("command_history_limit".to_string(), "4321".to_string()),
        ]));
        assert_eq!(stored.locale, crate::core::locale::Locale::ZhCN);
        assert_eq!(stored.theme_mode, crate::core::theme::ThemeMode::Dracula);
        assert_eq!(stored.terminal_font_size, 16.5);
        assert!(!stored.shell_integration);
        assert_eq!(stored.local_shell, LocalShell::PowerShell7);
        assert_eq!(stored.command_history_limit, 4321);

        // A corrupt or unknown value must not take the app down with it.
        let junk = PreferencesDomain::from_prefs(&HashMap::from([
            ("locale".to_string(), "klingon".to_string()),
            ("theme_mode".to_string(), "Neon".to_string()),
            ("terminal_font_size".to_string(), "huge".to_string()),
            ("command_history_limit".to_string(), "999999".to_string()),
        ]));
        assert_eq!(junk.locale, crate::core::locale::Locale::En);
        assert_eq!(junk.theme_mode, crate::core::theme::ThemeMode::Dark);
        assert_eq!(junk.terminal_font_size, 14.0);
        assert_eq!(
            junk.command_history_limit,
            crate::core::data::COMMAND_HISTORY_MAX_LIMIT,
            "a stored value past the ceiling is clamped, not honoured"
        );

        let unparsable = PreferencesDomain::from_prefs(&HashMap::from([(
            "command_history_limit".to_string(),
            "lots".to_string(),
        )]));
        assert_eq!(
            unparsable.command_history_limit,
            crate::core::data::COMMAND_HISTORY_DEFAULT_LIMIT
        );
    }

    // Tab bookkeeping
    //
    // These drive the real app through the same helpers the message handlers
    // use, without any of the entities (PTYs, SSH sessions) behind the tabs:
    // the registry only stores what kind of entity a tab hosts.

    fn session_tab(app: &mut App, id: u32) -> TabId {
        app.allocate_tab_id(TabKind::Session(SessionId(id)))
    }

    #[test]
    fn allocated_tabs_are_unique_and_keep_their_order() {
        let mut app = crate::core::test_support::app();

        let first = session_tab(&mut app, 1);
        let second = session_tab(&mut app, 2);
        let third = app.allocate_tab_id(TabKind::Draft(DraftId(1)));

        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_eq!(app.terminal.tabs, vec![first, second, third]);
        assert_eq!(app.visible_tab_ids(), app.terminal.tabs);
    }

    #[test]
    fn removing_a_tab_forgets_it_everywhere() {
        let mut app = crate::core::test_support::app();
        let first = session_tab(&mut app, 1);
        let second = session_tab(&mut app, 2);
        app.terminal.tab_width_anims.insert(first, 111.0);
        app.terminal.tab_width_anims.insert(second, 222.0);

        app.remove_tab(first);

        assert_eq!(app.terminal.tabs, vec![second]);
        assert!(!app.terminal.tab_registry.contains_key(&first));
        assert!(
            !app.terminal.tab_width_anims.contains_key(&first),
            "a stale animation entry would resurrect the closed tab's width"
        );
        assert_eq!(app.tab_id_for_session(SessionId(1)), None);
    }

    #[test]
    fn tabs_can_be_found_through_the_entity_they_host() {
        let mut app = crate::core::test_support::app();
        let tab = session_tab(&mut app, 5);
        let draft_tab = app.allocate_tab_id(TabKind::Draft(DraftId(3)));

        assert_eq!(app.tab_id_for_session(SessionId(5)), Some(tab));
        assert_eq!(app.tab_id_for_draft(DraftId(3)), Some(draft_tab));
        assert_eq!(app.tab_id_for_session(SessionId(6)), None);
        assert_eq!(app.tab_id_for_draft(DraftId(4)), None);
        assert_eq!(app.tab_id_for_split(SplitId(1)), None);
    }

    #[test]
    fn locking_a_tab_is_per_tab_and_only_counts_terminals() {
        let mut app = crate::core::test_support::app();
        let terminal = session_tab(&mut app, 1);
        let draft = app.allocate_tab_id(TabKind::Draft(DraftId(1)));

        app.set_tab_locked(SessionId(1), true);
        assert!(app.tab_is_locked(SessionId(1)));
        assert!(app.tab_is_locked_by_id(terminal));
        assert_eq!(app.locked_terminal_tab_count(), 1);

        // Drafts are connecting screens, not terminals; locking one is not part
        // of the "you are about to close a locked terminal" warning.
        app.set_tab_locked_by_id(draft, true);
        assert!(app.tab_is_locked_by_id(draft));
        assert_eq!(app.locked_terminal_tab_count(), 1);

        app.set_tab_locked(SessionId(1), false);
        assert!(!app.tab_is_locked(SessionId(1)));
        assert_eq!(app.locked_terminal_tab_count(), 0);
    }

    #[test]
    fn locking_is_ignored_for_tabs_and_sessions_that_do_not_exist() {
        let mut app = crate::core::test_support::app();

        app.set_tab_locked(SessionId(404), true);
        app.set_tab_locked_by_id(TabId(404), true);

        assert!(!app.tab_is_locked(SessionId(404)));
        assert!(!app.tab_is_locked_by_id(TabId(404)));
        assert_eq!(app.locked_terminal_tab_count(), 0);
    }

    #[test]
    fn closing_the_active_tab_hands_over_to_its_left_neighbour() {
        let mut app = crate::core::test_support::app();
        let first = session_tab(&mut app, 1);
        let second = session_tab(&mut app, 2);
        let third = session_tab(&mut app, 3);
        app.active_tab = ActiveTab::Tab(third);

        // The close paths activate first and remove second, while the closing
        // tab is still in the order list — that is what makes "the tab to the
        // left" well defined.
        app.activate_neighbor_after_close(third);
        app.remove_tab(third);
        assert_eq!(app.active_tab, ActiveTab::Tab(second));

        // The leftmost tab has no neighbour to its left, so the next one over
        // takes the workspace.
        app.active_tab = ActiveTab::Tab(first);
        app.activate_neighbor_after_close(first);
        app.remove_tab(first);
        assert_eq!(app.active_tab, ActiveTab::Tab(second));
    }

    #[test]
    fn closing_the_last_tab_falls_back_to_the_vault_workspace() {
        let mut app = crate::core::test_support::app();
        let only = session_tab(&mut app, 1);
        app.active_tab = ActiveTab::Tab(only);

        app.activate_neighbor_after_close(only);
        app.remove_tab(only);

        assert_eq!(app.active_tab, ActiveTab::Workspace(Workspace::Vaults));
    }

    #[test]
    fn closing_a_background_tab_does_not_move_the_active_one() {
        let mut app = crate::core::test_support::app();
        let first = session_tab(&mut app, 1);
        let second = session_tab(&mut app, 2);
        app.active_tab = ActiveTab::Tab(second);

        app.activate_neighbor_after_close(first);
        app.remove_tab(first);

        assert_eq!(app.active_tab, ActiveTab::Tab(second));
    }

    #[test]
    fn the_active_tabs_width_is_seeded_wider_than_the_rest() {
        let mut app = crate::core::test_support::app();
        let first = session_tab(&mut app, 1);
        let second = session_tab(&mut app, 2);
        app.active_tab = ActiveTab::Tab(second);
        app.top_tabs_can_scroll = true;

        app.seed_tab_width_anims();

        assert!(!app.top_tabs_can_scroll, "the strip is re-measured");
        let active_width = app.terminal.tab_width_anims[&second];
        let idle_width = app.terminal.tab_width_anims[&first];
        assert!(
            active_width > idle_width,
            "the active tab grows, the others shrink"
        );
        assert!(
            active_width <= crate::views::MAX_TAB_WIDTH,
            "a tab never grows past the cap"
        );
    }

    #[test]
    fn sftp_state_travels_with_its_tab() {
        let mut app = crate::core::test_support::app();
        let first = app.sftp.active_session;
        let second = SftpSessionId(app.sftp.next_session_id as u32);
        app.sftp.next_session_id += 1;
        app.sftp.sessions.push(SftpSession {
            id: second,
            label: "second".into(),
            state: SftpState::default(),
        });

        app.sftp.state.left.pane.path = "/srv/one".into();
        app.load_sftp_state(second);

        // The tab we left kept its own listing…
        assert_eq!(
            app.sftp
                .sessions
                .iter()
                .find(|s| s.id == first)
                .unwrap()
                .state
                .left
                .pane
                .path,
            "/srv/one"
        );
        assert_eq!(app.sftp.active_session, second);
        assert_ne!(
            app.sftp.state.left.pane.path, "/srv/one",
            "the new tab is its own"
        );
    }

    #[test]
    fn a_workspace_tab_has_no_terminal_sidebar_to_reach() {
        let mut app = crate::core::test_support::app();
        app.active_tab = ActiveTab::Workspace(Workspace::Vaults);

        assert_eq!(app.active_terminal_session_id(), None);
        assert!(app.sidebar().is_none());
        assert!(app.sidebar_mut().is_none());
        assert!(!app.is_active_session_in_split());
    }

    #[test]
    fn a_draft_tab_is_not_a_terminal_session() {
        let mut app = crate::core::test_support::app();
        let draft_tab = app.allocate_tab_id(TabKind::Draft(DraftId(1)));
        app.active_tab = ActiveTab::Tab(draft_tab);

        assert_eq!(app.active_terminal_session_id(), None);
        assert_eq!(app.active_draft_id(), Some(DraftId(1)));
        assert!(!app.is_active_session_in_split());
    }

    #[test]
    fn right_sidebar_opens_on_command_history_by_default() {
        assert_eq!(
            SidebarState::default().active_tab,
            RightSidebarTab::Commands
        );
    }

    #[test]
    fn each_right_sidebar_tab_keeps_an_independent_cursor() {
        let mut cursors = SidebarCursors::default();
        let snippets = [SnippetId(7), SnippetId(9)];
        let commands = ["cargo test".to_string(), "git status".to_string()];
        let themes = ThemeMode::all();

        for tab in [
            RightSidebarTab::Snippets,
            RightSidebarTab::Commands,
            RightSidebarTab::Theme,
        ] {
            cursors.move_in(tab, &snippets, &commands, themes, GridDirection::Down);
        }

        assert_eq!(cursors.selected_snippet(), Some(&SnippetId(7)));
        assert_eq!(
            cursors.selected_command().map(String::as_str),
            Some("cargo test")
        );
        assert_eq!(cursors.selected_theme(), Some(&ThemeMode::Auto));
    }

    #[test]
    fn a_command_stays_selected_when_mru_reorders_the_visible_list() {
        let mut cursors = SidebarCursors::default();
        cursors.select_command("git status");
        let snippets = [];
        let themes = [];

        cursors.move_in(
            RightSidebarTab::Commands,
            &snippets,
            &["cargo test".to_string(), "git status".to_string()],
            &themes,
            GridDirection::Up,
        );

        assert_eq!(
            cursors.selected_command().map(String::as_str),
            Some("cargo test")
        );
    }
}
