//! Explicit, session-only JSON requests. Request/header files are host inputs,
//! never mapping metadata, editor history, preferences, or diagnostic text.
//!
//! The REST JSON route plan is queried before the request form opens and
//! again for the exact snapshot a started request owns, so a design the route
//! refuses never reads a request file or opens a connection. The worker is
//! owned by its exact thread handle (see `owned_worker.rs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use super::owned_worker::{OwnedWorker, WorkerExit};
use super::*;

const PREVIEW_BYTES: usize = 64 * 1024;
const RESPONSE_IDENTITY: &str = "response.json";
const OUTPUT_IDENTITY: &str = "result.json";

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

/// The mapping options every GUI request uses. Preflight and the worker share
/// them, so the plan admits exactly the observers the worker attaches: none.
/// Cancellation is the worker's separate continuation gate.
fn mapping_options(
    runtime_parameters: Option<&engine::RuntimeParameters>,
) -> cli::RestJsonMappingOptions<'_> {
    let mut options =
        cli::RestJsonMappingOptions::new(Path::new(RESPONSE_IDENTITY), Path::new(OUTPUT_IDENTITY));
    options.runtime_parameters = runtime_parameters;
    options
}

/// Prepares the REST JSON route for `project` without reading any host
/// request file or opening a connection.
pub(super) fn rest_run_preflight(
    project: &Project,
    project_path: &Path,
) -> Result<cli::RoutePlan, cli::RestJsonError> {
    cli::plan_rest_json_run(project, project_path, &mapping_options(None))
}

/// A preflight refusal. No request or response data has been read, so the
/// design's own policy or mapping reason is shown rather than only a category.
fn refusal_message(error: &cli::RestJsonError) -> String {
    match error {
        cli::RestJsonError::RequestPolicy(reason) => format!("This design cannot run: {reason}."),
        cli::RestJsonError::Mapping(error) => format!("{error:#}"),
        other => format!("Request refused during {}.", other.phase()),
    }
}

/// One approved request, fixed when it starts: the project snapshot and
/// mapping identity its plan admitted, the host files, the one-request grant
/// and the run values. The worker owns it, so later editor state cannot
/// change what executes.
struct RestRunRequest {
    project: Project,
    project_path: PathBuf,
    request_path: PathBuf,
    header_path: Option<PathBuf>,
    policy: cli::RestExecutionPolicy,
    parameters: engine::RuntimeParameters,
}

/// What the worker reports: the in-memory outcome or a redacted category.
type RestRunCompletion = Result<cli::PayloadRunOutcome, &'static str>;

impl RestRunRequest {
    /// Admits the request only when the route plan accepts this exact snapshot.
    fn prepare(
        project: Project,
        project_path: PathBuf,
        request_path: PathBuf,
        header_path: Option<PathBuf>,
        policy: cli::RestExecutionPolicy,
        parameters: engine::RuntimeParameters,
    ) -> Result<Self, cli::RestJsonError> {
        rest_run_preflight(&project, &project_path)?;
        Ok(Self {
            project,
            project_path,
            request_path,
            header_path,
            policy,
            parameters,
        })
    }

    fn execute(&self, cancelled: &AtomicBool) -> RestRunCompletion {
        let gate = || !cancelled.load(Ordering::Acquire);
        let mut options = mapping_options(Some(&self.parameters));
        options.should_continue = Some(&gate);
        cli::run_project_value_rest_json_request_file_payloads(
            &self.project,
            &self.project_path,
            &self.request_path,
            self.header_path.as_deref(),
            self.policy,
            &options,
        )
        // Keep native causes in the public API. The GUI deliberately reports
        // only the category, without logging a cause that could quote data.
        .map_err(|error| error.phase())
    }
}

/// A live request owned by its exact worker thread; its outcome is reported
/// only after that thread was joined.
pub(super) struct PendingRestRun {
    worker: OwnedWorker<RestRunCompletion>,
    cancelled: Arc<AtomicBool>,
}

impl PendingRestRun {
    /// Every owner of the cancellation flag, including the worker's own.
    #[cfg(test)]
    pub(super) fn cancellation_for_test(&self) -> std::sync::Weak<AtomicBool> {
        Arc::downgrade(&self.cancelled)
    }
}

impl Drop for PendingRestRun {
    /// Cancels, then drains the exact worker: the `worker` field is dropped
    /// after this runs. An in-flight request cannot be interrupted, so the
    /// drain waits for it within its configured timeout.
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
    /// Explains unavailable request routes without reading files or requesting
    /// network approval.
    pub(super) fn rest_run_unavailable_reason(&self) -> Option<String> {
        if self.primary_document.is_some() {
            return Some("Document projects do not run JSON requests.".into());
        }
        let project_path = self
            .document
            .saved_path()
            .unwrap_or_else(|| self.document.suggested_path());
        rest_run_preflight(&self.project, project_path)
            .err()
            .map(|error| refusal_message(&error))
            .or_else(|| {
                self.document
                    .saved_path()
                    .is_none()
                    .then(|| "Save the mapping before running a JSON request.".into())
            })
    }

