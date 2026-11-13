//! Console-subsystem entry point for the in-process SSH bridge.

use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(error) = termlet::ensure_data_dir() {
        eprintln!("termlet-bridge: failed to secure ~/.termlet: {error}");
        return ExitCode::FAILURE;
    }

    termlet::init_crash_reporter();
    rust_i18n::set_locale("en");

    let host_id = std::env::args().nth(1).unwrap_or_default();
    termlet::run_ssh_bridge(&host_id)
}
