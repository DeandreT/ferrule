//! Bounded host-owned payload preview of a complete typed stage graph.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, bail};
use engine::{ExecutionContext, ExecutionPurpose};
use mapping::{FormatOptions, Pipeline};

use super::cancellation::{CancellableHook, check_cancelled};
use super::format_selection::{self, Direction, SelectedFormat};
use super::payload::{
    PayloadDestination, read_payload, render_payload, target_artifact_count, validate_logical_path,
    visit_target_documents,
};
use super::pipeline::{
    HostBoundary, PipelineHostFile, PipelineRunOptions, PipelineSourceFieldProbeCallback,
    PipelineStageDebugCallback, PipelineStageFunctionFailureDebugCallback,
    PipelineStageFunctionInputDebugCallback, PipelineStageFunctionNodeDebugCallback,
    PipelineStageInputDebugCallback, PipelineStageNodeDebugCallback,
    PipelineStageNodeFailureDebugCallback, PipelineStageTraceCallback, StageDebugHook,
    StageTraceSink, infer_host_boundaries,
};
use super::{
    FilesystemPdfReader, MAX_PAYLOAD_ARTIFACTS, MAX_PAYLOAD_DOCUMENT_BYTES, MAX_PAYLOAD_NAME_BYTES,
    MAX_PAYLOAD_RUN_BYTES, PayloadDocument, extension_of, http_url, read_instance_with_pdf_reader,
    resolve_run_path,
};

/// One host input supplied entirely as bounded bytes and a logical identity.
#[derive(Debug, Clone, Copy)]
pub struct PipelineHostPayload<'a> {
    name: &'a str,
    document: PayloadDocument<'a>,
}

impl<'a> PipelineHostPayload<'a> {
    pub fn new(name: &'a str, document: PayloadDocument<'a>) -> anyhow::Result<Self> {
        validate_name(name, "host input")?;
        Ok(Self { name, document })
    }

    pub fn name(self) -> &'a str {
        self.name
    }

    pub fn document(self) -> PayloadDocument<'a> {
        self.document
    }
}

/// An explicit logical output path; None selects the primary stage target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelinePreviewOutputIdentity {
    pub stage: String,
    pub target: Option<String>,
    pub path: PathBuf,
}

/// Where preview host inputs come from. The choice is explicit: payloads are
/// never reopened from their logical paths, and only file inputs can reach a
/// caller-owned PDF reader.
#[derive(Clone, Copy)]
enum PreviewHosts<'a> {
    Payloads(&'a [PipelineHostPayload<'a>]),
    Files {
        inputs: &'a [PipelineHostFile],
        pdf_reader: Option<&'a dyn FilesystemPdfReader>,
    },
}

impl<'a> PreviewHosts<'a> {
    fn len(self) -> usize {
        match self {
            Self::Payloads(hosts) => hosts.len(),
            Self::Files { inputs, .. } => inputs.len(),
        }
    }

    fn pdf_reader(self) -> Option<&'a dyn FilesystemPdfReader> {
        match self {
            Self::Payloads(_) => None,
            Self::Files { pdf_reader, .. } => pdf_reader,
        }
    }
}

/// Input payloads or files and observers for preview. Stored previews are
/// always enabled.
pub struct PipelinePreviewOptions<'a> {
    hosts: PreviewHosts<'a>,
    identities: &'a [PipelinePreviewOutputIdentity],
    callbacks: PipelineRunOptions<'a>,
    cancellation: Option<&'a AtomicBool>,
}

