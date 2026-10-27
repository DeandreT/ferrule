//! Filesystem `run`: an immutable prepared request, then acquisition,
//! evaluation and atomic publication of every planned target.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, bail};
use ir::{Instance, SchemaNode};
use mapping::FormatOptions;

use crate::cancellation::{CancellableHook, NoDebugHook, check_cancelled};
use crate::execution_policy::TRUSTED;
use crate::output_documents::{OutputDestination, TargetOutput, write_target_outputs};
use crate::payload::PayloadRunOutcome;
use crate::route_capability::{HostRoute, InputRole, PlannedPublication, RoutePlan, RouteRefusal};
use crate::sqlite_query::PreparedQuery;
use crate::{
    ExecutionPolicy, FilesystemPdfReader, FilesystemRunPolicy, ProjectDynamicSourceLoader,
    TargetSelection, TraceSink, absolute_mapping_path, http_url, load_project,
    read_instance_with_pdf_reader, read_set, require_valid, resolve_run_path, resolve_stored_path,
};

/// Result of running a project after resolving its input and output paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub records_written: usize,
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    /// Files emitted by a per-document dynamic primary target. Ordinary
    /// single-file targets leave this empty.
    pub primary_outputs: Vec<WrittenOutput>,
    pub extra_outputs: Vec<WrittenOutput>,
    /// Every published file in deterministic primary-then-extra target order.
    pub artifacts: Vec<WrittenOutput>,
}

/// One target file written during a project run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenOutput {
    pub name: String,
    pub records_written: usize,
    pub path: PathBuf,
}

/// Host-supplied paths, scalar parameters, and optional observers for one run.
///
/// This legacy combined form is split into an immutable [`RunRequest`] and the
/// [`RunObservers`] that execute it.
#[derive(Default)]
pub struct RunOptions<'a> {
    pub input_path: Option<&'a Path>,
    pub output_path: Option<&'a Path>,
    pub target: Option<TargetSelection<'a>>,
    pub runtime_parameters: Option<&'a engine::RuntimeParameters>,
    pub trace_sink: Option<&'a dyn TraceSink>,
    /// Optional live control point before ordinary target-field writes.
    pub debug_hook: Option<&'a dyn engine::DebugHook>,
    /// Called after evaluation succeeds and before any output staging begins.
    /// Returning false cancels the run without publishing target files.
    pub before_publish: Option<&'a dyn Fn() -> bool>,
    /// Host-owned artifact paths that mapping outputs must not replace. The
    /// run also refuses to publish when one names the design or a consumed
    /// local input, because the host replaces these paths afterwards.
    pub protected_output_paths: &'a [&'a Path],
    /// Host authority for the run; `None` is the trusted default.
    pub execution_policy: Option<&'a ExecutionPolicy>,
    /// Host cancellation, independent of tracing and debugging. See
    /// [`RunObservers::cancellation`].
    pub cancellation: Option<&'a AtomicBool>,
}

impl<'a> RunOptions<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_input_path(mut self, path: &'a Path) -> Self {
        self.input_path = Some(path);
        self
    }

    pub fn with_output_path(mut self, path: &'a Path) -> Self {
        self.output_path = Some(path);
        self
    }

    pub fn with_target(mut self, target: TargetSelection<'a>) -> Self {
        self.target = Some(target);
        self
    }

    pub fn with_runtime_parameters(mut self, parameters: &'a engine::RuntimeParameters) -> Self {
        self.runtime_parameters = Some(parameters);
        self
    }

    pub fn with_trace_sink(mut self, trace_sink: &'a dyn TraceSink) -> Self {
        self.trace_sink = Some(trace_sink);
        self
    }

    pub fn with_debug_hook(mut self, debug_hook: &'a dyn engine::DebugHook) -> Self {
        self.debug_hook = Some(debug_hook);
        self
    }

    pub fn with_before_publish(mut self, gate: &'a dyn Fn() -> bool) -> Self {
        self.before_publish = Some(gate);
        self
    }

    pub fn with_protected_output_paths(mut self, paths: &'a [&'a Path]) -> Self {
        self.protected_output_paths = paths;
        self
    }

    pub fn with_execution_policy(mut self, policy: &'a ExecutionPolicy) -> Self {
        self.execution_policy = Some(policy);
        self
    }

    pub fn with_cancellation(mut self, cancellation: &'a AtomicBool) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    /// The immutable request part, declaring exactly the observers supplied here.
    pub fn request(&self) -> RunRequest<'a> {
        let observers = self.observers();
        RunRequest {
            input_path: self.input_path,
            output_path: self.output_path,
            target: self.target,
            runtime_parameters: self.runtime_parameters,
            protected_output_paths: self.protected_output_paths,
            execution_policy: self.execution_policy,
            observers: observers.declaration(),
        }
    }

    pub fn observers(&self) -> RunObservers<'a> {
        RunObservers {
            trace_sink: self.trace_sink,
            debug_hook: self.debug_hook,
            before_publish: self.before_publish,
            cancellation: self.cancellation,
        }
    }
}

