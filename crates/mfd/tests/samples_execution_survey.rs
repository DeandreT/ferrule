//! Read-only execution survey over the local (gitignored) MapForce samples.
//!
//! Run with:
//! `cargo test -p mfd --test samples_execution_survey -- --ignored --nocapture`.
//! Set `FERRULE_EXECUTION_SURVEY_JSON=/path/to/report.json` for a versioned
//! machine-readable report and `FERRULE_EXECUTION_SURVEY_DETAILS=1` for every
//! per-file outcome. A manifest produced by `samples_reference_survey` can be
//! supplied through `FERRULE_REFERENCE_SAMPLES_MANIFEST`; use the platform
//! path-list separator to combine independently generated manifests.
//!
//! The harness resolves every input beneath the sample directory, including
//! data-dependent secondary sources through a contained host loader, rejects
//! network access, and writes projects and outputs only to a unique temporary
//! workspace. Reference comparison prefers isolated outputs recorded by the
//! reference survey, then falls back to an existing explicit primary
//! `outputinstance` that is neither an input nor an update template.

#[path = "samples_execution_survey/format_io.rs"]
mod format_io;
#[path = "samples_execution_survey/output_support.rs"]
mod output_support;
#[path = "samples_execution_survey/reference_support.rs"]
mod reference_support;
#[path = "samples_execution_survey/roundtrip.rs"]
mod roundtrip;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use format_io::{inferred_extension, is_http, portable_path, read_instance, resolve_sample_input};
use ir::{Instance, SchemaNode};
use mapping::{
    DynamicSourcePath, EdiAutocomplete, ExternalPayloadFormat, ExternalSourceOrigin, FormatOptions,
    Graph, NamedSource, Node, Project, RuntimeValue, Scope, TabularBoundaryKind,
};
use output_support::{
    compare_generated_references, prepare_database_output, prepare_xlsx_update_output,
    validate_document_paths, write_outputs,
};
use reference_support::{
    GeneratedReferences, first_instance_difference, load_generated_references,
    requested_generated_references,
};

const SAMPLES_DIR: &str = "../../samples/ReferenceSamples";
const JSON_REPORT_ENV: &str = "FERRULE_EXECUTION_SURVEY_JSON";
const DETAILS_ENV: &str = "FERRULE_EXECUTION_SURVEY_DETAILS";
const REPORT_SCHEMA_VERSION: u32 = 1;
const FIXED_CURRENT_DATETIME: &str = "2000-01-01T00:00:00-08:00";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Passed,
    Failed,
    Skipped,
}

impl Status {
    const fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug)]
struct StageOutcome {
    status: Status,
    message: Option<String>,
}

impl StageOutcome {
    fn passed() -> Self {
        Self {
            status: Status::Passed,
            message: None,
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self {
            status: Status::Failed,
            message: Some(message.into()),
        }
    }

    fn skipped(message: impl Into<String>) -> Self {
        Self {
            status: Status::Skipped,
            message: Some(message.into()),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "status": self.status.label(),
            "message": self.message,
        })
    }
}

#[derive(Debug)]
struct SampleOutcome {
    file: String,
    source: Option<String>,
    output: Option<String>,
    import: StageOutcome,
    validation: StageOutcome,
    execution: StageOutcome,
    output_write: StageOutcome,
    reference_match: StageOutcome,
}

