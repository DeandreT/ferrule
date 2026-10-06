//! Explicit, session-only JSON requests. Request/header files are host inputs,
//! never mapping metadata, editor history, preferences, or diagnostic text.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use super::*;

const PREVIEW_BYTES: usize = 64 * 1024;

enum RestPicker {
    Request,
    Headers,
}

#[derive(Default)]
pub(super) struct RestRunDraft {
    request: Option<PathBuf>,
    headers: Option<PathBuf>,
    grant: bool,
    insecure_http: bool,
    picker: Option<(RestPicker, Receiver<Option<String>>)>,
}

impl RestRunDraft {
    fn ready(&self) -> bool {
        self.grant && self.request.is_some() && self.picker.is_none()
    }

    fn poll_picker(&mut self) {
        let Some((_, receiver)) = &self.picker else {
            return;
        };
        let selected = match receiver.try_recv() {
            Ok(selected) => selected,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => None,
        };
        if let Some((kind, _)) = self.picker.take() {
            if let Some(path) = selected {
                match kind {
                    RestPicker::Request => self.request = Some(PathBuf::from(path)),
                    RestPicker::Headers => self.headers = Some(PathBuf::from(path)),
                }
            }
            // Selecting a different host file never inherits an earlier grant.
            self.grant = false;
        }
    }
}

struct RestControls {
    request: egui::Response,
    headers: egui::Response,
    clear_headers: egui::Response,
    grant: egui::Response,
    http: egui::Response,
    run: egui::Response,
    cancel: egui::Response,
}

fn rest_controls(ui: &mut egui::Ui, draft: &mut RestRunDraft) -> RestControls {
    ui.label("Choose a host-owned JSON request file (GET or POST).");
    ui.weak("Request and header values stay separate from the mapping project.");
    let picking = draft.picker.is_some();
    let request = ui.add_enabled(!picking, egui::Button::new("Choose request file..."));
    ui.label(if draft.request.is_some() {
        "Request file selected"
    } else {
        "No request file"
    });
    let headers = ui.add_enabled(!picking, egui::Button::new("Choose header-values file..."));
    let clear_headers = ui.add_enabled(
        !picking && draft.headers.is_some(),
        egui::Button::new("Clear headers"),
    );
    ui.label(if draft.headers.is_some() {
        "Header-values file selected"
    } else {
        "No host headers"
    });
    let http = ui.checkbox(
        &mut draft.insecure_http,
        "Allow cleartext HTTP for this request",
    );
    let grant = ui.checkbox(&mut draft.grant, "Allow one live request now");
    ui.weak("The request may have remote effects. Cancel cannot undo them or interrupt an in-flight request; its configured timeout still applies.");
    ui.label("Logical JSON input: response.json; output: result.json");
    let run = ui.add_enabled(draft.ready(), egui::Button::new("Run request"));
    let cancel = ui.button("Cancel");
    RestControls {
        request,
        headers,
        clear_headers,
        grant,
        http,
        run,
        cancel,
    }
}

pub(super) struct PendingRestRun {
    receiver: Receiver<Result<cli::PayloadRunOutcome, &'static str>>,
    worker: Option<JoinHandle<()>>,
    cancelled: Arc<AtomicBool>,
    completed: Option<Result<cli::PayloadRunOutcome, &'static str>>,
}

impl Drop for PendingRestRun {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

pub(super) struct RestRunResult {
    outcome: cli::PayloadRunOutcome,
    preview: String,
    truncated: bool,
}

impl RestRunResult {
    fn new(outcome: cli::PayloadRunOutcome) -> Self {
        let bytes = outcome
            .artifacts
            .first()
            .map(|artifact| artifact.bytes.as_slice())
            .unwrap_or_default();
        let mut end = bytes.len().min(PREVIEW_BYTES);
        // A bounded preview must not split an existing UTF-8 scalar. The
        // complete original buffer remains owned in outcome, without rerendering.
        while end < bytes.len() && end > 0 && bytes[end] & 0xc0 == 0x80 {
            end -= 1;
        }
        let preview = String::from_utf8_lossy(&bytes[..end]).into_owned();
        let truncated = end < bytes.len();
        Self {
            outcome,
            preview,
            truncated,
        }
    }
}

impl FerruleApp {
    pub(super) fn rest_run_busy(&self) -> bool {
        self.rest_run_draft.is_some() || self.pending_rest_run.is_some()
    }