/// An immutable run request, fixed before anything is acquired.
///
/// Explicit paths keep their process-relative meaning and stored paths
/// resolve from the design's directory. A path of `-` selects a standard
/// stream only through [`select_run_route`](crate::select_run_route); the
/// filesystem route treats it as an ordinary path.
#[derive(Clone, Copy, Default)]
pub struct RunRequest<'a> {
    pub input_path: Option<&'a Path>,
    pub output_path: Option<&'a Path>,
    pub target: Option<TargetSelection<'a>>,
    pub runtime_parameters: Option<&'a engine::RuntimeParameters>,
    /// Host-owned artifacts published after the run, such as a JSON trace.
    pub protected_output_paths: &'a [&'a Path],
    /// Host authority for the run; `None` is the trusted default.
    pub execution_policy: Option<&'a ExecutionPolicy>,
    /// Observers the host will attach. Capability admission sees exactly these,
    /// and execution refuses an observer that was not declared.
    pub observers: engine::SourceOwnershipObservers,
}

impl RunRequest<'_> {
    fn policy(&self) -> &ExecutionPolicy {
        self.execution_policy.unwrap_or(&TRUSTED)
    }
}

/// Observers and host controls attached when a prepared run executes.
#[derive(Clone, Copy, Default)]
pub struct RunObservers<'a> {
    pub trace_sink: Option<&'a dyn TraceSink>,
    pub debug_hook: Option<&'a dyn engine::DebugHook>,
    /// Called after evaluation succeeds and before any output staging begins.
    /// Returning false cancels the run without publishing target files.
    pub before_publish: Option<&'a dyn Fn() -> bool>,
    /// Checked before each input acquisition, at interpreter control points,
    /// and before publication; a set flag fails with
    /// `EngineError::DebugCancelled`. It needs no trace sink or host debug
    /// hook. The interpreter's control points are its debug protocol, so a
    /// cancellable run is admitted as a debugging run. Cooperative cancellation
    /// cannot interrupt one synchronous parser or request.
    pub cancellation: Option<&'a AtomicBool>,
}

impl RunObservers<'_> {
    /// The capability declaration matching these observers.
    pub fn declaration(&self) -> engine::SourceOwnershipObservers {
        engine::SourceOwnershipObservers {
            tracing: self.trace_sink.is_some(),
            debugging: self.debug_hook.is_some() || self.cancellation.is_some(),
        }
    }

    pub(crate) fn require_declared(
        &self,
        declared: engine::SourceOwnershipObservers,
    ) -> Result<(), RouteRefusal> {
        let requested = self.declaration();
        if (requested.tracing && !declared.tracing) || (requested.debugging && !declared.debugging)
        {
            return Err(RouteRefusal::UndeclaredObservers);
        }
        Ok(())
    }
}

/// Loads the project at `project_path`, runs it against `input_path` (plus
/// any extra sources the project declares), and writes the result to
/// `output_path`. Returns the number of top-level records written (rows
/// for a CSV output, 1 for an XML document).
pub fn run_project(
    project_path: &Path,
    input_path: &Path,
    output_path: &Path,
) -> anyhow::Result<usize> {
    Ok(run_project_with_paths(project_path, Some(input_path), Some(output_path))?.records_written)
}

/// Loads and runs a project, using explicit paths when provided and falling
/// back to the project's `source_path` and `target_path`. Relative stored
/// paths are resolved from the project file's directory; explicit paths keep
/// their normal process-relative semantics.
pub fn run_project_with_paths(
    project_path: &Path,
    input_path: Option<&Path>,
    output_path: Option<&Path>,
) -> anyhow::Result<RunOutcome> {
    run_project_with_options(
        project_path,
        &RunOptions {
            input_path,
            output_path,
            ..RunOptions::default()
        },
    )
}