impl SampleOutcome {
    fn pending(path: &Path) -> Self {
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let blocked = StageOutcome::skipped("an earlier execution-survey stage did not complete");
        Self {
            file,
            source: None,
            output: None,
            import: blocked.clone(),
            validation: blocked.clone(),
            execution: blocked.clone(),
            output_write: blocked.clone(),
            reference_match: blocked,
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "file": self.file,
            "resolved": {
                "source": self.source,
                "output": self.output,
            },
            "stages": {
                "import": self.import.to_json(),
                "validation": self.validation.to_json(),
                "execution": self.execution.to_json(),
                "output_write": self.output_write.to_json(),
                "reference_match": self.reference_match.to_json(),
            },
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Summary {
    total: usize,
    imported: usize,
    valid: usize,
    execution_attempted: usize,
    execution_passed: usize,
    outputs_written: usize,
    references_available: usize,
    references_matched: usize,
    references_mismatched: usize,
}

impl Summary {
    fn from_outcomes(outcomes: &[SampleOutcome]) -> Self {
        Self {
            total: outcomes.len(),
            imported: count(outcomes, |sample| sample.import.status == Status::Passed),
            valid: count(outcomes, |sample| {
                sample.validation.status == Status::Passed
            }),
            execution_attempted: count(outcomes, |sample| {
                sample.execution.status != Status::Skipped
            }),
            execution_passed: count(outcomes, |sample| sample.execution.status == Status::Passed),
            outputs_written: count(outcomes, |sample| {
                sample.output_write.status == Status::Passed
            }),
            references_available: count(outcomes, |sample| {
                sample.reference_match.status != Status::Skipped
            }),
            references_matched: count(outcomes, |sample| {
                sample.reference_match.status == Status::Passed
            }),
            references_mismatched: count(outcomes, |sample| {
                sample.reference_match.status == Status::Failed
            }),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total": self.total,
            "imported": self.imported,
            "valid": self.valid,
            "execution_attempted": self.execution_attempted,
            "execution_passed": self.execution_passed,
            "outputs_written": self.outputs_written,
            "references_available": self.references_available,
            "references_matched": self.references_matched,
            "references_mismatched": self.references_mismatched,
        })
    }
}

fn count(outcomes: &[SampleOutcome], predicate: impl Fn(&SampleOutcome) -> bool) -> usize {
    outcomes.iter().filter(|sample| predicate(sample)).count()
}

struct SurveyWorkspace(PathBuf);

impl SurveyWorkspace {
    fn new() -> io::Result<Self> {
        let base = std::env::temp_dir();
        for attempt in 0..1_024 {
            let path = base.join(format!(
                "ferrule-mfd-execution-survey-{}-{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique execution-survey workspace",
        ))
    }

    fn sample_dir(&self, index: usize) -> io::Result<PathBuf> {
        let path = self.0.join(format!("sample-{index}"));
        std::fs::create_dir(&path)?;
        Ok(path)
    }
}

impl Drop for SurveyWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn explicit_reference(
    mfd_path: &Path,
    samples_root: &Path,
    stored: Option<&str>,
    input_paths: &BTreeSet<PathBuf>,
    options: &FormatOptions,
) -> Result<PathBuf, String> {
    if options.xlsx_update_existing {
        return Err("update-in-place workbook is a template, not an unambiguous reference".into());
    }
    let stored = stored
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "target has no explicit output instance".to_string())?;
    if is_http(stored) {
        return Err("target output instance is a URL".to_string());
    }
    let document = std::fs::read_to_string(mfd_path)
        .map_err(|error| format!("reading MFD metadata failed: {error}"))?;
    let document = roxmltree::Document::parse(&document)
        .map_err(|error| format!("parsing MFD metadata failed: {error}"))?;
    let declared = document
        .descendants()
        .filter(|node| node.is_element())
        .any(|node| {
            node.attribute("outputinstance") == Some(stored)
                || (node.has_tag_name("file")
                    && node.attribute("role") == Some("outputinstance")
                    && node.attribute("name") == Some(stored))
        });
    if !declared {
        return Err("target path is not an explicit outputinstance declaration".to_string());
    }
    let reference = resolve_sample_input(samples_root, stored)?;
    if input_paths.contains(&reference) {
        return Err("target output instance is also used as an input".to_string());
    }
    let owners = output_instance_owners(samples_root, &reference)?;
    if owners.len() != 1 || owners.first().is_none_or(|owner| owner != mfd_path) {
        return Err(format!(
            "target output instance is shared by {} mapping designs; an isolated generated reference is required",
            owners.len()
        ));
    }
    Ok(reference)
}

fn output_instance_owners(samples_root: &Path, reference: &Path) -> Result<Vec<PathBuf>, String> {
    let mut designs = Vec::new();
    collect_mfd_paths(samples_root, &mut designs)?;
    let mut owners = Vec::new();
    for design in designs {
        let document = std::fs::read_to_string(&design)
            .map_err(|error| format!("reading MFD metadata failed: {error}"))?;
        let document = roxmltree::Document::parse(&document)
            .map_err(|error| format!("parsing MFD metadata failed: {error}"))?;
        let owns_reference = document
            .descendants()
            .filter(|node| node.is_element())
            .any(|node| {
                let declared = node.attribute("outputinstance").or_else(|| {
                    (node.has_tag_name("file") && node.attribute("role") == Some("outputinstance"))
                        .then(|| node.attribute("name"))
                        .flatten()
                });
                declared
                    .filter(|stored| !is_http(stored))
                    .and_then(|stored| resolve_sample_input(samples_root, stored).ok())
                    .is_some_and(|declared| declared == reference)
            });
        if owns_reference {
            owners.push(design);
        }
    }
    owners.sort();
    owners.dedup();
    Ok(owners)
}

fn collect_mfd_paths(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(directory)
        .map_err(|error| format!("reading sample directory failed: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("reading sample directory entry failed: {error}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("reading sample entry type failed: {error}"))?;
        if file_type.is_dir() {
            collect_mfd_paths(&path, paths)?;
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("mfd"))
        {
            paths.push(path);
        }
    }
    Ok(())
}

struct LoadedSources {
    primary: Instance,
    extras: Vec<(String, Instance)>,
    paths: BTreeSet<PathBuf>,
}

fn primary_source_path<'a>(
    stored: Option<&'a str>,
    options: &FormatOptions,
) -> Result<&'a str, String> {
    stored.ok_or_else(|| {
        let Some(boundary) = options.external_source.as_ref() else {
            return "primary source has no input instance path".to_string();
        };
        let payload = match boundary.payload() {
            ExternalPayloadFormat::Json => "JSON",
            ExternalPayloadFormat::Xml => "XML",
        };
        match boundary.origin() {
            ExternalSourceOrigin::UserFunction { name, reason } => format!(
                "external user-function source `{name}` requires an explicitly supplied captured {payload} response because Ferrule does not execute its body ({reason})"
            ),
            ExternalSourceOrigin::HttpPost { .. } => format!(
                "external HTTP POST source requires an explicitly supplied captured {payload} response because the read-only survey does not make network requests"
            ),
        }
    })
}

