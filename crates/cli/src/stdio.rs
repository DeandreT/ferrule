use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use crate::file_run::RunRequest;
use crate::read_set::ReadSet;
use crate::route_capability::{
    HostRoute, InputRole, PlannedAcquisition, PlannedPublication, RoutePlan, RouteRefusal,
};
use crate::{
    MAX_PAYLOAD_DOCUMENT_BYTES, NamedPayloadInput, PayloadDocument, PayloadRunOptions,
    PayloadRunOutcome, TargetSelection, TraceSink, load_project, resolve_run_path,
    resolve_stored_path, run_project_value_payloads,
};

/// Stream-specific overrides for one `ferrule run` execution.
///
/// A path equal to `-` selects the corresponding standard stream. The primary
/// source may use stdin, while stdout is reserved for exactly one serialized
/// target artifact. Secondary inputs always retain their configured filesystem
/// paths.
#[derive(Default)]
pub struct StandardIoRunOptions<'a> {
    pub input_path: Option<&'a Path>,
    pub output_path: Option<&'a Path>,
    pub target: Option<TargetSelection<'a>>,
    pub runtime_parameters: Option<&'a engine::RuntimeParameters>,
    pub trace_sink: Option<&'a dyn TraceSink>,
    /// Host-owned artifact paths, such as a trace, that the host replaces
    /// after the run. The run refuses when one names the design or a consumed
    /// local input.
    pub protected_output_paths: &'a [&'a Path],
    /// Host authority for local input files and budgets; `None` is trusted.
    /// Standard streams themselves are host-owned and never checked.
    pub execution_policy: Option<&'a engine::ExecutionPolicy>,
}

impl<'a> StandardIoRunOptions<'a> {
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

    pub fn with_protected_output_paths(mut self, paths: &'a [&'a Path]) -> Self {
        self.protected_output_paths = paths;
        self
    }
}

/// Runs a payload-compatible mapping intended for one stdout artifact.
///
/// `output_path` must be `-`. `input_path` may be `-` to read the primary
/// source from `stdin`, or a normal path to stream a filesystem input to
/// stdout. The project retains responsibility for named static sources; they
/// are read from their configured ordinary paths. Dynamic named sources and
/// persistent SQLite/update-existing XLSX operations reject before output is
/// written. The returned outcome contains exactly one artifact, which the
/// caller publishes only after related durable outputs, such as a trace, are
/// finalized successfully.
pub fn run_project_with_standard_streams(
    project_path: &Path,
    options: &StandardIoRunOptions<'_>,
    stdin: &mut dyn Read,
) -> anyhow::Result<PayloadRunOutcome> {
    if options.output_path != Some(Path::new("-")) {
        return Err(RouteRefusal::StandardStreamsRequireStdout.into());
    }
    let project = load_project(project_path)?;
    let request = RunRequest {
        input_path: options.input_path,
        output_path: options.output_path,
        target: options.target,
        runtime_parameters: options.runtime_parameters,
        protected_output_paths: options.protected_output_paths,
        execution_policy: options.execution_policy,
        observers: engine::SourceOwnershipObservers {
            tracing: options.trace_sink.is_some(),
            debugging: false,
        },
    };
    PreparedStreamRun::prepare(&project, project_path, request)?.execute(stdin, options.trace_sink)
}

/// A standard-stream run whose identities, named inputs and capabilities are
/// fixed before stdin or any named input is read.
pub struct PreparedStreamRun<'a> {
    project: &'a mapping::Project,
    project_path: &'a Path,
    request: RunRequest<'a>,
    /// None reads the primary document from stdin.
    input_file: Option<PathBuf>,
    input_identity: PathBuf,
    output_identity: PathBuf,
    named_sources: Vec<(&'a str, PathBuf)>,
    plan: RoutePlan,
}