/// Loads and runs a project with one unified host execution contract.
pub fn run_project_with_options(
    project_path: &Path,
    options: &RunOptions<'_>,
) -> anyhow::Result<RunOutcome> {
    let project = load_project(project_path)?;
    run_project_value_with_options(&project, project_path, options)
}

/// Loads and runs a project while reporting successful graph evaluations to
/// `trace_sink` in deterministic execution order.
pub fn run_project_with_paths_and_trace(
    project_path: &Path,
    input_path: Option<&Path>,
    output_path: Option<&Path>,
    trace_sink: &dyn TraceSink,
) -> anyhow::Result<RunOutcome> {
    run_project_with_options(
        project_path,
        &RunOptions {
            input_path,
            output_path,
            trace_sink: Some(trace_sink),
            ..RunOptions::default()
        },
    )
}

/// Runs an in-memory project without requiring it to be saved first.
///
/// `project_path` supplies the base directory for relative resources and the
/// value exposed by mapping-path runtime nodes. The path itself may be a
/// not-yet-created file, which lets graphical hosts execute an unsaved draft.
pub fn run_project_value_with_paths(
    project: &mapping::Project,
    project_path: &Path,
    input_path: Option<&Path>,
    output_path: Option<&Path>,
) -> anyhow::Result<RunOutcome> {
    run_project_value_with_options(
        project,
        project_path,
        &RunOptions {
            input_path,
            output_path,
            ..RunOptions::default()
        },
    )
}

/// Runs an in-memory project and reports successful graph evaluations to
/// `trace_sink` in deterministic execution order.
pub fn run_project_value_with_paths_and_trace(
    project: &mapping::Project,
    project_path: &Path,
    input_path: Option<&Path>,
    output_path: Option<&Path>,
    trace_sink: &dyn TraceSink,
) -> anyhow::Result<RunOutcome> {
    run_project_value_with_options(
        project,
        project_path,
        &RunOptions {
            input_path,
            output_path,
            trace_sink: Some(trace_sink),
            ..RunOptions::default()
        },
    )
}

/// Runs an in-memory project with one unified host execution contract.
pub fn run_project_value_with_options(
    project: &mapping::Project,
    project_path: &Path,
    options: &RunOptions<'_>,
) -> anyhow::Result<RunOutcome> {
    run_project_value_with_filesystem_policy(project, project_path, options, None)
}

/// Previews an in-memory project against its filesystem inputs.
///
/// Inputs, including declared SQLite queries, are acquired exactly as by
/// [`run_project_value_with_options`]; each query connection is closed before
/// evaluation. Targets are returned as serialized in-memory artifacts and no
/// file is staged or published.
pub fn preview_project_value_from_files(
    project: &mapping::Project,
    project_path: &Path,
    options: &RunOptions<'_>,
) -> anyhow::Result<PayloadRunOutcome> {
    PreparedFileRun::prepare_preview(project, project_path, options.request())?
        .preview(&options.observers())
}

/// Like [`preview_project_value_from_files`], with a caller-owned reader for
/// configured filesystem PDF inputs. The reader is used exactly where a file
/// run would read a PDF: the primary once, selected static sources, and
/// dynamic sources the mapping requests.
pub fn preview_project_value_from_files_with_pdf_reader(
    project: &mapping::Project,
    project_path: &Path,
    options: &RunOptions<'_>,
    reader: &dyn FilesystemPdfReader,
) -> anyhow::Result<PayloadRunOutcome> {
    PreparedFileRun::prepare_preview(project, project_path, options.request())?
        .preview_with_pdf_reader(&options.observers(), reader)
}

/// Loads a filesystem project with a caller-owned PDF reader.
pub fn run_project_with_options_and_pdf_reader(
    project_path: &Path,
    options: &RunOptions<'_>,
    reader: &dyn FilesystemPdfReader,
) -> anyhow::Result<RunOutcome> {
    let project = load_project(project_path)?;
    run_project_value_with_options_and_pdf_reader(&project, project_path, options, reader)
}

/// Runs an in-memory project with a borrowed reader for configured filesystem PDF inputs.
pub fn run_project_value_with_options_and_pdf_reader(
    project: &mapping::Project,
    project_path: &Path,
    options: &RunOptions<'_>,
    reader: &dyn FilesystemPdfReader,
) -> anyhow::Result<RunOutcome> {
    run_project_value_with_pdf_reader(project, project_path, options, Some(reader))
}