impl<'a> PipelinePreviewOptions<'a> {
    /// Host inputs supplied entirely as bounded caller-owned bytes. No file
    /// is opened and no PDF worker is involved.
    pub fn new(hosts: &'a [PipelineHostPayload<'a>]) -> Self {
        Self::with_hosts(PreviewHosts::Payloads(hosts))
    }

    /// Host inputs read once each from local files, with the same format
    /// dispatch as a pipeline file run. PDF hosts are parsed in-process.
    pub fn from_files(inputs: &'a [PipelineHostFile]) -> Self {
        Self::with_hosts(PreviewHosts::Files {
            inputs,
            pdf_reader: None,
        })
    }

    /// Like [`Self::from_files`], acquiring configured PDF hosts through a
    /// caller-owned reader instead of the in-process parser.
    pub fn from_files_with_pdf_reader(
        inputs: &'a [PipelineHostFile],
        reader: &'a dyn FilesystemPdfReader,
    ) -> Self {
        Self::with_hosts(PreviewHosts::Files {
            inputs,
            pdf_reader: Some(reader),
        })
    }

    fn with_hosts(hosts: PreviewHosts<'a>) -> Self {
        Self {
            hosts,
            identities: &[],
            callbacks: PipelineRunOptions::default(),
            cancellation: None,
        }
    }

    pub fn with_output_identities(
        mut self,
        identities: &'a [PipelinePreviewOutputIdentity],
    ) -> Self {
        self.identities = identities;
        self
    }

    /// Applies a restricted policy's budgets to every previewed stage. Preview
    /// payloads are host-owned bytes, so no location is acquired.
    pub fn with_execution_policy(mut self, policy: &'a engine::ExecutionPolicy) -> Self {
        self.callbacks.execution_policy = Some(policy);
        self
    }

    pub fn with_runtime_parameters(mut self, parameters: &'a engine::RuntimeParameters) -> Self {
        self.callbacks.runtime_parameters = Some(parameters);
        self
    }

    /// Cancels at loading/rendering boundaries and interpreter debug control points.
    pub fn with_cancellation(mut self, cancellation: &'a AtomicBool) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    pub fn with_stage_debug_hook(mut self, hook: &'a PipelineStageDebugCallback<'a>) -> Self {
        self.callbacks.stage_debug_hook = Some(hook);
        self
    }
    pub fn with_stage_node_debug_hook(
        mut self,
        hook: &'a PipelineStageNodeDebugCallback<'a>,
    ) -> Self {
        self.callbacks.stage_node_debug_hook = Some(hook);
        self
    }
    pub fn with_stage_function_node_debug_hook(
        mut self,
        hook: &'a PipelineStageFunctionNodeDebugCallback<'a>,
    ) -> Self {
        self.callbacks.stage_function_node_debug_hook = Some(hook);
        self
    }
    pub fn with_stage_input_debug_hook(
        mut self,
        hook: &'a PipelineStageInputDebugCallback<'a>,
    ) -> Self {
        self.callbacks.stage_input_debug_hook = Some(hook);
        self
    }
    pub fn with_stage_function_input_debug_hook(
        mut self,
        hook: &'a PipelineStageFunctionInputDebugCallback<'a>,
    ) -> Self {
        self.callbacks.stage_function_input_debug_hook = Some(hook);
        self
    }
    pub fn with_stage_node_failure_debug_hook(
        mut self,
        hook: &'a PipelineStageNodeFailureDebugCallback<'a>,
    ) -> Self {
        self.callbacks.stage_node_failure_debug_hook = Some(hook);
        self
    }
    pub fn with_stage_function_failure_debug_hook(
        mut self,
        hook: &'a PipelineStageFunctionFailureDebugCallback<'a>,
    ) -> Self {
        self.callbacks.stage_function_failure_debug_hook = Some(hook);
        self
    }
    pub fn with_stage_source_field_probe(
        mut self,
        probe: &'a PipelineSourceFieldProbeCallback<'a>,
    ) -> Self {
        self.callbacks.stage_source_field_probe = Some(probe);
        self
    }
    pub fn with_stage_trace_sink(mut self, sink: &'a PipelineStageTraceCallback<'a>) -> Self {
        self.callbacks.stage_trace_sink = Some(sink);
        self
    }

    pub fn with_stage_trace_source_row_capacity(mut self, capacity: &'a dyn Fn() -> bool) -> Self {
        self.callbacks.stage_trace_source_row_capacity = Some(capacity);
        self
    }
}

