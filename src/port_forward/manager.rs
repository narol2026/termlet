use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::core::data::PortForwardId;

const WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const WORKER_ABORT_TIMEOUT: Duration = Duration::from_secs(5);

/// Process-lifetime runtime shared by every port-forward worker.
///
/// The previous implementation created one `current_thread` runtime (and one
/// OS thread) per forward. Keeping a single multi-thread runtime here makes
/// resource usage bounded and gives every worker a stable reactor for russh
/// channel cleanup.
static PORT_FORWARD_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub(crate) fn port_forward_runtime() -> &'static tokio::runtime::Runtime {
    PORT_FORWARD_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("termlet-port-forward")
            .enable_all()
            .build()
            .expect("failed to build port-forward runtime")
    })
}

struct ForwardWorker {
    shutdown: Arc<Notify>,
    task: JoinHandle<()>,
    generation: u64,
}

/// Owns the live port-forward tasks and their shutdown signals.
///
/// UI state remains in `PortForwardState`; this type only tracks the async
/// side of the lifecycle. A worker is removed once its `PortForwardStopped`
/// message has been delivered, so the remaining entries are always the
/// forward tasks that may still need to be shut down at exit.
#[derive(Default)]
pub struct PortForwardManager {
    workers: HashMap<PortForwardId, ForwardWorker>,
    next_generation: u64,
}

impl PortForwardManager {
    pub fn spawn<F>(&mut self, id: PortForwardId, shutdown: Arc<Notify>, future: F) -> u64
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1);

        let task = port_forward_runtime().spawn(future);
        self.workers.insert(
            id,
            ForwardWorker {
                shutdown,
                task,
                generation,
            },
        );

        generation
    }

    pub fn signal(&self, id: PortForwardId) -> bool {
        match self.workers.get(&id) {
            Some(worker) => {
                worker.shutdown.notify_one();
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: PortForwardId, generation: u64) -> Option<JoinHandle<()>> {
        match self.workers.get(&id) {
            Some(worker) if worker.generation == generation => {
                self.workers.remove(&id).map(|worker| worker.task)
            }
            _ => None,
        }
    }

    /// Notify every worker and wait briefly for them to finish.
    ///
    /// This is only intended for process teardown, where blocking the UI
    /// thread for a bounded amount of time is preferable to leaking detached
    /// forward tasks.
    pub fn shutdown_all(&mut self) {
        if self.workers.is_empty() {
            return;
        }

        for worker in self.workers.values() {
            worker.shutdown.notify_one();
        }

        let workers = std::mem::take(&mut self.workers);
        let mut tasks: Vec<JoinHandle<()>> =
            workers.into_values().map(|worker| worker.task).collect();

        // `App::drop` runs synchronously inside the winit event loop and outside
        // any tokio runtime context, so `block_on` here cannot hit tokio's
        // "runtime within a runtime" panic. Keep it that way: if this teardown
        // ever moves into an async context, replace the blocking wait with a
        // detached cleanup task instead.
        port_forward_runtime().block_on(async {
            let drained = tokio::time::timeout(WORKER_SHUTDOWN_TIMEOUT, async {
                for task in &mut tasks {
                    let _ = task.await;
                }
            })
            .await;

            if drained.is_err() {
                for task in &mut tasks {
                    task.abort();
                }
                // Aborting is cooperative: joining must still be bounded in case
                // a worker is stuck at an uncancellable await point, otherwise
                // process teardown could block forever.
                let _ = tokio::time::timeout(WORKER_ABORT_TIMEOUT, async {
                    for task in &mut tasks {
                        let _ = task.await;
                    }
                })
                .await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_all_signals_and_joins_worker() {
        let mut manager = PortForwardManager::default();
        let shutdown = Arc::new(Notify::new());
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();

        manager.spawn(
            PortForwardId(1),
            std::sync::Arc::clone(&shutdown),
            async move {
                shutdown.notified().await;
                let _ = done_tx.send(());
            },
        );

        manager.shutdown_all();

        assert_eq!(port_forward_runtime().block_on(done_rx), Ok(()));
    }

    #[test]
    fn remove_checks_generation() {
        let mut manager = PortForwardManager::default();

        let gen_a = manager.spawn(
            PortForwardId(1),
            Arc::new(Notify::new()),
            std::future::pending(),
        );
        let gen_b = manager.spawn(
            PortForwardId(1),
            Arc::new(Notify::new()),
            std::future::pending(),
        );

        // `gen_a` was replaced by `gen_b`; a stale generation must not remove
        // the live worker.
        assert!(manager.remove(PortForwardId(1), gen_a).is_none());
        assert!(manager.signal(PortForwardId(1)));

        // Only the current generation can be removed.
        assert!(manager.remove(PortForwardId(1), gen_b).is_some());
    }
}