/// Loads a filesystem project with an explicit owned-PDF policy. Payload and
/// default pipeline entrypoints retain their existing in-process behavior.
pub fn run_project_with_options_and_filesystem_policy(
    project_path: &Path,
    options: &RunOptions<'_>,
    policy: &FilesystemRunPolicy,
) -> anyhow::Result<RunOutcome> {
    let project = load_project(project_path)?;
    run_project_value_with_options_and_filesystem_policy(&project, project_path, options, policy)
}

pub fn run_project_value_with_options_and_filesystem_policy(
    project: &mapping::Project,
    project_path: &Path,
    options: &RunOptions<'_>,
    policy: &FilesystemRunPolicy,
) -> anyhow::Result<RunOutcome> {
    run_project_value_with_filesystem_policy(project, project_path, options, Some(policy))
}

pub(crate) fn run_project_value_with_filesystem_policy(
    project: &mapping::Project,
    project_path: &Path,
    options: &RunOptions<'_>,
    policy: Option<&FilesystemRunPolicy>,
) -> anyhow::Result<RunOutcome> {
    run_project_value_with_pdf_reader(
        project,
        project_path,
        options,
        policy.map(|policy| policy as &dyn FilesystemPdfReader),
    )
}

fn run_project_value_with_pdf_reader(
    project: &mapping::Project,
    project_path: &Path,
    options: &RunOptions<'_>,
    reader: Option<&dyn FilesystemPdfReader>,
) -> anyhow::Result<RunOutcome> {
    PreparedFileRun::prepare(project, project_path, options.request())?
        .execute_with_optional_pdf_reader(&options.observers(), reader)
}

/// A filesystem run whose paths, targets and capabilities are fixed.
///
/// Preparation reads nothing but the already loaded design: it resolves every
/// input and output identity, plans the targets, admits the observers, and
/// applies the execution policy to every known location before the first input
/// is acquired. Execution then acquires, evaluates, and publishes atomically.
pub struct PreparedFileRun<'a> {
    project: &'a mapping::Project,
    project_path: &'a Path,
    request: RunRequest<'a>,
    input_path: PathBuf,
    targets: Vec<PlannedProjectTarget<'a>>,
    /// None loads every static named source; Some loads only those listed.
    required_static_names: Option<BTreeSet<&'a str>>,
    /// Declared SQLite queries, validated and bound before any acquisition.
    primary_query: Option<PreparedQuery>,
    named_queries: BTreeMap<&'a str, PreparedQuery>,
    delivery: Delivery,
    plan: RoutePlan,
}

/// Whether a prepared run publishes files or returns in-memory artifacts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Files,
    Preview,
}

/// Evaluated target values handed to the delivery step after every input,
/// including each declared query connection, has been released.
enum Evaluated {
    Selected(engine::SelectedTargetOutput),
    All(engine::ExecutionOutputs),
}

impl<'a> PreparedFileRun<'a> {
    pub fn prepare(
        project: &'a mapping::Project,
        project_path: &'a Path,
        request: RunRequest<'a>,
    ) -> anyhow::Result<Self> {
        Self::prepare_delivery(project, project_path, request, Delivery::Files)
    }

    /// Prepares a filesystem preview: inputs are acquired exactly as for
    /// [`Self::prepare`], but targets are rendered in memory and nothing is
    /// published, so target locations need no write authority.
    pub fn prepare_preview(
        project: &'a mapping::Project,
        project_path: &'a Path,
        request: RunRequest<'a>,
    ) -> anyhow::Result<Self> {
        Self::prepare_delivery(project, project_path, request, Delivery::Preview)
    }