/// One serialized document qualified by its producing stage and target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelinePreviewArtifact {
    pub stage: String,
    pub target: Option<String>,
    pub path: PathBuf,
    pub records_written: usize,
    pub bytes: Vec<u8>,
}

/// Complete results in execution order, primary target before named targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelinePreviewOutcome {
    pub stages_executed: Vec<String>,
    pub artifacts: Vec<PipelinePreviewArtifact>,
}

/// Execute once with typed stage edges, then render payloads without publishing.
/// SQLite, update-existing XLSX, XML file sets and dynamic named sources are
/// excluded. Payload input documents are never reopened from their logical
/// paths; file inputs are read once each after every preflight check passes.
pub fn preview_pipeline_value_payloads(
    pipeline: &Pipeline,
    pipeline_path: &Path,
    options: &PipelinePreviewOptions<'_>,
) -> anyhow::Result<PipelinePreviewOutcome> {
    let prepared = prepare_preview(pipeline, pipeline_path, options)?;
    execute_preview(pipeline, options, prepared)
}

/// Check identities, format support, byte sizes and mapping capabilities
/// without parsing documents or evaluating a mapping. Empty bytes can stand
/// in for pending host input; file inputs are checked by metadata only.
pub fn validate_pipeline_preview(
    pipeline: &Pipeline,
    pipeline_path: &Path,
    options: &PipelinePreviewOptions<'_>,
) -> anyhow::Result<()> {
    prepare_preview(pipeline, pipeline_path, options).map(|_| ())
}

/// One admitted host input, keyed by its host name.
#[derive(Clone, Copy)]
enum PreparedHost<'b> {
    Payload(PayloadDocument<'b>),
    File(&'b Path),
}

impl PreparedHost<'_> {
    fn path(&self) -> &Path {
        match self {
            Self::Payload(document) => document.path(),
            Self::File(path) => path,
        }
    }
}

struct PreparedPreview<'a, 'b> {
    boundaries: BTreeMap<&'a str, HostBoundary<'a>>,
    hosts: BTreeMap<&'b str, PreparedHost<'b>>,
    destinations: BTreeMap<(&'a str, Option<&'a str>), PayloadDestination>,
    mapping_paths: BTreeMap<&'a str, PathBuf>,
    main_path: PathBuf,
}

