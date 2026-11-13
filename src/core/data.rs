use crate::core::diagnostics::Level;
use crate::core::i18n::t_static;
use crate::core::time::format_system_time;
use rust_i18n::t;
use secrecy::{ExposeSecret, SecretString};
use std::fmt;
use std::path::Path;

// Re-export the entity identifiers alongside the structs they identify, so
// `use crate::core::data::{Host, HostId}` keeps working.
pub use crate::core::ids::{
    HostId, IdentityId, KeychainEntryId, PortForwardId, SnippetId, SnippetPackageId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavItem {
    Hosts,
    Keychain,
    PortForwarding,
    Snippets,
    KnownHosts,
    Logs,
    Settings,
}

impl NavItem {
    pub fn all() -> &'static [NavItem] {
        &[
            NavItem::Hosts,
            NavItem::Keychain,
            NavItem::PortForwarding,
            NavItem::Snippets,
            NavItem::KnownHosts,
            NavItem::Settings,
            NavItem::Logs,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Hosts => t_static(t!("menu.hosts")),
            Self::Keychain => t_static(t!("menu.keychain")),
            Self::PortForwarding => t_static(t!("menu.port_forwarding")),
            Self::Snippets => t_static(t!("menu.snippets")),
            Self::KnownHosts => t_static(t!("menu.known_hosts")),
            Self::Logs => t_static(t!("menu.logs")),
            Self::Settings => t_static(t!("menu.profiles")),
        }
    }

    pub fn icon(self) -> lucide_icons::Icon {
        match self {
            Self::Hosts => lucide_icons::Icon::Server,
            Self::Keychain => lucide_icons::Icon::KeyRound,
            Self::PortForwarding => lucide_icons::Icon::Shuffle,
            Self::Snippets => lucide_icons::Icon::Code,
            Self::KnownHosts => lucide_icons::Icon::ShieldCheck,
            Self::Logs => lucide_icons::Icon::ScrollText,
            Self::Settings => lucide_icons::Icon::User,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogCategory {
    Host,
    Terminal,
    Sftp,
    PortForward,
    Snippet,
    KnownHost,
    Keychain,
    System,
}

impl LogCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Host => t_static(t!("log_category.host")),
            Self::Terminal => t_static(t!("log_category.terminal")),
            Self::Sftp => t_static(t!("log_category.sftp")),
            Self::PortForward => t_static(t!("log_category.port_forward")),
            Self::Snippet => t_static(t!("log_category.snippet")),
            Self::KnownHost => t_static(t!("log_category.known_host")),
            Self::Keychain => t_static(t!("log_category.keychain")),
            Self::System => t_static(t!("log_category.system")),
        }
    }

    /// Stable, language-independent name of the category, written to the
    /// activity log file.
    ///
    /// [`LogCategory::label`] is translated, so it cannot serve as a file
    /// token: switching the interface language would leave yesterday's entries
    /// unreadable.
    pub fn token(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Terminal => "terminal",
            Self::Sftp => "sftp",
            Self::PortForward => "port_forward",
            Self::Snippet => "snippet",
            Self::KnownHost => "known_host",
            Self::Keychain => "keychain",
            Self::System => "system",
        }
    }

    /// The category a [`LogCategory::token`] was written from.
    ///
    /// `None` for a token this version doesn't know, which the reader keeps as
    /// a [`LogCategory::System`] entry rather than dropping the message.
    pub fn from_token(token: &str) -> Option<Self> {
        [
            Self::Host,
            Self::Terminal,
            Self::Sftp,
            Self::PortForward,
            Self::Snippet,
            Self::KnownHost,
            Self::Keychain,
            Self::System,
        ]
        .into_iter()
        .find(|category| category.token() == token)
    }
}

impl fmt::Display for LogCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp: i64,
    pub category: LogCategory,
    pub message: String,
}

/// How many entries the Logs page holds, and how far back a restart reads.
///
/// The page is a window on the activity log rather than the whole of it: the
/// file keeps the day, this keeps the tail.
pub const MAX_LOG_ENTRIES: usize = 200;

/// How many commands a terminal session keeps unless the user says otherwise.
///
/// Deeper than the fixed 100 the sidebar used to hold, which is what makes the
/// scrollable command history worth opening; the ring is per host scope and is
/// mirrored to `<termlet_dir>/command_history/<scope>.json` on every recorded
/// command, so this value also bounds how much that file grows.
pub const COMMAND_HISTORY_DEFAULT_LIMIT: usize = 1000;

/// Hard ceiling on the command-history limit a user may configure.
///
/// The Profiles picker stops here and a stored value above this is clamped on
/// read. Each entry carries its command text, and the whole file is rewritten
/// as the ring changes, so an unbounded ring would cost both disk writes and
/// sidebar rendering time.
pub const COMMAND_HISTORY_MAX_LIMIT: usize = 10_000;

/// The history sizes the Profiles picker offers, smallest first.
///
/// The limit bounds how much the sidebar keeps rendering, so it is picked from
/// a fixed set rather than typed: the list spans the old fixed cap (100) up to
/// [`COMMAND_HISTORY_MAX_LIMIT`], and carries
/// [`COMMAND_HISTORY_DEFAULT_LIMIT`] in between.
pub const COMMAND_HISTORY_LIMITS: [usize; 6] = [100, 500, 1000, 2000, 5000, 10_000];

/// The shell used for new local terminals on Windows.
///
/// `CommandPrompt` is shown as Default and keeps the historical `ComSpec`
/// behavior. The PowerShell choices resolve by executable name so the selected
/// preference remains valid across install locations and Termlet upgrades.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LocalShell {
    #[default]
    CommandPrompt,
    WindowsPowerShell,
    PowerShell7,
}

