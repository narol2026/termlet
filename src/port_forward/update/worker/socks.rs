//! Minimal SOCKS5 CONNECT handling over an SSH direct-tcpip channel.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::io::{CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT, IDLE_TIMEOUT, copy_bidirectional_idle};
use crate::ssh::Client;

pub(super) async fn handle(
    session: Arc<russh::client::Handle<Client>>,
    mut socket: TcpStream,
    peer: std::net::SocketAddr,
) {
    let Ok(Ok((host, port))) =
        tokio::time::timeout(HANDSHAKE_TIMEOUT, negotiate(&mut socket)).await
    else {
        return;
    };
    let channel = tokio::time::timeout(
        CONNECT_TIMEOUT,
        session.channel_open_direct_tcpip(
            host,
            u32::from(port),
            peer.ip().to_string(),
            u32::from(peer.port()),
        ),
    )
    .await;

    let channel = if let Ok(Ok(channel)) = channel {
        // BND.ADDR=0.0.0.0:0 is accepted by common SOCKS clients.
        if tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            socket.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]),
        )
        .await
        .is_err()
        {
            return;
        }
        channel
    } else {
        let _ = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            socket.write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]),
        )
        .await;
        return;
    };

    let _ = copy_bidirectional_idle(socket, channel.into_stream(), IDLE_TIMEOUT).await;
}

async fn negotiate(socket: &mut TcpStream) -> std::io::Result<(String, u16)> {
    let mut header = [0u8; 2];
    socket.read_exact(&mut header).await?;
    if header[0] != 0x05 {
        return Err(invalid_data("unsupported SOCKS version"));
    }

    let mut methods = vec![0u8; header[1] as usize];
    socket.read_exact(&mut methods).await?;
    socket.write_all(&[0x05, 0x00]).await?;

    let mut request = [0u8; 4];
    socket.read_exact(&mut request).await?;
    if request[0] != 0x05 {
        return Err(invalid_data("unsupported SOCKS version"));
    }
    if request[1] != 0x01 {
        let _ = socket
            .write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await;
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "SOCKS command not supported",
        ));
    }

    let host = match request[3] {
        0x01 => {
            let mut address = [0u8; 4];
            socket.read_exact(&mut address).await?;
            std::net::Ipv4Addr::from(address).to_string()
        }
        0x03 => {
            let mut length = [0u8; 1];
            socket.read_exact(&mut length).await?;
            let mut domain = vec![0u8; length[0] as usize];
            socket.read_exact(&mut domain).await?;
            String::from_utf8(domain).map_err(|_error| invalid_data("invalid SOCKS domain"))?
        }
        0x04 => {
            let mut address = [0u8; 16];
            socket.read_exact(&mut address).await?;
            std::net::Ipv6Addr::from(address).to_string()
        }
        _ => {
            let _ = socket
                .write_all(&[0x05, 0x08, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await;
            return Err(invalid_data("unsupported SOCKS address type"));
        }
    };

    let mut port = [0u8; 2];
    socket.read_exact(&mut port).await?;
    Ok((host, u16::from_be_bytes(port)))
}

fn invalid_data(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}
