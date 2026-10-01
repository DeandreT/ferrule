//! Bounded host-owned payload preview of a complete typed stage graph.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, bail};
use engine::{DebugDecision, DebugHook, ExecutionContext, ExecutionPurpose};
use mapping::{FormatOptions, Pipeline};

use super::payload::{
    PayloadDestination, read_payload, render_payload, target_artifact_count, validate_logical_path,
    visit_target_documents,
};
use super::pipeline::{
    HostBoundary, PipelineRunOptions, PipelineSourceFieldProbeCallback, PipelineStageDebugCallback,
    PipelineStageFunctionFailureDebugCallback, PipelineStageFunctionInputDebugCallback,
    PipelineStageFunctionNodeDebugCallback, PipelineStageInputDebugCallback,
    PipelineStageNodeDebugCallback, PipelineStageNodeFailureDebugCallback,
    PipelineStageTraceCallback, StageDebugHook, StageTraceSink, infer_host_boundaries,
};
use super::{
    MAX_PAYLOAD_ARTIFACTS, MAX_PAYLOAD_DOCUMENT_BYTES, MAX_PAYLOAD_NAME_BYTES,
    MAX_PAYLOAD_RUN_BYTES, PayloadDocument, extension_for_dispatch, resolve_run_path,
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

/// Input payloads and observers for preview. Stored previews are always enabled.
pub struct PipelinePreviewOptions<'a> {
    hosts: &'a [PipelineHostPayload<'a>],
    identities: &'a [PipelinePreviewOutputIdentity],
    callbacks: PipelineRunOptions<'a>,
    cancellation: Option<&'a AtomicBool>,
}