fn load_sources(project: &Project, samples_root: &Path) -> Result<LoadedSources, String> {
    let source_path = primary_source_path(project.source_path.as_deref(), &project.source_options)?;
    let (primary, mut paths) = load_source(
        samples_root,
        source_path,
        &project.source,
        &project.source_options,
    )
    .map_err(|error| format!("reading primary input failed: {error}"))?;
    let mut extras = Vec::with_capacity(project.extra_sources.len());
    for source in &project.extra_sources {
        if source.dynamic_path.is_some() {
            continue;
        }
        let (instance, source_paths) =
            load_source(samples_root, &source.path, &source.schema, &source.options).map_err(
                |error| format!("reading extra source `{}` failed: {error}", source.name),
            )?;
        paths.extend(source_paths);
        extras.push((source.name.clone(), instance));
    }
    Ok(LoadedSources {
        primary,
        extras,
        paths,
    })
}

struct SurveyDynamicSourceLoader<'a> {
    samples_root: PathBuf,
    sources: &'a [NamedSource],
    cache: RefCell<BTreeMap<(String, PathBuf), Arc<Instance>>>,
    loaded_paths: RefCell<BTreeSet<PathBuf>>,
}

impl<'a> SurveyDynamicSourceLoader<'a> {
    fn new(samples_root: &Path, sources: &'a [NamedSource]) -> Result<Self, String> {
        let samples_root = std::fs::canonicalize(samples_root)
            .map_err(|error| format!("resolving sample root failed: {error}"))?;
        Ok(Self {
            samples_root,
            sources,
            cache: RefCell::new(BTreeMap::new()),
            loaded_paths: RefCell::new(BTreeSet::new()),
        })
    }

    fn loaded_paths(&self) -> BTreeSet<PathBuf> {
        self.loaded_paths.borrow().clone()
    }
}

impl engine::DynamicSourceLoader for SurveyDynamicSourceLoader<'_> {
    fn load(&self, source_name: &str, path: &str) -> Result<Arc<Instance>, String> {
        let source = self
            .sources
            .iter()
            .find(|source| source.name == source_name && source.dynamic_path.is_some())
            .ok_or_else(|| format!("dynamic source `{source_name}` is not declared"))?;
        let resolved = resolve_sample_input(&self.samples_root, path)?;
        let key = (source_name.to_string(), resolved.clone());
        if let Some(instance) = self.cache.borrow().get(&key).cloned() {
            return Ok(instance);
        }
        let instance = Arc::new(read_instance(&resolved, &source.schema, &source.options)?);
        self.loaded_paths.borrow_mut().insert(resolved);
        self.cache.borrow_mut().insert(key, Arc::clone(&instance));
        Ok(instance)
    }
}