    fn prepare_delivery(
        project: &'a mapping::Project,
        project_path: &'a Path,
        request: RunRequest<'a>,
        delivery: Delivery,
    ) -> anyhow::Result<Self> {
        require_valid(project)?;
        let policy = request.policy();

        let input_path = resolve_run_path(
            project_path,
            request.input_path,
            project.source_path.as_deref(),
            "input",
            "source_path",
            true,
        )?;
        let targets =
            plan_project_targets(project, project_path, request.output_path, request.target)?;
        crate::output_documents::reject_protected_static_targets(
            &targets,
            request.protected_output_paths,
        )?;
        if delivery == Delivery::Files {
            for target in &targets {
                policy.authorize_write(target.destination.path())?;
            }
        }
        engine::check_source_ownership_capability_with_observers(
            project,
            request.target,
            request.observers,
        )?;
        let required_sources = request
            .target
            .map(|selection| engine::required_sources_for_target(project, selection))
            .transpose()?;
        let required_static_names = required_sources.as_ref().map(|sources| {
            sources
                .static_sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<BTreeSet<_>>()
        });
        if targets.is_empty() {
            bail!("project run selected no targets");
        }

        let mut plan = RoutePlan::new(HostRoute::Run, project_path);
        plan.acquire_location(InputRole::Primary, &input_path, &project.source_options);
        for extra in static_sources(project, required_static_names.as_ref()) {
            plan.acquire_location(
                InputRole::Named(extra.name.clone()),
                &named_source_path(project_path, extra),
                &extra.options,
            );
        }
        let dynamic = match &required_sources {
            Some(sources) => sources
                .dynamic_sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            None => project
                .extra_sources
                .iter()
                .filter(|source| source.dynamic_path.is_some())
                .map(|source| source.name.as_str())
                .collect(),
        };
        for name in dynamic {
            plan.acquire_dynamic(name);
        }
        for target in &targets {
            plan.publish(match (&target.destination, delivery) {
                (_, Delivery::Preview) => PlannedPublication::Payload {
                    target: target.name.to_owned(),
                },
                (OutputDestination::Static(path), Delivery::Files) => PlannedPublication::File {
                    target: target.name.to_owned(),
                    path: path.clone(),
                },
                (OutputDestination::DynamicBase(base), Delivery::Files) => {
                    PlannedPublication::Documents {
                        target: target.name.to_owned(),
                        base: base.clone(),
                    }
                }
            });
        }
        plan.publish_host_artifacts(request.protected_output_paths);
        // Every known location is admitted before the first one is acquired;
        // each acquisition still checks its own location again.
        plan.authorize_reads(policy, |role| match role {
            InputRole::Named(name) => Some(format!("loading extra source `{name}`")),
            InputRole::Primary | InputRole::PipelineHost(_) => None,
        })?;

        // Every declared query and binding is checked before any input opens.
        let primary_query = project
            .source_options
            .sqlite_read
            .as_ref()
            .map(|profile| {
                PreparedQuery::prepare(
                    &InputRole::Primary,
                    &input_path,
                    &project.source,
                    profile,
                    request.runtime_parameters,
                )
            })
            .transpose()?;
        let mut named_queries = BTreeMap::new();
        for extra in static_sources(project, required_static_names.as_ref()) {
            if let Some(profile) = &extra.options.sqlite_read {
                let query = PreparedQuery::prepare(
                    &InputRole::Named(extra.name.clone()),
                    &named_source_path(project_path, extra),
                    &extra.schema,
                    profile,
                    request.runtime_parameters,
                )?;
                named_queries.insert(extra.name.as_str(), query);
            }
        }

        Ok(Self {
            project,
            project_path,
            request,
            input_path,
            targets,
            required_static_names,
            primary_query,
            named_queries,
            delivery,
            plan,
        })
    }

    pub fn plan(&self) -> &RoutePlan {
        &self.plan
    }

    pub fn execute(self, observers: &RunObservers<'_>) -> anyhow::Result<RunOutcome> {
        self.execute_with_optional_pdf_reader(observers, None)
    }

    /// Executes with a caller-owned reader for configured filesystem PDF inputs.
    pub fn execute_with_pdf_reader(
        self,
        observers: &RunObservers<'_>,
        reader: &dyn FilesystemPdfReader,
    ) -> anyhow::Result<RunOutcome> {
        self.execute_with_optional_pdf_reader(observers, Some(reader))
    }

    fn execute_with_optional_pdf_reader(
        self,
        observers: &RunObservers<'_>,
        reader: Option<&dyn FilesystemPdfReader>,
    ) -> anyhow::Result<RunOutcome> {
        if self.delivery != Delivery::Files {
            bail!("a prepared preview returns in-memory artifacts; use `preview`");
        }
        let input_path = self.input_path.clone();
        let output_path = self.targets[0].destination.path().to_path_buf();
        let request = self.request;
        let policy = request.policy();
        let (targets, evaluated, current_datetime) = self.evaluate(observers, reader)?;
        let written = match &evaluated {
            Evaluated::Selected(output) => write_selected_target(
                &targets[0],
                output,
                &current_datetime,
                request.protected_output_paths,
                policy,
            )?,
            Evaluated::All(outputs) => write_all_targets(
                &targets,
                outputs,
                &current_datetime,
                request.protected_output_paths,
                policy,
            )?,
        };

        Ok(RunOutcome {
            records_written: written.records,
            input_path,
            output_path,
            primary_outputs: written.primary_outputs,
            extra_outputs: written.extra_outputs,
            artifacts: written.artifacts,
        })
    }