impl<'a> PipelinePreviewOptions<'a> {
    pub fn new(hosts: &'a [PipelineHostPayload<'a>]) -> Self {
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
/// excluded. Input documents are never reopened from their logical paths.
pub fn preview_pipeline_value_payloads(
    pipeline: &Pipeline,
    pipeline_path: &Path,
    options: &PipelinePreviewOptions<'_>,
) -> anyhow::Result<PipelinePreviewOutcome> {
    let PreparedPreview {
        boundaries,
        payloads,
        destinations,
        mapping_paths,
        main_path,
    } = prepare_preview(pipeline, pipeline_path, options)?;
    execute_preview(
        pipeline,
        options,
        PreparedPreview {
            boundaries,
            payloads,
            destinations,
            mapping_paths,
            main_path,
        },
    )
}

/// Check identities, format support and byte sizes without parsing documents
/// or evaluating any mapping. Empty bytes can stand in for pending host input.
pub fn validate_pipeline_preview(
    pipeline: &Pipeline,
    pipeline_path: &Path,
    options: &PipelinePreviewOptions<'_>,
) -> anyhow::Result<()> {
    prepare_preview(pipeline, pipeline_path, options).map(|_| ())
}

struct PreparedPreview<'a, 'b> {
    boundaries: BTreeMap<&'a str, HostBoundary<'a>>,
    payloads: BTreeMap<&'b str, PayloadDocument<'b>>,
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
    let mut payloads = BTreeMap::new();
    let mut total_input = 0usize;
    for host in options.hosts {
        validate_name(host.name, "host input")?;
        validate_logical_path(host.document.path(), "input payload")?;
        if !boundaries.contains_key(host.name) {
            bail!("host input `{}` is not used by the pipeline", host.name);
        }
        if payloads.insert(host.name, host.document).is_some() {
            bail!("host input `{}` was supplied more than once", host.name);
        }
        if host.document.bytes().len() > MAX_PAYLOAD_DOCUMENT_BYTES {
            bail!("pipeline preview input exceeds the per-document payload limit");
        }
        total_input = total_input
            .checked_add(host.document.bytes().len())
            .context("pipeline preview input byte count overflowed")?;
    }
    if total_input > MAX_PAYLOAD_RUN_BYTES {
        bail!("pipeline preview inputs exceed the 256 MiB total limit");
    }
    for (name, boundary) in &boundaries {
        let document = payloads
            .get(name)
            .with_context(|| format!("host input `{name}` is missing"))?;
        preflight_format(document.path(), boundary.options, false)?;
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
    Ok(PreparedPreview {
        boundaries,
        payloads,
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
        payloads,
        destinations,
        mapping_paths,
        main_path,
    } = prepared;
    let mut hosts = BTreeMap::new();
    for (name, boundary) in &boundaries {
        check_cancelled(options.cancellation)?;
        hosts.insert(
            (*name).to_owned(),
            read_payload(payloads[name], boundary.schema, boundary.options)
                .with_context(|| format!("reading pipeline host input `{name}`"))?,
        );
    }
    check_cancelled(options.cancellation)?;
    let callbacks = &options.callbacks;
    let debug = PreviewDebugHook {
        inner: StageDebugHook {
            stage: RefCell::new(String::new()),
            hook: callbacks.stage_debug_hook,
            node_hook: callbacks.stage_node_debug_hook,
            function_node_hook: callbacks.stage_function_node_debug_hook,
            input_hook: callbacks.stage_input_debug_hook,
            function_input_hook: callbacks.stage_function_input_debug_hook,
            node_failure_hook: callbacks.stage_node_failure_debug_hook,
            function_failure_hook: callbacks.stage_function_failure_debug_hook,
            probe: callbacks.stage_source_field_probe,
        },
        cancellation: options.cancellation,
    };
    let trace = callbacks.stage_trace_sink.map(|sink| StageTraceSink {
        stage: RefCell::new(String::new()),
        sink,
        source_row_capacity: callbacks.stage_trace_source_row_capacity,
    });
    let current_datetime = jiff::Zoned::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
        .to_string();
    let outputs = engine::run_pipeline_with_stage_contexts(pipeline, &hosts, |stage| {
        debug.inner.stage.replace(stage.to_owned());
        let mut context =
            ExecutionContext::with_main_mapping_file_path(&mapping_paths[stage], &main_path)
                .with_purpose(ExecutionPurpose::Preview)
                .with_current_datetime(&current_datetime)
                .with_debug_hook(&debug);
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
    preflight_typed_options(options, output)?;
    if options.json_document || options.json5 || options.json_lines {
        super::json5_selected(path, options)?;
    }
    if options.xbrl.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.external_source.is_some()
        || options.pdf.is_some()
        || options.flextext.is_some()
        || options.protobuf.is_some()
        || options.edi_kind.is_some()
        || options.xml_document
        || options.json_document
        || options.json5
        || options.json_lines
        || options.fixed_width.is_some()
    {
        return Ok(());
    }
    super::validate_tabular_fallback(path, options, if output { "output" } else { "input" })?;
    match extension_for_dispatch(path, options)?.as_str() {
        "json" | "json5" | "jsonl" | "ndjson" => {
            super::json5_selected(path, options)?;
            Ok(())
        }
        "csv" | "txt" | "xlsx" | "xml" | "edi" | "x12" | "edifact" | "hl7" => Ok(()),
        "db" | "sqlite" | "sqlite3" => bail!(
            "SQLite requires persistent filesystem state and is unavailable in pipeline preview"
        ),
        other => bail!("unsupported pipeline preview payload extension: .{other}"),
    }
}

fn preflight_output_options(options: &FormatOptions) -> anyhow::Result<()> {
    format_csv::require_executable_dependency(options.csv_text_repair_dependency)?;
    reject_stateful_options(options)?;
    preflight_typed_options(options, true)?;
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

fn preflight_typed_options(options: &FormatOptions, output: bool) -> anyhow::Result<()> {
    let side = if output { "output" } else { "input" };
    if options.xbrl.is_some() {
        super::reject_xbrl_conflicts(options, side)
    } else if options.idoc.is_some() {
        super::reject_idoc_conflicts(options, side)
    } else if options.swift_mt.is_some() {
        super::reject_swift_conflicts(options, side)
    } else if options.external_source.is_some() {
        super::reject_external_source_conflicts(options, side)
    } else if options.pdf.is_some() {
        super::reject_pdf_conflicts(options, side)
    } else if options.flextext.is_some() {
        super::reject_flextext_conflicts(options, side)
    } else if options.protobuf.is_some() {
        super::reject_protobuf_conflicts(options, side)
    } else if options.edi_kind.is_some() {
        super::reject_edi_conflicts(options, side)
    } else if options.xml_document {
        super::reject_xml_conflicts(options, side)
    } else if options.json_document || options.json5 || options.json_lines {
        super::reject_json_conflicts(options, side)
    } else if options.fixed_width.is_some() {
        super::reject_fixed_width_csv_options(options, side)
    } else {
        Ok(())
    }
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

fn check_cancelled(cancellation: Option<&AtomicBool>) -> anyhow::Result<()> {
    if cancellation.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return Err(engine::EngineError::DebugCancelled.into());
    }
    Ok(())
}

struct PreviewDebugHook<'a> {
    inner: StageDebugHook<'a>,
    cancellation: Option<&'a AtomicBool>,
}

impl PreviewDebugHook<'_> {
    fn control(&self, callback: impl FnOnce() -> DebugDecision) -> DebugDecision {
        if check_cancelled(self.cancellation).is_err() {
            return DebugDecision::Cancel;
        }
        let decision = callback();
        if check_cancelled(self.cancellation).is_err() {
            DebugDecision::Cancel
        } else {
            decision
        }
    }
}

impl DebugHook for PreviewDebugHook<'_> {
    fn wants_node_values(&self) -> bool {
        self.inner.wants_node_values()
    }
    fn wants_function_node_values(&self) -> bool {
        self.inner.wants_function_node_values()
    }
    fn wants_node_inputs(&self) -> bool {
        self.inner.wants_node_inputs()
    }
    fn wants_function_node_inputs(&self) -> bool {
        self.inner.wants_function_node_inputs()
    }
    fn wants_node_failures(&self) -> bool {
        self.inner.wants_node_failures()
    }
    fn wants_function_node_failures(&self) -> bool {
        self.inner.wants_function_node_failures()
    }
    fn source_field_probe(&self) -> Option<(usize, String)> {
        self.inner.source_field_probe()
    }
    fn before_target_write(&self, value: &engine::PendingTargetWrite) -> DebugDecision {
        self.control(|| self.inner.before_target_write(value))
    }
    fn after_node_value(&self, value: &engine::PendingNodeValue) -> DebugDecision {
        self.control(|| self.inner.after_node_value(value))
    }
    fn after_function_node_value(&self, value: &engine::PendingFunctionNodeValue) -> DebugDecision {
        self.control(|| self.inner.after_function_node_value(value))
    }
    fn after_node_input(&self, value: &engine::PendingNodeInput) -> DebugDecision {
        self.control(|| self.inner.after_node_input(value))
    }
    fn after_function_node_input(&self, value: &engine::PendingFunctionNodeInput) -> DebugDecision {
        self.control(|| self.inner.after_function_node_input(value))
    }
    fn after_node_failure(&self, value: &engine::PendingNodeFailure) -> DebugDecision {
        self.control(|| self.inner.after_node_failure(value))
    }
    fn after_function_node_failure(
        &self,
        value: &engine::PendingFunctionNodeFailure,
    ) -> DebugDecision {
        self.control(|| self.inner.after_function_node_failure(value))
    }
}