impl LocalShell {
    /// Options shown in the Windows settings page.
    pub const fn windows_options() -> &'static [Self] {
        &[
            Self::CommandPrompt,
            Self::WindowsPowerShell,
            Self::PowerShell7,
        ]
    }

    pub fn label(self) -> &'static str {
        self.label_for_locale(&rust_i18n::locale())
    }

    fn label_for_locale(self, locale: &str) -> &'static str {
        match self {
            Self::CommandPrompt => t_static(t!("settings.local_shell_default", locale = locale)),
            Self::WindowsPowerShell => t_static(t!(
                "settings.local_shell_windows_powershell",
                locale = locale
            )),
            Self::PowerShell7 => t_static(t!("settings.local_shell_pwsh", locale = locale)),
        }
    }

    /// Stable storage token. Never persist the translated label.
    pub const fn to_str(self) -> &'static str {
        match self {
            Self::CommandPrompt => "cmd",
            Self::WindowsPowerShell => "windows_powershell",
            Self::PowerShell7 => "pwsh",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "cmd" => Some(Self::CommandPrompt),
            "windows_powershell" => Some(Self::WindowsPowerShell),
            "pwsh" => Some(Self::PowerShell7),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightSidebarTab {
    Snippets,
    Commands,
    Theme,
}

impl RightSidebarTab {
    pub fn all() -> &'static [RightSidebarTab] {
        &[
            RightSidebarTab::Commands,
            RightSidebarTab::Snippets,
            RightSidebarTab::Theme,
        ]
    }

    pub fn icon(self) -> lucide_icons::Icon {
        match self {
            Self::Snippets => lucide_icons::Icon::Braces,
            Self::Commands => lucide_icons::Icon::History,
            Self::Theme => lucide_icons::Icon::Palette,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Snippets => t_static(t!("menu.snippets")),
            Self::Commands => t_static(t!("right_sidebar.commands")),
            Self::Theme => t_static(t!("settings.theme")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Ssh,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Protocol::Ssh => write!(f, "SSH"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    Password,
    Key,
}

impl AuthType {
    pub fn all() -> &'static [AuthType] {
        &[AuthType::Password, AuthType::Key]
    }
}

impl fmt::Display for AuthType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthType::Password => f.write_str(t!("auth_type.password").as_ref()),
            AuthType::Key => f.write_str(t!("auth_type.key").as_ref()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OsIcon {
    Lucide(lucide_icons::Icon),
    Os(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsType {
    Debian,
    Ubuntu,
    Linux,
    CentOS,
    Rocky,
    Alpine,
    Arch,
    Fedora,
    MacOS,
    Windows,
    Unknown,
}

impl OsType {
    pub fn icon(self) -> OsIcon {
        match self {
            Self::Debian => OsIcon::Os(crate::core::fonts::my_icon::DEBIAN),
            Self::Ubuntu => OsIcon::Os(crate::core::fonts::my_icon::UBUNTU),
            Self::CentOS => OsIcon::Os(crate::core::fonts::my_icon::CENTOS),
            Self::MacOS => OsIcon::Os(crate::core::fonts::my_icon::MACOS),
            Self::Linux => OsIcon::Os(crate::core::fonts::my_icon::LINUX),
            Self::Windows => OsIcon::Os(crate::core::fonts::my_icon::WINDOWS),
            Self::Arch => OsIcon::Lucide(lucide_icons::Icon::Terminal),
            Self::Rocky | Self::Fedora => OsIcon::Lucide(lucide_icons::Icon::Server),
            Self::Alpine => OsIcon::Lucide(lucide_icons::Icon::Mountain),
            Self::Unknown => OsIcon::Lucide(lucide_icons::Icon::Server),
        }
    }

    pub fn icon_bg(self) -> iced::Color {
        match self {
            Self::Ubuntu => iced::Color {
                r: 0.902,
                g: 0.290,
                b: 0.153,
                a: 1.0,
            },
            Self::Debian => iced::Color {
                r: 0.839,
                g: 0.153,
                b: 0.302,
                a: 1.0,
            },
            Self::CentOS => iced::Color {
                r: 0.620,
                g: 0.145,
                b: 0.525,
                a: 1.0,
            },
            Self::Rocky => iced::Color {
                r: 0.106,
                g: 0.588,
                b: 0.459,
                a: 1.0,
            },
            Self::Alpine => iced::Color {
                r: 0.176,
                g: 0.310,
                b: 0.592,
                a: 1.0,
            },
            Self::Arch => iced::Color {
                r: 0.106,
                g: 0.639,
                b: 0.820,
                a: 1.0,
            },
            Self::Fedora => iced::Color {
                r: 0.220,
                g: 0.314,
                b: 0.710,
                a: 1.0,
            },
            Self::MacOS => iced::Color {
                r: 0.533,
                g: 0.533,
                b: 0.533,
                a: 1.0,
            },
            Self::Windows => iced::Color {
                r: 0.000,
                g: 0.471,
                b: 0.839,
                a: 1.0,
            },
            Self::Linux | Self::Unknown => iced::Color {
                r: 0.243,
                g: 0.467,
                b: 0.894,
                a: 1.0,
            },
        }
    }

    pub fn to_str(self) -> &'static str {
        match self {
            Self::Debian => "Debian",
            Self::Ubuntu => "Ubuntu",
            Self::Linux => "Linux",
            Self::CentOS => "CentOS",
            Self::Rocky => "Rocky",
            Self::Alpine => "Alpine",
            Self::Arch => "Arch",
            Self::Fedora => "Fedora",
            Self::MacOS => "MacOS",
            Self::Windows => "Windows",
            Self::Unknown => "Unknown",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Debian" => Self::Debian,
            "Ubuntu" => Self::Ubuntu,
            "Linux" => Self::Linux,
            "CentOS" => Self::CentOS,
            "Rocky" => Self::Rocky,
            "Alpine" => Self::Alpine,
            "Arch" => Self::Arch,
            "Fedora" => Self::Fedora,
            "MacOS" => Self::MacOS,
            "Windows" => Self::Windows,
            _ => Self::Unknown,
        }
    }

    pub fn from_os_release(output: &str) -> Self {
        for line in output.lines() {
            if let Some(id) = line.strip_prefix("ID=") {
                let id = id.trim().trim_matches('"').trim_matches('\'');
                return match id {
                    "debian" => Self::Debian,
                    "ubuntu" => Self::Ubuntu,
                    "centos" => Self::CentOS,
                    "rocky" => Self::Rocky,
                    "alpine" => Self::Alpine,
                    "arch" => Self::Arch,
                    "fedora" => Self::Fedora,
                    _ => Self::Unknown,
                };
            }
        }
        Self::Linux
    }

    pub fn is_unix_like(self) -> bool {
        !matches!(self, Self::Windows)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortForwardKind {
    Local,
    Remote,
    Dynamic,
}

impl PortForwardKind {
    pub fn all() -> &'static [PortForwardKind] {
        &[
            PortForwardKind::Local,
            PortForwardKind::Remote,
            PortForwardKind::Dynamic,
        ]
    }

    #[allow(
        dead_code,
        reason = "unused helper; kept beside the CLI flag names it mirrors"
    )]
    pub fn flag(self) -> &'static str {
        match self {
            Self::Local => "-L",
            Self::Remote => "-R",
            Self::Dynamic => "-D",
        }
    }
}

impl fmt::Display for PortForwardKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local => f.write_str(t!("port_forward_kind.local").as_ref()),
            Self::Remote => f.write_str(t!("port_forward_kind.remote").as_ref()),
            Self::Dynamic => f.write_str(t!("port_forward_kind.dynamic").as_ref()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SnippetPackage {
    pub id: SnippetPackageId,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct Snippet {
    pub id: SnippetId,
    pub description: String,
    pub script: String,
    pub package_id: Option<SnippetPackageId>,
    pub last_used_at: String,
}

#[derive(Debug, Clone)]
pub struct PortForward {
    pub id: PortForwardId,
    pub name: String,
    pub kind: PortForwardKind,
    pub host_id: HostId,
    pub bind_address: String,
    pub listen_port: u16,
    pub dest_host: String,
    pub dest_port: u16,
}

impl PortForward {
    #[allow(dead_code, reason = "unused helper; kept beside `flag`")]
    pub fn spec(&self) -> String {
        let bind = self.bind_address.trim();
        match self.kind {
            PortForwardKind::Dynamic => {
                if bind.is_empty() {
                    format!("{}", self.listen_port)
                } else {
                    format!("{}:{}", bind, self.listen_port)
                }
            }
            PortForwardKind::Local | PortForwardKind::Remote => {
                if bind.is_empty() {
                    format!("{}:{}:{}", self.listen_port, self.dest_host, self.dest_port)
                } else {
                    format!(
                        "{}:{}:{}:{}",
                        bind, self.listen_port, self.dest_host, self.dest_port
                    )
                }
            }
        }
    }

    pub fn human_summary(&self) -> String {
        let bind = if self.bind_address.trim().is_empty() {
            "*".to_string()
        } else {
            self.bind_address.clone()
        };
        match self.kind {
            PortForwardKind::Local => format!(
                "{}:{} → {}:{}",
                bind, self.listen_port, self.dest_host, self.dest_port
            ),
            PortForwardKind::Remote => format!(
                "server {}:{} → {}:{}",
                bind, self.listen_port, self.dest_host, self.dest_port
            ),
            PortForwardKind::Dynamic => format!("SOCKS {}:{}", bind, self.listen_port),
        }
    }
}

/// Single-select filter for the vaults host list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum HostTagFilter {
    /// Show every host regardless of tags.
    #[default]
    All,
    /// Hosts carrying a specific tag (exact, trimmed match).
    Tag(String),
}

impl HostTagFilter {
    /// Whether `host` passes this filter.
    pub fn matches(&self, host: &Host) -> bool {
        match self {
            HostTagFilter::All => true,
            HostTagFilter::Tag(name) => host.tags.iter().any(|t| t == name),
        }
    }

    /// The tag name when filtering by a single tag (`None` otherwise).
    pub fn active_tag(&self) -> Option<&str> {
        match self {
            HostTagFilter::Tag(name) => Some(name.as_str()),
            HostTagFilter::All => None,
        }
    }
}

/// Trim a tag name typed by the user; `None` when it is (or becomes) empty.
pub fn clean_tag(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

const fn tag_rgb(hex: u32) -> iced::Color {
    iced::Color {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

/// Fixed palette used to color-code tags by name.
pub const TAG_COLORS: [iced::Color; 10] = [
    tag_rgb(0x00ef_4444), // red
    tag_rgb(0x00f9_7316), // orange
    tag_rgb(0x00ea_b308), // yellow
    tag_rgb(0x0022_c55e), // green
    tag_rgb(0x0014_b8a6), // teal
    tag_rgb(0x003b_82f6), // blue
    tag_rgb(0x0063_66f1), // indigo
    tag_rgb(0x00a8_55f7), // purple
    tag_rgb(0x00ec_4899), // pink
    tag_rgb(0x0064_748b), // slate
];

/// Deterministic color for a tag name. FNV-1a over the lowercased name so the
/// same tag always maps to the same color (and "Prod" == "prod") across runs.
pub fn tag_color(name: &str) -> iced::Color {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.to_lowercase().bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    TAG_COLORS[(hash % TAG_COLORS.len() as u64) as usize]
}

#[derive(Clone)]
pub struct Host {
    pub id: HostId,
    pub name: String,
    pub hostname: String,
    pub port: u16,
    pub protocol: Protocol,
    pub username: String,
    pub auth_type: AuthType,
    /// Saved host used as the single-hop `ProxyJump` for this target. `None`
    /// means connect directly to `hostname:port`.
    pub jump_host_id: Option<HostId>,
    /// Reference to a saved identity (username+password credential) this host
    /// authenticates with. When `Some`, SSH auth resolves the identity's
    /// credentials; `username`/`password` below stay in sync as a snapshot.
    pub identity_id: Option<IdentityId>,
    pub key_path: Option<String>,
    /// Passphrase for an encrypted private key. Kept in memory only for the
    /// current connection attempt; it is never persisted to the host store.
    /// Materialized only at the russh auth boundary.
    pub key_passphrase: Option<SecretString>,
    /// Saved SSH password. Held in a zeroizing container so the plaintext is
    /// wiped on drop; materialize it only at the russh auth boundary via
    /// [`secrecy::ExposeSecret::expose_secret`].
    pub password: Option<SecretString>,
    pub os_type: OsType,
    /// Free-form tags this host belongs to. A host with no tags is
    /// "uncategorized". Persisted in the `hosts.tags` column as JSON.
    pub tags: Vec<String>,
    /// Session-level expected host key fingerprint override.
    /// When set, `check_server_key` uses this instead of the
    /// stored `known_hosts` entry.  Used by "Continue" to verify
    /// the key without persisting it.
    #[allow(dead_code, reason = "reserved for the host-key override flow")]
    pub expected_fingerprint: Option<String>,
}

impl fmt::Debug for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("hostname", &self.hostname)
            .field("port", &self.port)
            .field("protocol", &self.protocol)
            .field("username", &self.username)
            .field("auth_type", &self.auth_type)
            .field("jump_host_id", &self.jump_host_id)
            .field("identity_id", &self.identity_id)
            .field("key_path", &self.key_path)
            .field(
                "key_passphrase",
                &self.key_passphrase.as_ref().map(|_| "<redacted>"),
            )
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("os_type", &self.os_type)
            .field("tags", &self.tags)
            .field("expected_fingerprint", &self.expected_fingerprint)
            .finish()
    }
}

impl Host {
    /// Whether a non-empty password is saved (used to decide if the connect
    /// flow must prompt for one).
    pub fn has_password(&self) -> bool {
        self.password
            .as_ref()
            .is_some_and(|p| !p.expose_secret().is_empty())
    }

    /// Materialize the saved password as plaintext for the edit form.
    /// UI boundary only — callers should drop the result as soon as possible.
    pub fn password_for_edit(&self) -> String {
        self.password
            .as_ref()
            .map(|p| p.expose_secret().to_string())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct KnownHost {
    /// 1-indexed line number in `~/.ssh/known_hosts`. 0 means not yet persisted.
    pub line_number: usize,
    /// Display hostname. For hashed entries this is `"(hashed)"`.
    pub hostname: String,
    /// Port decoded from the host pattern (22 if not explicitly specified).
    pub port: u16,
    /// OpenSSH key type string, e.g. `"ssh-ed25519"`.
    pub key_type: String,
    /// Full `"key-type base64-data"` string as it appears in the file.
    pub public_key: String,
    /// SHA256 fingerprint for display / verification.
    pub fingerprint: String,
    /// Optional marker: `"@cert-authority"` or `"@revoked"`.
    pub marker: Option<String>,
    /// The original host pattern as it appears in the file, preserved for
    /// display and for rebuilding the file during delete operations.
    pub raw_pattern: String,
}

#[allow(
    clippy::upper_case_acronyms,
    reason = "the acronym is the protocol's own spelling"
)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KeyType {
    #[default]
    ED25519,
    ECDSA,
    RSA,
}

impl KeyType {
    pub fn all() -> [KeyType; 3] {
        [KeyType::ED25519, KeyType::ECDSA, KeyType::RSA]
    }

    pub fn label(self) -> &'static str {
        match self {
            KeyType::ED25519 => "ED25519",
            KeyType::ECDSA => "ECDSA",
            KeyType::RSA => "RSA",
        }
    }

    pub fn ssh_type(self) -> &'static str {
        match self {
            KeyType::ED25519 => "ed25519",
            KeyType::ECDSA => "ecdsa",
            KeyType::RSA => "rsa",
        }
    }

    pub fn ssh_key_type(self) -> &'static str {
        match self {
            KeyType::ED25519 => "ssh-ed25519",
            KeyType::ECDSA => "ecdsa-sha2-nistp256",
            KeyType::RSA => "ssh-rsa",
        }
    }
}

#[derive(Debug, Clone)]
pub struct KeychainEntry {
    pub id: KeychainEntryId,
    pub name: String,
    pub key_type: String,
    pub fingerprint: String,
    pub public_key: String,
    pub path: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Identity {
    pub id: IdentityId,
    pub label: String,
    pub username: String,
    pub password: SecretString,
    pub created_at: String,
}

impl Identity {
    /// Materialize the password as plaintext for the edit form.
    /// UI boundary only — callers should drop the result as soon as possible.
    pub fn password_for_edit(&self) -> String {
        self.password.expose_secret().to_string()
    }
}

/// Captured server key info returned from SSH authentication.
#[derive(Debug, Clone)]
pub struct ServerKeyInfo {
    pub hostname: String,
    pub port: u16,
    pub key_type: String,
    pub public_key: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEntryType {
    Folder,
    File,
    Symlink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SftpSortColumn {
    Name,
    DateModified,
    Size,
    Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SftpSortDirection {
    Ascending,
    Descending,
}

impl SftpSortDirection {
    pub fn toggle(self) -> Self {
        match self {
            SftpSortDirection::Ascending => SftpSortDirection::Descending,
            SftpSortDirection::Descending => SftpSortDirection::Ascending,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub file_type: FileEntryType,
    pub size: Option<u64>,
    pub modified: String,
    pub modified_epoch: Option<u64>,
    pub permissions: String,
    pub kind: String,
}

/// Permission text used when the platform or SFTP server did not provide a
/// usable permission mode.
pub const UNKNOWN_PERMISSIONS: &str = "—";

pub fn read_local_dir(path: &str, show_hidden: bool) -> Vec<FileEntry> {
    let _guard = crate::core::diagnostics::perf_guard_dyn(
        Level::Debug,
        "IO",
        format!("read_local_dir({path})"),
        std::time::Duration::from_millis(100),
    );
    let mut entries = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(path) else {
        return entries;
    };

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }

        if let Some(entry) = read_local_file_entry(&entry.path()) {
            entries.push(entry);
        }
    }

    sort_file_entries(&mut entries);

    entries
}

pub fn read_local_file_entry(path: &Path) -> Option<FileEntry> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let is_symlink = path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    let is_dir = path.is_dir();
    let metadata = path.metadata().ok();

    let file_type = if is_symlink {
        FileEntryType::Symlink
    } else if is_dir {
        FileEntryType::Folder
    } else {
        FileEntryType::File
    };

    let size = metadata.as_ref().map(std::fs::Metadata::len);

    let modified_epoch = metadata
        .as_ref()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let modified = metadata
        .as_ref()
        .and_then(|metadata| metadata.modified().ok())
        .map_or_else(|| "—".to_string(), format_system_time);

    let permissions = format_permissions(path, metadata.as_ref(), &file_type);

    let kind = match &file_type {
        FileEntryType::Folder => "Folder".to_string(),
        FileEntryType::Symlink => "Symlink".to_string(),
        FileEntryType::File => path.extension().map_or_else(
            || "File".to_string(),
            |extension| format!("{} File", extension.to_string_lossy().to_uppercase()),
        ),
    };

    Some(FileEntry {
        name,
        size: if file_type == FileEntryType::Folder {
            None
        } else {
            size
        },
        file_type,
        modified,
        modified_epoch,
        permissions,
        kind,
    })
}

pub fn sort_file_entries(entries: &mut [FileEntry]) {
    sort_file_entries_by(entries, SftpSortColumn::Name, SftpSortDirection::Ascending);
}

pub fn sort_file_entries_by(
    entries: &mut [FileEntry],
    column: SftpSortColumn,
    direction: SftpSortDirection,
) {
    entries.sort_by(|a, b| {
        // Folders always come first
        let a_dir = a.file_type == FileEntryType::Folder;
        let b_dir = b.file_type == FileEntryType::Folder;
        let dir_cmp = b_dir.cmp(&a_dir);

        if dir_cmp != std::cmp::Ordering::Equal {
            return dir_cmp;
        }

        let cmp = match column {
            SftpSortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SftpSortColumn::DateModified => a.modified_epoch.cmp(&b.modified_epoch),
            SftpSortColumn::Size => {
                // Treat None size (folders) as 0 for sorting within folders
                let a_size = a.size.unwrap_or(0);
                let b_size = b.size.unwrap_or(0);
                a_size.cmp(&b_size)
            }
            SftpSortColumn::Kind => a.kind.to_lowercase().cmp(&b.kind.to_lowercase()),
        };

        match direction {
            SftpSortDirection::Ascending => cmp,
            SftpSortDirection::Descending => cmp.reverse(),
        }
    });
}

/// Soft cap for in-app text previews. 1 MiB covers ~99% of logs/configs while
/// avoiding a UI freeze when someone double-clicks a giant file by accident.
pub const TEXT_PREVIEW_MAX_BYTES: u64 = 1024 * 1024;

/// Hard file size limit for preview. Files larger than this are refused
/// upfront to avoid wasting bandwidth reading metadata/contents over SFTP.
pub const PREVIEW_MAX_FILE_SIZE: u64 = 60 * 1024 * 1024;

pub struct TextPreviewContent {
    pub text: String,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileNameKind {
    Text,
    Binary,
    Unknown,
}

impl FileNameKind {
    pub fn allows_text_preview(self) -> bool {
        matches!(self, FileNameKind::Text | FileNameKind::Unknown)
    }
}

/// Classify a filename into Text / Binary / Unknown.
///
/// Files we recognize as text by name/extension are previewed directly.
/// Files we recognize as binary (archives, images, executables, ...) skip
/// the preview entirely and fall back to transfer-on-double-click.
/// Unknown names (e.g. extensionless files of mystery origin) attempt the
/// preview — the UTF-8 + null-byte heuristic in `finalize_text_preview`
/// rejects anything that turns out to be binary.
pub fn classify_file_name(name: &str) -> FileNameKind {
    const DOTFILE_PREFIXES: &[&str] = &[
        ".env",
        ".bashrc",
        ".zshrc",
        ".profile",
        ".bash_profile",
        ".gitignore",
        ".gitattributes",
        ".gitconfig",
        ".editorconfig",
        ".npmrc",
        ".yarnrc",
        ".dockerignore",
        ".eslintrc",
        ".prettierrc",
        ".nvmrc",
    ];

    const EXTENSIONLESS_TEXT_NAMES: &[&str] = &[
        "readme",
        "license",
        "licence",
        "changelog",
        "authors",
        "contributors",
        "todo",
        "copying",
        "install",
        "news",
        "notice",
        "makefile",
        "gnumakefile",
        "dockerfile",
        "containerfile",
        "vagrantfile",
        "rakefile",
        "gemfile",
        "procfile",
        "jenkinsfile",
    ];

    const TEXT_EXTS: &[&str] = &[
        "txt",
        "md",
        "markdown",
        "rst",
        "adoc",
        "tex",
        "log",
        "conf",
        "cfg",
        "config",
        "ini",
        "yaml",
        "yml",
        "json",
        "json5",
        "jsonc",
        "toml",
        "xml",
        "svg",
        "html",
        "htm",
        "css",
        "scss",
        "sass",
        "less",
        "js",
        "mjs",
        "cjs",
        "ts",
        "tsx",
        "jsx",
        "vue",
        "svelte",
        "astro",
        "py",
        "pyw",
        "pyi",
        "rb",
        "go",
        "rs",
        "c",
        "h",
        "cc",
        "cpp",
        "hpp",
        "hxx",
        "cxx",
        "m",
        "mm",
        "java",
        "kt",
        "kts",
        "scala",
        "swift",
        "dart",
        "lua",
        "php",
        "pl",
        "pm",
        "r",
        "jl",
        "ex",
        "exs",
        "erl",
        "hrl",
        "elm",
        "clj",
        "cljs",
        "edn",
        "sh",
        "bash",
        "zsh",
        "fish",
        "bat",
        "ps1",
        "psm1",
        "cmd",
        "sql",
        "graphql",
        "gql",
        "proto",
        "csv",
        "tsv",
        "env",
        "properties",
        "dockerfile",
        "diff",
        "patch",
        "lock",
        "sum",
        "service",
        "desktop",
        "gradle",
        "bazel",
        "bzl",
        "cmake",
        "nix",
        "plist",
        "xaml",
    ];

    const BINARY_EXTS: &[&str] = &[
        // archives
        "zip", "tar", "tgz", "gz", "bz2", "xz", "zst", "7z", "rar", "lz", "lzma", "lz4", "lzo",
        "cab", "ar", "deb", "rpm", "apk", "ipa", "jar", "war", "ear", "msi", "dmg", "pkg", "iso",
        "img", "vhd", "vmdk", // images
        "png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "ico", "webp", "heic", "heif", "avif",
        "psd", "ai", "eps", "raw", "cr2", "nef", "arw", "dng", // audio / video
        "mp3", "m4a", "aac", "flac", "ogg", "opus", "wav", "wma", "aiff", "mp4", "m4v", "mov",
        "mkv", "webm", "avi", "wmv", "flv", "mpg", "mpeg", "3gp",
        // documents (binary formats)
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf",
        // executables / objects
        "exe", "dll", "so", "dylib", "a", "o", "obj", "lib", "class", "pyc", "pyo", "pyd", "wasm",
        "elf", "bin", // databases
        "db", "sqlite", "sqlite3", "mdb", "accdb", // fonts
        "ttf", "otf", "woff", "woff2", "eot",
    ];
    let lower = name.to_lowercase();

    for prefix in DOTFILE_PREFIXES {
        if lower == *prefix || lower.starts_with(&format!("{prefix}.")) {
            return FileNameKind::Text;
        }
    }

    if EXTENSIONLESS_TEXT_NAMES.contains(&lower.as_str()) {
        return FileNameKind::Text;
    }

    let ext = Path::new(&lower)
        .extension()
        .map(|e| e.to_string_lossy().into_owned());

    let Some(ext) = ext else {
        // No extension and not on the well-known text list — let the byte
        // sniffer decide.
        return FileNameKind::Unknown;
    };

    if TEXT_EXTS.contains(&ext.as_str()) {
        return FileNameKind::Text;
    }

    if BINARY_EXTS.contains(&ext.as_str()) {
        return FileNameKind::Binary;
    }

    FileNameKind::Unknown
}

pub fn read_local_text_preview(path: &Path) -> Result<TextPreviewContent, String> {
    use std::io::Read;
    let _guard = crate::core::diagnostics::perf_guard_dyn(
        Level::Debug,
        "IO",
        format!("read_local_text_preview({})", path.display()),
        std::time::Duration::from_millis(100),
    );

    let total_bytes = std::fs::metadata(path).ok().map(|m| m.len());
    if let Some(size) = total_bytes.filter(|s| *s > PREVIEW_MAX_FILE_SIZE) {
        return Err(t!(
            "sftp.file_too_large",
            size = format_size(size),
            limit = format_size(PREVIEW_MAX_FILE_SIZE),
        )
        .to_string());
    }
    let mut file =
        std::fs::File::open(path).map_err(|e| t!("sftp.file_open_failed", e = e).to_string())?;

    let cap = TEXT_PREVIEW_MAX_BYTES as usize + 1;
    let mut buffer = Vec::with_capacity(cap.min(64 * 1024));
    file.by_ref()
        .take(TEXT_PREVIEW_MAX_BYTES + 1)
        .read_to_end(&mut buffer)
        .map_err(|e| t!("sftp.file_read_failed", e = e).to_string())?;

    let truncated = buffer.len() as u64 > TEXT_PREVIEW_MAX_BYTES;
    if truncated {
        buffer.truncate(TEXT_PREVIEW_MAX_BYTES as usize);
    }

    finalize_text_preview(buffer, truncated, total_bytes)
}

pub fn finalize_text_preview(
    mut bytes: Vec<u8>,
    truncated: bool,
    total_bytes: Option<u64>,
) -> Result<TextPreviewContent, String> {
    // Trim partial UTF-8 char at the tail when truncated so `from_utf8`
    // doesn't spuriously reject an otherwise-valid file.
    if truncated {
        trim_trailing_utf8_partial(&mut bytes);
    }

    if looks_binary(&bytes) {
        return Err(binary_preview_error(&rust_i18n::locale()));
    }

    let text = String::from_utf8(bytes).map_err(|_error| t!("sftp.utf8_no_preview").to_string())?;

    Ok(TextPreviewContent { text, total_bytes })
}

fn binary_preview_error(locale: &str) -> String {
    t!("sftp.binary_no_preview", locale = locale).to_string()
}

fn looks_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    // A single NUL byte in the first 8 KiB is a strong signal — `file(1)` and
    // most editors use the same heuristic.
    let sniff_len = bytes.len().min(8192);
    bytes[..sniff_len].contains(&0)
}

fn trim_trailing_utf8_partial(bytes: &mut Vec<u8>) {
    if let Err(e) = std::str::from_utf8(bytes) {
        bytes.truncate(e.valid_up_to());
    }
}

pub fn format_speed(bytes_per_second: f64) -> String {
    if bytes_per_second < 1024.0 {
        format!("{bytes_per_second:.0} B/s")
    } else if bytes_per_second < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", bytes_per_second / 1024.0)
    } else if bytes_per_second < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bytes_per_second / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB/s", bytes_per_second / (1024.0 * 1024.0 * 1024.0))
    }
}

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(unix)]
fn format_permissions(
    _path: &Path,
    metadata: Option<&std::fs::Metadata>,
    file_type: &FileEntryType,
) -> String {
    use std::os::unix::fs::PermissionsExt;
    match metadata {
        Some(m) => {
            let mode = m.permissions().mode();
            let ft = match file_type {
                FileEntryType::Folder => 'd',
                FileEntryType::Symlink => 'l',
                FileEntryType::File => '-',
            };
            let bits = [
                (0o400, 'r'),
                (0o200, 'w'),
                (0o100, 'x'),
                (0o040, 'r'),
                (0o020, 'w'),
                (0o010, 'x'),
                (0o004, 'r'),
                (0o002, 'w'),
                (0o001, 'x'),
            ];
            let perm: String = bits
                .iter()
                .map(|&(bit, ch)| if mode & bit != 0 { ch } else { '-' })
                .collect();
            format!("{ft}{perm}")
        }
        None => UNKNOWN_PERMISSIONS.to_string(),
    }
}

#[cfg(target_os = "windows")]
fn format_permissions(
    path: &Path,
    _metadata: Option<&std::fs::Metadata>,
    file_type: &FileEntryType,
) -> String {
    crate::core::windows_permissions::format_permissions(path, *file_type == FileEntryType::Folder)
}

#[cfg(not(any(unix, target_os = "windows")))]
fn format_permissions(
    _path: &Path,
    _metadata: Option<&std::fs::Metadata>,
    _file_type: &FileEntryType,
) -> String {
    UNKNOWN_PERMISSIONS.to_string()
}

/// Recursively walk a local directory, returning (`file_count`, `total_size`).
/// Symlinks are skipped. Directories are not counted as files.
pub fn count_files_recursive(path: &Path) -> Result<(u64, u64), String> {
    let _guard = crate::core::diagnostics::perf_guard_dyn(
        Level::Debug,
        "IO",
        format!("count_files_recursive({})", path.display()),
        std::time::Duration::from_millis(100),
    );
    let mut file_count = 0u64;
    let mut total_size = 0u64;

    let entries = std::fs::read_dir(path)
        .map_err(|e| t!("sftp.dir_read_failed", path = path.display(), e = e).to_string())?;

    for entry in entries {
        let entry = entry.map_err(|e| t!("sftp.entry_read_failed", e = e).to_string())?;
        let ft = entry
            .file_type()
            .map_err(|e| t!("sftp.filetype_failed", e = e).to_string())?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            let (sub_files, sub_size) = count_files_recursive(&entry.path())?;
            file_count += sub_files;
            total_size += sub_size;
        } else if ft.is_file() {
            file_count += 1;
            if let Ok(meta) = entry.metadata() {
                total_size += meta.len();
            }
        }
    }

    Ok((file_count, total_size))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_i18n::t;

    #[test]
    fn the_history_picker_spans_the_old_cap_up_to_the_ceiling() {
        assert_eq!(
            COMMAND_HISTORY_LIMITS.first(),
            Some(&100),
            "the fixed cap history used to have stays selectable"
        );
        assert_eq!(
            COMMAND_HISTORY_LIMITS.last(),
            Some(&COMMAND_HISTORY_MAX_LIMIT),
            "the ceiling is offered, so the picker needs no separate note about it"
        );
        assert!(
            COMMAND_HISTORY_LIMITS.contains(&COMMAND_HISTORY_DEFAULT_LIMIT),
            "the default has to be one of the choices"
        );
        assert!(
            COMMAND_HISTORY_LIMITS.is_sorted(),
            "the picker reads smaller to larger"
        );
    }

    #[test]
    fn local_shell_tokens_round_trip_without_translated_labels() {
        for shell in LocalShell::windows_options() {
            assert_eq!(LocalShell::from_str(shell.to_str()), Some(*shell));
        }
        assert_eq!(LocalShell::from_str("PowerShell 7"), None);
    }

    /// Locale-sensitive UI strings must resolve through the translation table.
    #[test]
    fn locale_sensitive_strings_resolve_for_the_requested_locale() {
        // A NUL byte is what `looks_binary` keys on.
        let binary = || match finalize_text_preview(vec![0x00, 0x01], false, None) {
            Err(e) => e,
            Ok(_) => panic!("NUL byte should be rejected as binary"),
        };

        assert!(!binary().is_empty());
        assert_eq!(LocalShell::CommandPrompt.label_for_locale("zh-CN"), "默认");
        assert_eq!(
            binary_preview_error("zh-CN"),
            "文件似乎是二进制文件，无法预览"
        );
        // Interpolated keys use `%{name}`, not `{name}` — a mismatch would
        // silently render the placeholder instead of the value.
        let too_large = t!(
            "sftp.file_too_large",
            locale = "zh-CN",
            size = "2 MB",
            limit = "1 MB"
        )
        .to_string();
        assert!(
            too_large.contains("2 MB"),
            "no size interpolation: {too_large}"
        );
        assert!(
            too_large.contains("1 MB"),
            "no limit interpolation: {too_large}"
        );

        assert_eq!(LocalShell::CommandPrompt.label_for_locale("zh-TW"), "預設");
        assert_eq!(LocalShell::CommandPrompt.label_for_locale("en"), "Default");
        assert_eq!(
            binary_preview_error("en"),
            "File appears to be binary and can't be previewed"
        );
    }

    use crate::core::test_support::scratch_dir;
    use std::fs;
    use std::path::PathBuf;

    /// A directory tree to read back, under the test scratch space:
    ///
    /// ```text
    /// root/
    ///   notes.txt      "hello"          (5 bytes)
    ///   README                          (3 bytes)
    ///   .hidden                         (6 bytes)
    ///   sub/
    ///     deep.log                      (4 bytes)
    ///   link -> notes.txt
    /// ```
    fn tree(tag: &str) -> PathBuf {
        let root = scratch_dir(&format!("termlet-data-{tag}"));
        fs::create_dir(root.join("sub")).expect("create subdirectory");
        fs::write(root.join("notes.txt"), b"hello").expect("write notes.txt");
        fs::write(root.join("README"), b"abc").expect("write README");
        fs::write(root.join(".hidden"), b"secret").expect("write .hidden");
        fs::write(root.join("sub/deep.log"), b"deep").expect("write deep.log");

        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("notes.txt"), root.join("link"))
            .expect("create symlink");

        root
    }

    fn names(entries: &[FileEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn a_listing_sorts_folders_first_and_hides_dotfiles_on_request() {
        let root = tree("listing");

        let visible = read_local_dir(root.to_str().unwrap(), false);
        assert_eq!(
            names(&visible)[0],
            "sub",
            "a folder sorts ahead of the files, whatever its name"
        );
        assert!(
            !names(&visible).contains(&".hidden"),
            "a dotfile stays out of the default listing"
        );
        assert!(
            names(&visible).contains(&"link") || cfg!(not(unix)),
            "a symlink is listed as its own entry: {:?}",
            names(&visible)
        );

        let all = read_local_dir(root.to_str().unwrap(), true);
        assert!(names(&all).contains(&".hidden"));
        assert_eq!(all.len(), visible.len() + 1);
    }

    #[test]
    fn a_listing_of_something_that_is_not_there_is_empty_rather_than_an_error() {
        let root = scratch_dir("termlet-data-missing");

        let missing = root.join("no-such-directory");

        assert!(read_local_dir(missing.to_str().unwrap(), true).is_empty());
        assert!(
            read_local_dir("/definitely/not/a/directory", true).is_empty(),
            "an unreadable path falls back to an empty listing"
        );
    }

    #[test]
    fn a_file_entry_carries_its_type_size_and_kind() {
        let root = tree("entry");

        let folder = read_local_file_entry(&root.join("sub")).expect("the folder reads");
        assert_eq!(folder.file_type, FileEntryType::Folder);
        assert_eq!(
            folder.size, None,
            "a folder reports no size, so the column stays blank"
        );
        assert_eq!(folder.kind, "Folder");
        #[cfg(unix)]
        {
            assert_eq!(folder.permissions.len(), 10);
            assert!(
                folder.permissions.starts_with('d'),
                "the permissions string marks a directory: {}",
                folder.permissions
            );
        };
        #[cfg(target_os = "windows")]
        assert!(
            folder.permissions == UNKNOWN_PERMISSIONS || folder.permissions.starts_with("d "),
            "Windows access should be read from the ACL or reported as unknown: {}",
            folder.permissions
        );

        let file = read_local_file_entry(&root.join("notes.txt")).expect("the file reads");
        assert_eq!(file.file_type, FileEntryType::File);
        assert_eq!(file.size, Some(5));
        assert_eq!(file.kind, "TXT File", "the extension names the kind");
        #[cfg(unix)]
        assert!(file.permissions.starts_with('-'));
        #[cfg(target_os = "windows")]
        assert!(
            file.permissions == UNKNOWN_PERMISSIONS || file.permissions.starts_with("- "),
            "Windows access should be read from the ACL or reported as unknown: {}",
            file.permissions
        );
        assert!(file.modified_epoch.is_some(), "a real file has a mtime");

        let extensionless = read_local_file_entry(&root.join("README")).expect("reads");
        assert_eq!(extensionless.kind, "File");

        #[cfg(unix)]
        {
            let link = read_local_file_entry(&root.join("link")).expect("the link reads");
            assert_eq!(
                link.file_type,
                FileEntryType::Symlink,
                "a symlink is not followed into a plain file"
            );
        }
    }

    #[test]
    fn counting_walks_subdirectories_and_skips_symlinks() {
        let root = tree("count");

        let (files, size) = count_files_recursive(&root).expect("the tree is readable");

        assert_eq!(files, 4, "notes.txt, README, .hidden and sub/deep.log");
        assert_eq!(size, 5 + 3 + 6 + 4);

        assert!(
            count_files_recursive(&root.join("nope")).is_err(),
            "an unreadable directory is reported instead of counted as empty"
        );
    }

    #[test]
    fn sorting_orders_by_the_chosen_column_and_folds_folders_first() {
        let file = |name: &str, size: u64, epoch: u64, kind: &str| FileEntry {
            name: name.to_string(),
            file_type: FileEntryType::File,
            size: Some(size),
            modified: String::new(),
            modified_epoch: Some(epoch),
            permissions: String::new(),
            kind: kind.to_string(),
        };
        let folder = |name: &str| FileEntry {
            name: name.to_string(),
            file_type: FileEntryType::Folder,
            size: None,
            modified: String::new(),
            modified_epoch: None,
            permissions: String::new(),
            kind: "Folder".to_string(),
        };

        let build = || {
            vec![
                file("beta.txt", 30, 200, "LOG File"),
                folder("zeta"),
                file("Alpha.txt", 10, 300, "TXT File"),
                folder("alpha"),
            ]
        };

        let sorted = |column, direction| {
            let mut entries = build();
            sort_file_entries_by(&mut entries, column, direction);
            names(&entries)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };

        assert_eq!(
            sorted(SftpSortColumn::Name, SftpSortDirection::Ascending),
            ["alpha", "zeta", "Alpha.txt", "beta.txt"],
            "names compare case-insensitively, and both folders lead"
        );
        assert_eq!(
            sorted(SftpSortColumn::Name, SftpSortDirection::Descending),
            ["zeta", "alpha", "beta.txt", "Alpha.txt"],
            "descending reverses the files but not the folders-first rule"
        );
        assert_eq!(
            sorted(SftpSortColumn::Size, SftpSortDirection::Ascending),
            ["zeta", "alpha", "Alpha.txt", "beta.txt"]
        );
        assert_eq!(
            sorted(SftpSortColumn::DateModified, SftpSortDirection::Ascending),
            ["zeta", "alpha", "beta.txt", "Alpha.txt"],
            "an unknown mtime sorts before a known one"
        );
        assert_eq!(
            sorted(SftpSortColumn::Kind, SftpSortDirection::Ascending),
            ["zeta", "alpha", "beta.txt", "Alpha.txt"],
            "a log sorts before a text file, and the two folders keep their order"
        );
    }

    #[test]
    fn sorting_a_listing_of_one_is_harmless() {
        let mut single = vec![FileEntry {
            name: "notes.txt".to_string(),
            file_type: FileEntryType::File,
            size: Some(1),
            modified: String::new(),
            modified_epoch: None,
            permissions: String::new(),
            kind: "TXT File".to_string(),
        }];

        sort_file_entries(&mut single);
        assert_eq!(names(&single), ["notes.txt"]);
    }

    #[test]
    fn a_name_is_classified_for_the_preview_by_its_shape_not_its_contents() {
        for text in [
            ".env.local",
            ".gitignore",
            "README",
            "Makefile",
            "notes.txt",
            "config.yaml",
            "main.rs",
            "index.tsx",
            "compose.yml",
        ] {
            assert_eq!(
                classify_file_name(text),
                FileNameKind::Text,
                "{text} previews as text"
            );
        }

        for binary in ["archive.tar.gz", "photo.png", "app.bin", "font.ttf"] {
            assert_eq!(
                classify_file_name(binary),
                FileNameKind::Binary,
                "{binary} skips the preview"
            );
        }

        for unknown in ["Makefile.am", "notes", "data.weird"] {
            assert_eq!(
                classify_file_name(unknown),
                FileNameKind::Unknown,
                "{unknown} falls through to the byte sniffer"
            );
        }

        assert!(FileNameKind::Text.allows_text_preview());
        assert!(
            FileNameKind::Unknown.allows_text_preview(),
            "an unknown name is still worth sniffing"
        );
        assert!(!FileNameKind::Binary.allows_text_preview());
    }

    #[test]
    fn a_release_file_names_the_distribution_and_a_missing_id_means_plain_linux() {
        let cases = [
            ("ID=debian\n", OsType::Debian),
            ("ID=\"ubuntu\"\nVERSION=\"24.04\"\n", OsType::Ubuntu),
            ("ID='rocky'\n", OsType::Rocky),
            ("ID=alpine\n", OsType::Alpine),
            ("ID=fedora\n", OsType::Fedora),
            ("ID=arch\n", OsType::Arch),
            ("ID=centos\n", OsType::CentOS),
            ("ID=linuxmint\n", OsType::Unknown),
            ("NAME=\"Debian GNU/Linux\"\n", OsType::Linux),
            ("", OsType::Linux),
        ];

        for (release, expected) in cases {
            assert_eq!(
                OsType::from_os_release(release),
                expected,
                "for release file {release:?}"
            );
        }
    }

    #[test]
    fn a_preview_reads_a_small_text_file_whole() {
        let root = tree("preview");
        let path = root.join("notes.txt");

        let preview = read_local_text_preview(&path).expect("a small text file previews");

        assert_eq!(preview.text, "hello");
        assert_eq!(preview.total_bytes, Some(5));

        assert!(
            read_local_text_preview(&root.join("nope.txt")).is_err(),
            "a missing file is reported rather than previewed as empty"
        );
    }

    #[test]
    fn a_preview_of_something_too_big_is_refused_before_it_is_read() {
        let root = scratch_dir("termlet-data-huge");
        let path = root.join("huge.bin");
        // Sparse: the size check only needs the metadata to be over the limit.
        let file = fs::File::create(&path).expect("create huge.bin");
        file.set_len(PREVIEW_MAX_FILE_SIZE + 1).expect("grow it");
        drop(file);

        let Err(error) = read_local_text_preview(&path) else {
            panic!("a file over the preview limit should be refused unread")
        };

        assert!(
            error.contains(&format_size(PREVIEW_MAX_FILE_SIZE)),
            "the refusal names the limit: {error}"
        );
    }

    #[test]
    fn command_history_is_the_leftmost_right_sidebar_tab() {
        assert_eq!(
            RightSidebarTab::all(),
            &[
                RightSidebarTab::Commands,
                RightSidebarTab::Snippets,
                RightSidebarTab::Theme,
            ]
        );
    }
}