fn prepare_preview<'a, 'b>(
    pipeline: &'a Pipeline,
    pipeline_path: &Path,
    options: &PipelinePreviewOptions<'b>,
) -> anyhow::Result<PreparedPreview<'a, 'b>> {
    check_cancelled(options.cancellation)?;
    validate_logical_path(pipeline_path, "pipeline")?;
    if pipeline.stages.len() > 1024
        || options.hosts.len() > 4096
        || options.identities.len() > MAX_PAYLOAD_ARTIFACTS
    {
        bail!("pipeline preview exceeds its stage, input, or output identity limit");
    }
    let declared_targets = pipeline
        .stages
        .iter()
        .try_fold(0usize, |count, stage| {
            count
                .checked_add(1)?
                .checked_add(stage.project.extra_targets.len())
        })
        .context("pipeline preview target count overflowed")?;
    if declared_targets > MAX_PAYLOAD_ARTIFACTS {
        bail!("pipeline preview exceeds the limit of {MAX_PAYLOAD_ARTIFACTS} stage targets");
    }
    let issues = engine::validate_pipeline(pipeline);
    if !issues.is_empty() {
        bail!("pipeline validation failed: {issues:?}");
    }
    for stage in &pipeline.stages {
        validate_name(&stage.id, "stage")?;
        reject_stateful_options(&stage.project.source_options)?;
        for source in &stage.project.extra_sources {
            if source.dynamic_path.is_some() {
                bail!(
                    "pipeline preview cannot load dynamic named source `{}` in stage `{}`",
                    source.name,
                    stage.id
                );
            }
            reject_stateful_options(&source.options)?;
        }
    }
    let boundaries = infer_host_boundaries(pipeline)?;
    let supplied = match options.hosts {
        PreviewHosts::Payloads(hosts) => hosts
            .iter()
            .map(|host| (host.name, PreparedHost::Payload(host.document)))
            .collect::<Vec<_>>(),
        PreviewHosts::Files { inputs, .. } => inputs
            .iter()
            .map(|input| {
                (
                    input.name.as_str(),
                    PreparedHost::File(input.path.as_path()),
                )
            })
            .collect(),
    };
    let mut hosts = BTreeMap::new();
    for (name, host) in supplied {
        validate_name(name, "host input")?;
        validate_logical_path(host.path(), "input payload")?;
        if !boundaries.contains_key(name) {
            bail!("host input `{name}` is not used by the pipeline");
        }
        if hosts.insert(name, host).is_some() {
            bail!("host input `{name}` was supplied more than once");
        }
    }
    for (name, boundary) in &boundaries {
        let host = hosts
            .get(name)
            .with_context(|| format!("host input `{name}` is missing"))?;
        preflight_format(host.path(), boundary.options, false)?;
        if let PreparedHost::File(path) = host {
            if http_url(path).is_some() {
                bail!("pipeline preview reads host input `{name}` only from a local file");
            }
            options
                .callbacks
                .policy()
                .authorize_read(super::execution_policy::read_location(
                    path,
                    boundary.options,
                ))?;
        }
    }
    // Byte budgets apply to every host before any document is opened. File
    // sizes come from metadata; each format reader keeps its own read cap.
    let mut total_input = 0usize;
    for (name, host) in &hosts {
        let len = match host {
            PreparedHost::Payload(document) => document.bytes().len(),
            PreparedHost::File(path) => {
                let len = std::fs::metadata(path)
                    .with_context(|| format!("inspecting pipeline host input `{name}`"))?
                    .len();
                usize::try_from(len).unwrap_or(usize::MAX)
            }
        };
        if len > MAX_PAYLOAD_DOCUMENT_BYTES {
            bail!("pipeline preview input exceeds the per-document payload limit");
        }
        total_input = total_input
            .checked_add(len)
            .context("pipeline preview input byte count overflowed")?;
    }
    if total_input > MAX_PAYLOAD_RUN_BYTES {
        bail!("pipeline preview inputs exceed the 256 MiB total limit");
    }
    let directory = pipeline_path.parent().unwrap_or_else(|| Path::new("."));
    let mut identities = BTreeMap::new();
    for identity in options.identities {
        validate_name(&identity.stage, "output stage")?;
        if let Some(target) = &identity.target {
            validate_name(target, "output target")?;
        }
        validate_logical_path(&identity.path, "output identity")?;
        let stage = pipeline
            .stages
            .iter()
            .find(|stage| stage.id == identity.stage)
            .with_context(|| format!("output identity has unknown stage `{}`", identity.stage))?;
        if identity.target.as_ref().is_some_and(|name| {
            !stage
                .project
                .extra_targets
                .iter()
                .any(|target| target.name == *name)
        }) {
            bail!(
                "output identity has unknown target in stage `{}`",
                identity.stage
            );
        }
        if identities
            .insert(
                (identity.stage.as_str(), identity.target.as_deref()),
                identity.path.as_path(),
            )
            .is_some()
        {
            bail!(
                "output identity for stage `{}` was supplied more than once",
                identity.stage
            );
        }
    }
    let mut destinations = BTreeMap::new();
    let mut mapping_paths = BTreeMap::new();
    for stage in &pipeline.stages {
        let path = stage
            .mapping_path
            .as_ref()
            .map_or_else(|| pipeline_path.to_owned(), |path| directory.join(path));
        validate_logical_path(&path, "stage mapping")?;
        let absolute_path = logical_absolute(&path)?;
        validate_logical_path(&absolute_path, "absolute stage mapping")?;
        mapping_paths.insert(stage.id.as_str(), absolute_path);
        // Stage edges carry typed instances. Stored source-file hints have no
        // decoding role here; format preflight uses actual host identities.
        for (target, stored, dynamic, format) in std::iter::once((
            None,
            stage.project.target_path.as_deref(),
            stage.project.root.output_path().is_some(),
            &stage.project.target_options,
        ))
        .chain(stage.project.extra_targets.iter().map(|target| {
            (
                Some(target.name.as_str()),
                target.path.as_deref(),
                target.root.output_path().is_some(),
                &target.options,
            )
        })) {
            if let Some(target) = target {
                validate_name(target, "stage target")?;
            }
            let key = (stage.id.as_str(), target);
            let destination = resolve_run_path(
                pipeline_path,
                identities.get(&key).copied(),
                stored,
                "output",
                "target_path",
                false,
            )
            .with_context(|| {
                format!(
                    "stage `{}` target {target:?} needs a logical output identity",
                    stage.id
                )
            })?;
            validate_logical_path(&destination, "output target")?;
            preflight_output_options(format)?;
            if !dynamic {
                preflight_format(&destination, format, true)?;
            }
            destinations.insert(
                key,
                if dynamic {
                    PayloadDestination::DynamicBase(destination)
                } else {
                    PayloadDestination::Static(destination)
                },
            );
        }
    }
    let main_path = logical_absolute(
        &pipeline
            .main_mapping_path
            .as_ref()
            .map_or_else(|| pipeline_path.to_owned(), |path| directory.join(path)),
    )?;
    validate_logical_path(&main_path, "main mapping")?;
    // Keep cancellation ahead of the final pre-input capability check.
    check_cancelled(options.cancellation)?;
    // Admit every stage/target in the same dependency order as execution.
    engine::check_pipeline_source_ownership_capability_with_observers(
        pipeline,
        engine::SourceOwnershipObservers {
            tracing: options.callbacks.stage_trace_sink.is_some(),
            debugging: options.cancellation.is_some() || options.callbacks.requests_debug_hook(),
        },
    )?;
    Ok(PreparedPreview {
        boundaries,
        hosts,
        destinations,
        mapping_paths,
        main_path,
    })
}