impl<'a> PreparedStreamRun<'a> {
    pub fn prepare(
        project: &'a mapping::Project,
        project_path: &'a Path,
        request: RunRequest<'a>,
    ) -> anyhow::Result<Self> {
        if request.output_path != Some(Path::new("-")) {
            return Err(RouteRefusal::StandardStreamsRequireStdout.into());
        }
        if request.observers.debugging {
            return Err(RouteRefusal::StandardStreamsDebugging.into());
        }
        // Stream acquisition happens before payload decoding. A retained MFD
        // dependency must refuse here, not after stdin or named files are read.
        for options in std::iter::once(&project.source_options)
            .chain(std::iter::once(&project.target_options))
            .chain(project.extra_sources.iter().map(|source| &source.options))
            .chain(project.extra_targets.iter().map(|target| &target.options))
        {
            if let Some(dependency) = &options.json5_interchange_dependency {
                return Err(dependency.clone().into());
            }
        }
        crate::sqlite_query::reject_query_inputs(project, HostRoute::StandardStreams)?;
        reject_dynamic_sources(project, request.target)?;
        let policy = request
            .execution_policy
            .unwrap_or(&crate::execution_policy::TRUSTED);

        let input_from_stdin = request.input_path == Some(Path::new("-"));
        let input_identity = if input_from_stdin {
            stream_source_identity(project_path, project)?
        } else {
            resolve_run_path(
                project_path,
                request.input_path,
                project.source_path.as_deref(),
                "input",
                "source_path",
                true,
            )?
        };
        let output_identity = stream_target_identity(project_path, project, request.target)?;
        engine::check_source_ownership_capability_with_observers(
            project,
            request.target,
            request.observers,
        )?;
        let named_sources = static_extra_source_paths(project_path, project, request.target)?;

        let mut plan = RoutePlan::new(HostRoute::StandardStreams, project_path);
        if input_from_stdin {
            plan.acquire(PlannedAcquisition::Stdin {
                identity: input_identity.clone(),
            });
        } else {
            plan.acquire_location(
                InputRole::Primary,
                &input_identity,
                &mapping::FormatOptions::default(),
            );
        }
        for (name, path) in &named_sources {
            plan.acquire_location(
                InputRole::Named((*name).to_owned()),
                path,
                &mapping::FormatOptions::default(),
            );
        }
        plan.publish(PlannedPublication::Stdout {
            target: match request.target {
                Some(TargetSelection::Named(name)) => name.to_owned(),
                Some(TargetSelection::Primary) | None => project.target.name.clone(),
            },
            identity: output_identity.clone(),
        });
        plan.publish_host_artifacts(request.protected_output_paths);
        // Streams carry raw bytes, so each location is the exact file read.
        plan.authorize_reads(policy, |_| None)?;

        Ok(Self {
            project,
            project_path,
            request,
            input_file: (!input_from_stdin).then(|| input_identity.clone()),
            input_identity,
            output_identity,
            named_sources,
            plan,
        })
    }

    pub fn plan(&self) -> &RoutePlan {
        &self.plan
    }

