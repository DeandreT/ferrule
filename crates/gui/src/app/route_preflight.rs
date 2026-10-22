//! Route capability preflight for GUI actions. Run, Debug Run, pipeline runs
//! and previews query the shared host preparation before they save the project
//! or start a worker, so an unsupported combination is refused with nothing
//! saved, read or written. Library generation uses the generation plan the
//! same way (see `library_generation.rs`).

use std::path::Path;
use std::sync::atomic::AtomicBool;

use super::FerruleApp;
use super::run_ui::nonempty_path;

/// The observers every saved-file run attaches: its trace collector and the
/// runner's cancellation, which capability admission treats as debugging.
/// Debug Run's hook adds no further context, so both declare these.
pub(super) const SAVED_RUN_OBSERVERS: engine::SourceOwnershipObservers =
    engine::SourceOwnershipObservers {
        tracing: true,
        debugging: true,
    };

impl FerruleApp {
    /// Prepares the filesystem run that Run or Debug Run would start for the
    /// in-memory project, without saving it or acquiring any input. An unsaved
    /// project is prepared at its suggested path; preparation reads nothing
    /// there, and the saved run prepares again at its chosen location.
    pub(super) fn saved_run_preflight(
        &self,
        runtime_parameters: &engine::RuntimeParameters,
    ) -> anyhow::Result<()> {
        let project_path = self
            .document
            .saved_path()
            .unwrap_or_else(|| self.document.suggested_path());
        let input_path = nonempty_path(&self.input_path);
        let output_path = nonempty_path(&self.output_path);
        let request = cli::RunRequest {
            input_path: input_path.as_deref(),
            output_path: output_path.as_deref(),
            runtime_parameters: Some(runtime_parameters),
            observers: SAVED_RUN_OBSERVERS,
            ..cli::RunRequest::default()
        };
        cli::plan_file_run(&self.project, project_path, request).map(drop)
    }
}

/// Stands in for the trace collector every GUI pipeline run attaches. With
/// the worker's cancellation, plain and debug runs declare the same observers.
fn trace_placeholder(_: &str, _: cli::TraceEvent) {}

/// Prepares a pipeline run without reading any host input or creating any
/// output.
pub(super) fn pipeline_run(
    path: &Path,
    inputs: &[cli::PipelineHostFile],
    outputs: &[cli::PipelineOutputFile],
    runtime_parameters: &engine::RuntimeParameters,
    cancellation: &AtomicBool,
) -> anyhow::Result<()> {
    let mut options = cli::PipelineRunOptions::default()
        .with_stage_trace_sink(&trace_placeholder)
        .with_cancellation(cancellation);
    options.runtime_parameters = Some(runtime_parameters);
    cli::plan_pipeline_run(path, inputs, outputs, &options).map(drop)
}

/// Validates a pipeline preview against empty host payloads, before any host
/// file is read.
pub(super) fn pipeline_preview(
    pipeline: &mapping::Pipeline,
    path: &Path,
    inputs: &[cli::PipelineHostFile],
    identities: &[cli::PipelinePreviewOutputIdentity],
    runtime_parameters: &engine::RuntimeParameters,
    cancellation: &AtomicBool,
) -> anyhow::Result<()> {
    let empty_hosts = inputs
        .iter()
        .map(|input| {
            let document = cli::PayloadDocument::new(&input.path, &[])?;
            cli::PipelineHostPayload::new(&input.name, document)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let options = cli::PipelinePreviewOptions::new(&empty_hosts)
        .with_output_identities(identities)
        .with_runtime_parameters(runtime_parameters)
        .with_stage_trace_sink(&trace_placeholder)
        .with_cancellation(cancellation);
    cli::validate_pipeline_preview(pipeline, path, &options)
}

#[cfg(test)]
#[path = "route_preflight_tests.rs"]
mod route_preflight_tests;