fn execute_preview(
    pipeline: &Pipeline,
    options: &PipelinePreviewOptions<'_>,
    prepared: PreparedPreview<'_, '_>,
) -> anyhow::Result<PipelinePreviewOutcome> {
    let PreparedPreview {
        boundaries,
        hosts: inputs,
        destinations,
        mapping_paths,
        main_path,
    } = prepared;
    let mut hosts = BTreeMap::new();
    for (name, boundary) in &boundaries {
        check_cancelled(options.cancellation)?;
        let instance = match inputs[name] {
            PreparedHost::Payload(document) => {
                read_payload(document, boundary.schema, boundary.options)
            }
            PreparedHost::File(path) => read_instance_with_pdf_reader(
                path,
                boundary.schema,
                boundary.options,
                options.hosts.pdf_reader(),
                options.callbacks.policy(),
            ),
        }
        .with_context(|| format!("reading pipeline host input `{name}`"))?;
        hosts.insert((*name).to_owned(), instance);
    }
    check_cancelled(options.cancellation)?;
    let callbacks = &options.callbacks;
    let stage_hook =
        (options.cancellation.is_some() || callbacks.requests_debug_hook()).then(|| {
            StageDebugHook {
                stage: RefCell::new(String::new()),
                hook: callbacks.stage_debug_hook,
                node_hook: callbacks.stage_node_debug_hook,
                function_node_hook: callbacks.stage_function_node_debug_hook,
                input_hook: callbacks.stage_input_debug_hook,
                function_input_hook: callbacks.stage_function_input_debug_hook,
                node_failure_hook: callbacks.stage_node_failure_debug_hook,
                function_failure_hook: callbacks.stage_function_failure_debug_hook,
                probe: callbacks.stage_source_field_probe,
            }
        });
    let debug = stage_hook.as_ref().map(|inner| CancellableHook {
        inner,
        cancellation: options.cancellation,
    });
    let trace = callbacks.stage_trace_sink.map(|sink| StageTraceSink {
        stage: RefCell::new(String::new()),
        sink,
        source_row_capacity: callbacks.stage_trace_source_row_capacity,
    });
    let current_datetime = jiff::Zoned::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
        .to_string();
    // Preview renders every stage target for inspection, so unlike a file run
    // it intentionally retains all intermediates until rendering completes.
    let outputs = engine::run_pipeline_with_stage_contexts(pipeline, &hosts, |stage| {
        let mut context =
            ExecutionContext::with_main_mapping_file_path(&mapping_paths[stage], &main_path)
                .with_purpose(ExecutionPurpose::Preview)
                .with_current_datetime(&current_datetime)
                .with_policy(callbacks.policy());
        if let Some(debug) = &debug {
            debug.inner.stage.replace(stage.to_owned());
            context = context.with_debug_hook(debug);
        }
        if let Some(parameters) = callbacks.runtime_parameters {
            context = context.with_parameters(parameters);
        }
        if let Some(trace) = &trace {
            trace.stage.replace(stage.to_owned());
            context = context.with_trace_sink(trace);
        }
        context
    })?;
    check_cancelled(options.cancellation)?;
    let projects = pipeline
        .stages
        .iter()
        .map(|stage| (stage.id.as_str(), &stage.project))
        .collect::<BTreeMap<_, _>>();
    let mut artifact_count = 0usize;
    for stage in &outputs.stages {
        let project = projects[stage.id.as_str()];
        for (_, _, instance, _, dynamic) in target_values(project, &stage.outputs) {
            artifact_count = artifact_count
                .checked_add(target_artifact_count(instance, dynamic)?)
                .context("pipeline preview artifact count overflowed")?;
            if artifact_count > MAX_PAYLOAD_ARTIFACTS {
                bail!(
                    "pipeline preview exceeds the limit of {MAX_PAYLOAD_ARTIFACTS} output artifacts"
                );
            }
        }
    }
    let mut artifacts = Vec::with_capacity(artifact_count);
    let mut total_output = 0usize;
    for stage in &outputs.stages {
        let project = projects[stage.id.as_str()];
        for (target, schema, instance, format, _) in target_values(project, &stage.outputs) {
            check_cancelled(options.cancellation)?;
            visit_target_documents(
                &destinations[&(stage.id.as_str(), target)],
                instance,
                |path, instance| {
                    check_cancelled(options.cancellation)?;
                    let (bytes, records_written) =
                        render_payload(&path, schema, instance, format, &current_datetime)
                            .with_context(|| {
                                format!("rendering stage `{}` target {target:?}", stage.id)
                            })?;
                    check_cancelled(options.cancellation)?;
                    if bytes.len() > MAX_PAYLOAD_DOCUMENT_BYTES {
                        bail!("pipeline preview output exceeds the 64 MiB per-document limit");
                    }
                    total_output = total_output
                        .checked_add(bytes.len())
                        .context("pipeline preview output byte count overflowed")?;
                    if total_output > MAX_PAYLOAD_RUN_BYTES {
                        bail!("pipeline preview outputs exceed the 256 MiB total limit");
                    }
                    artifacts.push(PipelinePreviewArtifact {
                        stage: stage.id.clone(),
                        target: target.map(str::to_owned),
                        path,
                        records_written,
                        bytes,
                    });
                    Ok(())
                },
            )?;
            check_cancelled(options.cancellation)?;
        }
    }
    Ok(PipelinePreviewOutcome {
        stages_executed: outputs.stages.into_iter().map(|stage| stage.id).collect(),
        artifacts,
    })
}

