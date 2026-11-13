use super::completion::finish_for_test;
use super::*;
use crate::sftp::{
    SftpQueuedTransfer, SftpQueuedTransferKind, SftpTransferDirection, SftpTransferFailure,
    SftpTransferProgress, SftpTransferResultStatus, SftpTransferTarget,
};
use std::sync::{Arc, Mutex};

fn progress(file_name: &str) -> Arc<Mutex<SftpTransferProgress>> {
    Arc::new(Mutex::new(SftpTransferProgress::file(
        file_name.into(),
        SftpTransferDirection::Download,
        1024,
        512,
        false,
    )))
}

#[test]
fn failed_transfers_accumulate_and_can_be_dismissed_individually() {
    let mut app = crate::core::test_support::app();
    app.sftp.state.transfer_progress = Some(progress("release.tar.gz"));
    finish_for_test(
        &mut app,
        SftpTransferResultStatus::Failed,
        Some("permission denied while writing the destination".into()),
    );
    app.sftp.state.transfer_progress = Some(progress("archive.zip"));
    finish_for_test(
        &mut app,
        SftpTransferResultStatus::Failed,
        Some("connection reset".into()),
    );

    assert!(app.sftp.state.transfer_results.latest().is_none());
    assert_eq!(app.sftp.state.transfer_results.failures().len(), 2);
    let result = &app.sftp.state.transfer_results.failures()[0];
    assert_eq!(result.file_name, "release.tar.gz");
    assert_eq!(result.status, SftpTransferResultStatus::Failed);
    assert_eq!(
        result.error.as_deref(),
        Some("permission denied while writing the destination")
    );
    assert!(app.sftp.state.transfer_modal.is_none());

    let _ = update(&mut app, DomainMessage::SftpDismissTransferFailure(0));
    assert_eq!(app.sftp.state.transfer_results.failures().len(), 1);
    assert_eq!(
        app.sftp.state.transfer_results.failures()[0].file_name,
        "archive.zip"
    );
}

#[test]
fn only_the_ten_most_recent_failures_are_retained() {
    let mut app = crate::core::test_support::app();

    for index in 0..12 {
        let file_name = format!("file-{index}.txt");
        app.sftp.state.transfer_progress = Some(progress(&file_name));
        finish_for_test(
            &mut app,
            SftpTransferResultStatus::Failed,
            Some(format!("failure {index}")),
        );
    }

    let failures = app.sftp.state.transfer_results.failures();
    assert_eq!(failures.len(), 10);
    assert_eq!(failures[0].file_name, "file-2.txt");
    assert_eq!(failures[9].file_name, "file-11.txt");
}

#[test]
fn a_connection_failure_for_one_file_starts_the_next_queued_transfer() {
    let mut app = crate::core::test_support::app();
    let session_id = app.sftp.active_session;
    let first_target = SftpTransferTarget::Local {
        directory: "/tmp".into(),
        name: "first.iso".into(),
    };
    app.sftp.state.transfer_progress = Some(progress("first.iso"));
    app.sftp.state.transfer_queue = vec![SftpQueuedTransfer {
        file_name: "second.iso".into(),
        direction: SftpTransferDirection::Download,
        total_bytes: 1024,
        total_files: 1,
        target: SftpTransferTarget::Local {
            directory: "/tmp".into(),
            name: "second.iso".into(),
        },
        kind: SftpQueuedTransferKind::DownloadFile {
            host: crate::core::test_support::host(1, "web", "10.0.0.1"),
            remote_path: "/srv/second.iso".into(),
            local_path: "/tmp/termlet-never-created-second.iso".into(),
            remote_size: 1024,
        },
    }];

    let _ = update(
        &mut app,
        DomainMessage::SftpTransferFinished {
            target: first_target,
            result: Err(SftpTransferFailure::Connection {
                message: "Download incomplete".into(),
            }),
            session_id,
        },
    );

    assert_eq!(app.sftp.state.transfer_results.failures().len(), 1);
    assert!(app.sftp.state.transfer_queue.is_empty());
    let next = app
        .sftp
        .state
        .transfer_progress
        .as_ref()
        .expect("the next transfer starts");
    assert_eq!(next.lock().unwrap().file_name, "second.iso");
}
