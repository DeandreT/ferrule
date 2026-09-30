//! File host for a typed mapping stage graph.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use ir::{Instance, SchemaNode};
use mapping::{FormatOptions, Pipeline, PipelineInput};

use crate::{
    OutputDestination, ProjectDynamicSourceLoader, TargetOutput, absolute_mapping_path, http_url,
    read_instance, write_target_outputs,
};

/// One explicitly supplied host document. Paths follow the process working
/// directory, as they do for `ferrule run --input`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineHostFile {
    pub name: String,
    pub path: PathBuf,
}

/// One stage target selected for publication. `None` selects the primary
/// target; `Some(name)` selects an independent named target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineOutputFile {
    pub stage: String,
    pub target: Option<String>,
    pub path: PathBuf,
}

/// A host callback for pending writes in a named pipeline stage.
pub type PipelineStageDebugCallback<'a> =
    dyn Fn(&str, &engine::PendingTargetWrite) -> engine::DebugDecision + 'a;

/// A host callback after a graph node succeeds in a named stage.
pub type PipelineStageNodeDebugCallback<'a> =
    dyn Fn(&str, &engine::PendingNodeValue) -> engine::DebugDecision + 'a;

/// A host callback after a reusable-function body node succeeds in a stage.
pub type PipelineStageFunctionNodeDebugCallback<'a> =
    dyn Fn(&str, &engine::PendingFunctionNodeValue) -> engine::DebugDecision + 'a;

/// A host callback after a value reaches one recorded consumer input.
pub type PipelineStageInputDebugCallback<'a> =
    dyn Fn(&str, &engine::PendingNodeInput) -> engine::DebugDecision + 'a;

/// A host callback after a value reaches one reusable-function body input in a stage.
pub type PipelineStageFunctionInputDebugCallback<'a> =
    dyn Fn(&str, &engine::PendingFunctionNodeInput) -> engine::DebugDecision + 'a;

/// A host callback for the first failing main-graph evaluation in a stage.
pub type PipelineStageNodeFailureDebugCallback<'a> =
    dyn Fn(&str, &engine::PendingNodeFailure) -> engine::DebugDecision + 'a;

/// A host callback for the first failing reusable-function body node in a stage.
pub type PipelineStageFunctionFailureDebugCallback<'a> =
    dyn Fn(&str, &engine::PendingFunctionNodeFailure) -> engine::DebugDecision + 'a;

/// An optional exact source-field probe for the active pipeline stage.
pub type PipelineSourceFieldProbeCallback<'a> = dyn Fn(&str) -> Option<(usize, String)> + 'a;

/// A host observer for one interpreter trace event in a named pipeline stage.
pub type PipelineStageTraceCallback<'a> = dyn Fn(&str, engine::TraceEvent) + 'a;

/// Optional host values shared by every stage in one execution.
#[derive(Default)]
pub struct PipelineRunOptions<'a> {
    pub runtime_parameters: Option<&'a engine::RuntimeParameters>,
    /// A live write hook receives the ID of the stage being evaluated.
    pub stage_debug_hook: Option<&'a PipelineStageDebugCallback<'a>>,
    pub stage_node_debug_hook: Option<&'a PipelineStageNodeDebugCallback<'a>>,
    pub stage_function_node_debug_hook: Option<&'a PipelineStageFunctionNodeDebugCallback<'a>>,
    pub stage_input_debug_hook: Option<&'a PipelineStageInputDebugCallback<'a>>,
    pub stage_function_input_debug_hook: Option<&'a PipelineStageFunctionInputDebugCallback<'a>>,
    pub stage_node_failure_debug_hook: Option<&'a PipelineStageNodeFailureDebugCallback<'a>>,
    pub stage_function_failure_debug_hook:
        Option<&'a PipelineStageFunctionFailureDebugCallback<'a>>,
    pub stage_source_field_probe: Option<&'a PipelineSourceFieldProbeCallback<'a>>,
    pub stage_trace_sink: Option<&'a PipelineStageTraceCallback<'a>>,
    /// Called after every stage succeeds, before any selected output is staged.
    pub before_publish: Option<&'a dyn Fn() -> bool>,
}

impl<'a> PipelineRunOptions<'a> {
    pub fn with_stage_debug_hook(mut self, hook: &'a PipelineStageDebugCallback<'a>) -> Self {
        self.stage_debug_hook = Some(hook);
        self
    }

    pub fn with_stage_node_debug_hook(
        mut self,
        hook: &'a PipelineStageNodeDebugCallback<'a>,
    ) -> Self {
        self.stage_node_debug_hook = Some(hook);
        self
    }

