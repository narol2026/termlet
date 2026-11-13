// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use russh::client::{self, AuthResult, KeyboardInteractiveAuthResponse, Msg as ClientMsg};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, load_secret_key, ssh_key};
use russh::{Channel, ChannelStream, Disconnect, MethodSet};
use russh_sftp::client::SftpSession;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::core::data::{AuthType, Host};
use crate::core::diagnostics::Level;
use crate::ssh::known_hosts_file;
use rust_i18n::t;
use secrecy::{ExposeSecret, SecretString};

use super::connection_pool::{
    MAX_SESSIONS_PER_HOST, MAX_TOTAL_SESSIONS, SESSION_IDLE_TIMEOUT, SessionLease, SshSessions,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const SFTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn ssh_config(inactivity_timeout: Option<Duration>) -> Arc<client::Config> {
    Arc::new(client::Config {
        inactivity_timeout,
        keepalive_interval: inactivity_timeout
            .is_none()
            .then_some(Duration::from_secs(15)),
        keepalive_max: 1,
        nodelay: true,
        ..Default::default()
    })
}

struct HandshakeSuccess {
    session: client::Handle<Client>,
    jump: Option<client::Handle<Client>>,
    server_key: Arc<Mutex<Option<ssh_key::PublicKey>>>,
}

struct HandshakeFailure {
    timeout: bool,
    error: Option<String>,
    key_info: Option<crate::core::data::ServerKeyInfo>,
    key_check: Option<KeyCheckResult>,
    expected_fingerprints: Option<HashSet<String>>,
}

async fn direct_transport(
    host: &Host,
) -> Result<(tokio::net::TcpStream, Option<client::Handle<Client>>), String> {
    let address = format!("{}:{}", host.hostname, host.port);
    let stream = tokio::net::TcpStream::connect(address)
        .await
        .map_err(|error| t!("ssh.connect_failed", e = error).to_string())?;
    if let Err(error) = stream.set_nodelay(true) {
        crate::core::diagnostics::log(
            Level::Warn,
            "SSH",
            format!(
                "failed to enable TCP_NODELAY for {}:{}: {error}",
                host.hostname, host.port
            ),
        );
    }
    Ok((stream, None))
}

async fn jump_transport(
    jump_host: &Host,
    target: &Host,
) -> Result<(ChannelStream<ClientMsg>, Option<client::Handle<Client>>), String> {
    let jump_session = open_authenticated_session_with(jump_host, Client::new()).await?;
    let channel = jump_session
        .channel_open_direct_tcpip(
            target.hostname.clone(),
            u32::from(target.port),
            "127.0.0.1".to_string(),
            0,
        )
        .await
        .map_err(|error| {
            format!(
                "failed to open jump channel through {} to {}:{}: {error}",
                jump_host.hostname, target.hostname, target.port
            )
        })?;
    let stream = channel.into_stream();
    Ok((stream, Some(jump_session)))
}

async fn direct_probe_transport(
    hostname: &str,
    port: u16,
) -> Result<(tokio::net::TcpStream, Option<client::Handle<Client>>), String> {
    let address = format!("{hostname}:{port}");
    let stream = tokio::net::TcpStream::connect(address)
        .await
        .map_err(|error| t!("ssh.connect_failed", e = error).to_string())?;
    Ok((stream, None))
}

async fn jump_probe_transport(
    jump_host: &Host,
    hostname: String,
    port: u16,
) -> Result<(ChannelStream<ClientMsg>, Option<client::Handle<Client>>), String> {
    let jump_session = open_authenticated_session_with(jump_host, Client::new()).await?;
    let channel = jump_session
        .channel_open_direct_tcpip(
            hostname.clone(),
            u32::from(port),
            "127.0.0.1".to_string(),
            0,
        )
        .await
        .map_err(|error| format!("failed to open jump channel to {hostname}:{port}: {error}"))?;
    let stream = channel.into_stream();
    Ok((stream, Some(jump_session)))
}

async fn connect_handshake<R, F>(
    host: &Host,
    handler: Client,
    inactivity_timeout: Option<Duration>,
    transport: F,
) -> Result<HandshakeSuccess, Box<HandshakeFailure>>
where
    R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    F: Future<Output = Result<(R, Option<client::Handle<Client>>), String>>,
{
    let mut handler = handler;
    handler.expected_fingerprints = expected_fingerprints(host);
    let expected_fingerprints = handler.expected_fingerprints.clone();
    let server_key_handle = handler.server_key_handle();
    let key_check_handle = handler.key_check_result_handle();

    let (stream, jump) = match transport.await {
        Ok(transport) => transport,
        Err(error) => {
            return Err(Box::new(HandshakeFailure {
                timeout: false,
                error: Some(error),
                key_info: None,
                key_check: None,
                expected_fingerprints,
            }));
        }
    };

    let config = ssh_config(inactivity_timeout);
    let connect_result = tokio::time::timeout(
        CONNECT_TIMEOUT,
        client::connect_stream(config, stream, handler),
    )
    .await;

    match connect_result {
        Ok(Ok(session)) => Ok(HandshakeSuccess {
            session,
            jump,
            server_key: server_key_handle,
        }),
        Ok(Err(error)) => {
            let key_info = server_key_handle
                .lock()
                .ok()
                .and_then(|mut guard| guard.take())
                .map(|pk| extract_server_key_info(&host.hostname, host.port, &pk));
            let key_check = key_check_handle.lock().ok().as_deref().cloned();
            Err(Box::new(HandshakeFailure {
                timeout: false,
                error: Some(error.to_string()),
                key_info,
                key_check,
                expected_fingerprints,
            }))
        }
        Err(_) => {
            let key_info = server_key_handle
                .lock()
                .ok()
                .and_then(|mut guard| guard.take())
                .map(|pk| extract_server_key_info(&host.hostname, host.port, &pk));
            let key_check = key_check_handle.lock().ok().as_deref().cloned();
            Err(Box::new(HandshakeFailure {
                timeout: true,
                error: None,
                key_info,
                key_check,
                expected_fingerprints,
            }))
        }
    }
}

fn simple_handshake_error(host: &Host, failure: Box<HandshakeFailure>) -> String {
    match failure.key_check {
        Some(KeyCheckResult::FirstTime) if failure.key_info.is_some() => {
            return t!("ssh.host_key_required").to_string();
        }
        Some(KeyCheckResult::Mismatch { .. }) => {
            return t!(
                "ssh.host_key_changed",
                host = &host.hostname,
                port = host.port
            )
            .to_string();
        }
        _ => {}
    }
    if failure.timeout {
        t!(
            "ssh.connect_timeout",
            host = &host.hostname,
            port = host.port
        )
        .to_string()
    } else {
        t!("ssh.connect_failed", e = failure.error.unwrap_or_default()).to_string()
    }
}

/// Load the saved host referenced by `host.jump_host_id`, if any.
pub(crate) fn load_jump_host(host: &Host) -> Result<Option<Host>, String> {
    let Some(jump_host_id) = host.jump_host_id else {
        return Ok(None);
    };
    let storage = crate::core::storage::Storage::open()?;
    storage
        .load_host(jump_host_id)?
        .ok_or_else(|| format!("jump host id {jump_host_id} not found in local store"))
        .map(Some)
}

/// Process-lifetime tokio runtime used for pooled SFTP work.
///
/// The previous code created a fresh current-thread runtime per operation.
/// A persistent runtime is required for connection pooling: russh spawns the
/// session driver on the runtime that starts it, so a pooled handle would die
/// as soon as a per-operation runtime was dropped.
static SSH_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static SSH_SESSION_POOL: OnceLock<Arc<SshSessions>> = OnceLock::new();

pub(super) fn ssh_runtime() -> &'static tokio::runtime::Runtime {
    SSH_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("termlet-ssh")
            .enable_all()
            .build()
            .expect("failed to build termlet SSH runtime")
    })
}

