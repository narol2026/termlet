//! Shared fixtures for the unit tests. Compiled only under `cfg(test)`.
//!
//! The app-level tests drive a real [`crate::app::App`] with the messages the UI
//! sends, which keeps the update handlers honest about their wiring (message in,
//! state changed, task out). That needs two things kept away from the developer's
//! machine: the database, opened in memory instead of on disk, and the data
//! directory, redirected to a scratch directory here so logs and caches land
//! there rather than in `~/.termlet/`.

use std::path::PathBuf;
use std::sync::Once;

use crate::core::data::{AuthType, Host, HostId, OsType, Protocol};
use crate::core::message::Message;

/// Point this process's app data directory at a scratch directory.
///
/// `Once` rather than a guard: the directory is shared by every test in the
/// binary and never restored, which is fine because nothing in the suite reads
/// the real one.
///
/// App-level tests reach this through [`app`]; the module tests that only write
/// a cache or a log file (command history and crash reports) call it
/// directly instead of paying for a booted [`crate::app::App`].
pub(crate) fn prepare_data_dir() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = scratch_dir(&format!("termlet-tests-{}", std::process::id()));
        crate::core::paths::set_test_data_dir(dir);
    });
}

/// A scratch directory under the system temp dir, emptied first so a previous
/// run cannot leak state into this one.
pub(crate) fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

/// A directory of this thread's own for the saved-workspace files.
///
/// The app draws one row per saved workspace, so a test that saves a few would
/// change the pages the app-level tests render. This gives the caller a private
/// directory — invisible to every other test — and returns it for the file
/// checks that need to poke at what was written.
pub(crate) fn prepare_workspace_dir(tag: &str) -> PathBuf {
    let dir = scratch_dir(&format!("termlet-workspaces-{tag}"));
    crate::core::paths::set_test_workspace_dir(dir.clone());
    dir
}

/// A real app on an in-memory database, ready to be driven with messages.
///
/// The startup task is dropped: nothing runs without the iced runtime, so a
/// handler's `Task::perform` work simply never happens. That is what makes
/// these tests deterministic — the state transitions on the way to the task
/// are exactly the part under test.
pub(crate) fn app() -> crate::app::App {
    app_with_startup_task().0
}

/// The same app together with its startup task, for the headless runtime tests
/// that let the real boot pipeline run.
pub(crate) fn app_with_startup_task() -> (crate::app::App, iced::Task<crate::core::message::Message>)
{
    prepare_data_dir();
    crate::app::App::for_test_with_boot()
}

/// The boot fixture for a vault that is already in the database: the saved
/// hosts come back through the real startup load, as they do on a second
/// launch of the app.
pub(crate) fn app_with_saved_hosts(
    hosts: &[Host],
) -> (crate::app::App, iced::Task<crate::core::message::Message>) {
    prepare_data_dir();
    crate::app::App::for_test_with_saved_hosts(hosts)
}

/// A saved host with the fields the tests do not care about filled in.
///
/// Password auth with a known password is the default because it connects
/// without touching the filesystem (a key host would have its key path read).
pub(crate) fn host(id: u32, name: &str, hostname: &str) -> Host {
    Host {
        id: HostId(id),
        name: name.to_string(),
        hostname: hostname.to_string(),
        port: 22,
        protocol: Protocol::Ssh,
        username: "root".to_string(),
        auth_type: AuthType::Password,
        jump_host_id: None,
        identity_id: None,
        key_path: None,
        key_passphrase: None,
        password: Some(secrecy::SecretString::from("hunter2")),
        os_type: OsType::Unknown,
        tags: Vec::new(),
        expected_fingerprint: None,
    }
}

/// What a handler's returned [`iced::Task`] carries.
///
/// A task is how the update layer asks for something to happen next, so a test
/// that only asserts on state change stops at the boundary of the handler and
/// leaves the request itself unchecked. Reading the task back closes that gap:
/// `messages` is what the app would feed into `update` next, and
/// `other_actions` counts the requests that are not messages — a focus move, a
/// window call — which a test can only assert was asked for.
#[derive(Debug, Default)]
pub(crate) struct Drained {
    pub(crate) messages: Vec<Message>,
    pub(crate) other_actions: usize,
}

/// Read back what `task` carries, running none of it further.
///
/// Only the tasks built by `Task::done`, `Task::none` and the widget operation
/// helpers can be read this way: those produce their value on the first poll.
/// A `Task::perform` starts its future here, so do not hand one that does real
/// work to this function.
pub(crate) fn drain(task: iced::Task<Message>) -> Drained {
    use iced::futures::StreamExt;

    let Some(stream) = iced_runtime::task::into_stream(task) else {
        return Drained::default();
    };

    iced::futures::executor::block_on(async move {
        let mut drained = Drained::default();
        let mut stream = std::pin::pin!(stream);

        while let Some(action) = stream.next().await {
            match action {
                iced_runtime::Action::Output(message) => drained.messages.push(message),
                _ => drained.other_actions += 1,
            }
        }

        drained
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the seam above: a test run must never read or write
    /// the developer's own `~/.termlet/`. If this fails, every app-level test
    /// that touches storage, caches or the diagnostics log is writing into the
    /// real profile.
    #[test]
    fn the_test_app_confines_itself_to_a_scratch_directory() {
        let _ = app();

        let dir = crate::core::paths::config_dir();
        assert!(
            dir.starts_with(std::env::temp_dir()),
            "the test data directory is outside the temp dir: {}",
            dir.display()
        );
        assert!(dir.exists(), "the scratch directory was not created");
        assert!(
            crate::core::paths::termlet_dir().starts_with(std::env::temp_dir()),
            "termlet_dir still points at the real profile"
        );
    }
}