    pub(super) fn project_editing_enabled(&self) -> bool {
        self.library_generation_draft.is_none()
            && self.pending_library_generation.is_none()
            && self.pending_dialog.is_none()
            && self.pending_destructive_action.is_none()
            && self.new_mapping_setup.is_none()
            && self.extra_source_draft.is_none()
            && self.pending_extra_source_removal.is_none()
            && self.extra_target_draft.is_none()
            && self.pending_extra_target_removal.is_none()
            && self.pending_auto_connect.is_none()
            && self.preview_draft.is_none()
            && self.pending_preview.is_none()
            && self.pending_file_run.is_none()
            && self.pending_pipeline_run.is_none()
            && !self.rest_run_busy()
    }

    pub(super) fn clear_rest_run_session(&mut self) {
        // A running worker is retained until it finishes; a document reset
        // cannot silently discard its ownership or authorize another request.
        if self.pending_rest_run.is_some() {
            return;
        }
        self.rest_run_draft = None;
        self.rest_run_result = None;
        self.close_after_rest_run = false;
    }

    pub(super) fn begin_rest_run(&mut self) {
        if !self.project_editing_enabled()
            || self.pipeline_editor.is_some()
            || self.pipeline_run_draft.is_some()
            || self.pending_save_continuation.is_some()
        {
            return;
        }
        if self.document.saved_path().is_none() {
            self.status = "request blocked".into();
            self.diagnostics.error(
                "Request blocked",
                "Save the mapping once to establish its active mapping identity.",
            );
            return;
        }
        self.rest_run_result = None;
        self.rest_run_draft = Some(RestRunDraft::default());
    }