fn ssh_session_pool() -> &'static Arc<SshSessions> {
    SSH_SESSION_POOL.get_or_init(|| {
        let pool = Arc::new(SshSessions::new(
            SESSION_IDLE_TIMEOUT,
            MAX_TOTAL_SESSIONS,
            MAX_SESSIONS_PER_HOST,
        ));
        pool.start_reaper();
        pool
    })
}

/// Outcome of the server host-key check performed during the SSH handshake.
/// Used after a connection attempt to decide whether the user must be prompted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum KeyCheckResult {
    /// No stored fingerprint exists — first-time connection.
    #[default]
    FirstTime,
    /// The presented key matched the stored fingerprint.
    Match,
    /// The presented key did NOT match the stored fingerprint.
    Mismatch { expected: String, presented: String },
}

pub(crate) struct Client {
    forwarded_tcpip: Option<mpsc::UnboundedSender<ForwardedTcpIp>>,
    server_key: Arc<Mutex<Option<ssh_key::PublicKey>>>,
    expected_fingerprints: Option<HashSet<String>>,
    /// When true, `check_server_key` captures the server key but returns
    /// `Ok(false)` so the connection is aborted before any authentication
    /// data is exchanged.  Used to probe an unknown host's key.
    probe_only: bool,
    /// Records the outcome of the server key check so callers can
    /// inspect it after `client::connect()` returns (or fails).
    key_check_result: Arc<Mutex<KeyCheckResult>>,
}

impl Client {
    pub(crate) fn new() -> Self {
        Self {
            forwarded_tcpip: None,
            server_key: Arc::new(Mutex::new(None)),
            expected_fingerprints: None,
            probe_only: false,
            key_check_result: Arc::new(Mutex::new(KeyCheckResult::default())),
        }
    }

    pub(crate) fn with_forwarded_tcpip(sender: mpsc::UnboundedSender<ForwardedTcpIp>) -> Self {
        Self {
            forwarded_tcpip: Some(sender),
            server_key: Arc::new(Mutex::new(None)),
            expected_fingerprints: None,
            probe_only: false,
            key_check_result: Arc::new(Mutex::new(KeyCheckResult::default())),
        }
    }

    /// Build a handler that captures the server key and then immediately
    /// aborts the handshake — no authentication data is ever sent.
    pub(crate) fn new_probe() -> Self {
        let mut c = Self::new();
        c.probe_only = true;
        c
    }