fn load_source(
    samples_root: &Path,
    stored: &str,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> Result<(Instance, BTreeSet<PathBuf>), String> {
    if !options.local_xml_file_set {
        let path = resolve_sample_input(samples_root, stored)?;
        let instance = read_instance(&path, schema, options)?;
        return Ok((instance, BTreeSet::from([path])));
    }
    if stored.trim().is_empty() {
        return Err("local XML file-set pattern is empty".into());
    }
    if is_http(stored) {
        return Err("network XML file sets are disabled".into());
    }
    let pattern = portable_path(stored);
    let loaded = format_xml::read_local_file_set(
        samples_root,
        &pattern,
        schema,
        format_xml::LocalFileSetLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    Ok((loaded.instance, loaded.paths.into_iter().collect()))
}

fn survey_file(
    index: usize,
    mfd_path: &Path,
    samples_root: &Path,
    workspace: &SurveyWorkspace,
    generated_references: Option<&GeneratedReferences>,
) -> SampleOutcome {
    let mut outcome = SampleOutcome::pending(mfd_path);
    let imported = match mfd::import(mfd_path) {
        Ok(imported) => imported,
        Err(error) => {
            outcome.import = StageOutcome::failed(error.to_string());
            return outcome;
        }
    };
    outcome.import = if imported.warnings.is_empty() {
        StageOutcome::passed()
    } else {
        StageOutcome::failed(format!(
            "import emitted {} warning(s): {}",
            imported.warnings.len(),
            imported.warnings.join(" | ")
        ))
    };
    let validation = engine::validate(&imported.project);
    if !validation.is_empty() {
        outcome.validation = StageOutcome::failed(
            validation
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" | "),
        );
        return outcome;
    }
    outcome.validation = StageOutcome::passed();
    outcome.source = imported.project.source_path.clone();

    let sample_dir = match workspace.sample_dir(index) {
        Ok(path) => path,
        Err(error) => {
            outcome.execution =
                StageOutcome::failed(format!("creating temp directory failed: {error}"));
            return outcome;
        }
    };
    let project_path = sample_dir.join("project.ferrule.json");
    let encoded = match serde_json::to_vec_pretty(&imported.project) {
        Ok(encoded) => encoded,
        Err(error) => {
            outcome.execution =
                StageOutcome::failed(format!("serializing project failed: {error}"));
            return outcome;
        }
    };
    if let Err(error) = std::fs::write(&project_path, encoded) {
        outcome.execution = StageOutcome::failed(format!("writing temp project failed: {error}"));
        return outcome;
    }

    let mut sources = match load_sources(&imported.project, samples_root) {
        Ok(sources) => sources,
        Err(reason) => {
            outcome.execution = StageOutcome::skipped(reason);
            return outcome;
        }
    };
    let dynamic_loader =
        match SurveyDynamicSourceLoader::new(samples_root, &imported.project.extra_sources) {
            Ok(loader) => loader,
            Err(reason) => {
                outcome.execution = StageOutcome::failed(reason);
                return outcome;
            }
        };
    let runtime_mapping_path = match std::fs::canonicalize(mfd_path) {
        Ok(path) => path,
        Err(error) => {
            outcome.execution =
                StageOutcome::failed(format!("resolving the active mapping path failed: {error}"));
            return outcome;
        }
    };
    let execution = engine::ExecutionContext::new(&runtime_mapping_path)
        .with_current_datetime(FIXED_CURRENT_DATETIME)
        .with_dynamic_source_loader(&dynamic_loader);
    let outputs = match engine::run_outputs_with_sources_and_context(
        &imported.project,
        &sources.primary,
        sources.extras,
        &execution,
    ) {
        Ok(outputs) => outputs,
        Err(error) => {
            outcome.execution = StageOutcome::failed(error.to_string());
            return outcome;
        }
    };
    sources.paths.extend(dynamic_loader.loaded_paths());
    outcome.execution = StageOutcome::passed();

    let written = match write_outputs(&imported.project, &outputs, &sample_dir, samples_root) {
        Ok(written) => written,
        Err(reason) => {
            outcome.output_write = StageOutcome::skipped(reason);
            return outcome;
        }
    };
    outcome.output = Some(written.primary.display().to_string());
    outcome.output_write = StageOutcome::passed();

    let generated = match generated_references
        .map(|references| references.for_sample(mfd_path, samples_root))
        .transpose()
    {
        Ok(generated) => generated.flatten(),
        Err(reason) => {
            outcome.reference_match = StageOutcome::failed(reason);
            return outcome;
        }
    };
    if let Some(references) = generated {
        if has_nondeterministic_current_time(&imported.project.graph)
            || has_nondeterministic_edi_autocomplete(
                std::iter::once((&imported.project.target_options, &imported.project.root)).chain(
                    imported
                        .project
                        .extra_targets
                        .iter()
                        .map(|target| (&target.options, &target.root)),
                ),
            )
        {
            outcome.reference_match = StageOutcome::skipped(
                "exact reference comparison is nondeterministic because the mapping reads or derives the current dateTime",
            );
            return outcome;
        }
        outcome.reference_match =
            match compare_generated_references(&imported.project, &written, references) {
                Ok(()) => StageOutcome::passed(),
                Err(reason) => StageOutcome::failed(reason),
            };
        return outcome;
    }

    let reference = match explicit_reference(
        mfd_path,
        samples_root,
        imported.project.target_path.as_deref(),
        &sources.paths,
        &imported.project.target_options,
    ) {
        Ok(reference) => reference,
        Err(reason) => {
            outcome.reference_match = StageOutcome::skipped(reason);
            return outcome;
        }
    };
    let expected = match read_instance(
        &reference,
        &imported.project.target,
        &imported.project.target_options,
    ) {
        Ok(instance) => instance,
        Err(error) => {
            outcome.reference_match =
                StageOutcome::failed(format!("reading reference output failed: {error}"));
            return outcome;
        }
    };
    let actual = match read_instance(
        &written.primary,
        &imported.project.target,
        &imported.project.target_options,
    ) {
        Ok(instance) => instance,
        Err(error) => {
            outcome.reference_match =
                StageOutcome::failed(format!("reading ferrule output failed: {error}"));
            return outcome;
        }
    };
    if expected == actual {
        outcome.reference_match = StageOutcome::passed();
    } else {
        outcome.reference_match = StageOutcome::failed(format!(
            "written output differs from reference `{}`: {}",
            reference.display(),
            first_instance_difference(&expected, &actual)
        ));
    }
    outcome
}

fn has_nondeterministic_current_time(graph: &Graph) -> bool {
    graph.nodes.values().any(|node| {
        matches!(
            node,
            Node::RuntimeValue {
                value: RuntimeValue::CurrentDateTime
            }
        )
    })
}

fn has_nondeterministic_edi_autocomplete<'a>(
    targets: impl IntoIterator<Item = (&'a FormatOptions, &'a Scope)>,
) -> bool {
    targets
        .into_iter()
        .any(|(options, root)| match options.edi_autocomplete.as_ref() {
            Some(EdiAutocomplete::X12(_)) => {
                !scope_binds_fields(root, "ISA", &["FI08", "FI09"])
                    || !scope_binds_fields(root, "GS", &["F373", "F337"])
            }
            Some(EdiAutocomplete::Edifact(_)) => {
                !scope_binds_fields(root, "UNB", &["F0017", "F0019"])
            }
            Some(_) => true,
            None => false,
        })
}