    /// Acquires and evaluates like [`Self::execute`], then renders every
    /// planned target in memory. Nothing is staged or published.
    pub fn preview(self, observers: &RunObservers<'_>) -> anyhow::Result<PayloadRunOutcome> {
        self.preview_with_optional_pdf_reader(observers, None)
    }

    /// Previews with a caller-owned reader for configured filesystem PDF inputs.
    pub fn preview_with_pdf_reader(
        self,
        observers: &RunObservers<'_>,
        reader: &dyn FilesystemPdfReader,
    ) -> anyhow::Result<PayloadRunOutcome> {
        self.preview_with_optional_pdf_reader(observers, Some(reader))
    }

    fn preview_with_optional_pdf_reader(
        self,
        observers: &RunObservers<'_>,
        reader: Option<&dyn FilesystemPdfReader>,
    ) -> anyhow::Result<PayloadRunOutcome> {
        if self.delivery != Delivery::Preview {
            bail!("a prepared file run publishes its targets; use `prepare_preview`");
        }
        let project = self.project;
        let project_path = self.project_path;
        let output_path = self.request.output_path;
        let (_, evaluated, current_datetime) = self.evaluate(observers, reader)?;
        match &evaluated {
            Evaluated::Selected(output) => crate::payload::render_selected_outcome(
                project,
                project_path,
                output_path,
                output,
                &current_datetime,
            ),
            Evaluated::All(outputs) => crate::payload::render_all_outcome(
                project,
                project_path,
                output_path,
                outputs,
                &current_datetime,
            ),
        }
    }

    /// Acquires every input in plan order, evaluates the selected targets and
    /// releases all inputs before returning their values.
    fn evaluate(
        self,
        observers: &RunObservers<'_>,
        reader: Option<&dyn FilesystemPdfReader>,
    ) -> anyhow::Result<(Vec<PlannedProjectTarget<'a>>, Evaluated, String)> {
        observers.require_declared(self.request.observers)?;
        let Self {
            project,
            project_path,
            request,
            input_path,
            targets,
            required_static_names,
            primary_query,
            named_queries,
            delivery: _,
            plan: _,
        } = self;
        let policy = request.policy();

        check_cancelled(observers.cancellation)?;
        let source_instance = match &primary_query {
            Some(query) => {
                query.acquire(&input_path, &project.source, policy, observers.cancellation)?
            }
            None => read_instance_with_pdf_reader(
                &input_path,
                &project.source,
                &project.source_options,
                reader,
                policy,
            )?,
        };
        let mut read_set = read_set::ReadSet::new(project_path);
        read_set.push_source(&input_path, &source_instance);

        let project_dir = project_path.parent().unwrap_or_else(|| Path::new("."));
        let mut extras = Vec::with_capacity(project.extra_sources.len());
        for extra in static_sources(project, required_static_names.as_ref()) {
            check_cancelled(observers.cancellation)?;
            let path = named_source_path(project_path, extra);
            let instance = match named_queries.get(extra.name.as_str()) {
                Some(query) => query.acquire(&path, &extra.schema, policy, observers.cancellation),
                None => read_instance_with_pdf_reader(
                    &path,
                    &extra.schema,
                    &extra.options,
                    reader,
                    policy,
                ),
            }
            .with_context(|| format!("loading extra source `{}`", extra.name))?;
            read_set.push_source(&path, &instance);
            extras.push((extra.name.clone(), instance));
        }
        read_set.reject_protected(request.protected_output_paths)?;
        check_cancelled(observers.cancellation)?;

        let runtime_project_path = absolute_mapping_path(project_path)?;
        let current_datetime = jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
            .to_string();
        let dynamic_loader = ProjectDynamicSourceLoader::with_pdf_reader(
            project_dir,
            &project.extra_sources,
            reader,
        )
        .with_trace_sink(observers.trace_sink)
        .with_execution_policy(policy);
        let mut execution = engine::ExecutionContext::new(&runtime_project_path)
            .with_current_datetime(&current_datetime)
            .with_dynamic_source_loader(&dynamic_loader)
            .with_policy(policy);
        if let Some(parameters) = request.runtime_parameters {
            execution = execution.with_parameters(parameters);
        }
        if let Some(trace_sink) = observers.trace_sink {
            execution = execution.with_trace_sink(trace_sink);
        }
        let host_hook = observers.debug_hook.unwrap_or(&NoDebugHook);
        let cancellable = CancellableHook {
            inner: host_hook,
            cancellation: observers.cancellation,
        };
        // Without cancellation the host hook, if any, is attached unchanged.
        match (observers.cancellation, observers.debug_hook) {
            (Some(_), _) => execution = execution.with_debug_hook(&cancellable),
            (None, Some(debug_hook)) => execution = execution.with_debug_hook(debug_hook),
            (None, None) => {}
        }
        let evaluated = if let Some(selection) = request.target {
            Evaluated::Selected(engine::run_selected_target_with_sources_and_context(
                project,
                &source_instance,
                extras,
                &execution,
                selection,
            )?)
        } else {
            Evaluated::All(engine::run_outputs_with_sources_and_context(
                project,
                &source_instance,
                extras,
                &execution,
            )?)
        };
        allow_output_publication(&request, observers, &mut read_set, &dynamic_loader)?;
        // Target values own their data; inputs are no longer needed during serialization.
        drop(dynamic_loader);
        drop(source_instance);
        Ok((targets, evaluated, current_datetime))
    }
}