    pub(crate) fn server_key_handle(&self) -> Arc<Mutex<Option<ssh_key::PublicKey>>> {
        std::sync::Arc::clone(&self.server_key)
    }

    pub(crate) fn key_check_result_handle(&self) -> Arc<Mutex<KeyCheckResult>> {
        std::sync::Arc::clone(&self.key_check_result)
    }
}

/// Build the set of fingerprints that are considered trusted for a host.
///
/// When the user has explicitly pinned a fingerprint on the host, only that
/// fingerprint is trusted. Otherwise every key recorded for `hostname:port`
/// in `~/.ssh/known_hosts` is trusted, matching OpenSSH's behavior of
/// accepting a presented key that matches any stored entry.
fn expected_fingerprints(host: &Host) -> Option<HashSet<String>> {
    let fingerprints: HashSet<String> = match host.expected_fingerprint.as_deref() {
        Some(fingerprint) => std::iter::once(fingerprint.to_string()).collect(),
        None => known_hosts_file::fingerprints(&host.hostname, host.port)
            .into_iter()
            .collect(),
    };
    if fingerprints.is_empty() {
        None
    } else {
        Some(fingerprints)
    }
}

/// Join a set of fingerprints into a deterministic, human-readable string.
fn sorted_fingerprints(fingerprints: &HashSet<String>) -> String {
    let mut sorted: Vec<&String> = fingerprints.iter().collect();
    sorted.sort();
    sorted
        .into_iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Classify a presented server key against the trusted fingerprint set.
fn classify_presented_key(expected: Option<&HashSet<String>>, presented: &str) -> KeyCheckResult {
    match expected {
        Some(fingerprints) if !fingerprints.contains(presented) => KeyCheckResult::Mismatch {
            expected: sorted_fingerprints(fingerprints),
            presented: presented.to_string(),
        },
        Some(_) => KeyCheckResult::Match,
        None => KeyCheckResult::FirstTime,
    }
}

/// Forwarded `direct-tcpip` channel delivered by the server when a remote
/// listener accepts a connection (used by `-R` port forwards).
pub(crate) struct ForwardedTcpIp {
    pub channel: Channel<ClientMsg>,
    #[allow(
        dead_code,
        reason = "captured from the forwarded channel; not read yet"
    )]
    pub connected_address: String,
    #[allow(
        dead_code,
        reason = "captured from the forwarded channel; not read yet"
    )]
    pub connected_port: u32,
}

/// An authenticated SSH session that may be reached through a jump host.
///
/// russh closes a session when its [`client::Handle`] is dropped, so a
/// jump-chained session must keep the jump handle alive for as long as the
/// target handle exists. This wrapper owns both; normal callers interact with
/// it through the target handle via [`Deref`].
pub(crate) struct AuthenticatedSession {
    handle: client::Handle<Client>,
    _jump: Option<client::Handle<Client>>,
}

impl AuthenticatedSession {
    fn direct(handle: client::Handle<Client>) -> Self {
        Self {
            handle,
            _jump: None,
        }
    }

    fn chained(handle: client::Handle<Client>, jump: client::Handle<Client>) -> Self {
        Self {
            handle,
            _jump: Some(jump),
        }
    }
}

impl Deref for AuthenticatedSession {
    type Target = client::Handle<Client>;

    fn deref(&self) -> &Self::Target {
        &self.handle
    }
}

impl DerefMut for AuthenticatedSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.handle
    }
}

impl client::Handler for Client {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        // Always capture the presented key so callers can inspect it later.
        if let Ok(mut key) = self.server_key.lock() {
            *key = Some(server_public_key.clone());
        }

        let presented = server_public_key.fingerprint(HashAlg::Sha256).to_string();

        // Probe-only: capture the key, then abort before authentication.
        if self.probe_only {
            if let Ok(mut result) = self.key_check_result.lock() {
                *result = KeyCheckResult::FirstTime;
            }
            return std::future::ready(Ok(false));
        }

        let result = classify_presented_key(self.expected_fingerprints.as_ref(), &presented);
        // Authentication is allowed only after a known key or an explicit session pin matches.
        let accept = matches!(result, KeyCheckResult::Match);
        if let Ok(mut out) = self.key_check_result.lock() {
            *out = result;
        }
        std::future::ready(Ok(accept))
    }

    fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<ClientMsg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::client::Session,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let connected_address = connected_address.to_string();
        let sender = self.forwarded_tcpip.clone();
        async move {
            if let Some(sender) = sender {
                let _ = sender.send(ForwardedTcpIp {
                    channel,
                    connected_address,
                    connected_port,
                });
            }
            Ok(())
        }
    }
}

/// Connect to a host and complete authentication, returning the live russh session.
///
/// Used by both interactive shells (via the ssh-bridge subprocess) and SFTP code paths
/// to avoid depending on the system `ssh` binary.
pub(crate) async fn open_authenticated_session(
    host: &Host,
) -> Result<client::Handle<Client>, String> {
    open_authenticated_session_with(host, Client::new()).await
}