    pub(super) fn rest_run_busy(&self) -> bool {
        // Existing external-action entry points share this ownership guard.
        self.rest_run_draft.is_some()
            || self.pending_rest_run.is_some()
            || self.stage_document_actions_blocked()
    }

    pub(super) fn project_editing_enabled(&self) -> bool {
        self.library_generation_draft.is_none()
            && self.pending_library_generation.is_none()
            && self.pending_dialog.is_none()
            && self.pending_destructive_action.is_none()
            && self.pending_pipeline_editor_action.is_none()
            && self.mfd_import_dialog.is_none()
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
            && self.rest_run_draft.is_none()
            && self.pending_rest_run.is_none()
            && self.pipeline_stage_canvas.is_none()
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
        if self.stage_document_actions_blocked()
            || !self.project_editing_enabled()
            || self.pipeline_editor.is_some()
            || self.pipeline_run_draft.is_some()
            || self.pending_save_continuation.is_some()
        {
            return;
        }
        let issues = cli::validate(&self.project);
        if !issues.is_empty() {
            self.status = format!(
                "request blocked by {}",
                crate::ui_kit::plural(issues.len(), "validation issue")
            );
            self.diagnostics.validation(&self.project, issues);
            return;
        }
        // Refuse an unsupported route before the form asks for host files or
        // a grant. An unsaved design is prepared at its suggested path, which
        // preparation does not read.
        let project_path = self
            .document
            .saved_path()
            .unwrap_or_else(|| self.document.suggested_path());
        if let Err(error) = rest_run_preflight(&self.project, project_path) {
            self.block_rest_run(&error);
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

    fn block_rest_run(&mut self, error: &cli::RestJsonError) {
        self.status = "request blocked".into();
        self.diagnostics
            .error("Request blocked", refusal_message(error));
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
        let policy = cli::RestExecutionPolicy::AllowSingleRequest {
            allow_insecure_http: draft.insecure_http,
        };
        // The snapshot is cloned once: the worker owns the copy its plan
        // admitted, independent of later editor state.
        let request = match RestRunRequest::prepare(
            self.project.clone(),
            project_path,
            request_path,
            draft.headers.take(),
            policy,
            parameters,
        ) {
            Ok(request) => request,
            Err(error) => {
                self.block_rest_run(&error);
                return;
            }
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = match OwnedWorker::spawn("rest-json-request", move || {
            request.execute(&worker_cancelled)
        }) {
            Ok(worker) => worker,
            Err(_) => {
                self.status = "live request failed".into();
                self.diagnostics
                    .error("Live request failed", "Request worker could not start.");
                return;
            }
        };
        self.pending_rest_run = Some(PendingRestRun { worker, cancelled });
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
        // A received result is not completion: the outcome arrives only
        // after the exact worker thread was joined.
        let Some(exit) = pending.worker.poll() else {
            context.request_repaint_after(Duration::from_millis(100));
            return;
        };
        let stopped = pending.cancelled.load(Ordering::Acquire);
        self.pending_rest_run = None;
        self.rest_run_result = None;
        match exit {
            _ if stopped => {
                self.status = "live request cancelled".into();
                self.diagnostics.clear();
            }
            WorkerExit::Reported(Ok(outcome)) => {
                self.rest_run_result = Some(RestRunResult::new(outcome));
                self.status = "live request complete (in memory)".into();
                self.diagnostics.clear();
            }
            WorkerExit::Reported(Err(category)) => {
                self.status = "live request failed".into();
                self.diagnostics.error(
                    "Live request failed",
                    format!("Request failed during {category}."),
                );
            }
            WorkerExit::Unreported | WorkerExit::Panicked => {
                self.status = "live request failed".into();
                self.diagnostics.error(
                    "Live request failed",
                    "Request worker stopped unexpectedly.",
                );
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
                .id(egui::Id::unique("rest_request_setup"))
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
                .id(egui::Id::unique("rest_request_progress"))
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
            egui::Window::new("Live request result (in memory)").id(egui::Id::unique("rest_request_result")).open(&mut open).show(context, |ui| {
                ui.label(format!("{}; no configured output file was written.", crate::ui_kit::plural(result.outcome.records_written, "record")));
                if let Some(artifact) = result.outcome.artifacts.first() {
                    ui.label(format!("{}: {} bytes", artifact.path.display(), artifact.bytes.len()));
                }
                if result.truncated { ui.weak("Preview is limited to 64 KiB. Complete output is retained for this session."); }
                egui::ScrollArea::both().max_height(crate::theme::ListHeight::Tall.px()).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut result.preview).interactive(false).desired_width(f32::INFINITY));
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
