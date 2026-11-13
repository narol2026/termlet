use secrecy::SecretString;

use crate::app::DraftId;
use crate::core::data::{Host, ServerKeyInfo};
use crate::terminal::SshSessionSecrets;

#[derive(Debug, Clone)]
pub enum Message {
    SshRetry,
    SshEditHost,
    SshClose,
    CopySshLogs,
    SshTick,
    SshPasswordChanged(SecretString),
    SshPasswordSubmit,
    SshTogglePasswordVisible,
    SshAuthFinished {
        tab_id: DraftId,
        success: bool,
        logs: Vec<String>,
        server_key: Option<ServerKeyInfo>,
    },
    SshKeyProbeFinished {
        tab_id: DraftId,
        result: Result<ServerKeyInfo, String>,
    },
    SshKeyConfirmAccept,
    SshKeyConfirmContinue,
    SshOpenTerminal(Host, SshSessionSecrets, Option<DraftId>),
}
