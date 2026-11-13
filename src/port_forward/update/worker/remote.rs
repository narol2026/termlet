//! Remote port-forward worker.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::sync::{Notify, Semaphore, oneshot};
use tokio::task::JoinSet;

use super::bind_address_for_listen;
use super::io::{
    CONNECT_TIMEOUT, IDLE_TIMEOUT, MAX_CLIENT_CONNECTIONS, SHUTDOWN_DRAIN_TIMEOUT,
    copy_bidirectional_idle, drain_tasks,
};
use crate::core::data::{Host, PortForward};
use crate::ssh::{Client, ForwardedTcpIp, open_authenticated_session_with};
use rust_i18n::t;

type StartedSender = oneshot::Sender<(bool, Vec<String>)>;
type StoppedSender = oneshot::Sender<Vec<String>>;

pub(super) async fn run(
    host: Host,
    forward: PortForward,
    shutdown: Arc<Notify>,
    started: StartedSender,
    stopped: StoppedSender,
) {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<ForwardedTcpIp>();
    let session =
        match open_authenticated_session_with(&host, Client::with_forwarded_tcpip(sender)).await {
            Ok(session) => session,
            Err(error) => {
                let _ = started.send((false, vec![error]));
                let _ = stopped.send(Vec::new());
                return;
            }
        };

    let bind = bind_address_for_listen(&forward.bind_address);
    let listen_port = u32::from(forward.listen_port);
    let bound_port = match session.tcpip_forward(bind, listen_port).await {
        Ok(port) => port,
        Err(error) => {
            let _ = started.send((
                false,
                vec![
                    t!(
                        "port_forward.remote_refused",
                        bind = bind,
                        port = listen_port,
                        error = &error.to_string()
                    )
                    .to_string(),
                ],
            ));
            let _ = session
                .disconnect(russh::Disconnect::ByApplication, "", "English")
                .await;
            let _ = stopped.send(Vec::new());
            return;
        }
    };

    let _ = started.send((
        true,
        vec![format!(
            "Remote {bind}:{} → {}:{}",
            if bound_port == 0 {
                listen_port
            } else {
                bound_port
            },
            forward.dest_host,
            forward.dest_port
        )],
    ));

    let dest_host = forward.dest_host;
    let dest_port = forward.dest_port;
    let connection_slots = Arc::new(Semaphore::new(MAX_CLIENT_CONNECTIONS));
    let mut final_logs = Vec::new();
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            () = shutdown.notified() => {
                final_logs.push(t!("port_forward.stopped").to_string());
                break;
            }
            forwarded = receiver.recv() => {
                if let Some(forwarded) = forwarded {
                    let Ok(permit) =
                        Arc::clone(&connection_slots).try_acquire_owned()
                    else {
                        final_logs.push(
                            "client connection limit reached, rejecting forwarded channel"
                                .to_string(),
                        );
                        continue;
                    };
                    let dest_host = dest_host.clone();
                    tasks.spawn(async move {
                        handle(forwarded, dest_host, dest_port).await;
                        drop(permit);
                    });
                } else {
                    final_logs.push("forwarded channel sender dropped".to_string());
                    break;
                }
            }
            Some(result) = tasks.join_next(), if !tasks.is_empty() => {
                if result.is_err() {
                    final_logs.push("forward connection task failed".to_string());
                }
            }
        }
    }

    let _ = session.cancel_tcpip_forward(bind, listen_port).await;
    let _ = session
        .disconnect(russh::Disconnect::ByApplication, "", "English")
        .await;
    drain_tasks(&mut tasks, SHUTDOWN_DRAIN_TIMEOUT).await;
    let _ = stopped.send(final_logs);
}

async fn handle(forwarded: ForwardedTcpIp, dest_host: String, dest_port: u16) {
    let stream = forwarded.channel.into_stream();
    let Ok(Ok(socket)) = tokio::time::timeout(
        CONNECT_TIMEOUT,
        TcpStream::connect((dest_host.as_str(), dest_port)),
    )
    .await
    else {
        return;
    };
    let _ = copy_bidirectional_idle(stream, socket, IDLE_TIMEOUT).await;
}