/// Like [`open_authenticated_session`] but allows the caller to provide a custom
/// [`Client`] handler instance (for example, one that wants to receive
/// server-initiated forwarded-tcpip channels).
pub(crate) async fn open_authenticated_session_with(
    host: &Host,
    handler: Client,
) -> Result<client::Handle<Client>, String> {
    let handshake = connect_handshake(host, handler, None, direct_transport(host))
        .await
        .map_err(|failure| simple_handshake_error(host, failure))?;
    let mut session = handshake.session;
    let mut logs = Vec::new();
    authenticate_session(&mut session, host, &mut logs).await?;
    Ok(session)
}

/// Like [`open_authenticated_session`], but resolves `host.jump_host_id` and
/// connects through that saved host when present.
pub(crate) async fn open_authenticated_session_with_jump(
    host: &Host,
    jump_key_passphrase: Option<SecretString>,
    jump_expected_fingerprint: Option<String>,
) -> Result<AuthenticatedSession, String> {
    match load_jump_host(host)? {
        Some(mut jump_host) => {
            // Passphrases are never persisted with a host, and the bridge
            // reloads the jump host from storage. Reattach the values the GUI
            // captured for this terminal before authenticating the first hop.
            if jump_host.auth_type == AuthType::Key
                && let Some(passphrase) = jump_key_passphrase
            {
                jump_host.key_passphrase = Some(passphrase);
            }
            if let Some(fingerprint) = jump_expected_fingerprint {
                jump_host.expected_fingerprint = Some(fingerprint);
            }
            open_authenticated_session_via_jump(host, &jump_host).await
        }
        None => open_authenticated_session(host)
            .await
            .map(AuthenticatedSession::direct),
    }
}

/// Connect to a target by first authenticating to `jump_host`, opening a
/// `direct-tcpip` channel to the target's SSH port, and running a second SSH
/// handshake over that channel.
async fn open_authenticated_session_via_jump(
    target: &Host,
    jump_host: &Host,
) -> Result<AuthenticatedSession, String> {
    let handshake = connect_handshake(
        target,
        Client::new(),
        None,
        jump_transport(jump_host, target),
    )
    .await
    .map_err(|failure| simple_handshake_error(target, failure))?;
    // The jump transport sets this on every successful handshake; if it is ever
    // absent the connection is unusable, so report it as a connection error.
    let Some(jump_session) = handshake.jump else {
        return Err(t!("ssh.jump_session_missing").to_string());
    };
    let mut target_session = handshake.session;
    let mut logs = Vec::new();
    authenticate_session(&mut target_session, target, &mut logs).await?;
    Ok(AuthenticatedSession::chained(target_session, jump_session))
}

fn extract_server_key_info(
    hostname: &str,
    port: u16,
    key: &ssh_key::PublicKey,
) -> crate::core::data::ServerKeyInfo {
    let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
    let public_key = key.to_openssh().unwrap_or_default();
    let key_type = public_key
        .split_whitespace()
        .next()
        .unwrap_or("ssh-unknown")
        .to_string();

    crate::core::data::ServerKeyInfo {
        hostname: hostname.to_string(),
        port,
        key_type,
        public_key,
        fingerprint,
    }
}

/// Connect to the remote host just long enough to capture its server key,
/// then abort the handshake before any authentication data is exchanged.
/// Returns the key info so the caller can present a "Trust This Host?"
/// confirmation to the user before proceeding with a real connection.
pub async fn probe_host_key(
    hostname: String,
    port: u16,
) -> Result<crate::core::data::ServerKeyInfo, String> {
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create russh runtime: {e}"))?;
        runtime.block_on(probe_host_key_async(&hostname, port))
    })
    .await
    .map_err(|e| format!("russh probe task failed: {e}"))?
}

async fn probe_host_key_async(
    hostname: &str,
    port: u16,
) -> Result<crate::core::data::ServerKeyInfo, String> {
    probe_host_key_over_transport(hostname, port, direct_probe_transport(hostname, port)).await
}

/// Capture a target server key through a jump host, without sending target
/// authentication data. The jump host is authenticated first using its saved
/// configuration.
pub async fn probe_host_key_via_jump(
    jump_host: Host,
    hostname: String,
    port: u16,
) -> Result<crate::core::data::ServerKeyInfo, String> {
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create russh runtime: {e}"))?;
        runtime.block_on(probe_host_key_via_jump_async(jump_host, hostname, port))
    })
    .await
    .map_err(|e| format!("russh jump probe task failed: {e}"))?
}

async fn probe_host_key_via_jump_async(
    jump_host: Host,
    hostname: String,
    port: u16,
) -> Result<crate::core::data::ServerKeyInfo, String> {
    probe_host_key_over_transport(
        &hostname,
        port,
        jump_probe_transport(&jump_host, hostname.clone(), port),
    )
    .await
}

