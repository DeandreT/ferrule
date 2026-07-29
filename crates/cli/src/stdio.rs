use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

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
        bail!(
            "standard-stream execution requires `--output -`; filesystem outputs use the ordinary `run` path"
        );
    }

    let project = load_project(project_path)?;
    reject_dynamic_sources(&project, options.target)?;

    let input_from_stdin = options.input_path == Some(Path::new("-"));
    let input_path = if input_from_stdin {
        stream_source_identity(project_path, &project)?
    } else {
        resolve_run_path(
            project_path,
            options.input_path,
            project.source_path.as_deref(),
            "input",
            "source_path",
            true,
        )?
    };
    let output_path = stream_target_identity(project_path, &project, options.target)?;
    let primary_bytes = if input_from_stdin {
        read_bounded(stdin, "stdin")?
    } else {
        read_path_bounded(&input_path, "primary input")?
    };
    let primary = PayloadDocument::new(&input_path, &primary_bytes)?;

    let extra_storage = load_static_extra_sources(project_path, &project, options.target)?;
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
        .with_output_path(&output_path);
    if let Some(target) = options.target {
        payload_options = payload_options.with_target(target);
    }
    if let Some(parameters) = options.runtime_parameters {
        payload_options = payload_options.with_runtime_parameters(parameters);
    }
    if let Some(trace_sink) = options.trace_sink {
        payload_options = payload_options.with_trace_sink(trace_sink);
    }
    let outcome = run_project_value_payloads(&project, project_path, &payload_options)?;
    if !matches!(outcome.artifacts.as_slice(), [_]) {
        bail!(
            "stdout requires exactly one output artifact, but this mapping produced {}",
            outcome.artifacts.len()
        );
    }
    Ok(outcome)
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
    bail!(
        "standard-stream execution cannot supply per-item dynamic source(s): {}; use the filesystem runner or PayloadRunOptions",
        dynamic.join(", ")
    )
}

fn load_static_extra_sources(
    project_path: &Path,
    project: &mapping::Project,
    target: Option<TargetSelection<'_>>,
) -> anyhow::Result<Vec<ExtraSourceBytes>> {
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
            let bytes = read_path_bounded(&path, &format!("named source `{}`", source.name))?;
            Ok(ExtraSourceBytes {
                name: source.name.clone(),
                path,
                bytes,
            })
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
