//! Shared state for interactive SSH connection attempts.

use std::fmt;

use secrecy::{ExposeSecret, SecretString};

use crate::core::data::{Host, ServerKeyInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SshConnectionState {
    #[default]
    Idle,
    WaitingForPassword,
    WaitingForKeyConfirmation,
    Connecting,
    Connected,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SshConnectStage {
    Jump,
    #[default]
    Target,
}

/// Connection-scoped state shared by terminal, SFTP, and port forwarding.
#[derive(Clone)]
pub struct ConnectionAttempt {
    pub state: SshConnectionState,
    pub pending_host: Option<Host>,
    pub jump_host: Option<Host>,
    pub stage: SshConnectStage,
    pub auth_started: bool,
    pub password_input: SecretString,
    pub password_visible: bool,
    pub pending_key_info: Option<ServerKeyInfo>,
    pub save_password: bool,
    pub expected_fingerprint: Option<String>,
    /// When true, the next terminal tick skips its initial key probe.
    pub probe_skip: bool,
}

impl Default for ConnectionAttempt {
    fn default() -> Self {
        Self {
            state: SshConnectionState::Idle,
            pending_host: None,
            jump_host: None,
            stage: SshConnectStage::default(),
            auth_started: false,
            password_input: SecretString::from(String::new()),
            password_visible: false,
            pending_key_info: None,
            save_password: false,
            expected_fingerprint: None,
            probe_skip: false,
        }
    }
}

impl ConnectionAttempt {
    /// Creates an attempt that starts at its configured jump hop or target.
    pub(crate) fn new(pending_host: Host, jump_host: Option<Host>) -> Self {
        let stage = if jump_host.is_some() {
            SshConnectStage::Jump
        } else {
            SshConnectStage::Target
        };
        Self {
            pending_host: Some(pending_host),
            jump_host,
            stage,
            ..Self::default()
        }
    }

    pub(crate) fn jump_stage(&self) -> bool {
        self.jump_host.is_some() && self.stage == SshConnectStage::Jump
    }

    pub(crate) fn current_host(&self) -> Option<&Host> {
        if self.jump_stage() {
            self.jump_host.as_ref()
        } else {
            self.pending_host.as_ref()
        }
    }

    pub(crate) fn current_host_mut(&mut self) -> Option<&mut Host> {
        if self.jump_stage() {
            self.jump_host.as_mut()
        } else {
            self.pending_host.as_mut()
        }
    }

    pub(crate) fn current_label(&self) -> String {
        self.current_host()
            .map(|host| format!("{}@{}:{}", host.username, host.hostname, host.port))
            .unwrap_or_default()
    }

    pub(crate) fn reset_for_retry(&mut self, jump_host: Option<Host>, jump_changed: bool) {
        self.jump_host = jump_host;
        self.auth_started = false;
        self.pending_key_info = None;
        self.probe_skip = false;
        if jump_changed {
            self.stage = if self.jump_host.is_some() {
                SshConnectStage::Jump
            } else {
                SshConnectStage::Target
            };
        }
    }

    pub(crate) fn waiting_for_password(&self) -> bool {
        self.state == SshConnectionState::WaitingForPassword
    }

    pub(crate) fn waiting_for_key(&self) -> bool {
        self.state == SshConnectionState::WaitingForKeyConfirmation
    }
}

impl fmt::Debug for ConnectionAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionAttempt")
            .field("state", &self.state)
            .field(
                "pending_host",
                &self.pending_host.as_ref().map(|host| host.id),
            )
            .field("jump_host", &self.jump_host.as_ref().map(|host| host.id))
            .field("stage", &self.stage)
            .field("auth_started", &self.auth_started)
            .field(
                "password_input",
                &(!self.password_input.expose_secret().is_empty()).then_some("<redacted>"),
            )
            .field("password_visible", &self.password_visible)
            .field("pending_key_info", &self.pending_key_info)
            .field("save_password", &self.save_password)
            .field("expected_fingerprint", &self.expected_fingerprint)
            .field("probe_skip", &self.probe_skip)
            .finish()
    }
}

#[derive(Clone, Default)]
pub struct SshConnectState {
    pub attempt: ConnectionAttempt,
    pub logs: Vec<String>,
    pub anim_t: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_support::host;

    #[test]
    fn an_attempt_starts_at_the_jump_host_when_one_is_configured() {
        let target = host(1, "target", "target.example");
        let jump = host(2, "jump", "jump.example");
        let attempt = ConnectionAttempt::new(target, Some(jump));

        assert!(attempt.jump_stage());
        assert_eq!(
            attempt.current_host().map(|host| host.id),
            Some(crate::core::ids::HostId(2))
        );
    }

    #[test]
    fn debug_output_redacts_the_password() {
        let mut attempt = ConnectionAttempt::new(host(1, "target", "target.example"), None);
        attempt.password_input = SecretString::from("do-not-print");

        let output = format!("{attempt:?}");

        assert!(output.contains("<redacted>"));
        assert!(!output.contains("do-not-print"));
    }
}
