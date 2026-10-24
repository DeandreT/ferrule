//! Previews of saved primary filename document projects: the CLI document run
//! on the preview's own worker, with nothing published.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Instant;

use super::super::FerruleApp;
use super::super::owned_worker::OwnedWorker;
use super::{PendingPreview, PreviewCompletion, PreviewPhase, PreviewRunError};

impl FerruleApp {
    /// Previews a document project through the CLI document run on its own
    /// worker: the same input root and limits as Run, with nothing published.
    pub(super) fn start_primary_document_preview(
        &mut self,
        debug: bool,
        runtime_parameters: engine::RuntimeParameters,
    ) {
        let Some(session) = &self.primary_document else {
            return;
        };
        let blocked = |app: &mut Self, message: String| {
            app.status = "preview blocked".into();
            app.diagnostics.error("Preview blocked", message);
        };
        if debug {
            blocked(
                self,
                "Debug preview is not available for primary document projects".into(),
            );
            return;
        }
        let policy = match session
            .policy()
            .and_then(|policy| self.primary_document_preflight().map(|()| policy))
        {
            Ok(policy) => policy,
            Err(error) => {
                blocked(self, format!("{error:#}"));
                return;
            }
        };
        let project_path = match self.preview_project_path() {
            Ok(path) => path,
            Err(error) => {
                blocked(self, format!("{error:#}"));
                return;
            }
        };
        let document = session.document(&self.project);
        let target_label = self.preview_draft.as_ref().map_or_else(
            || "Primary target".to_owned(),
            |draft| draft.target.label().to_owned(),
        );
        let output_identity = PathBuf::from(
            self.project
                .target_path
                .clone()
                .unwrap_or_else(|| "output.csv".into()),
        );
        let input_path = policy.input_root.clone();
        let (_events, receiver) = mpsc::channel();
        let (commands, _command_receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker = OwnedWorker::spawn("ferrule-preview", move || -> PreviewCompletion {
            let result = cli::execute_primary_document(
                &document,
                &project_path,
                &policy,
                Some(&runtime_parameters),
            )
            .map(|output| cli::PayloadRunOutcome {
                records_written: output.records,
                artifacts: vec![cli::PayloadArtifact {
                    target: document.project().target.name.clone(),
                    records_written: output.records,
                    path: output_identity,
                    bytes: output.bytes,
                }],
            })
            .map_err(|error| PreviewRunError::Failed(format!("{:#}", anyhow::Error::new(error))));
            (result, crate::run_report::TraceReport::default())
        });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                self.status = "preview failed".to_string();
                self.diagnostics.error(
                    "Preview failed",
                    format!("Could not start the preview worker: {error}"),
                );
                return;
            }
        };
        self.pending_preview = Some(PendingPreview {
            worker,
            receiver,
            commands,
            cancelled,
            started: Instant::now(),
            input_path,
            target_label,
            debug: false,
            phase: PreviewPhase::Running,
            extraction: None,
            service: None,
        });
        self.show_run_report = false;
        self.diagnostics.clear();
        self.status = "preview running".into();
    }
}