fn target_values<'a>(
    project: &'a mapping::Project,
    output: &'a engine::ExecutionOutputs,
) -> impl Iterator<
    Item = (
        Option<&'a str>,
        &'a ir::SchemaNode,
        &'a ir::Instance,
        &'a FormatOptions,
        bool,
    ),
> {
    std::iter::once((
        None,
        &project.target,
        &output.primary,
        &project.target_options,
        project.root.output_path().is_some(),
    ))
    .chain(
        project
            .extra_targets
            .iter()
            .zip(&output.extras)
            .map(|(target, output)| {
                (
                    Some(target.name.as_str()),
                    &target.schema,
                    &output.instance,
                    &target.options,
                    target.root.output_path().is_some(),
                )
            }),
    )
}

fn validate_name(name: &str, label: &str) -> anyhow::Result<()> {
    if name.is_empty() || name.contains('\0') || name.len() > MAX_PAYLOAD_NAME_BYTES {
        bail!(
            "{label} name must be nonempty, NUL-free and at most {MAX_PAYLOAD_NAME_BYTES} UTF-8 bytes"
        );
    }
    Ok(())
}

fn reject_stateful_options(options: &FormatOptions) -> anyhow::Result<()> {
    if options.sqlite_read.is_some() {
        bail!("pipeline preview does not support declared SQLite query inputs");
    }
    if options.local_xml_file_set {
        bail!("pipeline preview does not support local XML file sets");
    }
    if options.xlsx_update_existing {
        bail!("pipeline preview does not support update-existing XLSX");
    }
    Ok(())
}