async fn probe_host_key_over_transport<R, F>(
    hostname: &str,
    port: u16,
    transport: F,
) -> Result<crate::core::data::ServerKeyInfo, String>
where
    R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    F: Future<Output = Result<(R, Option<client::Handle<Client>>), String>>,
{
    let client = Client::new_probe();
    let server_key_handle = client.server_key_handle();
    let (stream, _jump) = transport.await?;
    let config = ssh_config(Some(CONNECT_TIMEOUT));
    let _connect = tokio::time::timeout(
        CONNECT_TIMEOUT,
        client::connect_stream(config, stream, client),
    )
    .await;

    if let Some(key) = server_key_handle
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
    {
        return Ok(extract_server_key_info(hostname, port, &key));
    }

    Err(format!(
        "Timed out or failed connecting to {hostname}:{port} for key probe"
    ))
}

pub async fn authenticate_host(
    host: Host,
) -> (bool, Vec<String>, Option<crate::core::data::ServerKeyInfo>) {
    match tokio::task::spawn_blocking(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                return (
                    false,
                    vec![format!("Failed to create russh runtime: {e}", e = error)],
                    None,
                );
            }
        };

        runtime.block_on(authenticate_host_async(host))
    })
    .await
    {
        Ok(result) => result,
        Err(error) => (
            false,
            vec![format!("russh authentication task failed: {e}", e = error)],
            None,
        ),
    }
}

/// Authenticate a target host through a jump host. This is the same
/// pre-terminal check used by the SSH connect screen, but for hosts with a
/// `jump_host_id`.
pub async fn authenticate_host_via_jump(
    target: Host,
    jump_host: Host,
) -> (bool, Vec<String>, Option<crate::core::data::ServerKeyInfo>) {
    match tokio::task::spawn_blocking(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                return (
                    false,
                    vec![format!("Failed to create russh runtime: {e}", e = error)],
                    None,
                );
            }
        };

        runtime.block_on(authenticate_host_via_jump_async(target, jump_host))
    })
    .await
    {
        Ok(result) => result,
        Err(error) => (
            false,
            vec![format!(
                "russh jump authentication task failed: {e}",
                e = error
            )],
            None,
        ),
    }
}

async fn authenticate_host_via_jump_async(
    target: Host,
    jump_host: Host,
) -> (bool, Vec<String>, Option<crate::core::data::ServerKeyInfo>) {
    let jump_target = format!(
        "{}@{}:{}",
        jump_host.username, jump_host.hostname, jump_host.port
    );
    let target_label = format!("{}@{}:{}", target.username, target.hostname, target.port);
    let mut logs = vec![
        format!("Authenticating jump host {jump_target}"),
        format!("Connecting to {target_label} through {jump_target}"),
        "SSH backend: russh".to_string(),
    ];

    match authenticate_host_over_handshake(&target, &mut logs, jump_transport(&jump_host, &target))
        .await
    {
        Ok(key_info) => (true, logs, key_info),
        Err(error) => {
            logs.push(error);
            (false, logs, None)
        }
    }
}

async fn authenticate_host_async(
    host: Host,
) -> (bool, Vec<String>, Option<crate::core::data::ServerKeyInfo>) {
    let target = format!("{}@{}:{}", host.username, host.hostname, host.port);
    let mut logs = vec![
        format!("Authenticating to {target}"),
        "SSH backend: russh".to_string(),
    ];

    match authenticate_host_over_handshake(&host, &mut logs, direct_transport(&host)).await {
        Ok(key_info) => (true, logs, key_info),
        Err(error) => {
            logs.push(error);
            (false, logs, None)
        }
    }
}

async fn authenticate_host_over_handshake<R, F>(
    host: &Host,
    logs: &mut Vec<String>,
    transport: F,
) -> Result<Option<crate::core::data::ServerKeyInfo>, String>
where
    R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    F: Future<Output = Result<(R, Option<client::Handle<Client>>), String>>,
{
    logs.push(format!("Auth method: {}", host.auth_type));

    let handshake =
        match connect_handshake(host, Client::new(), Some(CONNECT_TIMEOUT), transport).await {
            Ok(handshake) => handshake,
            Err(failure) => {
                if failure
                    .key_check
                    .as_ref()
                    .is_some_and(|check| matches!(check, KeyCheckResult::Mismatch { .. }))
                {
                    if let Some(ref key_info) = failure.key_info {
                        logs.push(
                            "WARNING: Host key verification failed — the server key has changed!"
                                .to_string(),
                        );
                        let expected_text = failure
                            .expected_fingerprints
                            .as_ref()
                            .map_or_else(|| "unknown".to_string(), sorted_fingerprints);
                        logs.push(format!("  Stored fingerprint(s): {expected_text}"));
                        logs.push(format!("  Presented fingerprint: {}", key_info.fingerprint));
                        logs.push(
                            "  This could indicate a man-in-the-middle attack, or the server"
                                .to_string(),
                        );
                        logs.push(
                            "  key was legitimately changed. Delete the old key in Known Hosts"
                                .to_string(),
                        );
                        logs.push("  to accept the new one.".to_string());
                    }
                    return Err(t!(
                        "ssh.host_key_changed",
                        host = &host.hostname,
                        port = host.port,
                    )
                    .to_string());
                }
                return Err(simple_handshake_error(host, failure));
            }
        };

    let mut session = handshake.session;
    authenticate_session(&mut session, host, logs).await?;
    let key_info = handshake
        .server_key
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
        .map(|pk| extract_server_key_info(&host.hostname, host.port, &pk));

    let disconnect = session
        .disconnect(Disconnect::ByApplication, "", "English")
        .await;
    if let Err(error) = disconnect {
        logs.push(format!("Disconnect warning: {error}"));
    }

    Ok(key_info)
}