fn scope_binds_fields(scope: &Scope, segment: &str, fields: &[&str]) -> bool {
    (scope.target_field == segment
        && fields.iter().all(|field| {
            scope
                .bindings
                .iter()
                .any(|binding| binding.target_field == *field)
        }))
        || scope
            .children
            .iter()
            .any(|child| scope_binds_fields(child, segment, fields))
}

fn write_json_report(
    report_path: &Path,
    samples_root: &Path,
    summary: &Summary,
    outcomes: &[SampleOutcome],
) -> Result<(), Box<dyn Error>> {
    let parent = report_path.parent().unwrap_or_else(|| Path::new("."));
    let report_parent = std::fs::canonicalize(parent)?;
    let samples_root = std::fs::canonicalize(samples_root)?;
    if report_parent.starts_with(&samples_root) {
        return Err(format!(
            "{JSON_REPORT_ENV} must not write inside the read-only sample directory"
        )
        .into());
    }
    let report = serde_json::json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "kind": "ferrule.mfd_sample_execution",
        "samples_dir": samples_root,
        "safety": {
            "network_access": false,
            "inputs_restricted_to_samples_dir": true,
            "generated_files_restricted_to_temp_dir": true,
            "reference_policy": "isolated generated manifest when available; otherwise an existing explicit outputinstance not reused as input or update template",
            "fixed_current_datetime": FIXED_CURRENT_DATETIME,
        },
        "summary": summary.to_json(),
        "samples": outcomes.iter().map(SampleOutcome::to_json).collect::<Vec<_>>(),
    });
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report_path)?;
    output.write_all(&serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

fn print_details(outcomes: &[SampleOutcome]) {
    for sample in outcomes {
        println!("\n{}", sample.file);
        for (name, stage) in [
            ("import", &sample.import),
            ("validation", &sample.validation),
            ("execution", &sample.execution),
            ("output", &sample.output_write),
            ("reference", &sample.reference_match),
        ] {
            println!(
                "  {name}: {}{}",
                stage.status.label(),
                stage
                    .message
                    .as_deref()
                    .map(|message| format!(" ({message})"))
                    .unwrap_or_default()
            );
        }
    }
}

#[test]
fn summary_counts_attempts_separately_from_skips() {
    let mut passed = SampleOutcome::pending(Path::new("passed.mfd"));
    passed.import = StageOutcome::passed();
    passed.validation = StageOutcome::passed();
    passed.execution = StageOutcome::passed();
    passed.output_write = StageOutcome::passed();
    passed.reference_match = StageOutcome::passed();
    let mut failed = SampleOutcome::pending(Path::new("failed.mfd"));
    failed.import = StageOutcome::passed();
    failed.validation = StageOutcome::passed();
    failed.execution = StageOutcome::failed("runtime failure");
    failed.reference_match = StageOutcome::failed("different");

    assert_eq!(
        Summary::from_outcomes(&[passed, failed]),
        Summary {
            total: 2,
            imported: 2,
            valid: 2,
            execution_attempted: 2,
            execution_passed: 1,
            outputs_written: 1,
            references_available: 2,
            references_matched: 1,
            references_mismatched: 1,
        }
    );
}

#[test]
fn pathless_external_sources_are_classified_as_captured_response_boundaries()
-> Result<(), Box<dyn Error>> {
    let external = FormatOptions {
        external_source: Some(mapping::ExternalSourceOptions::user_function(
            "FetchInventory",
            "recursive service function",
            ExternalPayloadFormat::Json,
        )?),
        ..FormatOptions::default()
    };
    assert_eq!(
        primary_source_path(Some("captured.json"), &external),
        Ok("captured.json")
    );

    let external_reason = primary_source_path(None, &external)
        .expect_err("a pathless external source must require captured input");
    assert!(external_reason.contains("external user-function source `FetchInventory`"));
    assert!(external_reason.contains("captured JSON response"));
    assert!(external_reason.contains("recursive service function"));
    assert_eq!(
        primary_source_path(None, &FormatOptions::default()),
        Err("primary source has no input instance path".to_string())
    );
    Ok(())
}

#[test]
fn current_datetime_references_are_classified_as_nondeterministic() {
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::RuntimeValue {
            value: RuntimeValue::MappingFilePath,
        },
    );
    assert!(!has_nondeterministic_current_time(&graph));
    graph.nodes.insert(
        1,
        Node::RuntimeValue {
            value: RuntimeValue::CurrentDateTime,
        },
    );
    assert!(has_nondeterministic_current_time(&graph));

    let deterministic = FormatOptions::default();
    let deterministic_root = Scope::default();
    assert!(!has_nondeterministic_edi_autocomplete([(
        &deterministic,
        &deterministic_root,
    )]));
    let autocomplete = FormatOptions {
        edi_autocomplete: Some(mapping::EdiAutocomplete::Edifact(
            mapping::EdifactAutocomplete {
                syntax_level: Some("A".into()),
                syntax_version: Some("4".into()),
                controlling_agency: Some("UNO".into()),
                message_type: Some("ORDERS".into()),
            },
        )),
        ..FormatOptions::default()
    };
    assert!(has_nondeterministic_edi_autocomplete([
        (&deterministic, &deterministic_root),
        (&autocomplete, &deterministic_root),
    ]));

    let x12 = FormatOptions {
        edi_autocomplete: Some(EdiAutocomplete::X12(mapping::X12Autocomplete {
            request_acknowledgement: false,
            transaction_set: Some("850".into()),
        })),
        ..FormatOptions::default()
    };
    let mut mapped_root = Scope::default();
    for (segment, fields) in [("ISA", ["FI08", "FI09"]), ("GS", ["F373", "F337"])] {
        mapped_root.children.push(Scope {
            target_field: segment.into(),
            bindings: fields
                .into_iter()
                .zip(0_u32..)
                .map(|(target_field, node)| mapping::Binding {
                    target_field: target_field.into(),
                    node,
                })
                .collect(),
            ..Scope::default()
        });
    }
    assert!(!has_nondeterministic_edi_autocomplete([(
        &x12,
        &mapped_root,
    )]));
}

