pub mod bridge;
pub mod client;
pub mod known_hosts_file;
mod local_tty;
pub mod shell_integration;

// Re-export the SSH client API so existing `crate::ssh::*` callers continue to work.
pub use client::*;