    pub fn with_stage_function_node_debug_hook(
        mut self,
        hook: &'a PipelineStageFunctionNodeDebugCallback<'a>,
    ) -> Self {
        self.stage_function_node_debug_hook = Some(hook);
        self
    }

    pub fn with_stage_input_debug_hook(
        mut self,
        hook: &'a PipelineStageInputDebugCallback<'a>,
    ) -> Self {
        self.stage_input_debug_hook = Some(hook);
        self
    }

    pub fn with_stage_function_input_debug_hook(
        mut self,
        hook: &'a PipelineStageFunctionInputDebugCallback<'a>,
    ) -> Self {
        self.stage_function_input_debug_hook = Some(hook);
        self
    }

    pub fn with_stage_node_failure_debug_hook(
        mut self,
        hook: &'a PipelineStageNodeFailureDebugCallback<'a>,
    ) -> Self {
        self.stage_node_failure_debug_hook = Some(hook);
        self
    }

    pub fn with_stage_function_failure_debug_hook(
        mut self,
        hook: &'a PipelineStageFunctionFailureDebugCallback<'a>,
    ) -> Self {
        self.stage_function_failure_debug_hook = Some(hook);
        self
    }

    pub fn with_stage_source_field_probe(
        mut self,
        probe: &'a PipelineSourceFieldProbeCallback<'a>,
    ) -> Self {
        self.stage_source_field_probe = Some(probe);
        self
    }

    pub fn with_stage_trace_sink(mut self, sink: &'a PipelineStageTraceCallback<'a>) -> Self {
        self.stage_trace_sink = Some(sink);
        self
    }

    pub fn with_before_publish(mut self, gate: &'a dyn Fn() -> bool) -> Self {
        self.before_publish = Some(gate);
        self
    }
}

struct StageDebugHook<'a> {
    stage: RefCell<String>,
    hook: Option<&'a PipelineStageDebugCallback<'a>>,
    node_hook: Option<&'a PipelineStageNodeDebugCallback<'a>>,
    function_node_hook: Option<&'a PipelineStageFunctionNodeDebugCallback<'a>>,
    input_hook: Option<&'a PipelineStageInputDebugCallback<'a>>,
    function_input_hook: Option<&'a PipelineStageFunctionInputDebugCallback<'a>>,
    node_failure_hook: Option<&'a PipelineStageNodeFailureDebugCallback<'a>>,
    function_failure_hook: Option<&'a PipelineStageFunctionFailureDebugCallback<'a>>,
    probe: Option<&'a PipelineSourceFieldProbeCallback<'a>>,
}

impl engine::DebugHook for StageDebugHook<'_> {
    fn wants_node_values(&self) -> bool {
        self.node_hook.is_some()
    }

    fn wants_function_node_values(&self) -> bool {
        self.function_node_hook.is_some()
    }

    fn wants_node_inputs(&self) -> bool {
        self.input_hook.is_some()
    }

    fn wants_function_node_inputs(&self) -> bool {
        self.function_input_hook.is_some()
    }

    fn wants_node_failures(&self) -> bool {
        self.node_failure_hook.is_some()
    }

    fn wants_function_node_failures(&self) -> bool {
        self.function_failure_hook.is_some()
    }

    fn source_field_probe(&self) -> Option<(usize, String)> {
        self.probe.and_then(|probe| probe(&self.stage.borrow()))
    }

    fn before_target_write(&self, write: &engine::PendingTargetWrite) -> engine::DebugDecision {
        self.hook.map_or(engine::DebugDecision::Resume, |hook| {
            hook(&self.stage.borrow(), write)
        })
    }

    fn after_node_value(&self, node: &engine::PendingNodeValue) -> engine::DebugDecision {
        self.node_hook
            .map_or(engine::DebugDecision::Resume, |hook| {
                hook(&self.stage.borrow(), node)
            })
    }

    fn after_function_node_value(
        &self,
        node: &engine::PendingFunctionNodeValue,
    ) -> engine::DebugDecision {
        self.function_node_hook
            .map_or(engine::DebugDecision::Resume, |hook| {
                hook(&self.stage.borrow(), node)
            })
    }

    fn after_node_input(&self, input: &engine::PendingNodeInput) -> engine::DebugDecision {
        self.input_hook
            .map_or(engine::DebugDecision::Resume, |hook| {
                hook(&self.stage.borrow(), input)
            })
    }

    fn after_function_node_input(
        &self,
        input: &engine::PendingFunctionNodeInput,
    ) -> engine::DebugDecision {
        self.function_input_hook
            .map_or(engine::DebugDecision::Resume, |hook| {
                hook(&self.stage.borrow(), input)
            })
    }

    fn after_node_failure(&self, failure: &engine::PendingNodeFailure) -> engine::DebugDecision {
        self.node_failure_hook
            .map_or(engine::DebugDecision::Resume, |hook| {
                hook(&self.stage.borrow(), failure)
            })
    }

    fn after_function_node_failure(
        &self,
        failure: &engine::PendingFunctionNodeFailure,
    ) -> engine::DebugDecision {
        self.function_failure_hook
            .map_or(engine::DebugDecision::Resume, |hook| {
                hook(&self.stage.borrow(), failure)
            })
    }
}