    /// Reads the primary document from `stdin` when the request selected it,
    /// then the named inputs, and returns exactly one serialized artifact.
    pub fn execute(
        self,
        stdin: &mut dyn Read,
        trace_sink: Option<&dyn TraceSink>,
    ) -> anyhow::Result<PayloadRunOutcome> {
        if trace_sink.is_some() && !self.request.observers.tracing {
            return Err(RouteRefusal::UndeclaredObservers.into());
        }
        let Self {
            project,
            project_path,
            request,
            input_file,
            input_identity,
            output_identity,
            named_sources,
            plan: _,
        } = self;
        let policy = request
            .execution_policy
            .unwrap_or(&crate::execution_policy::TRUSTED);
        let primary_bytes = match &input_file {
            None => read_bounded(stdin, "stdin")?,
            Some(path) => {
                policy.authorize_read(path)?;
                read_path_bounded(path, "primary input")?
            }
        };
        let primary = PayloadDocument::new(&input_identity, &primary_bytes)?;

        let extra_storage = named_sources
            .into_iter()
            .map(|(name, path)| {
                policy.authorize_read(&path)?;
                let bytes = read_path_bounded(&path, &format!("named source `{name}`"))?;
                Ok(ExtraSourceBytes {
                    name: name.to_owned(),
                    path,
                    bytes,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let mut read_set = ReadSet::new(project_path);
        if let Some(path) = &input_file {
            read_set.push(path);
        }
        for source in &extra_storage {
            read_set.push(&source.path);
        }
        read_set.reject_protected(request.protected_output_paths)?;
        let extra_documents = extra_storage
            .iter()
            .map(|source| PayloadDocument::new(&source.path, &source.bytes))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let extra_sources = extra_storage
            .iter()
            .zip(&extra_documents)
            .map(|(source, document)| NamedPayloadInput::new(&source.name, *document))
            .collect::<anyhow::Result<Vec<_>>>()?;

        let mut payload_options = PayloadRunOptions::new(primary)
            .with_extra_sources(&extra_sources)
            .with_output_path(&output_identity)
            .with_execution_policy(policy);
        if let Some(target) = request.target {
            payload_options = payload_options.with_target(target);
        }
        if let Some(parameters) = request.runtime_parameters {
            payload_options = payload_options.with_runtime_parameters(parameters);
        }
        if let Some(trace_sink) = trace_sink {
            payload_options = payload_options.with_trace_sink(trace_sink);
        }
        let outcome = run_project_value_payloads(project, project_path, &payload_options)?;
        if !matches!(outcome.artifacts.as_slice(), [_]) {
            bail!(
                "stdout requires exactly one output artifact, but this mapping produced {}",
                outcome.artifacts.len()
            );
        }
        Ok(outcome)
    }
}

struct ExtraSourceBytes {
    name: String,
    path: PathBuf,
    bytes: Vec<u8>,
}

fn stream_source_identity(
    project_path: &Path,
    project: &mapping::Project,
) -> anyhow::Result<PathBuf> {
    let configured = project.source_path.as_deref().with_context(|| {
        format!(
            "stdin needs `source_path` in {} to identify its input format",
            project_path.display()
        )
    })?;
    resolve_stored_path(project_path, configured, true)
}

fn stream_target_identity(
    project_path: &Path,
    project: &mapping::Project,
    target: Option<TargetSelection<'_>>,
) -> anyhow::Result<PathBuf> {
    let (configured, label) = match target {
        Some(TargetSelection::Primary) | None => (project.target_path.as_deref(), "target_path"),
        Some(TargetSelection::Named(name)) => (
            project
                .extra_targets
                .iter()
                .find(|candidate| candidate.name == name)
                .with_context(|| format!("unknown target `{name}`"))?
                .path
                .as_deref(),
            "named target path",
        ),
    };
    let configured = configured.with_context(|| {
        format!(
            "stdout needs a configured {label} in {} to identify its output format",
            project_path.display()
        )
    })?;
    resolve_stored_path(project_path, configured, false)
}

fn reject_dynamic_sources(
    project: &mapping::Project,
    target: Option<TargetSelection<'_>>,
) -> anyhow::Result<()> {
    let dynamic = match target {
        Some(target) => engine::required_sources_for_target(project, target)?
            .dynamic_sources
            .into_iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>(),
        None => project
            .extra_sources
            .iter()
            .filter(|source| source.dynamic_path.is_some())
            .map(|source| source.name.as_str())
            .collect(),
    };
    if dynamic.is_empty() {
        return Ok(());
    }
    Err(RouteRefusal::StandardStreamsDynamicSources {
        names: dynamic.into_iter().map(str::to_owned).collect(),
    }
    .into())
}

/// Resolves every required static named source to a local path.
fn static_extra_source_paths<'p>(
    project_path: &Path,
    project: &'p mapping::Project,
    target: Option<TargetSelection<'_>>,
) -> anyhow::Result<Vec<(&'p str, PathBuf)>> {
    let required = target
        .map(|target| engine::required_sources_for_target(project, target))
        .transpose()?;
    let required_names = required.as_ref().map(|sources| {
        sources
            .static_sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<BTreeSet<_>>()
    });
    project
        .extra_sources
        .iter()
        .filter(|source| {
            source.dynamic_path.is_none()
                && required_names
                    .as_ref()
                    .is_none_or(|names| names.contains(source.name.as_str()))
        })
        .map(|source| {
            let path = resolve_stored_path(project_path, &source.path, true)?;
            if crate::http_url(&path).is_some() {
                bail!(
                    "standard-stream execution requires named source `{}` to use a local path",
                    source.name
                );
            }
            Ok((source.name.as_str(), path))
        })
        .collect()
}

fn read_path_bounded(path: &Path, label: &str) -> anyhow::Result<Vec<u8>> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {label} {}", path.display()))?;
    read_bounded(&mut file, &format!("{label} {}", path.display()))
}

fn read_bounded(input: &mut dyn Read, label: &str) -> anyhow::Result<Vec<u8>> {
    let limit = u64::try_from(MAX_PAYLOAD_DOCUMENT_BYTES)
        .context("payload document limit does not fit in u64")?;
    let mut bytes = Vec::new();
    input
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {label}"))?;
    if bytes.len() > MAX_PAYLOAD_DOCUMENT_BYTES {
        bail!(
            "{label} exceeds the {} MiB per-document payload limit",
            MAX_PAYLOAD_DOCUMENT_BYTES / (1024 * 1024)
        );
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn project() -> mapping::Project {
        let text = include_str!("../tests/fixtures/project.json");
        serde_json::from_str(text).unwrap_or_else(|error| panic!("fixture project: {error}"))
    }

    fn test_directory(name: &str) -> anyhow::Result<PathBuf> {
        let path =
            std::env::temp_dir().join(format!("ferrule_cli_stdio_{name}_{}", std::process::id()));
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    #[test]
    fn stdin_stdout_uses_configured_paths_as_format_identity() -> anyhow::Result<()> {
        let mut project = project();
        project.source_path = Some("input.csv".into());
        project.target_path = Some("output.csv".into());
        let directory = test_directory("stdin_stdout")?;
        let project_path = directory.join("mapping.json");
        std::fs::write(&project_path, serde_json::to_vec(&project)?)?;
        let mut input = Cursor::new(b"first_name,last_name,age\nJane,Doe,29\n".to_vec());
        let outcome = run_project_with_standard_streams(
            &project_path,
            &StandardIoRunOptions::new()
                .with_input_path(Path::new("-"))
                .with_output_path(Path::new("-")),
            &mut input,
        )?;

        assert_eq!(outcome.artifacts.len(), 1);
        assert_eq!(
            outcome.artifacts[0].bytes,
            b"full_name,age_next_year\nJane Doe,30\n"
        );
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn stdout_rejects_multiple_artifacts_before_writing() -> anyhow::Result<()> {
        let mut project = project();
        project.source_path = Some("input.csv".into());
        project.target_path = Some("output.csv".into());
        project.extra_targets.push(mapping::NamedTarget {
            name: "second".into(),
            path: Some("second.csv".into()),
            schema: project.target.clone(),
            options: mapping::FormatOptions::default(),
            root: project.root.clone(),
        });
        let directory = test_directory("multiple_artifacts")?;
        let project_path = directory.join("mapping.json");
        std::fs::write(&project_path, serde_json::to_vec(&project)?)?;
        let mut input = Cursor::new(b"first_name,last_name,age\nJane,Doe,29\n".to_vec());
        let error = run_project_with_standard_streams(
            &project_path,
            &StandardIoRunOptions::new()
                .with_input_path(Path::new("-"))
                .with_output_path(Path::new("-")),
            &mut input,
        )
        .unwrap_err();

        assert!(error.to_string().contains("exactly one output artifact"));
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }
}

#[cfg(test)]
mod owned_source_stdio_admission_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn owned_refusal_leaves_stdin_unread() -> anyhow::Result<()> {
        struct Input(usize);
        impl Read for Input {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                self.0 += 1;
                panic!("stdin must remain unread")
            }
        }
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "ferrule-owned-stdio-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory)?;
        let path = directory.join("project.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&crate::closed_owned_source_test_project())?,
        )?;
        let mut input = Input(0);
        let error = run_project_with_standard_streams(
            &path,
            &StandardIoRunOptions::new()
                .with_input_path(Path::new("-"))
                .with_output_path(Path::new("-")),
            &mut input,
        )
        .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<engine::EngineError>(),
            Some(engine::EngineError::UnsupportedSourceOwnership { node: 9, function: Some(function), .. }) if *function == mapping::FunctionId::new(7)
        ));
        assert_eq!(input.0, 0);
        std::fs::remove_file(path)?;
        std::fs::remove_dir(directory)?;
        Ok(())
    }
}

#[cfg(test)]
mod owned_collection_stdio_admission_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn closed_source_function_refusal_with_owned_iteration_leaves_stdin_unread()
    -> anyhow::Result<()> {
        struct Input(usize);
        impl Read for Input {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                self.0 += 1;
                panic!("stdin must remain unread")
            }
        }
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "ferrule-owned-collection-stdio-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory)?;
        let path = directory.join("project.json");
        std::fs::write(
            &path,
            serde_json::to_vec(
                &crate::owned_collection_with_closed_source_function_test_project(),
            )?,
        )?;
        let mut input = Input(0);
        let error = run_project_with_standard_streams(
            &path,
            &StandardIoRunOptions::new()
                .with_input_path(Path::new("-"))
                .with_output_path(Path::new("-")),
            &mut input,
        )
        .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<engine::EngineError>(),
            Some(engine::EngineError::UnsupportedSourceOwnership {
                node: 9,
                function: Some(function),
                kind: "owned_source_field"
            }) if *function == mapping::FunctionId::new(7)
        ));
        assert_eq!(input.0, 0);
        std::fs::remove_file(path)?;
        std::fs::remove_dir(directory)?;
        Ok(())
    }
}