pub(super) async fn run_sftp_task<T, E, Fut, Op>(operation: Op) -> Result<T, E>
where
    T: Send + 'static,
    E: Send + 'static + From<String> + std::fmt::Debug,
    Fut: Future<Output = Result<T, E>> + 'static,
    Op: FnOnce() -> Fut + Send + 'static,
{
    let result = tokio::task::spawn_blocking(move || {
        let runtime = ssh_runtime();
        runtime.block_on(operation())
    })
    .await
    .map_err(|error| {
        let message = format!("russh-sftp task failed: {error}");
        crate::core::diagnostics::log(Level::Info, "SFTP", &message);
        E::from(message)
    })?;

    match &result {
        Ok(_) => {
            crate::core::diagnostics::log(
                Level::Info,
                "SFTP",
                "run_sftp_task: operation completed",
            );
        }
        Err(error) => crate::core::diagnostics::log(
            Level::Info,
            "SFTP",
            format!("run_sftp_task: operation failed: {error:?}"),
        ),
    }

    result
}

pub(super) fn sftp_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Failed to create russh-sftp runtime: {error}"))
}

/// SFTP connection borrowed from [`SSH_SESSION_POOL`].
///
/// The lease is held for the lifetime of one SFTP operation. Releasing the
/// operation returns both the SFTP channel and its authenticated SSH session to
/// the pool, so later operations skip the subsystem handshake.
pub(super) struct PooledSession {
    lease: Option<SessionLease>,
}

pub(super) async fn connect_sftp(
    host: Host,
) -> Result<((PooledSession, SftpSession), Vec<String>), String> {
    let target_label = format!("{}@{}:{}", host.username, host.hostname, host.port);
    let via_jump = host.jump_host_id.is_some();
    let mut logs = Vec::new();
    crate::core::diagnostics::log(
        Level::Info,
        "SFTP",
        format!("connect_sftp: acquiring pooled SSH session for {target_label} (jump={via_jump})"),
    );
    let mut lease = ssh_session_pool()
        .acquire(&host, || connect_pooled_session(&host, &mut logs))
        .await?;
    crate::core::diagnostics::log(
        Level::Info,
        "SFTP",
        "connect_sftp: SSH session acquired, resolving SFTP channel",
    );
    let sftp = if let Some(sftp) = lease.take_sftp(SESSION_IDLE_TIMEOUT).await {
        crate::core::diagnostics::log(
            Level::Info,
            "SFTP",
            "connect_sftp: reused pooled SFTP channel",
        );
        sftp
    } else {
        let channel = tokio::time::timeout(
            SFTP_REQUEST_TIMEOUT,
            lease.handle_mut().channel_open_session(),
        )
        .await
        .map_err(|_error| format!("Timed out opening SFTP channel for {target_label}"))?
        .map_err(|error| format!("Failed to open SFTP channel for {target_label}: {error}"))?;
        crate::core::diagnostics::log(
            Level::Info,
            "SFTP",
            "connect_sftp: SFTP channel open, requesting subsystem",
        );
        tokio::time::timeout(
            SFTP_REQUEST_TIMEOUT,
            channel.request_subsystem(true, "sftp"),
        )
        .await
        .map_err(|_error| format!("Timed out requesting SFTP subsystem for {target_label}"))?
        .map_err(|error| format!("Failed to request SFTP subsystem for {target_label}: {error}"))?;
        tokio::time::timeout(
            SFTP_REQUEST_TIMEOUT,
            SftpSession::new(channel.into_stream()),
        )
        .await
        .map_err(|_error| format!("Timed out starting SFTP session for {target_label}"))?
        .map_err(|error| {
            format!("Failed to start SFTP session for {target_label} (jump={via_jump}): {error}")
        })?
    };

    let session = PooledSession { lease: Some(lease) };
    crate::core::diagnostics::log(Level::Info, "SFTP", "connect_sftp: SFTP session ready");
    Ok(((session, sftp), logs))
}

pub(super) async fn connect_authenticated_session(
    host: &Host,
    logs: &mut Vec<String>,
) -> Result<AuthenticatedSession, String> {
    match load_jump_host(host)? {
        Some(jump_host) => {
            connect_authenticated_session_via_jump(host, &jump_host, logs, Some(CONNECT_TIMEOUT))
                .await
        }
        None => connect_authenticated_session_with_config(host, logs, Some(CONNECT_TIMEOUT))
            .await
            .map(AuthenticatedSession::direct),
    }
}

async fn connect_pooled_session(
    host: &Host,
    logs: &mut Vec<String>,
) -> Result<AuthenticatedSession, String> {
    match load_jump_host(host)? {
        Some(jump_host) => {
            connect_authenticated_session_via_jump(host, &jump_host, logs, None).await
        }
        None => connect_authenticated_session_with_config(host, logs, None)
            .await
            .map(AuthenticatedSession::direct),
    }
}