struct StageTraceSink<'a> {
    stage: RefCell<String>,
    sink: &'a PipelineStageTraceCallback<'a>,
}

impl engine::TraceSink for StageTraceSink<'_> {
    fn record(&self, event: engine::TraceEvent) {
        (self.sink)(&self.stage.borrow(), event);
    }
}

/// One published file. Dynamic document targets may produce several artifacts
/// for a single selected stage target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineArtifact {
    pub stage: String,
    pub target: Option<String>,
    pub path: PathBuf,
    pub records_written: usize,
}

/// The complete stage execution order and only the explicitly published files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineRunOutcome {
    pub stages_executed: Vec<String>,
    pub artifacts: Vec<PipelineArtifact>,
}

/// Load a serialized [`Pipeline`], evaluate every stage, and atomically publish
/// the selected outputs after all stages succeed.
pub fn run_pipeline_file(
    pipeline_path: &Path,
    inputs: &[PipelineHostFile],
    outputs: &[PipelineOutputFile],
) -> anyhow::Result<PipelineRunOutcome> {
    run_pipeline_file_with_options(
        pipeline_path,
        inputs,
        outputs,
        &PipelineRunOptions::default(),
    )
}

/// Like [`run_pipeline_file`], with bounded typed runtime parameters available
/// to mappings in every stage.
pub fn run_pipeline_file_with_options(
    pipeline_path: &Path,
    inputs: &[PipelineHostFile],
    outputs: &[PipelineOutputFile],
    options: &PipelineRunOptions<'_>,
) -> anyhow::Result<PipelineRunOutcome> {
    let json = std::fs::read_to_string(pipeline_path)
        .with_context(|| format!("reading pipeline file {}", pipeline_path.display()))?;
    let pipeline: Pipeline = serde_json::from_str(&json)
        .with_context(|| format!("parsing pipeline file {}", pipeline_path.display()))?;
    run_pipeline_value_with_options(&pipeline, pipeline_path, inputs, outputs, options)
}

