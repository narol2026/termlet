mod connection_pool;
mod exec;
mod path_utils;
mod session;
mod sftp_dir;
mod transfer;

pub(crate) use session::open_authenticated_session_with_jump;
pub(crate) use session::{
    Client, ForwardedTcpIp, open_authenticated_session, open_authenticated_session_with,
};
pub use session::{
    authenticate_host, authenticate_host_via_jump, key_requires_passphrase, probe_host_key,
    probe_host_key_via_jump,
};

pub(crate) use sftp_dir::try_rm_rf_fast;
pub use sftp_dir::{
    list_sftp_dir, remote_path_exists, resolve_and_list_default_sftp_dir, sftp_create_remote_dir,
    sftp_file_entry, sftp_read_text_preview, sftp_remove_dir_recursive, sftp_remove_file,
    sftp_rename_file,
};

pub(crate) use transfer::SFTP_ATOMIC_SIZE_THRESHOLD;
pub use transfer::{
    copy_sftp_file_between_hosts, copy_sftp_file_between_hosts_atomic,
    copy_sftp_folder_between_hosts, download_sftp_file, download_sftp_folder, upload_sftp_file,
    upload_sftp_file_atomic, upload_sftp_folder, upload_sftp_folder_atomic,
    verify_download_checksum, verify_remote_to_remote_checksum, verify_upload_checksum,
};

pub(crate) use exec::detect_remote_os;
#[cfg(test)]
pub(crate) use exec::ssh_exec_command;
