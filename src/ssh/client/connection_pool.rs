// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::Disconnect;
use russh::client;
use russh_sftp::client::SftpSession;
use tokio::sync::Notify;
use tokio::time::{Instant, MissedTickBehavior, interval};

use crate::core::data::{AuthType, Host};
use rust_i18n::t;
use secrecy::ExposeSecret;

use super::session::{AuthenticatedSession, Client, ssh_runtime};

pub(super) const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
pub(super) const MAX_TOTAL_SESSIONS: usize = 1024;
pub(super) const MAX_SESSIONS_PER_HOST: usize = 64;
const HEALTH_CHECK_AFTER: Duration = Duration::from_secs(10);
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(2);
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REAPER_INTERVAL: Duration = Duration::from_secs(30);

/// Cache identity for an authenticated SSH session.
///
/// `host_id` is included because persisted hosts have stable IDs, but ad-hoc
/// Quick Connect hosts all use `HostId(0)`; the endpoint and credential
/// fields prevent those from colliding with each other or with a saved host.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct PoolKey {
    host_id: u32,
    hostname: String,
    port: u16,
    username: String,
    auth_type: u8,
    jump_host_id: Option<u32>,
    key_path: Option<String>,
    password_fingerprint: Option<u64>,
    expected_fingerprint: Option<String>,
}

impl PoolKey {
    pub(super) fn for_host(host: &Host) -> Self {
        Self {
            host_id: host.id.0,
            hostname: host.hostname.clone(),
            port: host.port,
            username: host.username.clone(),
            auth_type: auth_type_id(host.auth_type),
            jump_host_id: host.jump_host_id.map(|id| id.0),
            key_path: host.key_path.clone(),
            password_fingerprint: host.password.as_ref().map(secret_fingerprint),
            expected_fingerprint: host.expected_fingerprint.clone(),
        }
    }
}

fn auth_type_id(auth_type: AuthType) -> u8 {
    match auth_type {
        AuthType::Password => 0,
        AuthType::Key => 1,
    }
}

fn secret_fingerprint(secret: &secrecy::SecretString) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    secret.expose_secret().hash(&mut hasher);
    hasher.finish()
}

struct PooledSftp {
    session: SftpSession,
    last_used: Instant,
}

struct PooledSession {
    handle: AuthenticatedSession,
    last_used: Instant,
    sftp: Option<PooledSftp>,
}

#[derive(Default)]
struct HostPool {
    idle: Vec<PooledSession>,
    live: usize,
}

#[derive(Default)]
struct PoolState {
    hosts: HashMap<PoolKey, HostPool>,
    total_live: usize,
}

impl PoolState {
    fn take_idle(&mut self, key: &PoolKey) -> Option<PooledSession> {
        self.hosts.get_mut(key)?.idle.pop()
    }

    fn try_reserve(&mut self, key: &PoolKey, max_total: usize, max_per_host: usize) -> bool {
        let host = self.hosts.entry(key.clone()).or_default();
        if host.live >= max_per_host || self.total_live >= max_total {
            return false;
        }
        host.live += 1;
        self.total_live += 1;
        true
    }

    fn release_slot(&mut self, key: &PoolKey) {
        let host = self
            .hosts
            .get_mut(key)
            .expect("released SSH session has no pool entry");
        host.live = host
            .live
            .checked_sub(1)
            .expect("released SSH session exceeds its host live count");
        self.total_live = self
            .total_live
            .checked_sub(1)
            .expect("released SSH session exceeds the total live count");
        if host.live == 0 && host.idle.is_empty() {
            self.hosts.remove(key);
        }
    }
}

enum AcquireStep {
    Reuse(PooledSession),
    Connect,
    Wait,
}

/// Process-wide authenticated SSH session pool.
///
/// Sessions are leased one operation at a time. Different operations for the
/// same host can use different sessions concurrently, up to the configured
/// per-host and total limits. Idle sessions are reused, while sessions that
/// have been idle too long or closed are discarded. An SFTP channel can remain
/// attached to its idle SSH session so the next operation skips subsystem setup.
pub(super) struct SshSessions {
    state: Mutex<PoolState>,
    changed: Notify,
    idle_timeout: Duration,
    max_total: usize,
    max_per_host: usize,
}

impl SshSessions {
    pub(super) fn new(idle_timeout: Duration, max_total: usize, max_per_host: usize) -> Self {
        Self {
            state: Mutex::new(PoolState::default()),
            changed: Notify::new(),
            idle_timeout,
            max_total,
            max_per_host,
        }
    }