#[cfg(test)]
mod owned_basic_stdio_transition_tests {
    use super::*;
    #[test]
    fn basic_field_and_collection_read_stdin_and_preserve_output_policy() -> anyhow::Result<()> {
        for collection in [false, true] {
            let directory = crate::owned_host_test_directory("stdio")?;
            let path = directory.join("project.json");
            let project = if collection {
                crate::owned_collection_host_positive_project()
            } else {
                crate::owned_source_test_project()
            };
            std::fs::write(&path, serde_json::to_vec(&project)?)?;
            let bytes: &[u8] = if collection {
                br#"[{"Value":"one"},{"Value":"two"}]"#
            } else {
                br#"{"Value":"supported"}"#
            };
            let options = StandardIoRunOptions::new()
                .with_input_path(Path::new("-"))
                .with_output_path(Path::new("-"));
            let mut input = std::io::Cursor::new(bytes);
            let outcome = run_project_with_standard_streams(&path, &options, &mut input)?;
            assert_eq!(input.position() as usize, bytes.len());
            assert_eq!(outcome.artifacts.len(), 1);
            assert_eq!(outcome.records_written, if collection { 2 } else { 1 });
            let actual: serde_json::Value = serde_json::from_slice(&outcome.artifacts[0].bytes)?;
            let expected = if collection {
                serde_json::json!([{"Value":"legacy"},{"Value":"legacy"}])
            } else {
                serde_json::json!({"Value":"supported"})
            };
            assert_eq!(actual, expected);
            let mut input = std::io::Cursor::new(b"not JSON".as_slice());
            let error = run_project_with_standard_streams(&path, &options, &mut input).unwrap_err();
            assert_eq!(input.position(), 8);
            assert_eq!(error.to_string(), "reading primary input payload");
            assert!(error.downcast_ref::<engine::EngineError>().is_none());
            let mut input = std::io::Cursor::new(bytes);
            let error = run_project_with_standard_streams(
                &path,
                &StandardIoRunOptions::new().with_input_path(Path::new("-")),
                &mut input,
            )
            .unwrap_err();
            assert!(error.to_string().contains("requires `--output -`"));
            assert_eq!(input.position(), 0);
            std::fs::remove_file(path)?;
            std::fs::remove_dir(directory)?;
        }
        Ok(())
    }
}