#[test]
fn retained_tabular_options_select_a_pathless_output_format() {
    let csv = FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Csv),
        delimiter: Some(';'),
        has_header_row: Some(false),
        ..FormatOptions::default()
    };
    assert_eq!(inferred_extension(&csv), Some("csv"));

    let xlsx = FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Xlsx),
        has_header_row: Some(true),
        xlsx_sheet: Some("Summary".into()),
        ..FormatOptions::default()
    };
    assert_eq!(inferred_extension(&xlsx), Some("xlsx"));
}

#[test]
fn xlsx_update_templates_are_atomically_detached_from_samples() -> Result<(), Box<dyn Error>> {
    let workspace = SurveyWorkspace::new()?;
    let sample_root = workspace.0.join("samples");
    let output_root = workspace.0.join("outputs");
    std::fs::create_dir(&sample_root)?;
    std::fs::create_dir(&output_root)?;
    let template = sample_root.join("template.xlsx");
    std::fs::write(&template, b"workbook-template")?;
    let output = output_root.join("result.xlsx");
    std::fs::hard_link(&template, &output)?;
    let options = FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Xlsx),
        xlsx_update_existing: true,
        ..FormatOptions::default()
    };

    prepare_xlsx_update_output(
        &sample_root,
        &output_root,
        Some("template.xlsx"),
        &output,
        &options,
    )?;
    std::fs::write(&output, b"changed writable copy")?;
    assert_eq!(std::fs::read(&template)?, b"workbook-template");

    let escaped = workspace.0.join("escaped.xlsx");
    assert!(
        prepare_xlsx_update_output(
            &sample_root,
            &output_root,
            Some("template.xlsx"),
            &escaped,
            &options,
        )
        .is_err()
    );
    assert!(!escaped.exists());

    assert!(
        prepare_xlsx_update_output(
            &sample_root,
            &sample_root,
            Some("template.xlsx"),
            &sample_root.join("forbidden.xlsx"),
            &options,
        )
        .is_err()
    );
    assert!(!sample_root.join("forbidden.xlsx").exists());
    Ok(())
}

#[test]
fn network_and_parent_paths_are_not_local_sample_inputs() -> Result<(), Box<dyn Error>> {
    let workspace = SurveyWorkspace::new()?;
    let sample_root = workspace.0.join("samples");
    std::fs::create_dir(&sample_root)?;
    std::fs::write(sample_root.join("input.xml"), "<root/>")?;
    std::fs::write(workspace.0.join("outside.xml"), "<root/>")?;

    assert!(resolve_sample_input(&sample_root, "input.xml").is_ok());
    assert!(resolve_sample_input(&sample_root, "https://example.test/input.xml").is_err());
    assert!(resolve_sample_input(&sample_root, "../outside.xml").is_err());
    Ok(())
}