/// Static named sources in declaration order, restricted to `required`.
fn static_sources<'p>(
    project: &'p mapping::Project,
    required: Option<&BTreeSet<&str>>,
) -> impl Iterator<Item = &'p mapping::NamedSource> {
    project.extra_sources.iter().filter(move |extra| {
        extra.dynamic_path.is_none()
            && required.is_none_or(|required| required.contains(extra.name.as_str()))
    })
}

fn named_source_path(project_path: &Path, extra: &mapping::NamedSource) -> PathBuf {
    let project_dir = project_path.parent().unwrap_or_else(|| Path::new("."));
    let path = PathBuf::from(&extra.path);
    if path.is_absolute() || http_url(&path).is_some() {
        path
    } else {
        project_dir.join(path)
    }
}

fn allow_output_publication(
    request: &RunRequest<'_>,
    observers: &RunObservers<'_>,
    read_set: &mut read_set::ReadSet,
    dynamic_loader: &ProjectDynamicSourceLoader<'_>,
) -> anyhow::Result<()> {
    read_set.push_dynamic_loads(dynamic_loader);
    read_set.reject_protected(request.protected_output_paths)?;
    check_cancelled(observers.cancellation)?;
    if observers.before_publish.is_some_and(|gate| !gate()) {
        return Err(engine::EngineError::DebugCancelled.into());
    }
    Ok(())
}

pub(crate) struct PlannedProjectTarget<'a> {
    name: &'a str,
    schema: &'a SchemaNode,
    options: &'a FormatOptions,
    pub(crate) destination: OutputDestination,
    primary: bool,
}

struct WrittenTargets {
    records: usize,
    primary_outputs: Vec<WrittenOutput>,
    extra_outputs: Vec<WrittenOutput>,
    artifacts: Vec<WrittenOutput>,
}

fn plan_project_targets<'a>(
    project: &'a mapping::Project,
    project_path: &Path,
    output_path: Option<&Path>,
    selection: Option<TargetSelection<'_>>,
) -> anyhow::Result<Vec<PlannedProjectTarget<'a>>> {
    match selection {
        None => {
            let mut targets = Vec::with_capacity(1 + project.extra_targets.len());
            targets.push(plan_primary_target(project, project_path, output_path)?);
            for target in &project.extra_targets {
                targets.push(plan_named_target(target, project_path, None)?);
            }
            Ok(targets)
        }
        Some(TargetSelection::Primary) => Ok(vec![plan_primary_target(
            project,
            project_path,
            output_path,
        )?]),
        Some(TargetSelection::Named(name)) => {
            let mut matches = project
                .extra_targets
                .iter()
                .filter(|target| target.name == name);
            let target = matches
                .next()
                .ok_or_else(|| engine::EngineError::UnknownTarget {
                    name: name.to_string(),
                })?;
            if matches.next().is_some() {
                return Err(engine::EngineError::AmbiguousTarget {
                    name: name.to_string(),
                }
                .into());
            }
            Ok(vec![plan_named_target(target, project_path, output_path)?])
        }
    }
}

fn plan_primary_target<'a>(
    project: &'a mapping::Project,
    project_path: &Path,
    output_path: Option<&Path>,
) -> anyhow::Result<PlannedProjectTarget<'a>> {
    let destination = if project.root.produces_documents() {
        OutputDestination::DynamicBase(output_path.map(Path::to_path_buf).unwrap_or_else(|| {
            project_path
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf()
        }))
    } else {
        OutputDestination::Static(resolve_run_path(
            project_path,
            output_path,
            project.target_path.as_deref(),
            "output",
            "target_path",
            false,
        )?)
    };
    Ok(PlannedProjectTarget {
        name: &project.target.name,
        schema: &project.target,
        options: &project.target_options,
        destination,
        primary: true,
    })
}

