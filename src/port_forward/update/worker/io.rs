//! Shared asynchronous I/O helpers for forward workers.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::task::JoinSet;

/// Maximum simultaneous client tunnels accepted by one forward.
pub(super) const MAX_CLIENT_CONNECTIONS: usize = 128;

/// Upper bound for establishing an SSH or TCP destination connection.
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Upper bound for a SOCKS negotiation exchange.
pub(super) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Idle tunnels are closed after this interval.
pub(super) const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Maximum time spent draining active tunnels after a worker stops.
pub(super) const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Per-direction buffer size for a tunnel.
const IO_BUFFER_SIZE: usize = 16 * 1024;

pub(super) async fn drain_tasks(tasks: &mut JoinSet<()>, timeout: Duration) {
    if tasks.is_empty() {
        return;
    }
    let drained = tokio::time::timeout(timeout, async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
}

pub(super) async fn copy_bidirectional_idle<A, B>(
    a: A,
    b: B,
    idle_timeout: Duration,
) -> std::io::Result<(u64, u64)>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (mut a_reader, mut a_writer) = tokio::io::split(a);
    let (mut b_reader, mut b_writer) = tokio::io::split(b);
    let mut a_to_b = [0u8; IO_BUFFER_SIZE];
    let mut b_to_a = [0u8; IO_BUFFER_SIZE];
    let mut a_to_b_done = false;
    let mut b_to_a_done = false;
    let mut a_to_b_bytes = 0;
    let mut b_to_a_bytes = 0;

    while !a_to_b_done || !b_to_a_done {
        tokio::select! {
            result = read_with_idle_timeout(&mut a_reader, &mut a_to_b, idle_timeout),
                if !a_to_b_done =>
            {
                match result? {
                    0 => {
                        b_writer.shutdown().await?;
                        a_to_b_done = true;
                    }
                    count => {
                        write_with_idle_timeout(
                            &mut b_writer,
                            &a_to_b[..count],
                            idle_timeout,
                        )
                        .await?;
                        a_to_b_bytes += count as u64;
                    }
                }
            }
            result = read_with_idle_timeout(&mut b_reader, &mut b_to_a, idle_timeout),
                if !b_to_a_done =>
            {
                match result? {
                    0 => {
                        a_writer.shutdown().await?;
                        b_to_a_done = true;
                    }
                    count => {
                        write_with_idle_timeout(
                            &mut a_writer,
                            &b_to_a[..count],
                            idle_timeout,
                        )
                        .await?;
                        b_to_a_bytes += count as u64;
                    }
                }
            }
        }
    }
    Ok((a_to_b_bytes, b_to_a_bytes))
}

async fn read_with_idle_timeout<R>(
    reader: &mut R,
    buffer: &mut [u8],
    timeout: Duration,
) -> std::io::Result<usize>
where
    R: AsyncRead + Unpin,
{
    match tokio::time::timeout(timeout, reader.read(buffer)).await {
        Ok(result) => result,
        Err(_) => Err(idle_timeout_error()),
    }
}

async fn write_with_idle_timeout<W>(
    writer: &mut W,
    buffer: &[u8],
    timeout: Duration,
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    match tokio::time::timeout(timeout, writer.write_all(buffer)).await {
        Ok(result) => result,
        Err(_) => Err(idle_timeout_error()),
    }
}

fn idle_timeout_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "forward connection idle timeout",
    )
}
