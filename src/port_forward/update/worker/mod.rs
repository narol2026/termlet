//! Port-forward worker dispatch and shared addressing rules.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use tokio::sync::{Notify, oneshot};

use crate::core::data::{Host, PortForward, PortForwardKind};

mod io;
mod remote;
mod socks;
mod tcp;

pub(super) async fn run(
    host: Host,
    forward: PortForward,
    shutdown: Arc<Notify>,
    started: oneshot::Sender<(bool, Vec<String>)>,
    stopped: oneshot::Sender<Vec<String>>,
) {
    match forward.kind {
        PortForwardKind::Local => {
            tcp::run_local(host, forward, shutdown, started, stopped).await;
        }
        PortForwardKind::Dynamic => {
            tcp::run_dynamic(host, forward, shutdown, started, stopped).await;
        }
        PortForwardKind::Remote => {
            remote::run(host, forward, shutdown, started, stopped).await;
        }
    }
}

pub(super) fn bind_address_for_listen(address: &str) -> &str {
    let trimmed = address.trim();
    if trimmed.is_empty() {
        "127.0.0.1"
    } else {
        trimmed
    }
}
