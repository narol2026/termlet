//! Local and dynamic TCP listener workers.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, Semaphore, oneshot};
use tokio::task::JoinSet;

use super::io::{
    CONNECT_TIMEOUT, IDLE_TIMEOUT, MAX_CLIENT_CONNECTIONS, SHUTDOWN_DRAIN_TIMEOUT,
    copy_bidirectional_idle, drain_tasks,
};
use super::{bind_address_for_listen, socks};
use crate::core::data::{Host, PortForward};
use crate::ssh::{Client, open_authenticated_session};
use rust_i18n::t;

type StartedSender = oneshot::Sender<(bool, Vec<String>)>;
type StoppedSender = oneshot::Sender<Vec<String>>;

#[derive(Clone)]
enum Mode {
    Local { dest_host: String, dest_port: u16 },
    Dynamic,
}

pub(super) async fn run_local(
    host: Host,
    forward: PortForward,
    shutdown: Arc<Notify>,
    started: StartedSender,
    stopped: StoppedSender,
) {
    let mode = Mode::Local {
        dest_host: forward.dest_host.clone(),
        dest_port: forward.dest_port,
    };
    run_listener(host, forward, shutdown, started, stopped, mode).await;
}

pub(super) async fn run_dynamic(
    host: Host,
    forward: PortForward,
    shutdown: Arc<Notify>,
    started: StartedSender,
    stopped: StoppedSender,
) {
    run_listener(host, forward, shutdown, started, stopped, Mode::Dynamic).await;
}

async fn run_listener(
    host: Host,
    forward: PortForward,
    shutdown: Arc<Notify>,
    started: StartedSender,
    stopped: StoppedSender,
    mode: Mode,
) {
    let bind = bind_address_for_listen(&forward.bind_address).to_string();
    let listen_addr = format!("{bind}:{}", forward.listen_port);
    let listener = match TcpListener::bind(&listen_addr).await {
        Ok(listener) => listener,
        Err(error) => {
            let _ = started.send((false, vec![format!("bind {listen_addr} failed: {error}")]));
            let _ = stopped.send(Vec::new());
            return;
        }
    };
    let session = match open_authenticated_session(&host).await {
        Ok(session) => Arc::new(session),
        Err(error) => {
            let _ = started.send((false, vec![error]));
            let _ = stopped.send(Vec::new());
            return;
        }
    };

    let ready = match &mode {
        Mode::Local { .. } => format!(
            "Listening on {listen_addr}, forwarding to {}:{}",
            forward.dest_host, forward.dest_port
        ),
        Mode::Dynamic => format!("SOCKS5 proxy listening on {listen_addr}"),
    };
    let _ = started.send((true, vec![ready]));

    let connection_slots = Arc::new(Semaphore::new(MAX_CLIENT_CONNECTIONS));
    let mut final_logs = Vec::new();
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            () = shutdown.notified() => {
                final_logs.push(t!("port_forward.stopped").to_string());
                break;
            }
            accept = listener.accept() => {
                match accept {
                    Ok((socket, peer)) => {
                        let Ok(permit) =
                            Arc::clone(&connection_slots).try_acquire_owned()
                        else {
                            final_logs.push(format!(
                                "client connection limit reached, rejecting {peer}"
                            ));
                            continue;
                        };
                        let session = Arc::clone(&session);
                        let mode = mode.clone();
                        tasks.spawn(async move {
                            match mode {
                                Mode::Local { dest_host, dest_port } => {
                                    handle_local(
                                        session,
                                        socket,
                                        dest_host,
                                        u32::from(dest_port),
                                        peer,
                                    )
                                    .await;
                                }
                                Mode::Dynamic => socks::handle(session, socket, peer).await,
                            }
                            drop(permit);
                        });
                    }
                    Err(error) => {
                        final_logs.push(format!("accept failed: {error}"));
                        break;
                    }
                }
            }
            Some(result) = tasks.join_next(), if !tasks.is_empty() => {
                if result.is_err() {
                    final_logs.push("forward connection task failed".to_string());
                }
            }
        }
    }

    drop(listener);
    let _ = session
        .disconnect(russh::Disconnect::ByApplication, "", "English")
        .await;
    drain_tasks(&mut tasks, SHUTDOWN_DRAIN_TIMEOUT).await;
    drop(session);
    let _ = stopped.send(final_logs);
}

async fn handle_local(
    session: Arc<russh::client::Handle<Client>>,
    socket: TcpStream,
    dest_host: String,
    dest_port: u32,
    peer: std::net::SocketAddr,
) {
    let Ok(Ok(channel)) = tokio::time::timeout(
        CONNECT_TIMEOUT,
        session.channel_open_direct_tcpip(
            dest_host,
            dest_port,
            peer.ip().to_string(),
            u32::from(peer.port()),
        ),
    )
    .await
    else {
        return;
    };
    let _ = copy_bidirectional_idle(socket, channel.into_stream(), IDLE_TIMEOUT).await;
}