    pub(super) async fn acquire<F, Fut>(
        self: &Arc<Self>,
        host: &Host,
        connect: F,
    ) -> Result<SessionLease, String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<AuthenticatedSession, String>> + Send,
    {
        let mut connect = Some(connect);
        loop {
            // Register before inspecting the pool so a session returned between
            // the capacity check and waiting cannot be missed.
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();

            let key = PoolKey::for_host(host);
            let step = {
                let mut state = self.state.lock().expect("SSH session pool lock poisoned");
                if let Some(session) = state.take_idle(&key) {
                    AcquireStep::Reuse(session)
                } else if state.try_reserve(&key, self.max_total, self.max_per_host) {
                    AcquireStep::Connect
                } else {
                    AcquireStep::Wait
                }
            };

            match step {
                AcquireStep::Reuse(session) => {
                    if session_is_reusable(&session, self.idle_timeout).await {
                        return Ok(SessionLease::new(Arc::clone(self), key, session));
                    }
                    discard_session(session).await;
                    self.release_slot(&key);
                }
                AcquireStep::Connect => {
                    let Some(connect) = connect.take() else {
                        self.release_slot(&key);
                        return Err(t!("ssh.connection_failed").to_string());
                    };
                    match connect().await {
                        Ok(handle) => {
                            return Ok(SessionLease::new(
                                Arc::clone(self),
                                key,
                                PooledSession {
                                    handle,
                                    last_used: Instant::now(),
                                    sftp: None,
                                },
                            ));
                        }
                        Err(error) => {
                            self.release_slot(&key);
                            return Err(error);
                        }
                    }
                }
                AcquireStep::Wait => changed.await,
            }
        }
    }

    pub(super) fn start_reaper(self: &Arc<Self>) {
        let pool = Arc::clone(self);
        ssh_runtime().spawn(async move {
            pool.reaper_loop().await;
        });
    }

    fn return_session(&self, key: &PoolKey, session: PooledSession) {
        let mut state = self.state.lock().expect("SSH session pool lock poisoned");
        state
            .hosts
            .entry(key.clone())
            .or_default()
            .idle
            .push(session);
        drop(state);
        self.changed.notify_waiters();
    }

    fn release_slot(&self, key: &PoolKey) {
        let mut state = self.state.lock().expect("SSH session pool lock poisoned");
        state.release_slot(key);
        drop(state);
        self.changed.notify_waiters();
    }

    async fn reaper_loop(&self) {
        let mut ticker = interval(REAPER_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            self.reap_once().await;
        }
    }

    async fn reap_once(&self) {
        let (stale, removed) = {
            let mut state = self.state.lock().expect("SSH session pool lock poisoned");
            let mut stale = Vec::new();
            let mut removed = 0;

            for host in state.hosts.values_mut() {
                let before = host.idle.len();
                let mut kept = Vec::with_capacity(before);
                for session in host.idle.drain(..) {
                    let closed = session.handle.is_closed();
                    let expired = session.last_used.elapsed() > self.idle_timeout;
                    if closed || expired {
                        stale.push(session);
                    } else {
                        kept.push(session);
                    }
                }
                let removed_here = before - kept.len();
                host.idle = kept;
                host.live = host.live.saturating_sub(removed_here);
                removed += removed_here;
            }

            state.total_live = state.total_live.saturating_sub(removed);
            state
                .hosts
                .retain(|_key, host| host.live > 0 || !host.idle.is_empty());
            (stale, removed)
        };

        for session in stale {
            discard_session(session).await;
        }
        if removed > 0 {
            self.changed.notify_waiters();
        }
    }
}

pub(super) struct SessionLease {
    pool: Arc<SshSessions>,
    key: PoolKey,
    session: Option<PooledSession>,
}

impl SessionLease {
    fn new(pool: Arc<SshSessions>, key: PoolKey, session: PooledSession) -> Self {
        Self {
            pool,
            key,
            session: Some(session),
        }
    }

    pub(super) fn handle_mut(&mut self) -> &mut client::Handle<Client> {
        &mut self
            .session
            .as_mut()
            .expect("acquired SSH session is unexpectedly empty")
            .handle
    }

    /// Take a cached SFTP channel when it is still safe to reuse.
    ///
    /// Recently active channels are trusted without another round trip. Older
    /// channels are probed so a dead connection falls back to a fresh channel
    /// instead of failing the next user operation.
    pub(super) async fn take_sftp(&mut self, idle_timeout: Duration) -> Option<SftpSession> {
        let session = self
            .session
            .as_mut()
            .expect("acquired SSH session is unexpectedly empty");
        let cached = session.sftp.take()?;
        let idle_for = cached.last_used.elapsed();
        if idle_for <= HEALTH_CHECK_AFTER {
            return Some(cached.session);
        }
        if idle_for <= idle_timeout
            && matches!(
                tokio::time::timeout(
                    HEALTH_CHECK_TIMEOUT,
                    cached.session.metadata(".".to_string())
                )
                .await,
                Ok(Ok(_))
            )
        {
            return Some(cached.session);
        }
        discard_sftp_session(cached.session).await;
        None
    }

