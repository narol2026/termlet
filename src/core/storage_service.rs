//! Background `SQLite` write service.
//!
//! `SQLite` writes (`next_id_for`, WAL checkpoints, encrypted preferences) can
//! take several milliseconds, and occasionally much longer, so running them
//! inline in the `update` dispatcher caused visible UI jank. They move onto one
//! dedicated worker thread instead: the UI side submits a closure describing
//! the write *intent*, gets a [`StorageRequest`] future back and runs it as an
//! `iced::Task`. The single worker and its job queue serialize writes, so the
//! event loop never blocks and two writes cannot interleave inside `SQLite`.

use std::pin::Pin;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::thread::JoinHandle;

use crate::core::storage::Storage;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// A queued storage operation.  Awaiting this value blocks only the async
/// task that owns it, never the UI thread.
pub struct StorageRequest<T> {
    receiver: tokio::sync::oneshot::Receiver<Result<T, String>>,
}

impl<T> Future for StorageRequest<T> {
    type Output = Result<T, String>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.receiver).poll(cx) {
            Poll::Ready(Ok(result)) => Poll::Ready(result),
            Poll::Ready(Err(_)) => Poll::Ready(Err(
                "storage worker stopped before completing the write".into(),
            )),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Background storage actor.  All write operations are serialized through a
/// single worker thread, while reads that are already performed in a
/// `spawn_blocking` startup task may continue to use `Arc<Storage>` directly.
pub struct StorageService {
    storage: Arc<Storage>,
    sender: Mutex<Sender<Job>>,
    _worker: Option<JoinHandle<()>>,
}

impl StorageService {
    /// Start the worker thread and return a handle that can be shared with UI
    /// code.  The thread runs until all `StorageService` handles and senders
    /// are dropped.
    pub fn start(storage: Arc<Storage>) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel::<Job>();
        let worker = std::thread::Builder::new()
            .name("termlet-storage-io".to_string())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    job();
                }
            })
            .expect("failed to spawn storage IO worker");

        Arc::new(Self {
            storage,
            sender: Mutex::new(sender),
            _worker: Some(worker),
        })
    }

    /// Queue a storage write.  This returns immediately; the returned future
    /// resolves once the dedicated worker has executed `f`.
    ///
    /// The closure runs on the worker thread and must be `Send`.  Returning an
    /// `Err` preserves the error string for the UI toast/log path.
    pub fn submit<R, F>(&self, f: F) -> StorageRequest<R>
    where
        R: Send + 'static,
        F: FnOnce(&Storage) -> Result<R, String> + Send + 'static,
    {
        let (result_sender, receiver) = tokio::sync::oneshot::channel();
        let storage = Arc::clone(&self.storage);
        let job: Job = Box::new(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&storage)))
                .unwrap_or_else(|_| Err("storage worker panicked while executing a write".into()));
            let _ = result_sender.send(result);
        });

        // `send` on an unbounded `std::sync::mpsc::Sender` is non-blocking.
        // If the worker has already gone away we still return a future; it
        // resolves immediately with a channel-disconnected error.
        match self.sender.lock() {
            Ok(sender) => {
                let _ = sender.send(job);
            }
            Err(_) => {
                drop(job);
            }
        }

        StorageRequest { receiver }
    }

    /// Convenience helper: submit a write that produces only `()` and convert
    /// the result into a toast task using the central helper.
    #[allow(dead_code, reason = "convenience wrapper around `submit`")]
    pub fn submit_unit<F>(&self, f: F) -> StorageRequest<()>
    where
        F: FnOnce(&Storage) -> Result<(), String> + Send + 'static,
    {
        self.submit(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_runs_write_on_worker_and_returns_result() {
        let storage = Arc::new(Storage::open_in_memory().unwrap());
        let service = StorageService::start(Arc::clone(&storage));

        let request = service.submit(|storage| {
            storage.save_preference("theme_mode", "dark")?;
            storage.load_preference("theme_mode")
        });

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded = runtime.block_on(request).unwrap();
        assert_eq!(loaded.as_deref(), Some("dark"));
    }
}
