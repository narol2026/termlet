use russh::{ChannelMsg, Disconnect};

use crate::core::data::Host;

use super::session::connect_authenticated_session;

// SSH command execution

pub(crate) async fn ssh_exec_command(host: Host, command: String) -> Result<String, String> {
    let mut logs = Vec::new();
    let session = connect_authenticated_session(&host, &mut logs).await?;
    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("Failed to open SSH channel: {e}"))?;

    channel
        .exec(true, command.as_bytes())
        .await
        .map_err(|e| format!("Failed to exec remote command: {e}"))?;

    // Drain the channel message-by-message instead of via `into_stream()`:
    // russh's AsyncRead reader only forwards `ChannelMsg::Data` (stdout) and
    // silently discards `ExtendedData` (stderr) and `ExitStatus`, so a command
    // that failed remotely (e.g. `rm -rf` denied) used to look like success.
    // Capturing stdout, stderr and the exit status lets callers such as the
    // SFTP delete fast-path distinguish a real success from a remote failure.
    let mut channel = channel;
    let mut output = Vec::new();
    let mut error_output = Vec::new();
    let mut remote_status: Option<u32> = None;
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data } => output.extend_from_slice(data.as_ref()),
            ChannelMsg::ExtendedData { data, .. } => error_output.extend_from_slice(data.as_ref()),
            ChannelMsg::ExitStatus { exit_status } => remote_status = Some(exit_status),
            // Do NOT stop on `Eof`: OpenSSH sends it before the exit-status
            // request, so breaking here would miss a non-zero exit.
            ChannelMsg::Eof => {}
            ChannelMsg::Close => break,
            _ => {}
        }
    }

    let _ = session
        .disconnect(Disconnect::ByApplication, "", "English")
        .await;

    match remote_status {
        Some(code) if code != 0 => {
            let stderr = String::from_utf8_lossy(&error_output);
            let stderr = stderr.trim();
            let hint = if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            };
            Err(format!("Remote command exited with status {code}{hint}"))
        }
        _ => String::from_utf8(output)
            .map_err(|e| format!("Remote command output is not valid UTF-8: {e}")),
    }
}

pub(crate) async fn detect_remote_os(host: Host) -> Result<crate::core::data::OsType, String> {
    let output = ssh_exec_command(
        host,
        "cat /etc/os-release 2>/dev/null && uname -s || uname -s".to_string(),
    )
    .await?;

    if output.contains("ID=") {
        return Ok(crate::core::data::OsType::from_os_release(&output));
    }

    let uname = output.trim();
    if uname == "Darwin" {
        return Ok(crate::core::data::OsType::MacOS);
    }

    // No recognizable distro or kernel — report Unknown rather than guessing
    // Linux (FreeBSD and friends would otherwise be mislabeled).
    Ok(crate::core::data::OsType::Unknown)
}

/// End-to-end checks that [`ssh_exec_command`] reports remote failures instead
/// of treating every round-trip as success.
///
/// Regression for the SFTP folder-delete fast path (`rm -rf`): a non-zero
/// remote exit used to be invisible, so a denied/busy deletion still showed
/// "Deleted" while the folder survived on the server.
///
/// Run explicitly with `cargo test -- --ignored ssh::client::exec::remote_exec_tests`.
/// Requires the current user's SSH key authorized for 127.0.0.1 and the host
/// key in `known_hosts`:
///
///   ssh-keyscan -p 22 127.0.0.1 >> ~/.`ssh/known_hosts`
///   cat ~/.`ssh/id_ecdsa.pub` >> ~/.`ssh/authorized_keys`
///
/// Skips (does not fail) when localhost SSH is unavailable, so it is safe to
/// leave in the suite.
#[cfg(test)]
mod remote_exec_tests {
    use std::sync::Arc;

    use super::super::session::probe_host_key;
    use super::super::transfer::download_sftp_file_async;
    use super::ssh_exec_command;
    use crate::core::data::{AuthType, Host, HostId, OsType, Protocol};
    use crate::sftp::SftpTransferProgress;

    async fn localhost_host() -> Option<Host> {
        let home = std::env::var_os("HOME")?;
        let username = std::env::var("USER").unwrap_or_else(|_| "root".into());
        let key = probe_host_key("127.0.0.1".into(), 22).await.ok()?;
        Some(Host {
            id: HostId(0),
            name: "localhost".into(),
            hostname: "127.0.0.1".into(),
            port: 22,
            protocol: Protocol::Ssh,
            username,
            auth_type: AuthType::Key,
            jump_host_id: None,
            identity_id: None,
            key_path: Some(
                std::path::Path::new(&home)
                    .join(".ssh/id_ecdsa")
                    .to_string_lossy()
                    .into(),
            ),
            key_passphrase: None,
            password: None,
            os_type: OsType::Unknown,
            tags: vec![],
            expected_fingerprint: Some(key.fingerprint),
        })
    }