fn plan_named_target<'a>(
    target: &'a mapping::NamedTarget,
    project_path: &Path,
    output_path: Option<&Path>,
) -> anyhow::Result<PlannedProjectTarget<'a>> {
    let destination = if target.root.produces_documents() {
        OutputDestination::DynamicBase(output_path.map(Path::to_path_buf).unwrap_or_else(|| {
            project_path
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf()
        }))
    } else {
        let path = match output_path {
            Some(path) => path.to_path_buf(),
            None => {
                let stored = target
                    .path
                    .as_deref()
                    .filter(|path| !path.trim().is_empty())
                    .with_context(|| {
                        format!("extra target `{}` has no stored output path", target.name)
                    })?;
                resolve_stored_path(project_path, stored, false)
                    .with_context(|| format!("resolving extra target `{}` output", target.name))?
            }
        };
        OutputDestination::Static(path)
    };
    Ok(PlannedProjectTarget {
        name: &target.name,
        schema: &target.schema,
        options: &target.options,
        destination,
        primary: false,
    })
}

fn write_all_targets(
    targets: &[PlannedProjectTarget<'_>],
    outputs: &engine::ExecutionOutputs,
    current_datetime: &str,
    protected_output_paths: &[&Path],
    policy: &ExecutionPolicy,
) -> anyhow::Result<WrittenTargets> {
    if outputs.extras.len() + 1 != targets.len() {
        bail!("engine returned an unexpected number of target values");
    }
    let mut writes = Vec::with_capacity(targets.len());
    writes.push(target_write(
        &targets[0],
        &outputs.primary,
        current_datetime,
    ));
    for (target, output) in targets.iter().skip(1).zip(&outputs.extras) {
        writes.push(target_write(target, &output.instance, current_datetime));
    }
    let mut written = write_target_outputs(&writes, protected_output_paths, policy)?.into_iter();
    let primary_result = written
        .next()
        .context("output batch did not return a primary target result")?;
    let row_count = primary_result.records_written;
    let mut primary_outputs = primary_result.outputs;
    let mut artifacts = primary_outputs.clone();
    if matches!(targets[0].destination, OutputDestination::Static(_)) {
        primary_outputs.clear();
    }
    let extra_outputs = written
        .flat_map(|target| target.outputs)
        .collect::<Vec<_>>();
    artifacts.extend(extra_outputs.iter().cloned());
    Ok(WrittenTargets {
        records: row_count,
        primary_outputs,
        extra_outputs,
        artifacts,
    })
}

fn write_selected_target(
    target: &PlannedProjectTarget<'_>,
    output: &engine::SelectedTargetOutput,
    current_datetime: &str,
    protected_output_paths: &[&Path],
    policy: &ExecutionPolicy,
) -> anyhow::Result<WrittenTargets> {
    let instance = match (target.primary, output) {
        (true, engine::SelectedTargetOutput::Primary(instance)) => instance,
        (false, engine::SelectedTargetOutput::Named(output)) if output.name == target.name => {
            &output.instance
        }
        _ => bail!("engine returned a different target than requested"),
    };
    let mut written = write_target_outputs(
        &[target_write(target, instance, current_datetime)],
        protected_output_paths,
        policy,
    )?
    .into_iter();
    let result = written
        .next()
        .context("output batch did not return the selected target result")?;
    let records_written = result.records_written;
    let artifacts = result.outputs;
    let (primary_outputs, extra_outputs) = if target.primary {
        let dynamic = matches!(target.destination, OutputDestination::DynamicBase(_))
            .then(|| artifacts.clone())
            .unwrap_or_default();
        (dynamic, Vec::new())
    } else {
        (Vec::new(), artifacts.clone())
    };
    Ok(WrittenTargets {
        records: records_written,
        primary_outputs,
        extra_outputs,
        artifacts,
    })
}

fn target_write<'a>(
    target: &'a PlannedProjectTarget<'_>,
    instance: &'a Instance,
    current_datetime: &'a str,
) -> TargetOutput<'a> {
    TargetOutput {
        destination: &target.destination,
        name: target.name,
        schema: target.schema,
        instance,
        options: target.options,
        current_datetime,
        additional: !target.primary,
    }
}
