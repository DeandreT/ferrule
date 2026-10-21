//! Exact-handle ownership of one background worker thread.
//!
//! A result message or a disconnected channel is not proof that a worker
//! exited. [`OwnedWorker`] reports an outcome only after its own retained
//! `JoinHandle` reports that the thread finished and has been joined, and
//! dropping an unfinished owner blocks until that exact thread exits. These
//! workers have no cooperative cancellation, so the drain waits for their
//! bounded work rather than abandoning it mid-publication.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

/// How an owned worker ended, observed after its thread was joined.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum WorkerExit<T> {
    Reported(T),
    /// The thread exited without reporting a result.
    Unreported,
    Panicked,
}

pub(super) struct OwnedWorker<T> {
    receiver: Receiver<T>,
    handle: Option<JoinHandle<()>>,
    reported: Option<T>,
}

impl<T: Send + 'static> OwnedWorker<T> {
    /// Runs `work` on a named thread and reports its value.
    pub(super) fn spawn(
        name: &str,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::spawn_reporting(name, move |sender| {
            let _ = sender.send(work());
        })
    }

    /// A live worker that reports the value the test sends, or exits without
    /// reporting when the sender is dropped.
    #[cfg(test)]
    pub(super) fn relaying_for_test() -> (Self, Sender<T>) {
        let (input, relay) = mpsc::channel::<T>();
        let worker = Self::spawn_reporting("owned-worker-relay", move |sender| {
            if let Ok(value) = relay.recv() {
                let _ = sender.send(value);
            }
        })
        .expect("relay test worker starts");
        (worker, input)
    }

    /// Runs `body`, which reports at most one value and may keep running
    /// after it does.
    fn spawn_reporting(
        name: &str,
        body: impl FnOnce(Sender<T>) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || body(sender))?;
        Ok(Self {
            receiver,
            handle: Some(handle),
            reported: None,
        })
    }
}

impl<T> OwnedWorker<T> {
    /// Returns the outcome once, after the exact worker thread was joined.
    pub(super) fn poll(&mut self) -> Option<WorkerExit<T>> {
        // Observe exit before probing the queue: a value sent just before the
        // thread exits is then always received below.
        let finished = self.handle.as_ref()?.is_finished();
        if self.reported.is_none()
            && let Ok(value) = self.receiver.try_recv()
        {
            self.reported = Some(value);
        }
        if !finished {
            return None;
        }
        let joined = self.handle.take()?.join();
        Some(match (joined, self.reported.take()) {
            (Err(_), _) => WorkerExit::Panicked,
            (Ok(()), Some(value)) => WorkerExit::Reported(value),
            (Ok(()), None) => WorkerExit::Unreported,
        })
    }
}

impl<T> Drop for OwnedWorker<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // A panic was already reported by the thread; draining only
            // guarantees that it no longer runs.
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    /// A worker that reports first, then stays alive until released and
    /// records its own exit as its last action.
    fn reporting_then_held() -> (OwnedWorker<u32>, Sender<()>, Arc<AtomicBool>) {
        let (release, released) = mpsc::channel::<()>();
        let exited = Arc::new(AtomicBool::new(false));
        let worker_exited = Arc::clone(&exited);
        let worker = OwnedWorker::spawn_reporting("owned-worker-test", move |sender| {
            let _ = sender.send(7);
            let _ = released.recv_timeout(WAIT);
            worker_exited.store(true, Ordering::Release);
        })
        .expect("test worker starts");
        (worker, release, exited)
    }

    fn poll_until_exit<T>(worker: &mut OwnedWorker<T>) -> WorkerExit<T> {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(exit) = worker.poll() {
                return exit;
            }
            assert!(Instant::now() < deadline, "worker exits");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_received_result_is_not_completion_until_the_thread_is_joined() {
        let (mut worker, release, exited) = reporting_then_held();
        let deadline = Instant::now() + WAIT;
        while worker.reported.is_none() {
            assert_eq!(worker.poll(), None, "the held worker is still running");
            assert!(Instant::now() < deadline, "worker reports");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(worker.poll(), None);
        assert!(!exited.load(Ordering::Acquire));

        release.send(()).expect("worker waits for release");
        assert_eq!(poll_until_exit(&mut worker), WorkerExit::Reported(7));
        assert!(exited.load(Ordering::Acquire));
        assert!(worker.handle.is_none(), "the exact handle was joined");
        assert_eq!(worker.poll(), None, "an outcome is reported once");
    }

    #[test]
    fn dropping_an_unfinished_owner_drains_the_exact_thread() {
        let (worker, release, exited) = reporting_then_held();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let _ = release.send(());
        });
        drop(worker);
        assert!(
            exited.load(Ordering::Acquire),
            "drop returned only after the worker exited"
        );
        releaser.join().expect("releaser finishes");
    }

    #[test]
    fn silent_and_panicking_workers_are_distinguished_after_join() {
        let mut silent =
            OwnedWorker::<u32>::spawn_reporting("owned-worker-silent", drop).expect("starts");
        assert_eq!(poll_until_exit(&mut silent), WorkerExit::Unreported);

        let mut panicking =
            OwnedWorker::<u32>::spawn("owned-worker-panic", || panic!("owned worker test panic"))
                .expect("starts");
        assert_eq!(poll_until_exit(&mut panicking), WorkerExit::Panicked);
    }
}