    fn start_rest_run(&mut self) {
        if self.pending_rest_run.is_some() {
            return;
        }
        let Some(draft) = self.rest_run_draft.as_ref() else {
            return;
        };
        if !draft.ready() {
            return;
        }
        let Some(project_path) = self.document.saved_path().map(PathBuf::from) else {
            return;
        };
        let parameters = match self.host_parameters.compile() {
            Ok(parameters) => parameters,
            Err(_) => {
                self.status = "request blocked".into();
                self.diagnostics
                    .error("Request blocked", "Run values are invalid.");
                return;
            }
        };
        let Some(mut draft) = self.rest_run_draft.take() else {
            return;
        };
        // Consume approval before any worker/file effect. No paths or parsed
        // credentials are carried into status, history, or the result window.
        draft.grant = false;
        let Some(request_path) = draft.request.take() else {
            return;
        };
        let header_path = draft.headers.take();
        let policy = cli::RestExecutionPolicy::AllowSingleRequest {
            allow_insecure_http: draft.insecure_http,
        };
        let project = self.project.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let gate = || !worker_cancelled.load(Ordering::Acquire);
            let mut options = cli::RestJsonMappingOptions::new(
                Path::new("response.json"),
                Path::new("result.json"),
            );
            options.runtime_parameters = Some(&parameters);
            options.should_continue = Some(&gate);
            let result = cli::run_project_value_rest_json_request_file_payloads(
                &project,
                &project_path,
                &request_path,
                header_path.as_deref(),
                policy,
                &options,
            )
            .map_err(|error| error.phase());
            // Keep native causes in the public API. The GUI deliberately sends
            // only the category, without logging a cause that could quote data.
            let _ = sender.send(result);
        });
        self.pending_rest_run = Some(PendingRestRun {
            receiver,
            worker: Some(worker),
            cancelled,
            completed: None,
        });
        self.status = "live request running".into();
        self.diagnostics.clear();
    }

    pub(super) fn cancel_rest_run(&mut self) {
        if let Some(pending) = &self.pending_rest_run {
            pending.cancelled.store(true, Ordering::Release);
            self.status = "stopping live request".into();
        } else {
            self.rest_run_draft = None;
        }
    }

    pub(super) fn guard_rest_run_close(&mut self, context: &egui::Context) -> bool {
        if self.pending_rest_run.is_some() {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_after_rest_run = true;
            self.cancel_rest_run();
            return true;
        }
        // Closing a form consumes no grant and creates no network effect.
        self.rest_run_draft = None;
        false
    }

    pub(super) fn poll_rest_run(&mut self, context: &egui::Context) {
        let Some(pending) = &mut self.pending_rest_run else {
            return;
        };
        if pending.completed.is_none() {
            match pending.receiver.try_recv() {
                Ok(result) => pending.completed = Some(result),
                Err(TryRecvError::Disconnected) => pending.completed = Some(Err("worker")),
                Err(TryRecvError::Empty) => {}
            }
        }
        if pending.completed.is_none()
            || !pending.worker.as_ref().is_some_and(JoinHandle::is_finished)
        {
            context.request_repaint_after(Duration::from_millis(100));
            return;
        }
        let Some(mut pending) = self.pending_rest_run.take() else {
            return;
        };
        let stopped = pending.cancelled.load(Ordering::Acquire);
        let result = pending.completed.take().unwrap_or(Err("worker"));
        // Join only after the exact owned handle reports completion. The
        // optional handle avoids moving a field out of this Drop type.
        let joined = pending
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_ok());
        self.rest_run_result = None;
        if stopped {
            self.status = "live request cancelled".into();
            self.diagnostics.clear();
        } else if !joined {
            self.status = "live request failed".into();
            self.diagnostics.error(
                "Live request failed",
                "Request worker stopped unexpectedly.",
            );
        } else {
            match result {
                Ok(outcome) => {
                    self.rest_run_result = Some(RestRunResult::new(outcome));
                    self.status = "live request complete (in memory)".into();
                    self.diagnostics.clear();
                }
                Err(category) => {
                    self.status = "live request failed".into();
                    self.diagnostics.error(
                        "Live request failed",
                        format!("Request failed during {category}."),
                    );
                }
            }
        }
        if std::mem::take(&mut self.close_after_rest_run) {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub(super) fn show_rest_run(&mut self, context: &egui::Context) {
        if let Some(mut draft) = self.rest_run_draft.take() {
            draft.poll_picker();
            let mut open = true;
            let mut launch = false;
            let mut cancel = false;
            egui::Window::new("Run JSON request")
                .id(egui::Id::new("rest_request_setup"))
                .open(&mut open)
                .collapsible(false)
                .show(context, |ui| {
                    let controls = rest_controls(ui, &mut draft);
                    if controls.request.clicked() {
                        draft.grant = false;
                        draft.picker =
                            Some((RestPicker::Request, pick_file("JSON request", &["json"])));
                    }
                    if controls.headers.clicked() {
                        draft.grant = false;
                        draft.picker =
                            Some((RestPicker::Headers, pick_file("Header values", &["json"])));
                    }
                    if controls.clear_headers.clicked() {
                        draft.headers = None;
                        draft.grant = false;
                    }
                    let _ = (controls.grant, controls.http);
                    launch = controls.run.clicked();
                    cancel = controls.cancel.clicked();
                });
            if open && !cancel {
                self.rest_run_draft = Some(draft);
            }
            if launch && open && !cancel {
                self.start_rest_run();
            }
            if self
                .rest_run_draft
                .as_ref()
                .is_some_and(|draft| draft.picker.is_some())
            {
                context.request_repaint_after(Duration::from_millis(100));
            }
        }
        if self.pending_rest_run.is_some() {
            egui::Window::new("Live request")
                .id(egui::Id::new("rest_request_progress"))
                .collapsible(false)
                .show(context, |ui| {
                    ui.label("Waiting for the request or mapping to finish.");
                    if ui.button("Cancel request").clicked() {
                        self.cancel_rest_run();
                    }
                });
        }
        if let Some(result) = &mut self.rest_run_result {
            let mut open = true;
            egui::Window::new("Live request result (in memory)").id(egui::Id::new("rest_request_result")).open(&mut open).show(context, |ui| {
                ui.label(format!("{} record(s); no configured output file was written.", result.outcome.records_written));
                if let Some(artifact) = result.outcome.artifacts.first() {
                    ui.label(format!("{}: {} bytes", artifact.path.display(), artifact.bytes.len()));
                }
                if result.truncated { ui.weak("Preview is limited to 64 KiB. Complete output is retained for this session."); }
                egui::ScrollArea::both().max_height(300.0).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut result.preview).interactive(false).desired_width(480.0));
                });
            });
            if !open {
                self.rest_run_result = None;
            }
        }
    }
}

#[cfg(test)]
#[path = "rest_run_tests.rs"]
mod tests;