fn run_pipeline_value_with_options(
    pipeline: &Pipeline,
    pipeline_path: &Path,
    inputs: &[PipelineHostFile],
    outputs: &[PipelineOutputFile],
    options: &PipelineRunOptions<'_>,
) -> anyhow::Result<PipelineRunOutcome> {
    let issues = engine::validate_pipeline(pipeline);
    if !issues.is_empty() {
        bail!(
            "pipeline validation failed with {} issue(s):\n{}",
            issues.len(),
            issues
                .iter()
                .map(|issue| format!("  - {issue}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    let host_boundaries = infer_host_boundaries(pipeline)?;
    let paths = validate_host_inputs(&host_boundaries, inputs)?;
    let selected = select_outputs(pipeline, outputs)?;

    let mut protected = vec![pipeline_path.to_path_buf()];
    let mut hosts = BTreeMap::new();
    for (name, boundary) in &host_boundaries {
        let path = paths[name];
        let instance = read_instance(path, boundary.schema, boundary.options)
            .with_context(|| format!("loading pipeline host input `{name}`"))?;
        if http_url(path).is_none() {
            protected.push(path.to_path_buf());
            if boundary.options.local_xml_file_set
                && let Instance::DocumentSet(documents) = &instance
            {
                let base = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                protected.extend(documents.iter().map(|member| base.join(member.path())));
            }
        }
        hosts.insert((*name).to_owned(), instance);
    }

    let pipeline_dir = pipeline_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let runtime_path = absolute_mapping_path(pipeline_path)?;
    let main_mapping_path = match &pipeline.main_mapping_path {
        Some(path) => absolute_mapping_path(&pipeline_dir.join(path))?.into_owned(),
        None => runtime_path.to_path_buf(),
    };
    protected.push(main_mapping_path.clone());
    let stage_mapping_paths = pipeline
        .stages
        .iter()
        .map(|stage| {
            let path = match &stage.mapping_path {
                Some(path) => absolute_mapping_path(&pipeline_dir.join(path))?.into_owned(),
                None => runtime_path.to_path_buf(),
            };
            Ok((stage.id.as_str(), path))
        })
        .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
    protected.extend(stage_mapping_paths.values().cloned());
    let current_datetime = jiff::Zoned::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
        .to_string();
    let dynamic_loaders = pipeline
        .stages
        .iter()
        .map(|stage| {
            (
                stage.id.as_str(),
                ProjectDynamicSourceLoader::new(pipeline_dir, &stage.project.extra_sources),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let stage_debug_hook = (options.stage_debug_hook.is_some()
        || options.stage_node_debug_hook.is_some()
        || options.stage_function_node_debug_hook.is_some()
        || options.stage_input_debug_hook.is_some()
        || options.stage_function_input_debug_hook.is_some()
        || options.stage_node_failure_debug_hook.is_some()
        || options.stage_function_failure_debug_hook.is_some())
    .then(|| StageDebugHook {
        stage: RefCell::new(String::new()),
        hook: options.stage_debug_hook,
        node_hook: options.stage_node_debug_hook,
        function_node_hook: options.stage_function_node_debug_hook,
        input_hook: options.stage_input_debug_hook,
        function_input_hook: options.stage_function_input_debug_hook,
        node_failure_hook: options.stage_node_failure_debug_hook,
        function_failure_hook: options.stage_function_failure_debug_hook,
        probe: options.stage_source_field_probe,
    });
    let stage_trace_sink = options.stage_trace_sink.map(|sink| StageTraceSink {
        stage: RefCell::new(String::new()),
        sink,
    });
    let results = engine::run_pipeline_with_stage_contexts(pipeline, &hosts, |stage| {
        let mut execution = engine::ExecutionContext::with_main_mapping_file_path(
            &stage_mapping_paths[stage],
            &main_mapping_path,
        )
        .with_current_datetime(&current_datetime)
        .with_dynamic_source_loader(&dynamic_loaders[stage]);
        if let Some(parameters) = options.runtime_parameters {
            execution = execution.with_parameters(parameters);
        }
        if let Some(hook) = &stage_debug_hook {
            hook.stage.replace(stage.to_owned());
            execution = execution.with_debug_hook(hook);
        }
        if let Some(sink) = &stage_trace_sink {
            sink.stage.replace(stage.to_owned());
            execution = execution.with_trace_sink(sink);
        }
        execution
    })?;
    for loader in dynamic_loaders.values() {
        for ((_, path), instance) in loader.cache.borrow().iter() {
            let path = PathBuf::from(path);
            if http_url(&path).is_none() {
                protected.push(if path.is_absolute() {
                    path
                } else {
                    pipeline_dir.join(path)
                });
                if let Instance::DocumentSet(documents) = instance.as_ref() {
                    protected.extend(
                        documents
                            .iter()
                            .map(|member| PathBuf::from(member.source_path())),
                    );
                }
            }
        }
    }

    let writes = selected
        .iter()
        .map(|selection| {
            let stage = results
                .stage(&selection.request.stage)
                .expect("selected stage was validated");
            let instance = match &selection.request.target {
                None => &stage.primary,
                Some(name) => {
                    &stage
                        .extras
                        .iter()
                        .find(|output| output.name == *name)
                        .expect("selected named target was validated")
                        .instance
                }
            };
            TargetOutput {
                destination: &selection.destination,
                name: selection.name,
                schema: selection.schema,
                instance,
                options: selection.options,
                current_datetime: &current_datetime,
                additional: selection.request.target.is_some(),
            }
        })
        .collect::<Vec<_>>();
    let protected_refs = protected.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    if options.before_publish.is_some_and(|gate| !gate()) {
        return Err(engine::EngineError::DebugCancelled.into());
    }
    let published = write_target_outputs(&writes, &protected_refs)?;
    let artifacts = selected
        .iter()
        .zip(published)
        .flat_map(|(selection, result)| {
            result
                .outputs
                .into_iter()
                .map(move |file| PipelineArtifact {
                    stage: selection.request.stage.clone(),
                    target: selection.request.target.clone(),
                    path: file.path,
                    records_written: file.records_written,
                })
        })
        .collect();
    Ok(PipelineRunOutcome {
        stages_executed: results.stages.into_iter().map(|stage| stage.id).collect(),
        artifacts,
    })
}

struct HostBoundary<'a> {
    schema: &'a SchemaNode,
    options: &'a FormatOptions,
}

fn infer_host_boundaries(pipeline: &Pipeline) -> anyhow::Result<BTreeMap<&str, HostBoundary<'_>>> {
    let mut hosts = BTreeMap::new();
    for stage in &pipeline.stages {
        if let PipelineInput::Host { name } = &stage.source {
            add_host_boundary(
                &mut hosts,
                name,
                &stage.project.source,
                &stage.project.source_options,
            )?;
        }
        for binding in &stage.extra_sources {
            let PipelineInput::Host { name } = &binding.from else {
                continue;
            };
            let source = stage
                .project
                .extra_sources
                .iter()
                .find(|source| source.name == binding.name)
                .expect("named source binding was validated");
            add_host_boundary(&mut hosts, name, &source.schema, &source.options)?;
        }
    }
    Ok(hosts)
}

fn add_host_boundary<'a>(
    hosts: &mut BTreeMap<&'a str, HostBoundary<'a>>,
    name: &'a str,
    schema: &'a SchemaNode,
    options: &'a FormatOptions,
) -> anyhow::Result<()> {
    if let Some(previous) = hosts.get(name)
        && (previous.options != options
            || !host_schema_compatible(previous.schema, schema, options))
    {
        bail!("pipeline host input `{name}` has inconsistent schema or format options");
    }
    hosts.insert(name, HostBoundary { schema, options });
    Ok(())
}

fn host_schema_compatible(
    previous: &SchemaNode,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> bool {
    previous == schema
        || ((options.json_document || options.json_lines) && boundary_compatible(previous, schema))
}

// Instance trees have no root-name field. Match the engine's stage-boundary
// rule so one host value may feed otherwise identical typed boundaries.
fn boundary_compatible(actual: &SchemaNode, expected: &SchemaNode) -> bool {
    if actual == expected {
        return true;
    }
    let mut renamed = actual.clone();
    renamed.name.clone_from(&expected.name);
    renamed == *expected
}

fn validate_host_inputs<'a>(
    required: &BTreeMap<&str, HostBoundary<'_>>,
    inputs: &'a [PipelineHostFile],
) -> anyhow::Result<BTreeMap<&'a str, &'a Path>> {
    let mut paths = BTreeMap::new();
    for input in inputs {
        if !required.contains_key(input.name.as_str()) {
            bail!("host input `{}` is not used by the pipeline", input.name);
        }
        if paths
            .insert(input.name.as_str(), input.path.as_path())
            .is_some()
        {
            bail!("host input `{}` was supplied more than once", input.name);
        }
    }
    for name in required.keys() {
        if !paths.contains_key(name) {
            bail!("host input `{name}` is missing");
        }
    }
    Ok(paths)
}

struct SelectedOutput<'a> {
    request: &'a PipelineOutputFile,
    name: &'a str,
    schema: &'a SchemaNode,
    options: &'a FormatOptions,
    destination: OutputDestination,
}

fn select_outputs<'a>(
    pipeline: &'a Pipeline,
    requests: &'a [PipelineOutputFile],
) -> anyhow::Result<Vec<SelectedOutput<'a>>> {
    if requests.is_empty() {
        bail!("pipeline run needs at least one selected output");
    }
    let mut selected = BTreeSet::new();
    let mut outputs = Vec::with_capacity(requests.len());
    for request in requests {
        let key = (request.stage.as_str(), request.target.as_deref());
        if !selected.insert(key) {
            bail!(
                "stage `{}` target `{}` was selected more than once",
                request.stage,
                request.target.as_deref().unwrap_or("primary")
            );
        }
        let stage = pipeline
            .stages
            .iter()
            .find(|stage| stage.id == request.stage)
            .with_context(|| format!("selected output has unknown stage `{}`", request.stage))?;
        let (name, schema, options, dynamic) = match &request.target {
            None => (
                stage.project.target.name.as_str(),
                &stage.project.target,
                &stage.project.target_options,
                stage.project.root.output_path().is_some(),
            ),
            Some(name) => {
                let target = stage
                    .project
                    .extra_targets
                    .iter()
                    .find(|target| target.name == *name)
                    .with_context(|| {
                        format!("stage `{}` has no named target `{name}`", request.stage)
                    })?;
                (
                    target.name.as_str(),
                    &target.schema,
                    &target.options,
                    target.root.output_path().is_some(),
                )
            }
        };
        let destination = if dynamic {
            OutputDestination::DynamicBase(request.path.clone())
        } else {
            OutputDestination::Static(request.path.clone())
        };
        outputs.push(SelectedOutput {
            request,
            name,
            schema,
            options,
            destination,
        });
    }
    Ok(outputs)
}