#[test]
fn dynamic_sources_load_only_declared_contained_files_and_cache_them() -> Result<(), Box<dyn Error>>
{
    let workspace = SurveyWorkspace::new()?;
    let sample_root = workspace.0.join("samples");
    let nested = sample_root.join("nested");
    std::fs::create_dir_all(&nested)?;
    let input = nested.join("input.xml");
    std::fs::write(&input, "<Root><Value>inside</Value></Root>")?;
    std::fs::write(
        workspace.0.join("outside.xml"),
        "<Root><Value>outside</Value></Root>",
    )?;
    let source = NamedSource {
        name: "document".into(),
        path: String::new(),
        schema: SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar("Value", ir::ScalarType::String)],
        ),
        options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        dynamic_path: Some(DynamicSourcePath {
            node: 0,
            iteration: Vec::new(),
        }),
    };
    let sources = [source];
    let loader = SurveyDynamicSourceLoader::new(&sample_root, &sources)?;

    let first = engine::DynamicSourceLoader::load(&loader, "document", "nested/input.xml")?;
    let second =
        engine::DynamicSourceLoader::load(&loader, "document", input.to_string_lossy().as_ref())?;
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(
        first.field("Value"),
        Some(&Instance::Scalar(ir::Value::String("inside".into())))
    );
    assert_eq!(
        loader.loaded_paths(),
        BTreeSet::from([input.canonicalize()?])
    );
    assert!(
        engine::DynamicSourceLoader::load(&loader, "document", "https://example.test/input.xml")
            .is_err()
    );
    assert!(engine::DynamicSourceLoader::load(&loader, "document", "../outside.xml").is_err());
    assert!(engine::DynamicSourceLoader::load(&loader, "document", "missing.xml").is_err());
    assert!(engine::DynamicSourceLoader::load(&loader, "undeclared", "nested/input.xml").is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn dynamic_sources_reject_symlinks_outside_the_sample_root() -> Result<(), Box<dyn Error>> {
    let workspace = SurveyWorkspace::new()?;
    let sample_root = workspace.0.join("samples");
    std::fs::create_dir(&sample_root)?;
    let outside = workspace.0.join("outside.xml");
    std::fs::write(&outside, "<Root/>")?;
    std::os::unix::fs::symlink(&outside, sample_root.join("linked.xml"))?;
    let source = NamedSource {
        name: "document".into(),
        path: String::new(),
        schema: SchemaNode::group("Root", Vec::new()),
        options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        dynamic_path: Some(DynamicSourcePath {
            node: 0,
            iteration: Vec::new(),
        }),
    };
    let sources = [source];
    let loader = SurveyDynamicSourceLoader::new(&sample_root, &sources)?;

    assert!(engine::DynamicSourceLoader::load(&loader, "document", "linked.xml").is_err());
    assert!(loader.loaded_paths().is_empty());
    Ok(())
}

#[test]
fn database_templates_are_copied_without_touching_the_sample() -> Result<(), Box<dyn Error>> {
    let workspace = SurveyWorkspace::new()?;
    let sample_root = workspace.0.join("samples");
    let output_root = workspace.0.join("outputs");
    std::fs::create_dir(&sample_root)?;
    std::fs::create_dir(&output_root)?;
    let template = sample_root.join("template.sqlite");
    std::fs::write(&template, b"sqlite-template")?;
    let output = output_root.join("result.sqlite");
    let schema = SchemaNode::group(
        "parents",
        vec![
            SchemaNode::group(
                "children|parent_id",
                vec![SchemaNode::scalar("id", ir::ScalarType::Int)],
            )
            .repeating(),
        ],
    )
    .repeating();

    prepare_database_output(&sample_root, Some("template.sqlite"), &output, &schema)?;
    assert_eq!(std::fs::read(&output)?, b"sqlite-template");
    std::fs::write(&output, b"changed output")?;
    assert_eq!(std::fs::read(&template)?, b"sqlite-template");

    let escaped = output_root.join("escaped.sqlite");
    assert!(
        prepare_database_output(&sample_root, Some("../outside.sqlite"), &escaped, &schema)
            .is_err()
    );
    assert!(!escaped.exists());
    Ok(())
}

#[test]
fn report_creation_never_follows_an_existing_leaf() -> Result<(), Box<dyn Error>> {
    let workspace = SurveyWorkspace::new()?;
    let sample_root = workspace.0.join("samples");
    std::fs::create_dir(&sample_root)?;
    let sample = sample_root.join("reference.xml");
    std::fs::write(&sample, "keep")?;
    let report = workspace.0.join("report.json");
    std::fs::hard_link(&sample, &report)?;

    let summary = Summary::from_outcomes(&[]);
    assert!(write_json_report(&report, &sample_root, &summary, &[]).is_err());
    assert_eq!(std::fs::read_to_string(sample)?, "keep");
    Ok(())
}

#[test]
fn explicit_reference_requires_one_owning_mapping() -> Result<(), Box<dyn Error>> {
    let workspace = SurveyWorkspace::new()?;
    let samples = workspace.0.join("samples");
    std::fs::create_dir(&samples)?;
    let reference = samples.join("result.xml");
    std::fs::write(&reference, "<Result/>")?;
    let first = samples.join("first.mfd");
    let second = samples.join("second.mfd");
    let design = r#"<mapping><component library="xml"><document outputinstance="result.xml"/></component></mapping>"#;
    std::fs::write(&first, design)?;

    assert_eq!(
        explicit_reference(
            &first,
            &samples,
            Some("result.xml"),
            &BTreeSet::new(),
            &FormatOptions::default(),
        )?,
        reference.canonicalize()?
    );

    std::fs::write(&second, design)?;
    let error = explicit_reference(
        &first,
        &samples,
        Some("result.xml"),
        &BTreeSet::new(),
        &FormatOptions::default(),
    )
    .unwrap_err();
    assert!(error.contains("shared by 2 mapping designs"), "{error}");
    Ok(())
}

#[test]
fn generated_reference_manifests_resolve_only_contained_outputs() -> Result<(), Box<dyn Error>> {
    let workspace = SurveyWorkspace::new()?;
    let references = workspace.0.join("references");
    let output_dir = references.join("000-example");
    let samples = workspace.0.join("samples/ReferenceSamples");
    std::fs::create_dir_all(&output_dir)?;
    std::fs::create_dir_all(&samples)?;
    let expected = output_dir.join("result.json");
    let second = output_dir.join("summary.json");
    std::fs::write(&expected, "{}")?;
    std::fs::write(&second, "{}")?;
    let sample = samples.join("example.mfd");
    std::fs::write(&sample, "<mapping/>")?;
    let manifest = references.join("manifest.json");
    std::fs::write(
        &manifest,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "kind": "ferrule.reference_samples_outputs",
            "samples": [{
                "file": "ReferenceSamples/example.mfd",
                "directory": "000-example",
                "status": "passed",
                "outputs": ["result.json", "summary.json"],
            }],
        }))?,
    )?;

    let loaded = load_generated_references(&manifest)?;
    let expected = [expected.canonicalize()?, second.canonicalize()?];
    assert_eq!(
        loaded.for_sample(&sample, &samples)?,
        Some(expected.as_slice())
    );

    std::fs::write(
        &manifest,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "kind": "ferrule.reference_samples_outputs",
            "samples": [{
                "file": "example.mfd",
                "directory": "..",
                "status": "passed",
                "outputs": ["outside.json"],
            }],
        }))?,
    )?;
    assert!(load_generated_references(&manifest).is_err());
    Ok(())
}