fn preflight_format(path: &Path, options: &FormatOptions, output: bool) -> anyhow::Result<()> {
    super::validate_csv_metadata_identity(path, options, if output { "output" } else { "input" })?;
    reject_stateful_options(options)?;
    if output {
        preflight_output_options(options)?;
    }
    let direction = if output {
        Direction::Output
    } else {
        Direction::Input
    };
    match format_selection::select_preview(path, options, direction)? {
        SelectedFormat::Sqlite => bail!(
            "SQLite requires persistent filesystem state and is unavailable in pipeline preview"
        ),
        SelectedFormat::Unsupported(other) => {
            bail!("unsupported pipeline preview payload extension: .{other}")
        }
        SelectedFormat::MissingIdoc | SelectedFormat::MissingSwift | SelectedFormat::MissingPdf => {
            bail!(
                "unsupported pipeline preview payload extension: .{}",
                extension_of(path)?
            )
        }
        _ => Ok(()),
    }
}

fn preflight_output_options(options: &FormatOptions) -> anyhow::Result<()> {
    format_csv::require_executable_dependency(options.csv_text_repair_dependency)?;
    reject_stateful_options(options)?;
    format_selection::validate_declared(options, Direction::Output)?;
    if options.json5 && options.json_lines {
        bail!("JSON5 cannot be combined with JSON Lines");
    }
    if options.swift_mt.is_some() || options.pdf.is_some() || options.external_source.is_some() {
        bail!("pipeline preview target uses an input-only format");
    }
    if options.xlsx_grid.is_some()
        || options.xlsx_worksheet_set.is_some()
        || options.xlsx_composite.is_some()
        || !options.xlsx_rows.is_empty()
    {
        bail!("pipeline preview target uses an input-only XLSX layout");
    }
    if options.xlsx_hierarchical.is_some() && super::has_legacy_xlsx_layout(options) {
        bail!("hierarchical XLSX output conflicts with legacy XLSX layout options");
    }
    Ok(())
}

fn logical_absolute(path: &Path) -> anyhow::Result<PathBuf> {
    Ok(if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .context("resolving logical mapping identity")?
            .join(path)
    })
}