async fn connect_authenticated_session_via_jump(
    target: &Host,
    jump_host: &Host,
    logs: &mut Vec<String>,
    inactivity_timeout: Option<Duration>,
) -> Result<AuthenticatedSession, String> {
    let handshake = connect_handshake(
        target,
        Client::new(),
        inactivity_timeout,
        jump_transport(jump_host, target),
    )
    .await
    .map_err(|failure| simple_handshake_error(target, failure))?;
    // As in `open_authenticated_session_via_jump`: a jump transport that
    // returns no session leaves nothing to chain onto, so surface it as an
    // error rather than panicking.
    let Some(jump_session) = handshake.jump else {
        return Err(t!("ssh.jump_session_missing").to_string());
    };
    let mut target_session = handshake.session;
    authenticate_session(&mut target_session, target, logs).await?;
    Ok(AuthenticatedSession::chained(target_session, jump_session))
}

async fn connect_authenticated_session_with_config(
    host: &Host,
    logs: &mut Vec<String>,
    inactivity_timeout: Option<Duration>,
) -> Result<client::Handle<Client>, String> {
    let handshake = connect_handshake(
        host,
        Client::new(),
        inactivity_timeout,
        direct_transport(host),
    )
    .await
    .map_err(|failure| simple_handshake_error(host, failure))?;
    let mut session = handshake.session;
    authenticate_session(&mut session, host, logs).await?;
    Ok(session)
}

/// Return a live SFTP channel to the SSH session pool.
///
/// The fallback close only runs when the lease was already released, which
/// keeps channel ownership unambiguous without leaving a detached subsystem.
pub(super) async fn release_sftp(session: &mut PooledSession, sftp: SftpSession) {
    if let Some(mut lease) = session.lease.take() {
        lease.cache_sftp(sftp);
    } else {
        let _ = tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.close()).await;
    }
}

async fn authenticate_session(
    session: &mut client::Handle<Client>,
    host: &Host,
    _logs: &mut Vec<String>,
) -> Result<(), String> {
    match host.auth_type {
        AuthType::Password => {
            let password = host
                .password
                .as_ref()
                .filter(|password| !password.expose_secret().is_empty())
                .ok_or_else(|| t!("ssh.password_missing").to_string())?;
            authenticate_with_password(session, &host.username, password).await
        }
        AuthType::Key => {
            let key_path = host
                .key_path
                .as_ref()
                .filter(|path| !path.is_empty())
                .ok_or_else(|| t!("ssh.key_path_required").to_string())?;
            let key = load_host_private_key(key_path, host)?;
            let hash_alg = session
                .best_supported_rsa_hash()
                .await
                .map_err(|error| format!("Failed to negotiate RSA signature algorithm: {error}"))?
                .flatten();
            let result = session
                .authenticate_publickey(
                    host.username.clone(),
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
                )
                .await
                .map_err(|error| format!("Public key authentication failed to run: {error}"))?;

            auth_result_to_result(result, t!("ssh.public_key_rejected").as_ref())
        }
    }
}

/// Perform password authentication, falling back to keyboard-interactive.
///
/// `password` is borrowed (never cloned) and only materialized as an owned
/// `String` at the exact russh boundary — `authenticate_password` takes
/// `&str` via `Into<String>`, and keyboard-interactive prompts are answered
/// with a fresh copy per prompt. The `SecretString` itself zeroizes on drop,
/// so no plaintext `String` copy survives the call.
async fn authenticate_with_password(
    session: &mut client::Handle<Client>,
    username: &str,
    password: &SecretString,
) -> Result<(), String> {
    let result = session
        .authenticate_password(username, password.expose_secret())
        .await
        .map_err(|error| format!("Password authentication failed to run: {error}"))?;

    if result.success() {
        return Ok(());
    }

    let mut response = session
        .authenticate_keyboard_interactive_start(username, None)
        .await
        .map_err(|error| format!("Keyboard-interactive authentication failed to start: {error}"))?;

    for _ in 0..5 {
        response = match response {
            KeyboardInteractiveAuthResponse::Success => return Ok(()),
            KeyboardInteractiveAuthResponse::Failure { .. } => {
                return Err(t!("ssh.password_rejected").to_string());
            }
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let responses = prompts
                    .iter()
                    .map(|_| password.expose_secret().to_string())
                    .collect();
                session
                    .authenticate_keyboard_interactive_respond(responses)
                    .await
                    .map_err(|error| {
                        format!("Keyboard-interactive authentication failed to respond: {error}")
                    })?
            }
        };
    }

    Err(t!("ssh.kbd_interactive_too_many").to_string())
}

fn expand_home_path(path: &str) -> String {
    crate::core::paths::expand_home(path)
}

/// Load a host's private key, passing the in-memory key passphrase when one
/// was provided. Centralizes path expansion and the russh load error mapping.
fn load_host_private_key(key_path: &str, host: &Host) -> Result<russh::keys::PrivateKey, String> {
    let key_path = expand_home_path(key_path);
    let passphrase = host
        .key_passphrase
        .as_ref()
        .map(secrecy::ExposeSecret::expose_secret);
    load_secret_key(&key_path, passphrase)
        .map_err(|error| t!("ssh.key_load_failed", key_path = &key_path, e = error).to_string())
}