#[test]
fn dynamic_document_paths_reject_escape_duplicates_and_ancestor_overlap() {
    let member = |path: &str| {
        ir::DocumentMember::new(path, Instance::Group(Vec::new()))
            .unwrap_or_else(|| panic!("valid test member path: {path}"))
    };
    assert!(validate_document_paths(&[member("nested/a.xml"), member("b.xml")]).is_ok());
    assert!(validate_document_paths(&[member("../escape.xml")]).is_err());
    assert!(validate_document_paths(&[member("same.xml"), member("same.xml")]).is_err());
    assert!(validate_document_paths(&[member("parent"), member("parent/child.xml")]).is_err());
}

#[test]
#[ignore = "needs the local MapForce sample set; informational only"]
fn survey_sample_execution() -> Result<(), Box<dyn Error>> {
    let samples_root = Path::new(env!("CARGO_MANIFEST_DIR")).join(SAMPLES_DIR);
    if !samples_root.is_dir() {
        eprintln!(
            "samples dir not found at {}; skipping",
            samples_root.display()
        );
        return Ok(());
    }
    let mut paths = std::fs::read_dir(&samples_root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("mfd"))
        })
        .collect::<Vec<_>>();
    paths.sort();

    let workspace = SurveyWorkspace::new()?;
    let generated_references = requested_generated_references()?;
    let outcomes = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            survey_file(
                index,
                path,
                &samples_root,
                &workspace,
                generated_references.as_ref(),
            )
        })
        .collect::<Vec<_>>();
    let summary = Summary::from_outcomes(&outcomes);

    println!("== mfd sample execution survey: {} files ==", summary.total);
    println!(
        "imported and engine-valid: {}/{}",
        summary.valid, summary.imported
    );
    println!(
        "execution: {}/{} attempted passed; {} skipped",
        summary.execution_passed,
        summary.execution_attempted,
        summary.total - summary.execution_attempted
    );
    println!("redirected outputs written: {}", summary.outputs_written);
    println!(
        "references: {} available; {} matched; {} mismatched",
        summary.references_available, summary.references_matched, summary.references_mismatched
    );

    if std::env::var_os(DETAILS_ENV).is_some() {
        print_details(&outcomes);
    }
    if let Some(report_path) = std::env::var_os(JSON_REPORT_ENV) {
        if report_path.is_empty() {
            return Err(format!("{JSON_REPORT_ENV} must name an output file").into());
        }
        let report_path = PathBuf::from(report_path);
        write_json_report(&report_path, &samples_root, &summary, &outcomes)?;
        println!("json report: {}", report_path.display());
    }
    Ok(())
}