    #[tokio::test]
    #[ignore = "requires a local sshd"]
    async fn e2e_remote_command_reports_exit_status() {
        let Some(host) = localhost_host().await else {
            eprintln!("skipping e2e (no local sshd)");
            return;
        };

        // A command that fails on the remote must surface as Err.
        let err = ssh_exec_command(host.clone(), "false".into())
            .await
            .unwrap_err();
        assert!(err.contains("status 1"), "unexpected error: {err}");

        // stderr text is captured and included in the error.
        let err = ssh_exec_command(host.clone(), "echo boom >&2; exit 7".into())
            .await
            .unwrap_err();
        assert!(err.contains("status 7"), "unexpected error: {err}");
        assert!(err.contains("boom"), "stderr missing from error: {err}");

        // Successful commands keep returning their stdout.
        let out = ssh_exec_command(host.clone(), "echo termlet-exec-ok".into())
            .await
            .unwrap();
        assert!(
            out.contains("termlet-exec-ok"),
            "unexpected output: {out:?}"
        );

        // `rm -rf` on an already-absent path exits 0, so the delete fast-path
        // still treats "nothing there" as success.
        ssh_exec_command(host, "rm -rf /tmp/termlet-e2e-does-not-exist-9f3c".into())
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires a local sshd"]
    async fn e2e_download_matches_remote_byte_count() {
        const SIZE: u64 = 300_000;
        let Some(host) = localhost_host().await else {
            eprintln!("skipping e2e (no local sshd)");
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let id = format!("{}-{now}", std::process::id());
        let remote = format!("/tmp/termlet-e2e-dl-{id}.bin");
        let remote_small = format!("/tmp/termlet-e2e-small-{id}.bin");
        let reference = std::env::temp_dir().join(format!("termlet-e2e-dl-ref-{id}.bin"));

        ssh_exec_command(
            host.clone(),
            format!("head -c {SIZE} /dev/urandom > {remote}"),
        )
        .await
        .unwrap();

        let progress = || {
            Arc::new(std::sync::Mutex::new(SftpTransferProgress {
                file_name: "file.bin".into(),
                direction: crate::sftp::SftpTransferDirection::Download,
                transferred_bytes: 0,
                total_bytes: SIZE,
                cancelled: false,
                current_file: "file.bin".into(),
                files_completed: 0,
                total_files: 1,
                resumed: false,
                start_time: None,
                speed: 0.0,
                last_speed_update: None,
            }))
        };

        // A clean full download must land exactly the remote byte count.
        download_sftp_file_async(
            host.clone(),
            remote.clone(),
            reference.clone(),
            progress(),
            None,
        )
        .await
        .unwrap();
        let full = std::fs::read(&reference).unwrap();
        assert_eq!(full.len() as u64, SIZE, "full download length mismatch");

        // Tie the local bytes back to the remote file so a both-sides-equal
        // truncation cannot slip through.
        let remote_md5 = ssh_exec_command(host.clone(), format!("md5sum {remote}"))
            .await
            .unwrap();
        let local_md5 = String::from_utf8(
            std::process::Command::new("md5sum")
                .arg(&reference)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let remote_hash = remote_md5.split_whitespace().next().unwrap().to_string();
        let local_hash = local_md5.split_whitespace().next().unwrap().to_string();
        assert_eq!(
            remote_hash, local_hash,
            "downloaded bytes differ from remote"
        );

        // Resume from a 100_000-byte local prefix: the byte count must catch up
        // to the remote size and reconstruct the same file.
        {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(&reference)
                .unwrap();
            file.set_len(100_000).unwrap();
        };
        download_sftp_file_async(
            host.clone(),
            remote.clone(),
            reference.clone(),
            progress(),
            Some(100_000),
        )
        .await
        .unwrap();
        let resumed = std::fs::read(&reference).unwrap();
        assert_eq!(
            resumed.len() as u64,
            SIZE,
            "resumed download length mismatch"
        );
        assert_eq!(
            resumed, full,
            "resumed download differs from clean download"
        );

        // A local partial file larger than the remote must be refused up front
        // instead of silently producing a mangled local file.
        ssh_exec_command(
            host.clone(),
            format!("head -c 1000 /dev/urandom > {remote_small}"),
        )
        .await
        .unwrap();
        // NB: on localhost the remote file lives in the same /tmp, so the local
        // partial file must use a different name or it would overwrite the remote.
        let small_local = std::env::temp_dir().join(format!("termlet-e2e-small-local-{id}.bin"));
        std::fs::write(&small_local, vec![0_u8; 2000]).unwrap();
        let err = download_sftp_file_async(
            host.clone(),
            remote_small.clone(),
            small_local.clone(),
            progress(),
            Some(2000),
        )
        .await
        .unwrap_err();
        assert!(
            err.message()
                .is_some_and(|message| message.contains("larger than the remote file")),
            "unexpected error: {err:?}"
        );

        let _ = ssh_exec_command(host.clone(), format!("rm -f {remote} {remote_small}")).await;
        let _ = std::fs::remove_file(&reference);
        let _ = std::fs::remove_file(&small_local);
    }
}