/// Whether the private key at `path` is encrypted and therefore requires a
/// passphrase. Returns `false` when the key is unencrypted or cannot be read
/// (the normal authentication path will surface the real error).
pub fn key_requires_passphrase(path: &str) -> bool {
    let path = expand_home_path(path);
    match load_secret_key(&path, None) {
        Ok(_) => false,
        Err(error) => {
            let message = error.to_string().to_ascii_lowercase();
            message.contains("encrypted")
                || message.contains("passphrase")
                || message.contains("decrypt")
        }
    }
}

fn auth_result_to_result(result: AuthResult, fallback: &str) -> Result<(), String> {
    match result {
        AuthResult::Success => Ok(()),
        AuthResult::Failure {
            remaining_methods,
            partial_success,
        } => Err(auth_failure_message(
            fallback,
            &remaining_methods,
            partial_success,
        )),
    }
}

fn auth_failure_message(
    fallback: &str,
    remaining_methods: &MethodSet,
    partial_success: bool,
) -> String {
    if remaining_methods.is_empty() {
        return fallback.to_string();
    }

    format!(
        "{fallback}; remaining methods: {remaining_methods:?}; partial success: {partial_success}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct AuthenticationCounter(Arc<AtomicUsize>);

    impl russh::server::Handler for AuthenticationCounter {
        type Error = russh::Error;

        fn auth_password(
            &mut self,
            _user: &str,
            _password: &str,
        ) -> impl Future<Output = Result<russh::server::Auth, Self::Error>> + Send {
            self.0.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(russh::server::Auth::Accept))
        }
    }

    async fn authenticate_test_server(trusted: Option<bool>) -> (bool, usize) {
        let key = ssh_key::PrivateKey::random(&mut rand::rng(), ssh_key::Algorithm::Ed25519)
            .expect("generate test server key");
        let mut host = crate::core::test_support::host(1, "test", "ssh-trust-test.invalid");
        host.expected_fingerprint = trusted.map(|matches| {
            if matches {
                key.public_key().fingerprint(HashAlg::Sha256).to_string()
            } else {
                "SHA256:different-key".into()
            }
        });
        let config = Arc::new(russh::server::Config {
            keys: vec![key],
            ..Default::default()
        });
        let attempts = Arc::new(AtomicUsize::new(0));
        // A bounded in-memory transport runs real SSH without sockets or host files.
        let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
        let counter = AuthenticationCounter(Arc::clone(&attempts));
        let server = tokio::spawn(async move {
            if let Ok(session) = russh::server::run_stream(config, server_stream, counter).await {
                let _ = session.await;
            }
        });
        let result = tokio::time::timeout(
            CONNECT_TIMEOUT,
            authenticate_host_over_handshake(&host, &mut Vec::new(), async {
                Ok((client_stream, None))
            }),
        )
        .await
        .expect("test handshake completes");
        server.abort();
        let _ = server.await;
        (result.is_ok(), attempts.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn unknown_server_keys_are_rejected_before_sending_a_password() {
        assert_eq!(authenticate_test_server(None).await, (false, 0));
    }

    #[tokio::test]
    async fn changed_server_keys_are_rejected_before_sending_a_password() {
        assert_eq!(authenticate_test_server(Some(false)).await, (false, 0));
    }

    #[tokio::test]
    async fn an_explicitly_trusted_server_key_allows_authentication() {
        assert_eq!(authenticate_test_server(Some(true)).await, (true, 1));
    }

    #[test]
    fn matches_any_known_fingerprint_not_just_first() {
        let mut expected = HashSet::new();
        expected.insert("SHA256:first".to_string());
        expected.insert("SHA256:second".to_string());

        // The server presents a key whose fingerprint is the second entry in
        // the set; this is the regression case the fix addresses.
        let result = classify_presented_key(Some(&expected), "SHA256:second");
        assert_eq!(result, KeyCheckResult::Match);
    }

    #[test]
    fn mismatches_untrusted_fingerprint() {
        let mut expected = HashSet::new();
        expected.insert("SHA256:known-a".to_string());
        expected.insert("SHA256:known-b".to_string());

        let result = classify_presented_key(Some(&expected), "SHA256:unknown");
        assert_eq!(
            result,
            KeyCheckResult::Mismatch {
                expected: "SHA256:known-a, SHA256:known-b".to_string(),
                presented: "SHA256:unknown".to_string(),
            }
        );
    }

    #[test]
    fn first_time_when_no_expected_fingerprints() {
        assert_eq!(
            classify_presented_key(None, "SHA256:whatever"),
            KeyCheckResult::FirstTime
        );
    }

    #[test]
    fn sorts_fingerprints_deterministically() {
        let mut set = HashSet::new();
        set.insert("SHA256:b".to_string());
        set.insert("SHA256:a".to_string());
        set.insert("SHA256:c".to_string());
        assert_eq!(sorted_fingerprints(&set), "SHA256:a, SHA256:b, SHA256:c");
    }
}
