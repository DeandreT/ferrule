//! The preview service retains previews whose thread may own a PDF worker.
//!
//! A pending owned preview stays in the app while the UI follows it. When its
//! UI owner is dropped (the preview window is replaced, the app is torn down,
//! or a receiver is lost) it requests cancellation and hands its exact thread
//! handle here instead of blocking the UI thread. Retained threads are joined
//! only once they have finished; a channel disconnect, a drop or elapsed time
//! never counts as completion. The desktop runner's owner joins whatever is
//! still retained after the window has closed, and an app close waits while
//! anything is retained.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use super::owned_worker::OwnedWorker;

/// A detached owner of one exact preview thread.
pub(super) trait RetainedWorker: Send {
    fn is_finished(&self) -> bool;
    /// Blocks until the exact thread has exited.
    fn join(self: Box<Self>);
}

impl<T: Send + 'static> RetainedWorker for OwnedWorker<T> {
    fn is_finished(&self) -> bool {
        OwnedWorker::is_finished(self)
    }

    fn join(self: Box<Self>) {
        // An unfinished owner's drop joins its exact thread.
        drop(self);
    }
}

#[derive(Default)]
struct Retained(Mutex<Vec<Box<dyn RetainedWorker>>>);

impl Retained {
    fn lock(&self) -> MutexGuard<'_, Vec<Box<dyn RetainedWorker>>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The app's handle on the shared preview service.
#[derive(Clone, Default)]
pub(crate) struct PreviewServicePort(Arc<Retained>);

impl PreviewServicePort {
    pub(super) fn retain(&self, worker: Box<dyn RetainedWorker>) {
        self.0.lock().push(worker);
    }

    /// Joins retained threads that have finished and returns how many are
    /// still running.
    pub(crate) fn collect_finished(&self) -> usize {
        let finished = {
            let mut retained = self.0.lock();
            let (finished, running): (Vec<_>, Vec<_>) =
                retained.drain(..).partition(|worker| worker.is_finished());
            *retained = running;
            finished
        };
        // Joining a finished thread does not wait; it happens outside the lock.
        for worker in finished {
            worker.join();
        }
        self.pending()
    }

    pub(crate) fn pending(&self) -> usize {
        self.0.lock().len()
    }
}

/// The desktop runner's owner of the preview service.
pub(crate) struct PreviewService {
    port: PreviewServicePort,
}

impl PreviewService {
    pub(crate) fn new() -> Self {
        Self {
            port: PreviewServicePort::default(),
        }
    }

    pub(crate) fn port(&self) -> PreviewServicePort {
        self.port.clone()
    }

    /// After the runner returns, waits for every retained preview thread.
    /// Each one was cancelled when it was retained and returns only after
    /// its extraction has closed.
    pub(crate) fn finish_after_runner(self) -> usize {
        let mut joined = 0;
        loop {
            let before = self.port.pending();
            let pending = self.port.collect_finished();
            joined += before.saturating_sub(pending);
            if pending == 0 {
                return joined;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Where a single-mapping preview reads its primary input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PreviewInputSource {
    /// Bounded editor text with a logical identity; no input file is opened.
    #[default]
    EditorText,
    /// The project's configured files, acquired as a run acquires them.
    ConfiguredFiles,
}

/// How a preview worker acquires its inputs.
pub(super) struct PreviewAcquisition {
    pub(super) source: PreviewInputSource,
    /// Present only for opted-in file previews; the reader is built on the
    /// preview thread.
    pub(super) pdf_worker: Option<(
        super::pdf_extraction::PdfWorkerPackage,
        Arc<super::pdf_extraction::ExtractionStatus>,
    )>,
    #[cfg(test)]
    pub(super) controls: super::pdf_extraction::ReaderControls,
}

impl PreviewAcquisition {
    /// The owned reader for this preview, constructed on the calling thread.
    pub(super) fn reader<'a>(
        &'a self,
        cancelled: &'a std::sync::atomic::AtomicBool,
    ) -> Option<super::pdf_extraction::OwnedPdfReader<'a>> {
        self.pdf_worker.as_ref().map(|(package, status)| {
            let reader = super::pdf_extraction::OwnedPdfReader::new(package, cancelled, status);
            #[cfg(test)]
            let reader = reader.with_controls(self.controls);
            reader
        })
    }
}

impl super::FerruleApp {
    /// Joins finished service-owned threads, and completes a close deferred
    /// for owned previews once nothing is retained.
    pub(super) fn poll_preview_service(&mut self, ctx: &egui::Context) {
        let retained = self.preview_service.collect_finished();
        let owned_pending = self
            .pending_preview
            .as_ref()
            .is_some_and(super::preview_ui::PendingPreview::is_service_owned);
        if retained > 0 || owned_pending {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if std::mem::take(&mut self.close_after_preview) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