    /// Cache a live SFTP channel for the next operation on this host.
    pub(super) fn cache_sftp(&mut self, sftp: SftpSession) {
        let session = self
            .session
            .as_mut()
            .expect("acquired SSH session is unexpectedly empty");
        session.sftp = Some(PooledSftp {
            session: sftp,
            last_used: Instant::now(),
        });
    }
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        if let Some(mut session) = self.session.take() {
            session.last_used = Instant::now();
            self.pool.return_session(&self.key, session);
        }
    }
}

async fn session_is_reusable(session: &PooledSession, idle_timeout: Duration) -> bool {
    let idle_for = session.last_used.elapsed();
    if session.handle.is_closed() || idle_for > idle_timeout {
        return false;
    }
    if idle_for <= HEALTH_CHECK_AFTER {
        return true;
    }
    matches!(
        tokio::time::timeout(HEALTH_CHECK_TIMEOUT, session.handle.send_ping()).await,
        Ok(Ok(()))
    )
}

async fn discard_sftp_session(sftp: SftpSession) {
    let _ = tokio::time::timeout(HEALTH_CHECK_TIMEOUT, sftp.close()).await;
}

async fn discard_session(mut session: PooledSession) {
    if let Some(sftp) = session.sftp.take() {
        discard_sftp_session(sftp.session).await;
    }
    let session = session.handle;
    let _ = tokio::time::timeout(
        DISCONNECT_TIMEOUT,
        session.disconnect(Disconnect::ByApplication, "", "English"),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use crate::core::data::{AuthType, Host, HostId, OsType, Protocol};

    use super::{PoolKey, PoolState};

    fn host(name: &str, hostname: &str, password: Option<&str>) -> Host {
        Host {
            id: HostId(0),
            name: name.to_string(),
            hostname: hostname.to_string(),
            port: 22,
            protocol: Protocol::Ssh,
            username: "user".to_string(),
            auth_type: AuthType::Password,
            jump_host_id: None,
            identity_id: None,
            key_path: None,
            key_passphrase: None,
            password: password.map(|value| SecretString::from(value.to_string())),
            os_type: OsType::Unknown,
            tags: vec![],
            expected_fingerprint: None,
        }
    }

    #[test]
    fn quick_connect_hosts_are_not_merged_by_id_zero() {
        let first = PoolKey::for_host(&host("a", "one.example", Some("secret")));
        let second = PoolKey::for_host(&host("b", "two.example", Some("secret")));
        assert_ne!(first, second);
    }

    #[test]
    fn credential_changes_produce_a_new_pool_key() {
        let old = PoolKey::for_host(&host("same", "same.example", Some("old")));
        let new = PoolKey::for_host(&host("same", "same.example", Some("new")));
        assert_ne!(old, new);
    }

    #[test]
    fn a_second_same_host_operation_does_not_wait_for_the_first() {
        let mut state = PoolState::default();
        let key = PoolKey::for_host(&host("one", "one.example", None));

        assert!(state.try_reserve(&key, 8, 4));
        assert!(state.try_reserve(&key, 8, 4));

        assert_eq!(state.total_live, 2, "both leases count as live sessions");
        assert_eq!(
            state.hosts.get(&key).map(|host| host.live),
            Some(2),
            "the second operation must not wait for the first operation"
        );
    }

    #[test]
    fn per_host_limit_bounds_parallel_sessions() {
        let mut state = PoolState::default();
        let key = PoolKey::for_host(&host("one", "one.example", None));

        assert!(state.try_reserve(&key, 8, 2));
        assert!(state.try_reserve(&key, 8, 2));
        assert!(!state.try_reserve(&key, 8, 2));
        assert_eq!(state.total_live, 2);
    }

    #[test]
    fn total_limit_is_shared_across_hosts() {
        let mut state = PoolState::default();
        let first = PoolKey::for_host(&host("one", "one.example", None));
        let second = PoolKey::for_host(&host("two", "two.example", None));
        let third = PoolKey::for_host(&host("three", "three.example", None));

        assert!(state.try_reserve(&first, 2, 2));
        assert!(state.try_reserve(&second, 2, 2));
        assert!(!state.try_reserve(&third, 2, 2));
    }

    #[test]
    fn released_slots_become_available_again() {
        let mut state = PoolState::default();
        let key = PoolKey::for_host(&host("one", "one.example", None));

        assert!(state.try_reserve(&key, 4, 1));
        assert!(!state.try_reserve(&key, 4, 1));
        state.release_slot(&key);

        assert!(
            state.try_reserve(&key, 4, 1),
            "a finished operation must free its host slot"
        );
    }
}
