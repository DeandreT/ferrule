//! Headless runner: loads a mapping project and runs it against an input
//! file (delimited/fixed-width text, XLSX, XML, JSON, SQLite, PDF, XBRL, or X12
//! EDI, chosen by extension and format options) or a static HTTP(S) XML
//! source to produce an output file. Split out from `main.rs` so it's
//! testable without shelling out to the built binary.
//!
//! For SQLite (`.db`/`.sqlite`/`.sqlite3`) the table name is the project's
//! source/target schema root `name`. For EDI (`.edi`/`.x12`/`.edifact`)
//! the schema describes the segment/loop structure and picks the dialect
//! by its first segment (ISA = X12, UNB = EDIFACT) -- see `format_edi`.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use format_selection::{
    Direction, JsonMode, Origin, SelectedFormat, reject_inactive_root_xml_read_options,
    validate_csv_metadata_identity, xml_read_options,
};
use ir::{Instance, SchemaNode};
use mapping::{
    EdiAutocomplete, EdiBoundaryKind, EdiConfigDependency, ExternalPayloadFormat, FormatOptions,
    ProtobufOptions, TabularBoundaryKind,
};

const DEFAULT_HTTP_TIMEOUT_SECONDS: u64 = 30;
const MAX_HTTP_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_HTTP_RESPONSE_HEADER_BYTES: usize = 64 * 1024;
const MAX_HTTP_REDIRECTS: u32 = 5;

mod bundle;
mod cancellation;
mod code_generation;
mod dynamic_source_fault;
mod dynamic_source_loader;
mod execution_policy;
mod file_run;
mod filesystem_pdf_reader;
#[cfg(test)]
mod filesystem_pdf_reader_tests;
mod output_documents;
mod payload;
mod pdf_worker_policy;
mod pdf_worker_signal;
mod pipeline;
mod pipeline_preview;
mod preview_inputs;
mod primary_document;
mod project_paths;
mod read_set;
mod route_capability;
mod sqlite_query;
pub use bundle::{BundleCatalog, BundleCatalogKind, BundleOptions, BundleOutcome, bundle_project};
pub use dynamic_source_fault::{
    DynamicSourceReadError, UndeclaredDynamicSource, UnsuppliedPayloadSource,
};
use dynamic_source_loader::ProjectDynamicSourceLoader;
pub use engine::{
    ExecutionBudgetKind, ExecutionBudgets, ExecutionPolicy, PathAccess, PolicyRefusal,
    RestrictedCapability, RestrictedPolicy, RootAccess,
};
pub use execution_policy::{load_execution_policy_file, parse_execution_policy};
#[cfg(test)]
use file_run::run_project_value_with_filesystem_policy;
pub use file_run::{
    PreparedFileRun, RunObservers, RunOptions, RunOutcome, RunRequest, WrittenOutput,
    preview_project_value_from_files, preview_project_value_from_files_with_pdf_reader,
    run_project, run_project_value_with_options,
    run_project_value_with_options_and_filesystem_policy,
    run_project_value_with_options_and_pdf_reader, run_project_value_with_paths,
    run_project_value_with_paths_and_trace, run_project_with_options,
    run_project_with_options_and_filesystem_policy, run_project_with_options_and_pdf_reader,
    run_project_with_paths, run_project_with_paths_and_trace,
};
pub use filesystem_pdf_reader::FilesystemPdfReader;
use output_documents::{OutputDestination, TargetOutput, write_target_outputs};
pub use pdf_worker_policy::FilesystemRunPolicy;
mod rest_json;
mod stdio;
mod trace_json;

#[cfg(test)]
mod dynamic_source_load_tests;
#[cfg(test)]
mod format_selection_tests;
#[cfg(test)]
mod owned_observer_preflight_tests;

pub use code_generation::{
    GenerateOutcome, GenerateProfile, GenerateSettings, GenerateTarget, check_generate_project,
    generate_project, generate_project_with_csv_document_adapters,
    generate_project_with_csv_json_adapters, generate_project_with_csv_output,
    generate_project_with_csv_x12_adapters, generate_project_with_csv_x12_envelope_profile,
    generate_project_with_json5_adapters, generate_project_with_profile,
    generate_project_with_raw_delimited_document_adapters, generate_project_with_settings,
    generate_project_with_static_document_adapters,
    generate_project_with_static_document_envelope_profiles, generate_project_with_x12_adapters,
    generate_project_with_x12_envelope_profiles, plan_generate, plan_generate_project,
};
pub use codegen::X12EnvelopeProfile;
pub use engine::{
    RequiredTargetSources, SourceOwnershipObservers, TargetSelection, TraceEvent, TraceFilterPhase,
    TraceGrouping, TraceIteration, TraceOutputKind, TracePosition, TraceScope, TraceSink,
    TraceSortKey, TraceSourceField, TraceSourceRow, TraceSourceTree, TraceTarget,
    TraceTargetFieldBinding, TraceValue, TraceWindow, required_sources_for_target,
};
pub use payload::{
    MAX_PAYLOAD_ARTIFACTS, MAX_PAYLOAD_DOCUMENT_BYTES, MAX_PAYLOAD_NAME_BYTES,
    MAX_PAYLOAD_PATH_BYTES, MAX_PAYLOAD_RUN_BYTES, NamedPayloadInput, PayloadArtifact,
    PayloadDocument, PayloadRunOptions, PayloadRunOutcome, plan_payload_run, run_project_payloads,
    run_project_value_payloads,
};
pub use pipeline::{
    PipelineArtifact, PipelineHostFile, PipelineOutputFile, PipelineRunOptions, PipelineRunOutcome,
    plan_pipeline_run, run_pipeline_file, run_pipeline_file_with_options,
    run_pipeline_file_with_options_and_pdf_reader,
};
pub use pipeline_preview::{
    PipelineHostPayload, PipelinePreviewArtifact, PipelinePreviewOptions, PipelinePreviewOutcome,
    PipelinePreviewOutputIdentity, preview_pipeline_value_payloads, validate_pipeline_preview,
};
pub use preview_inputs::{ProjectPreviewInputs, preview_project_value};
pub use primary_document::{
    DEFAULT_MAX_PRIMARY_DOCUMENTS, DocumentFormatError, MAX_PRIMARY_DOCUMENT_OUTPUT_BYTES,
    PrimaryDocumentError, PrimaryDocumentOutput, PrimaryDocumentPolicy, PrimaryInputError,
    check_primary_document, execute_primary_document, execute_primary_document_with_host,
    load_document_project, run_primary_document_file,
};
pub use project_paths::{
    mapping_identity_relative_to as pipeline_mapping_identity, rebase as rebase_project_paths,
    relocates as project_paths_relocate,
};
pub use rest_json::{
    MAX_REST_JSON_BODY_BYTES, MAX_REST_JSON_HEADER_BYTES, MAX_REST_JSON_HEADERS,
    MAX_REST_JSON_URL_BYTES, RestExecutionPolicy, RestJsonError, RestJsonHeader,
    RestJsonMappingOptions, RestJsonMethod, RestJsonRequest, RestJsonResponse, fetch_rest_json,
    plan_rest_json_run, run_project_rest_json_request_file_payloads,
    run_project_value_rest_json_payloads, run_project_value_rest_json_request_file_payloads,
};
pub use route_capability::{
    HostRoute, InputRole, PlannedAcquisition, PlannedPublication, RoutePlan, RouteRefusal,
    RunRouteArgs, plan_file_run, plan_run, select_run_route,
};
pub use stdio::{PreparedStreamRun, StandardIoRunOptions, run_project_with_standard_streams};
pub use trace_json::JsonTraceFile;

/// Typed CSV failures, including recoverable authoring sample limits.
pub use format_csv::CsvFormatError;
/// A bounded, quote-aware CSV sample for GUI schema authoring.
pub use format_csv::CsvSample;

pub fn sample_csv(
    path: &Path,
    delimiter: Option<char>,
    has_headers: bool,
) -> Result<CsvSample, format_csv::CsvFormatError> {
    format_csv::sample(path, delimiter, has_headers)
}

/// Bounded CSV preview with an explicit quote character.
pub fn sample_csv_with_quote(
    path: &Path,
    delimiter: Option<char>,
    quote: Option<char>,
    has_headers: bool,
) -> Result<CsvSample, format_csv::CsvFormatError> {
    format_csv::sample_with_quote(path, delimiter, quote, has_headers)
}

/// Bounded CSV preview with optional quote recognition disabled.
pub fn sample_csv_with_dialect(
    path: &Path,
    delimiter: Option<char>,
    quote: Option<char>,
    quote_disabled: bool,
    has_headers: bool,
) -> Result<CsvSample, format_csv::CsvFormatError> {
    format_csv::sample_with_dialect(path, delimiter, quote, quote_disabled, has_headers)
}

fn absolute_mapping_path(project_path: &Path) -> anyhow::Result<Cow<'_, Path>> {
    if let Ok(canonical) = std::fs::canonicalize(project_path) {
        return Ok(Cow::Owned(canonical));
    }
    if project_path.is_absolute() {
        return Ok(Cow::Borrowed(project_path));
    }
    Ok(Cow::Owned(
        std::env::current_dir()
            .context("resolving the current directory for an unsaved project")?
            .join(project_path),
    ))
}

fn resolve_run_path(
    project_path: &Path,
    explicit_path: Option<&Path>,
    stored_path: Option<&str>,
    argument: &str,
    project_field: &str,
    allow_http: bool,
) -> anyhow::Result<PathBuf> {
    if let Some(path) = explicit_path {
        return Ok(path.to_owned());
    }

    let stored_path = stored_path.filter(|path| !path.trim().is_empty()).with_context(|| {
        format!(
            "no {argument} path is configured; pass `--{argument} <PATH>` or set `{project_field}` in {}",
            project_path.display()
        )
    })?;
    resolve_stored_path(project_path, stored_path, allow_http)
}

fn resolve_stored_path(
    project_path: &Path,
    stored_path: &str,
    allow_http: bool,
) -> anyhow::Result<PathBuf> {
    let stored_path = PathBuf::from(stored_path);
    if http_url(&stored_path).is_some() {
        if allow_http {
            return Ok(stored_path);
        }
        bail!("HTTP output URLs are not supported; configure a local output path");
    }
    if stored_path.is_absolute() {
        return Ok(stored_path);
    }

    let project_dir = project_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(project_dir.join(stored_path))
}

/// Validates a loaded project without reading any instance data.
pub fn validate(project: &mapping::Project) -> Vec<engine::ValidationIssue> {
    engine::validate(project)
}

/// Loads and validates a project file, returning every issue found.
pub fn validate_project(project_path: &Path) -> anyhow::Result<Vec<engine::ValidationIssue>> {
    Ok(validate(&load_project(project_path)?))
}

/// Reads and decodes a project file without validating or running it.
pub fn load_project(project_path: &Path) -> anyhow::Result<mapping::Project> {
    let project_json = std::fs::read_to_string(project_path)
        .with_context(|| format!("reading project file {}", project_path.display()))?;
    mapping::project_file::decode_str(&project_json)
        .with_context(|| format!("parsing project file {}", project_path.display()))
}

fn require_valid(project: &mapping::Project) -> anyhow::Result<()> {
    let issues = validate(project);
    if issues.is_empty() {
        return Ok(());
    }
    let details = issues
        .iter()
        .map(|issue| format!("  - {issue}"))
        .collect::<Vec<_>>()
        .join("\n");
    bail!(
        "project validation failed with {} issue(s):\n{details}",
        issues.len()
    )
}

/// Imports the root element of an XSD file as a `SchemaNode`, printed as
/// pretty JSON -- a starting point for hand-authoring a project file's
/// `source`/`target` schema.
pub fn import_xsd(xsd_path: &Path) -> anyhow::Result<String> {
    let schema = format_xml::xsd::import(xsd_path)
        .with_context(|| format!("importing xsd {}", xsd_path.display()))?;
    Ok(serde_json::to_string_pretty(&schema)?)
}

/// Imports the root of a JSON Schema file as a `SchemaNode`, printed as
/// pretty JSON -- the JSON counterpart to [`import_xsd`].
pub fn import_json_schema(schema_path: &Path) -> anyhow::Result<String> {
    let schema = format_json::json_schema::import(schema_path)
        .with_context(|| format!("importing json schema {}", schema_path.display()))?;
    Ok(serde_json::to_string_pretty(&schema)?)
}

/// Converts an `.mfd` design into a Ferrule project file. Returns
/// the warnings for constructs that could not be converted.
pub fn import_mfd(
    mfd_path: &Path,
    out_path: &Path,
    package_root: Option<&Path>,
    package_manifest: Option<&Path>,
    edi_catalog_roots: &[PathBuf],
    json_schema_catalog_roots: &[PathBuf],
) -> anyhow::Result<Vec<String>> {
    import_mfd_with_profile(
        mfd_path,
        out_path,
        package_root,
        package_manifest,
        edi_catalog_roots,
        json_schema_catalog_roots,
        mfd::ImportProfile::BestEffort,
    )
}

/// Imports a project under an explicit static admission profile. Executable
/// import refuses incomplete designs before the destination is written and
/// does not change legacy global failure-rule ordering.
pub fn import_mfd_with_profile(
    mfd_path: &Path,
    out_path: &Path,
    package_root: Option<&Path>,
    package_manifest: Option<&Path>,
    edi_catalog_roots: &[PathBuf],
    json_schema_catalog_roots: &[PathBuf],
    profile: mfd::ImportProfile,
) -> anyhow::Result<Vec<String>> {
    let options = mfd_import_options(
        package_root,
        package_manifest,
        edi_catalog_roots,
        json_schema_catalog_roots,
    )?;
    publish_mfd_import(mfd_path, out_path, &options, profile)
}

/// Imports a project with an explicit row-error-order choice. Opting in
/// requires executable import before any project file is written.
pub fn import_mfd_with_exception_order(
    mfd_path: &Path,
    out_path: &Path,
    package_root: Option<&Path>,
    package_manifest: Option<&Path>,
    edi_catalog_roots: &[PathBuf],
    json_schema_catalog_roots: &[PathBuf],
    item_ordered_exceptions: bool,
) -> anyhow::Result<Vec<String>> {
    let options = mfd_import_options(
        package_root,
        package_manifest,
        edi_catalog_roots,
        json_schema_catalog_roots,
    )?;
    let (options, profile) = if item_ordered_exceptions {
        (
            options.with_item_ordered_exceptions(),
            mfd::ImportProfile::Executable,
        )
    } else {
        (options, mfd::ImportProfile::BestEffort)
    };
    publish_mfd_import(mfd_path, out_path, &options, profile)
}

fn publish_mfd_import(
    mfd_path: &Path,
    out_path: &Path,
    options: &mfd::ImportOptions,
    profile: mfd::ImportProfile,
) -> anyhow::Result<Vec<String>> {
    let imported = match profile {
        mfd::ImportProfile::BestEffort => mfd::import_with_options(mfd_path, options),
        mfd::ImportProfile::Executable => {
            mfd::import_with_profile(mfd_path, options, profile).map(|outcome| outcome.imported)
        }
    };
    let mut imported = imported.with_context(|| format!("importing {}", mfd_path.display()))?;
    rebase_project_paths(&mut imported.project, &imported.mapping_path, out_path)?;
    let json = mapping::project_file::encode_pretty(&imported.project)?;
    std::fs::write(out_path, json).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(imported.warnings)
}

/// Imports explicit EDI input/JSON output filename endpoints as a document plan.
/// Publication requires complete admission; connected filenames are never rebased.
pub fn import_mfd_filename_document(
    mfd_path: &Path,
    out_path: &Path,
    package_root: Option<&Path>,
    package_manifest: Option<&Path>,
    edi_catalog_roots: &[PathBuf],
    json_schema_catalog_roots: &[PathBuf],
    item_ordered_exceptions: bool,
) -> anyhow::Result<Vec<String>> {
    let mut options = mfd_import_options(
        package_root,
        package_manifest,
        edi_catalog_roots,
        json_schema_catalog_roots,
    )?;
    if item_ordered_exceptions {
        options = options.with_item_ordered_exceptions();
    }
    let imported = mfd::import_filename_document_with_options(mfd_path, &options)?;
    let json = mapping::document_project_file::encode_pretty(&imported.document)?;
    std::fs::write(out_path, json).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(imported.warnings)
}

/// Imports a bounded serial XML `.mfd` design as a runnable typed pipeline.
/// The complete pipeline is validated before its JSON file is written.
pub fn import_mfd_pipeline(
    mfd_path: &Path,
    out_path: &Path,
    package_root: Option<&Path>,
    package_manifest: Option<&Path>,
    edi_catalog_roots: &[PathBuf],
    json_schema_catalog_roots: &[PathBuf],
) -> anyhow::Result<Vec<String>> {
    import_mfd_pipeline_with_exception_order(
        mfd_path,
        out_path,
        package_root,
        package_manifest,
        edi_catalog_roots,
        json_schema_catalog_roots,
        false,
    )
}

/// Imports a pipeline with the same explicit row-error-order choice.
/// Unsupported exception stages refuse without single-project fallback.
pub fn import_mfd_pipeline_with_exception_order(
    mfd_path: &Path,
    out_path: &Path,
    package_root: Option<&Path>,
    package_manifest: Option<&Path>,
    edi_catalog_roots: &[PathBuf],
    json_schema_catalog_roots: &[PathBuf],
    item_ordered_exceptions: bool,
) -> anyhow::Result<Vec<String>> {
    let options = mfd_import_options(
        package_root,
        package_manifest,
        edi_catalog_roots,
        json_schema_catalog_roots,
    )?;
    let options = if item_ordered_exceptions {
        options.with_item_ordered_exceptions()
    } else {
        options
    };
    let mut imported = mfd::import_pipeline_with_options(mfd_path, &options)
        .with_context(|| format!("importing {} as a pipeline", mfd_path.display()))?;
    let active_mapping_path =
        project_paths::mapping_identity_relative_to(&imported.mapping_path, out_path)?;
    imported.pipeline.main_mapping_path = Some(active_mapping_path.clone());
    for stage in &mut imported.pipeline.stages {
        rebase_project_paths(&mut stage.project, &imported.mapping_path, out_path)?;
        stage.mapping_path = Some(active_mapping_path.clone());
    }
    let issues = engine::validate_pipeline(&imported.pipeline);
    if !issues.is_empty() {
        anyhow::bail!(
            "imported pipeline failed validation: {}",
            issues
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    let json = mapping::pipeline_file::encode_pretty(&imported.pipeline)?;
    std::fs::write(out_path, json).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(imported.warnings)
}

fn mfd_import_options(
    package_root: Option<&Path>,
    package_manifest: Option<&Path>,
    edi_catalog_roots: &[PathBuf],
    json_schema_catalog_roots: &[PathBuf],
) -> anyhow::Result<mfd::ImportOptions> {
    let mut options = mfd::ImportOptions::default()
        .with_edi_catalog_roots(edi_catalog_roots.iter().cloned())
        .with_json_schema_catalog_roots(json_schema_catalog_roots.iter().cloned());
    if package_root.is_some() && package_manifest.is_some() {
        anyhow::bail!("package root and package manifest are mutually exclusive");
    }
    if let Some(manifest) = package_manifest {
        options = options.with_package_manifest(manifest)?;
    } else if let Some(root) = package_root {
        options = options.with_package_root(root);
    }
    Ok(options)
}

/// Converts a Ferrule project file into an `.mfd` design (plus
/// generated XSDs next to it). Returns warnings for skipped constructs.
/// Static local instance paths retain their location relative to the saved
/// project when the exported design is written to another directory.
/// Existing callers retain the Ferrule extension profile.
pub fn export_mfd(project_path: &Path, out_path: &Path) -> anyhow::Result<Vec<String>> {
    export_mfd_with_profile(project_path, out_path, mfd::ExportProfile::default())
        .map(|report| report.warnings)
}

/// Renders an export and reports known MFD compatibility dependencies
/// without writing the design, generated schemas, or parent directories.
pub fn preflight_mfd_export(
    project_path: &Path,
    out_path: &Path,
) -> anyhow::Result<mfd::ExportReport> {
    let mut project = load_project(project_path)?;
    rebase_project_paths(&mut project, project_path, out_path)?;
    mfd::preflight_export(&project, out_path)
        .with_context(|| format!("checking export to {}", out_path.display()))
}

/// Inspect a bounded serial XML pipeline export without writing artifacts.
pub fn preflight_mfd_pipeline_export(
    pipeline_path: &Path,
    out_path: &Path,
) -> anyhow::Result<mfd::ExportReport> {
    let encoded = std::fs::read_to_string(pipeline_path)
        .with_context(|| format!("reading {}", pipeline_path.display()))?;
    let mut pipeline = mapping::pipeline_file::decode_str(&encoded)
        .with_context(|| format!("parsing {}", pipeline_path.display()))?;
    for stage in &mut pipeline.stages {
        rebase_project_paths(&mut stage.project, pipeline_path, out_path)?;
    }
    mfd::preflight_pipeline_export(&pipeline, out_path)
        .with_context(|| format!("checking pipeline export to {}", out_path.display()))
}

/// Write a bounded serial XML pipeline as one connected `.mfd` design.
/// Stage instance paths are rebased from the saved pipeline to the design.
pub fn export_mfd_pipeline_with_profile(
    pipeline_path: &Path,
    out_path: &Path,
    profile: mfd::ExportProfile,
) -> anyhow::Result<mfd::ExportReport> {
    let encoded = std::fs::read_to_string(pipeline_path)
        .with_context(|| format!("reading {}", pipeline_path.display()))?;
    let mut pipeline = mapping::pipeline_file::decode_str(&encoded)
        .with_context(|| format!("parsing {}", pipeline_path.display()))?;
    for stage in &mut pipeline.stages {
        rebase_project_paths(&mut stage.project, pipeline_path, out_path)?;
    }
    mfd::export_pipeline_with_profile(&pipeline, out_path, profile)
        .with_context(|| format!("writing {}", out_path.display()))
}

/// Exports under the selected compatibility policy. Native MFD mode
/// rejects known extension dependencies and lossy export warnings before any
/// artifacts are published; the error retains the complete export report.
pub fn export_mfd_with_profile(
    project_path: &Path,
    out_path: &Path,
    profile: mfd::ExportProfile,
) -> anyhow::Result<mfd::ExportReport> {
    let mut project = load_project(project_path)?;
    rebase_project_paths(&mut project, project_path, out_path)?;
    let report = mfd::export_with_profile(&project, out_path, profile)
        .with_context(|| format!("writing {}", out_path.display()))?;
    Ok(report)
}

/// Introspects a SQLite table as a `SchemaNode`, printed as pretty JSON --
/// the database counterpart to [`import_xsd`].
pub fn import_db(db_path: &Path, table: &str) -> anyhow::Result<String> {
    let schema = format_db::introspect(db_path, table)
        .with_context(|| format!("introspecting {} in {}", table, db_path.display()))?;
    Ok(serde_json::to_string_pretty(&schema)?)
}

fn output_format<'a>(
    path: &'a Path,
    schema: &SchemaNode,
    options: &'a FormatOptions,
) -> anyhow::Result<SelectedFormat<'a>> {
    validate_xml_scalar_output_format(path, schema, options)?;
    if options.sqlite_read.is_some() {
        bail!("`sqlite_read` is valid only for mapping sources");
    }
    options
        .validate_xml_schema_hint_options(true)
        .map_err(anyhow::Error::msg)?;
    validate_csv_metadata_identity(path, options, "output")?;
    reject_inactive_root_xml_read_options(path, schema, options, "output")?;
    if options.local_xml_file_set && !options.xml_document {
        bail!("`local_xml_file_set` requires `xml_document` for output");
    }
    if options.local_xml_file_set {
        bail!("`local_xml_file_set` is input-only");
    }
    format_selection::select(path, options, Direction::Output, false)
}

fn write_output(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
) -> anyhow::Result<usize> {
    let selected = output_format(path, schema, options)?;
    if let SelectedFormat::Xbrl(xbrl) = selected {
        format_xbrl::write(path, schema, instance, xbrl)
            .with_context(|| format!("writing XBRL output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Idoc(layout) = selected {
        let formatted = formatted_edi_output(instance, options)?;
        format_edi::idoc::write(path, schema, &formatted, layout)
            .with_context(|| format!("writing SAP IDoc output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Swift(_) = selected {
        let formatted = formatted_edi_output(instance, options)?;
        format_edi::swift::write_with_options(path, schema, &formatted, options)
            .with_context(|| format!("writing SWIFT MT output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Pdf(_) = selected {
        bail!("PDF output is not supported; `pdf` is input-only");
    }
    if let SelectedFormat::FlexText(layout) = selected {
        format_flextext::write(path, schema, instance, layout)
            .with_context(|| format!("writing output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Protobuf(protobuf) = selected {
        let layout =
            protobuf_layout(protobuf).context("parsing embedded Protocol Buffers schema")?;
        format_protobuf::write(path, &layout, &protobuf.root_message, instance)
            .with_context(|| format!("writing output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Edi(Some(kind)) = selected {
        let formatted = formatted_edi_output(instance, options)?;
        return write_edi_output(path, schema, &formatted, options, kind, current_datetime);
    }
    if let SelectedFormat::Xml(Origin::Explicit) = selected {
        format_xml::write_with_options(path, schema, instance, &xml_write_options(options))
            .with_context(|| format!("writing XML output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Json(mode, Origin::Explicit) = selected {
        let write = if mode == JsonMode::Lines {
            format_json::write_lines
        } else if mode == JsonMode::Json5 {
            format_json::write_json5
        } else {
            format_json::write
        };
        write(path, schema, instance)
            .with_context(|| format!("writing JSON output {}", path.display()))?;
        return Ok(instance.as_repeated().map_or(1, <[Instance]>::len));
    }
    if let SelectedFormat::FixedWidth(layout) = selected {
        let rows = instance
            .as_repeated()
            .context("mapping did not produce a repeating row set for a fixed-width output")?;
        format_csv::write_fixed_width(path, schema, rows, layout)
            .with_context(|| format!("writing output {}", path.display()))?;
        return Ok(rows.len());
    }

    match selected {
        SelectedFormat::Csv => {
            let rows = instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for a CSV output")?;
            format_csv::write_with_options(
                path,
                schema,
                rows,
                &format_csv::CsvWriteOptions::from(options),
            )
            .with_context(|| format!("writing output {}", path.display()))?;
            Ok(rows.len())
        }
        SelectedFormat::Xlsx => {
            if let Some(layout) = &options.xlsx_hierarchical {
                if options.xlsx_grid.is_some()
                    || options.xlsx_composite.is_some()
                    || options.xlsx_worksheet_set.is_some()
                    || has_flat_xlsx_layout(options)
                {
                    bail!("`xlsx_hierarchical` cannot be combined with other XLSX layout options");
                }
                if options.xlsx_update_existing {
                    return format_xlsx::update_hierarchical(
                        path,
                        schema,
                        instance,
                        layout,
                        &format_xlsx::XlsxLimits::default(),
                    )
                    .with_context(|| format!("updating output {}", path.display()));
                }
                return format_xlsx::write_hierarchical(path, schema, instance, layout)
                    .with_context(|| format!("writing output {}", path.display()));
            }
            if options.xlsx_grid.is_some() {
                bail!("grid XLSX output is not supported; `xlsx_grid` is input-only");
            }
            if options.xlsx_worksheet_set.is_some() {
                bail!(
                    "worksheet-set XLSX output is not supported; `xlsx_worksheet_set` is input-only"
                );
            }
            if options.xlsx_composite.is_some() {
                bail!("composite XLSX output is not supported; `xlsx_composite` is input-only");
            }
            if !options.xlsx_rows.is_empty() {
                let records = transposed_xlsx_records(instance, options)?;
                format_xlsx::write_transposed(
                    path,
                    schema,
                    records,
                    options.xlsx_sheet.as_deref(),
                    &options.xlsx_rows,
                )
                .with_context(|| format!("writing output {}", path.display()))?;
                return Ok(records.len());
            }
            let rows = instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for an XLSX output")?;
            let result = if options.xlsx_update_existing {
                format_xlsx::update_with_options(
                    path,
                    schema,
                    rows,
                    format_xlsx::FlatTableWriteOptions {
                        sheet: options.xlsx_sheet.as_deref(),
                        start_row: options.xlsx_start_row.unwrap_or(1),
                        columns: &options.xlsx_columns,
                        headers: &options.xlsx_headers,
                        has_header: options.has_header_row.unwrap_or(true),
                    },
                )
            } else {
                format_xlsx::write_with_options(
                    path,
                    schema,
                    rows,
                    format_xlsx::FlatTableWriteOptions {
                        sheet: options.xlsx_sheet.as_deref(),
                        start_row: options.xlsx_start_row.unwrap_or(1),
                        columns: &options.xlsx_columns,
                        headers: &options.xlsx_headers,
                        has_header: options.has_header_row.unwrap_or(true),
                    },
                )
            };
            result.with_context(|| format!("writing output {}", path.display()))?;
            Ok(rows.len())
        }
        SelectedFormat::Xml(Origin::Fallback) => {
            format_xml::write_with_options(path, schema, instance, &xml_write_options(options))
                .with_context(|| format!("writing output {}", path.display()))?;
            Ok(1)
        }
        SelectedFormat::Json(mode, Origin::Fallback) => {
            if mode == JsonMode::Lines {
                format_json::write_lines(path, schema, instance)
            } else if mode == JsonMode::Json5 {
                format_json::write_json5(path, schema, instance)
            } else {
                format_json::write(path, schema, instance)
            }
            .with_context(|| format!("writing output {}", path.display()))?;
            Ok(instance.as_repeated().map_or(1, <[Instance]>::len))
        }
        SelectedFormat::Sqlite => {
            format_db::write_instance(path, schema, instance)
                .with_context(|| format!("writing output {}", path.display()))?;
            Ok(instance.as_repeated().map_or(1, <[Instance]>::len))
        }
        SelectedFormat::Edi(None) => {
            let formatted = formatted_edi_output(instance, options)?;
            match format_edi::dialect_of(schema)? {
                format_edi::Dialect::X12 => {
                    write_x12_output(path, schema, &formatted, options, current_datetime)
                }
                format_edi::Dialect::Edifact => {
                    write_edifact_output(path, schema, &formatted, options, current_datetime)
                }
                format_edi::Dialect::Hl7 => format_edi::hl7::write(path, schema, &formatted),
                format_edi::Dialect::Tradacoms => {
                    format_edi::tradacoms::write(path, schema, &formatted)
                }
            }
            .with_context(|| format!("writing output {}", path.display()))?;
            Ok(1)
        }
        SelectedFormat::MissingPdf => bail!("PDF output is not supported; PDF is input-only"),
        SelectedFormat::MissingIdoc => bail!("unsupported output file extension: .idoc"),
        SelectedFormat::MissingSwift => {
            bail!("SWIFT MT output requires an embedded `swift_mt` layout and message headers")
        }
        SelectedFormat::Unsupported(other) => bail!("unsupported output file extension: .{other}"),
        _ => unreachable!("explicit formats returned above"),
    }
}

fn formatted_edi_output<'a>(
    instance: &'a Instance,
    options: &FormatOptions,
) -> anyhow::Result<Cow<'a, Instance>> {
    let formatted = if options.edi_lexical_formats.is_empty() {
        Cow::Borrowed(instance)
    } else {
        let mut formatted = instance.clone();
        format_edi::apply_output_lexical_formats(&mut formatted, &options.edi_lexical_formats)
            .context("applying EDI output lexical formats")?;
        Cow::Owned(formatted)
    };
    let report = format_edi::validate_values(&formatted, &options.edi_value_constraints)
        .context("validating EDI output values")?;
    if !report.is_empty() {
        return Err(format_edi::EdiFormatError::Validation(report))
            .context("validating EDI output values");
    }
    Ok(formatted)
}

/// Reads any supported instance file into an [`Instance`], shaped by `schema`.
/// A configured PDF, FlexText, or fixed-width layout takes precedence over the
/// extension. CSV, fixed-width text, flat/transposed/grid XLSX, and single-table
/// database inputs arrive wrapped in [`Instance::Repeated`]; composite XLSX,
/// PDF, and database schemas produce their grouped shapes directly.
///
/// Every filesystem-host read passes the execution policy before acquisition.
fn read_instance_with_pdf_reader(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
    reader: Option<&dyn FilesystemPdfReader>,
    execution_policy: &ExecutionPolicy,
) -> anyhow::Result<Instance> {
    if options.sqlite_read.is_some() {
        bail!(
            "declared SQLite query input {} is acquired only by the filesystem run and preview routes",
            path.display()
        );
    }
    execution_policy.authorize_read(execution_policy::read_location(path, options))?;
    options
        .validate_xml_schema_hint_options(false)
        .map_err(anyhow::Error::msg)?;
    validate_csv_metadata_identity(path, options, "input")?;
    reject_inactive_root_xml_read_options(path, schema, options, "input")?;
    if options.local_xml_file_set && !options.xml_document {
        bail!("`local_xml_file_set` requires `xml_document` for input");
    }
    let selected = format_selection::select(path, options, Direction::Input, true)?;
    if let SelectedFormat::Xbrl(xbrl) = selected {
        return format_xbrl::read_with_options(path, schema, xbrl)
            .with_context(|| format!("reading XBRL input {}", path.display()));
    }

    if let SelectedFormat::Idoc(layout) = selected {
        let mut instance = format_edi::idoc::read(path, schema, layout, options.lenient_segments)
            .with_context(|| format!("reading SAP IDoc input {}", path.display()))?;
        format_edi::apply_implied_decimals(&mut instance, &options.edi_implied_decimals)
            .with_context(|| format!("applying EDI numeric formats to {}", path.display()))?;
        return Ok(instance);
    }

    if let SelectedFormat::Swift(layout) = selected {
        let mut instance = format_edi::swift::read(path, schema, layout, options.lenient_segments)
            .with_context(|| format!("reading SWIFT MT input {}", path.display()))?;
        format_edi::apply_implied_decimals(&mut instance, &options.edi_implied_decimals)
            .with_context(|| format!("applying EDI numeric formats to {}", path.display()))?;
        return Ok(instance);
    }

    if let SelectedFormat::External(boundary) = selected {
        if let Some(url) = http_url(path) {
            bail!(
                "external HTTP POST response `{}` must be supplied as a local captured {} file; ferrule does not send POST requests",
                sanitize_url(url),
                match boundary.payload() {
                    ExternalPayloadFormat::Json => "JSON",
                    ExternalPayloadFormat::Xml => "XML",
                }
            );
        }
        return match boundary.payload() {
            ExternalPayloadFormat::Json => format_json::read(path, schema)
                .with_context(|| format!("reading captured JSON response {}", path.display())),
            ExternalPayloadFormat::Xml => format_xml::read(path, schema)
                .with_context(|| format!("reading captured XML response {}", path.display())),
        };
    }

    if let SelectedFormat::Pdf(pdf) = selected {
        let instance = match reader {
            Some(reader) => reader.read_pdf(path, pdf),
            None => format_pdf::read(path, pdf).map_err(anyhow::Error::new),
        };
        return instance.with_context(|| format!("reading input {}", path.display()));
    }

    if let SelectedFormat::FlexText(layout) = selected {
        return format_flextext::read(path, schema, layout)
            .with_context(|| format!("reading input {}", path.display()));
    }

    if let SelectedFormat::Protobuf(protobuf) = selected {
        let layout =
            protobuf_layout(protobuf).context("parsing embedded Protocol Buffers schema")?;
        return format_protobuf::read(path, &layout, &protobuf.root_message)
            .with_context(|| format!("reading Protocol Buffers input {}", path.display()));
    }

    if let SelectedFormat::Edi(Some(kind)) = selected {
        return read_edi_input(path, schema, options, kind);
    }

    if let SelectedFormat::Xml(Origin::Explicit) = selected {
        if options.local_xml_file_set {
            let base = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let pattern = path
                .file_name()
                .map(Path::new)
                .context("local XML file-set input has no filename pattern")?;
            return format_xml::read_local_file_set(
                base,
                pattern,
                schema,
                format_xml::LocalFileSetLimits::default(),
            )
            .map(|loaded| loaded.instance)
            .with_context(|| format!("reading local XML file set {}", path.display()));
        }
        if let Some(url) = http_url(path) {
            return read_http_xml(url, schema, options, execution_policy);
        }
        if let Some(wsdl) = &options.wsdl {
            return format_xml::read_wsdl_message(path, schema, wsdl.operation())
                .with_context(|| format!("reading WSDL message input {}", path.display()));
        }
        return format_xml::read_with_options(path, schema, &xml_read_options(options))
            .with_context(|| format!("reading XML input {}", path.display()));
    }

    if let SelectedFormat::Json(mode, Origin::Explicit) = selected {
        let read = if mode == JsonMode::Lines {
            format_json::read_lines
        } else if mode == JsonMode::Json5 {
            format_json::read_json5
        } else {
            format_json::read
        };
        return read(path, schema)
            .with_context(|| format!("reading JSON input {}", path.display()));
    }

    if let SelectedFormat::HttpXml(url) = selected {
        return read_http_xml(url, schema, options, execution_policy);
    }

    if let SelectedFormat::FixedWidth(layout) = selected {
        let rows = format_csv::read_fixed_width(path, schema, layout)
            .with_context(|| format!("reading input {}", path.display()))?;
        return Ok(Instance::Repeated(rows));
    }

    let instance = match selected {
        SelectedFormat::Csv => {
            let rows = format_csv::read_with_options(
                path,
                schema,
                &format_csv::CsvReadOptions::from(options),
            )
            .with_context(|| format!("reading input {}", path.display()))?;
            Instance::Repeated(rows)
        }
        SelectedFormat::Xlsx => {
            if let Some(layout) = &options.xlsx_hierarchical {
                if options.xlsx_grid.is_some()
                    || options.xlsx_composite.is_some()
                    || options.xlsx_worksheet_set.is_some()
                    || has_legacy_xlsx_layout(options)
                {
                    anyhow::bail!(
                        "`xlsx_hierarchical` cannot be combined with other XLSX layout options"
                    );
                }
                format_xlsx::read_hierarchical(path, schema, layout)
                    .with_context(|| format!("reading input {}", path.display()))?
            } else if let Some(layout) = &options.xlsx_grid {
                if options.xlsx_composite.is_some()
                    || options.xlsx_worksheet_set.is_some()
                    || has_legacy_xlsx_layout(options)
                {
                    anyhow::bail!(
                        "`xlsx_grid` cannot be combined with `xlsx_composite` or legacy XLSX sheet, row, column, transposed, or header options"
                    );
                }
                let rows = format_xlsx::read_grid(path, schema, layout)
                    .with_context(|| format!("reading input {}", path.display()))?;
                Instance::Repeated(rows)
            } else if let Some(layout) = &options.xlsx_worksheet_set {
                if options.xlsx_composite.is_some() || has_legacy_xlsx_layout(options) {
                    anyhow::bail!(
                        "`xlsx_worksheet_set` cannot be combined with `xlsx_composite` or legacy XLSX sheet, row, column, transposed, or header options"
                    );
                }
                format_xlsx::read_worksheet_set(path, schema, layout)
                    .with_context(|| format!("reading input {}", path.display()))?
            } else if let Some(layout) = &options.xlsx_composite {
                if has_legacy_xlsx_layout(options) {
                    anyhow::bail!(
                        "`xlsx_composite` cannot be combined with legacy XLSX sheet, row, column, or header options"
                    );
                }
                format_xlsx::read_composite(path, schema, layout)
                    .with_context(|| format!("reading input {}", path.display()))?
            } else {
                if !options.xlsx_rows.is_empty() && !options.xlsx_headers.is_empty() {
                    anyhow::bail!(
                        "transposed XLSX input cannot be combined with flat header overrides"
                    );
                }
                let rows = if options.xlsx_rows.is_empty() {
                    format_xlsx::read(
                        path,
                        schema,
                        options.xlsx_sheet.as_deref(),
                        options.xlsx_start_row.unwrap_or(1),
                        &options.xlsx_columns,
                        options.has_header_row.unwrap_or(true),
                    )
                } else {
                    format_xlsx::read_transposed(
                        path,
                        schema,
                        options.xlsx_sheet.as_deref(),
                        &options.xlsx_rows,
                    )
                }
                .with_context(|| format!("reading input {}", path.display()))?;
                Instance::Repeated(rows)
            }
        }
        SelectedFormat::Xml(Origin::Fallback) => format_xml::read(path, schema)
            .with_context(|| format!("reading input {}", path.display()))?,
        SelectedFormat::Json(mode, Origin::Fallback) => if mode == JsonMode::Lines {
            format_json::read_lines(path, schema)
        } else if mode == JsonMode::Json5 {
            format_json::read_json5(path, schema)
        } else {
            format_json::read(path, schema)
        }
        .with_context(|| format!("reading input {}", path.display()))?,
        SelectedFormat::Sqlite => format_db::read_instance(path, schema)
            .with_context(|| format!("reading input {}", path.display()))?,
        SelectedFormat::Edi(None) => {
            let read = match format_edi::dialect_of(schema)? {
                format_edi::Dialect::X12 => format_edi::x12::read,
                format_edi::Dialect::Edifact => format_edi::edifact::read,
                format_edi::Dialect::Hl7 => format_edi::hl7::read,
                format_edi::Dialect::Tradacoms => format_edi::tradacoms::read,
            };
            read(path, schema, options.lenient_segments)
                .with_context(|| format!("reading input {}", path.display()))?
        }
        SelectedFormat::MissingIdoc => bail!("SAP IDoc input requires an embedded `idoc` layout"),
        SelectedFormat::MissingSwift => {
            bail!("SWIFT MT input requires an embedded `swift_mt` layout")
        }
        SelectedFormat::MissingPdf => bail!("PDF input requires embedded `pdf` extraction options"),
        SelectedFormat::Unsupported(other) => bail!("unsupported input file extension: .{other}"),
        _ => unreachable!("explicit formats returned above"),
    };
    Ok(instance)
}

fn protobuf_layout(
    options: &ProtobufOptions,
) -> Result<format_protobuf::Layout, format_protobuf::ProtobufError> {
    format_protobuf::Layout::parse_files(
        options.schema_path.as_deref().unwrap_or("root.proto"),
        &options.schema,
        options
            .imports
            .iter()
            .map(|file| (file.path.as_str(), file.source.as_str())),
    )
}

fn http_url(path: &Path) -> Option<&str> {
    let value = path.to_str()?;
    let (scheme, _) = value.split_once("://")?;
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")).then_some(value)
}

fn read_http_xml(
    url: &str,
    schema: &SchemaNode,
    options: &FormatOptions,
    execution_policy: &ExecutionPolicy,
) -> anyhow::Result<Instance> {
    let uri = url
        .parse::<ureq::http::Uri>()
        .map_err(|_| anyhow::anyhow!("invalid HTTP source URL `{}`", sanitize_url(url)))?;
    let scheme = uri.scheme_str().unwrap_or_default();
    let is_http = scheme.eq_ignore_ascii_case("http");
    let is_https = scheme.eq_ignore_ascii_case("https");
    if (!is_http && !is_https) || uri.authority().is_none() {
        bail!("invalid HTTP source URL `{}`", sanitize_url(url));
    }
    if uri
        .authority()
        .is_some_and(|authority| authority.as_str().contains('@'))
    {
        bail!(
            "HTTP source URL `{}` must not contain credentials",
            sanitize_uri(&uri)
        );
    }

    let timeout_seconds = options
        .http_get
        .as_ref()
        .map(|http| u64::from(http.timeout_seconds().get()))
        .unwrap_or(DEFAULT_HTTP_TIMEOUT_SECONDS);
    let display_url = sanitize_uri(&uri);
    // Restricted runs follow no redirect: its target origin is unchecked.
    let max_redirects = if execution_policy.is_restricted() {
        0
    } else {
        MAX_HTTP_REDIRECTS
    };
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_seconds)))
        .max_redirects(max_redirects)
        .max_redirects_will_error(true)
        .max_response_header_size(MAX_HTTP_RESPONSE_HEADER_BYTES)
        .https_only(is_https)
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(url)
        .header("User-Agent", concat!("ferrule/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|error| http_request_error(error, &display_url, timeout_seconds))?;
    if response.status().is_redirection() {
        execution_policy.authorize_capability(engine::RestrictedCapability::HttpRedirect)?;
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_HTTP_RESPONSE_BYTES)
        .read_to_vec()
        .map_err(|error| http_body_error(error, &display_url, timeout_seconds))?;
    let text = String::from_utf8(bytes).map_err(|_| {
        anyhow::anyhow!("HTTP GET {display_url} returned a response that is not UTF-8")
    })?;
    let parsed = match &options.wsdl {
        Some(wsdl) => format_xml::from_wsdl_message_str(&text, schema, wsdl.operation()),
        None => format_xml::from_str(&text, schema),
    };
    parsed.with_context(|| format!("parsing XML response from HTTP GET {display_url}"))
}

fn http_request_error(error: ureq::Error, url: &str, timeout_seconds: u64) -> anyhow::Error {
    match error {
        ureq::Error::StatusCode(status) => {
            anyhow::anyhow!("HTTP GET {url} returned status {status}")
        }
        ureq::Error::Timeout(_) => {
            anyhow::anyhow!("HTTP GET {url} timed out after {timeout_seconds} seconds")
        }
        ureq::Error::TooManyRedirects => {
            anyhow::anyhow!("HTTP GET {url} exceeded {MAX_HTTP_REDIRECTS} redirects")
        }
        ureq::Error::RequireHttpsOnly(_) => {
            anyhow::anyhow!("HTTP GET {url} refused an insecure redirect")
        }
        ureq::Error::LargeResponseHeader(_, _) => anyhow::anyhow!(
            "HTTP GET {url} response headers exceeded {} KiB",
            MAX_HTTP_RESPONSE_HEADER_BYTES / 1024
        ),
        other => anyhow::anyhow!("HTTP GET {url} failed: {other}"),
    }
}

fn http_body_error(error: ureq::Error, url: &str, timeout_seconds: u64) -> anyhow::Error {
    match error {
        ureq::Error::BodyExceedsLimit(_) => anyhow::anyhow!(
            "HTTP GET {url} response exceeded {} MiB",
            MAX_HTTP_RESPONSE_BYTES / (1024 * 1024)
        ),
        other => http_request_error(other, url, timeout_seconds),
    }
}

fn sanitize_url(url: &str) -> String {
    let without_query = url.split_once('?').map_or(url, |(prefix, _)| prefix);
    let without_fragment = without_query
        .split_once('#')
        .map_or(without_query, |(prefix, _)| prefix);
    let Some((scheme, remainder)) = without_fragment.split_once("://") else {
        return without_fragment.to_string();
    };
    let authority_end = remainder.find('/').unwrap_or(remainder.len());
    let (authority, path) = remainder.split_at(authority_end);
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    format!("{scheme}://{authority}{path}")
}

fn sanitize_uri(uri: &ureq::http::Uri) -> String {
    match (uri.scheme_str(), uri.authority()) {
        (Some(scheme), Some(authority)) => {
            let authority = authority.as_str();
            let authority = authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host);
            format!("{scheme}://{authority}{}", uri.path())
        }
        _ => sanitize_url(&uri.to_string()),
    }
}

/// Internal boundary identity and conflict checking. Route admission, metadata
/// prerequisites and codec work deliberately stay at their original call sites.
mod format_selection {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Direction {
        Input,
        Output,
    }

    impl Direction {
        fn label(self) -> &'static str {
            match self {
                Self::Input => "input",
                Self::Output => "output",
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Origin {
        Explicit,
        Fallback,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum JsonMode {
        Document,
        Json5,
        Lines,
    }

    /// Only codec identity is selected here: inferred EDI dialects, layout parsing,
    /// acquisition and stateful/input-only restrictions remain route-local.
    pub(super) enum SelectedFormat<'a> {
        Xbrl(&'a mapping::XbrlBoundaryOptions),
        Idoc(&'a mapping::IdocLayout),
        Swift(&'a mapping::SwiftMtLayout),
        External(&'a mapping::ExternalSourceOptions),
        Pdf(&'a mapping::PdfLayout),
        FlexText(&'a mapping::FlexTextLayout),
        Protobuf(&'a ProtobufOptions),
        Edi(Option<EdiBoundaryKind>),
        Xml(Origin),
        Json(JsonMode, Origin),
        HttpXml(&'a str),
        FixedWidth(&'a mapping::FixedWidthLayout),
        Csv,
        Xlsx,
        Sqlite,
        MissingIdoc,
        MissingSwift,
        MissingPdf,
        Unsupported(String),
    }

    enum Declaration<'a> {
        Selected(SelectedFormat<'a>),
        Json,
    }

    // Preview checks declarations before it has a destination identity. Its legacy
    // input-only target check includes captured sources; ordinary output dispatch
    // never selects them. Fixed-width is checked after the filesystem HTTP fallback.
    fn declaration(
        options: &FormatOptions,
        direction: Direction,
        captured: bool,
    ) -> anyhow::Result<Option<Declaration<'_>>> {
        if let Some(dependency) = options.json5_interchange_blocker() {
            return Err(dependency.clone().into());
        }
        let side = direction.label();
        let selected = if let Some(value) = &options.xbrl {
            reject_xbrl_conflicts(options, side)?;
            SelectedFormat::Xbrl(value)
        } else if let Some(value) = &options.idoc {
            reject_idoc_conflicts(options, side)?;
            SelectedFormat::Idoc(value)
        } else if let Some(value) = &options.swift_mt {
            reject_swift_conflicts(options, side)?;
            SelectedFormat::Swift(value)
        } else if let Some(value) = options.external_source.as_ref().filter(|_| captured) {
            reject_external_source_conflicts(options, side)?;
            SelectedFormat::External(value)
        } else if let Some(value) = &options.pdf {
            reject_pdf_conflicts(options, side)?;
            SelectedFormat::Pdf(value)
        } else if let Some(value) = &options.flextext {
            reject_flextext_conflicts(options, side)?;
            SelectedFormat::FlexText(value)
        } else if let Some(value) = &options.protobuf {
            reject_protobuf_conflicts(options, side)?;
            SelectedFormat::Protobuf(value)
        } else if let Some(value) = options.edi_kind {
            reject_edi_conflicts(options, side)?;
            SelectedFormat::Edi(Some(value))
        } else if options.xml_document {
            reject_xml_conflicts(options, side)?;
            SelectedFormat::Xml(Origin::Explicit)
        } else if options.json_document || options.json5 || options.json_lines {
            reject_json_conflicts(options, side)?;
            return Ok(Some(Declaration::Json));
        } else {
            return Ok(None);
        };
        Ok(Some(Declaration::Selected(selected)))
    }

    pub(super) fn validate_declared(
        options: &FormatOptions,
        direction: Direction,
    ) -> anyhow::Result<bool> {
        if declaration(options, direction, true)?.is_some() {
            return Ok(true);
        }
        if options.fixed_width.is_some() {
            reject_fixed_width_csv_options(options, direction.label())?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) fn select<'a>(
        path: &'a Path,
        options: &'a FormatOptions,
        direction: Direction,
        filesystem_input: bool,
    ) -> anyhow::Result<SelectedFormat<'a>> {
        if let Some(declaration) = declaration(options, direction, direction == Direction::Input)? {
            return match declaration {
                Declaration::Selected(selected) => Ok(selected),
                Declaration::Json => Ok(SelectedFormat::Json(
                    json_mode(path, options, Origin::Explicit)?,
                    Origin::Explicit,
                )),
            };
        }
        if filesystem_input
            && direction == Direction::Input
            && let Some(url) = http_url(path)
        {
            return Ok(SelectedFormat::HttpXml(url));
        }
        if let Some(layout) = &options.fixed_width {
            reject_fixed_width_csv_options(options, direction.label())?;
            return Ok(SelectedFormat::FixedWidth(layout));
        }
        validate_tabular_fallback(path, options, direction.label())?;
        Ok(match extension_for_dispatch(path, options)?.as_str() {
            "csv" | "txt" => SelectedFormat::Csv,
            "xlsx" => SelectedFormat::Xlsx,
            "xml" => SelectedFormat::Xml(Origin::Fallback),
            "json" | "json5" | "jsonl" | "ndjson" => SelectedFormat::Json(
                json_mode(path, options, Origin::Fallback)?,
                Origin::Fallback,
            ),
            "db" | "sqlite" | "sqlite3" => SelectedFormat::Sqlite,
            "edi" | "x12" | "edifact" | "hl7" => SelectedFormat::Edi(None),
            "idoc" => SelectedFormat::MissingIdoc,
            "fin" | "swift" => SelectedFormat::MissingSwift,
            "pdf" => SelectedFormat::MissingPdf,
            other => SelectedFormat::Unsupported(other.to_owned()),
        })
    }

    /// Preview historically checks JSON flags even on a captured JSON source,
    /// whose runtime reader uses the captured-response codec unconditionally.
    pub(super) fn select_preview<'a>(
        path: &'a Path,
        options: &'a FormatOptions,
        direction: Direction,
    ) -> anyhow::Result<SelectedFormat<'a>> {
        let selected = select(path, options, direction, false)?;
        if options.json_document || options.json5 || options.json_lines {
            json5_selected(path, options)?;
        }
        Ok(selected)
    }

    fn json_mode(path: &Path, options: &FormatOptions, origin: Origin) -> anyhow::Result<JsonMode> {
        let json5 = json5_selected(path, options)?;
        let lines = options.json_lines
            || (origin == Origin::Fallback
                && matches!(extension_of(path)?.as_str(), "jsonl" | "ndjson"));
        if lines && json5 {
            bail!("JSON5 cannot be combined with JSON Lines");
        }
        Ok(if lines {
            JsonMode::Lines
        } else if json5 {
            JsonMode::Json5
        } else {
            JsonMode::Document
        })
    }

    pub(super) fn validate_csv_metadata_identity(
        path: &Path,
        options: &FormatOptions,
        side: &str,
    ) -> anyhow::Result<()> {
        format_csv::require_executable_dependency(options.csv_text_repair_dependency)?;
        if options.csv_preserve_empty_strings
            && !matches!(
                extension_for_dispatch(path, options)?.as_str(),
                "csv" | "txt"
            )
        {
            bail!("`csv_preserve_empty_strings` requires a CSV {side} boundary");
        }
        if options.csv_utf8_bom
            && !matches!(
                extension_for_dispatch(path, options)?.as_str(),
                "csv" | "txt"
            )
        {
            bail!("`csv_utf8_bom` requires a CSV {side} boundary");
        }
        Ok(())
    }

    fn reject_fixed_width_csv_options(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
        if options.delimiter.is_some()
            || options.csv_quote.is_some()
            || options.csv_quote_disabled
            || options.csv_utf8_bom
            || options.csv_preserve_empty_strings
            || options.has_header_row.is_some()
            || options.xml_document
            || options.local_xml_file_set
        {
            bail!(
                "`fixed_width` cannot be combined with `delimiter`, `csv_quote`, `has_header_row`, \
             `xml_document`, or `local_xml_file_set` for {side}"
            );
        }
        Ok(())
    }

    fn reject_idoc_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
        if options
            .edi_kind
            .is_some_and(|kind| kind != EdiBoundaryKind::Idoc)
            || options.delimiter.is_some()
            || options.csv_quote.is_some()
            || options.csv_quote_disabled
            || options.csv_utf8_bom
            || options.csv_preserve_empty_strings
            || options.has_header_row.is_some()
            || options.fixed_width.is_some()
            || options.flextext.is_some()
            || options.http_get.is_some()
            || options.external_source.is_some()
            || options.xml_document
            || options.local_xml_file_set
            || options.json_document
            || options.json5
            || options.json_lines
            || options.pdf.is_some()
            || options.protobuf.is_some()
            || options.xbrl.is_some()
            || options.swift_mt.is_some()
            || has_any_xlsx_layout(options)
        {
            bail!("`idoc` cannot be combined with another format's options for {side}");
        }
        Ok(())
    }

    fn reject_swift_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
        if options
            .edi_kind
            .is_some_and(|kind| kind != EdiBoundaryKind::SwiftMt)
            || options.delimiter.is_some()
            || options.csv_quote.is_some()
            || options.csv_quote_disabled
            || options.csv_utf8_bom
            || options.csv_preserve_empty_strings
            || options.has_header_row.is_some()
            || options.fixed_width.is_some()
            || options.flextext.is_some()
            || options.http_get.is_some()
            || options.external_source.is_some()
            || options.idoc.is_some()
            || options.xml_document
            || options.local_xml_file_set
            || options.json_document
            || options.json5
            || options.json_lines
            || options.pdf.is_some()
            || options.protobuf.is_some()
            || options.xbrl.is_some()
            || has_any_xlsx_layout(options)
        {
            bail!("`swift_mt` cannot be combined with another format's options for {side}");
        }
        Ok(())
    }

    fn reject_xbrl_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
        if options.lenient_segments
            || options.edi_kind.is_some()
            || options.delimiter.is_some()
            || options.csv_quote.is_some()
            || options.csv_quote_disabled
            || options.csv_utf8_bom
            || options.csv_preserve_empty_strings
            || options.has_header_row.is_some()
            || options.fixed_width.is_some()
            || options.flextext.is_some()
            || options.idoc.is_some()
            || options.swift_mt.is_some()
            || options.http_get.is_some()
            || options.external_source.is_some()
            || options.xml_document
            || options.local_xml_file_set
            || options.json_document
            || options.json5
            || options.json_lines
            || options.pdf.is_some()
            || options.protobuf.is_some()
            || has_any_xlsx_layout(options)
        {
            bail!("`xbrl` cannot be combined with another format's options for {side}");
        }
        Ok(())
    }

    fn reject_protobuf_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
        if options.lenient_segments
            || options.edi_kind.is_some()
            || options.delimiter.is_some()
            || options.csv_quote.is_some()
            || options.csv_quote_disabled
            || options.csv_utf8_bom
            || options.csv_preserve_empty_strings
            || options.has_header_row.is_some()
            || options.fixed_width.is_some()
            || options.flextext.is_some()
            || options.idoc.is_some()
            || options.swift_mt.is_some()
            || options.http_get.is_some()
            || options.external_source.is_some()
            || options.xml_document
            || options.local_xml_file_set
            || options.json_document
            || options.json5
            || options.json_lines
            || options.pdf.is_some()
            || options.xbrl.is_some()
            || has_any_xlsx_layout(options)
        {
            bail!("`protobuf` cannot be combined with another format's options for {side}");
        }
        Ok(())
    }

    fn reject_flextext_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
        if options.lenient_segments
            || options.edi_kind.is_some()
            || options.delimiter.is_some()
            || options.csv_quote.is_some()
            || options.csv_quote_disabled
            || options.csv_utf8_bom
            || options.csv_preserve_empty_strings
            || options.has_header_row.is_some()
            || options.fixed_width.is_some()
            || options.idoc.is_some()
            || options.swift_mt.is_some()
            || options.http_get.is_some()
            || options.external_source.is_some()
            || options.xml_document
            || options.local_xml_file_set
            || options.json_document
            || options.json5
            || options.json_lines
            || options.pdf.is_some()
            || options.protobuf.is_some()
            || options.xbrl.is_some()
            || has_any_xlsx_layout(options)
        {
            bail!("`flextext` cannot be combined with another format's options for {side}");
        }
        Ok(())
    }

    fn reject_pdf_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
        if options.lenient_segments
            || options.edi_kind.is_some()
            || options.delimiter.is_some()
            || options.csv_quote.is_some()
            || options.csv_quote_disabled
            || options.csv_utf8_bom
            || options.csv_preserve_empty_strings
            || options.has_header_row.is_some()
            || options.fixed_width.is_some()
            || options.flextext.is_some()
            || options.idoc.is_some()
            || options.swift_mt.is_some()
            || options.http_get.is_some()
            || options.external_source.is_some()
            || options.xml_document
            || options.local_xml_file_set
            || options.json_document
            || options.json5
            || options.json_lines
            || options.protobuf.is_some()
            || options.xbrl.is_some()
            || has_any_xlsx_layout(options)
        {
            bail!("`pdf` cannot be combined with another format's options for {side}");
        }
        Ok(())
    }

    fn reject_edi_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
        match options.edi_config_reference.as_ref() {
            Some(EdiConfigDependency::ExternalReference(reference)) => {
                bail!(
                    "{side} requires unresolved external EDI configuration `{reference}`; \
                 import from a package containing the exact configuration or a matching release pack"
                );
            }
            Some(EdiConfigDependency::MissingConfiguration) => {
                bail!(
                    "{side} requires an EDI configuration because its imported entry-tree schema is \
                 untyped; supply a complete EDI configuration and re-import the mapping"
                );
            }
            None => {}
        }
        let kind = options
            .edi_kind
            .context("missing EDI boundary kind during runtime dispatch")?;
        if matches!(kind, EdiBoundaryKind::Idoc | EdiBoundaryKind::SwiftMt) {
            bail!("`edi_kind` `{kind:?}` requires its embedded runtime layout for {side}");
        }
        if options.x12_separators.is_some() && kind != EdiBoundaryKind::X12 {
            bail!("X12 separator metadata requires `edi_kind` `X12` for {side}");
        }
        if options.x12_interchange_version.is_some() && kind != EdiBoundaryKind::X12 {
            bail!("X12 interchange version requires `edi_kind` `X12` for {side}");
        }
        if options.idoc.is_some()
            || options.swift_mt.is_some()
            || options.delimiter.is_some()
            || options.csv_quote.is_some()
            || options.csv_quote_disabled
            || options.csv_utf8_bom
            || options.csv_preserve_empty_strings
            || options.has_header_row.is_some()
            || options.fixed_width.is_some()
            || options.flextext.is_some()
            || options.http_get.is_some()
            || options.external_source.is_some()
            || options.xml_document
            || options.local_xml_file_set
            || options.json_document
            || options.json5
            || options.json_lines
            || options.pdf.is_some()
            || options.protobuf.is_some()
            || options.xbrl.is_some()
            || has_any_xlsx_layout(options)
        {
            bail!("`edi_kind` cannot be combined with another format's options for {side}");
        }
        Ok(())
    }

    fn reject_json_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
        if options.lenient_segments
            || options.edi_kind.is_some()
            || options.idoc.is_some()
            || options.swift_mt.is_some()
            || options.delimiter.is_some()
            || options.csv_quote.is_some()
            || options.csv_quote_disabled
            || options.csv_utf8_bom
            || options.csv_preserve_empty_strings
            || options.has_header_row.is_some()
            || options.fixed_width.is_some()
            || options.flextext.is_some()
            || options.http_get.is_some()
            || options.external_source.is_some()
            || options.xml_document
            || options.local_xml_file_set
            || options.pdf.is_some()
            || options.protobuf.is_some()
            || options.xbrl.is_some()
            || has_any_xlsx_layout(options)
        {
            bail!(
                "`json_document`/`json5`/`json_lines` cannot be combined with another format's options for {side}"
            );
        }
        Ok(())
    }

    pub(super) fn json5_selected(path: &Path, options: &FormatOptions) -> anyhow::Result<bool> {
        let selected = options.json5
            || path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json5"));
        if selected && options.json_lines {
            bail!("JSON5 cannot be combined with JSON Lines");
        }
        Ok(selected)
    }

    pub(super) fn reject_inactive_root_xml_read_options(
        path: &Path,
        schema: &SchemaNode,
        options: &FormatOptions,
        side: &str,
    ) -> anyhow::Result<()> {
        if !options.xml_allow_inactive_root_type_members && !options.xml_root_view_read_policy {
            return Ok(());
        }
        let accepted_options = FormatOptions {
            xml_document: true,
            xml_allow_inactive_root_type_members: true,
            xml_root_view_read_policy: options.xml_root_view_read_policy,
            ..Default::default()
        };
        if side != "input"
            || !options.xml_document
            || !options.xml_allow_inactive_root_type_members
            || *options != accepted_options
            || http_url(path).is_some()
            || !ir::xml_inactive_root_type_members_are_supported(schema)
            || (options.xml_root_view_read_policy
                && !ir::xml_root_view_read_policy_is_supported(schema))
        {
            bail!(
                "`xml_allow_inactive_root_type_members` requires a closed flat typed local XML input with no other format options"
            );
        }
        Ok(())
    }

    pub(super) fn xml_read_options(options: &FormatOptions) -> format_xml::XmlReadOptions {
        format_xml::XmlReadOptions {
            allow_inactive_root_type_members: options.xml_allow_inactive_root_type_members,
            root_view_policy: options.xml_root_view_read_policy,
        }
    }

    fn reject_xml_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
        let external_xml = options
            .external_source
            .as_ref()
            .is_some_and(|boundary| boundary.payload() == ExternalPayloadFormat::Xml);
        if options.lenient_segments
            || options.edi_kind.is_some()
            || options.idoc.is_some()
            || options.swift_mt.is_some()
            || options.delimiter.is_some()
            || options.csv_quote.is_some()
            || options.csv_quote_disabled
            || options.csv_utf8_bom
            || options.csv_preserve_empty_strings
            || options.has_header_row.is_some()
            || options.fixed_width.is_some()
            || options.flextext.is_some()
            || options.pdf.is_some()
            || options.json_document
            || options.json5
            || options.json_lines
            || options.protobuf.is_some()
            || options.xbrl.is_some()
            || has_any_xlsx_layout(options)
            || (options.external_source.is_some() && !external_xml)
            || (options.local_xml_file_set && side == "output")
            || (options.local_xml_file_set
                && (options.http_get.is_some() || options.external_source.is_some()))
            || (side == "output" && options.http_get.is_some())
            || (options.http_get.is_some() && options.external_source.is_some())
        {
            bail!("`xml_document` cannot be combined with another format's options for {side}");
        }
        Ok(())
    }

    fn reject_external_source_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
        let boundary = options
            .external_source
            .as_ref()
            .context("missing captured external source metadata")?;
        let owns_identity = match boundary.payload() {
            ExternalPayloadFormat::Json => !options.xml_document,
            ExternalPayloadFormat::Xml => {
                !options.json_document && !options.json5 && !options.json_lines
            }
        };
        if !owns_identity
            || options.lenient_segments
            || options.edi_kind.is_some()
            || options.idoc.is_some()
            || options.swift_mt.is_some()
            || options.delimiter.is_some()
            || options.csv_quote.is_some()
            || options.csv_quote_disabled
            || options.csv_utf8_bom
            || options.csv_preserve_empty_strings
            || options.has_header_row.is_some()
            || options.fixed_width.is_some()
            || options.flextext.is_some()
            || options.http_get.is_some()
            || options.local_xml_file_set
            || options.pdf.is_some()
            || options.protobuf.is_some()
            || options.xbrl.is_some()
            || has_any_xlsx_layout(options)
        {
            bail!(
                "captured external source metadata conflicts with another format's options for {side}"
            );
        }
        Ok(())
    }
}

fn read_edi_input(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
) -> anyhow::Result<Instance> {
    let mut instance = match kind {
        EdiBoundaryKind::X12 => format_edi::x12::read_with_separators(
            path,
            schema,
            options.lenient_segments,
            options.x12_separators.map(x12_separators),
        ),
        EdiBoundaryKind::Edifact => {
            format_edi::edifact::read(path, schema, options.lenient_segments)
        }
        EdiBoundaryKind::Hl7 => format_edi::hl7::read(path, schema, options.lenient_segments),
        EdiBoundaryKind::Tradacoms => {
            format_edi::tradacoms::read(path, schema, options.lenient_segments)
        }
        EdiBoundaryKind::Idoc | EdiBoundaryKind::SwiftMt => {
            bail!("EDI boundary `{kind:?}` requires an embedded runtime layout")
        }
    }
    .with_context(|| format!("reading input {}", path.display()))?;
    format_edi::apply_implied_decimals(&mut instance, &options.edi_implied_decimals)
        .with_context(|| format!("applying EDI numeric formats to {}", path.display()))?;
    Ok(instance)
}

fn write_edi_output(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
    current_datetime: &str,
) -> anyhow::Result<usize> {
    match kind {
        EdiBoundaryKind::X12 => write_x12_output(path, schema, instance, options, current_datetime),
        EdiBoundaryKind::Edifact => {
            write_edifact_output(path, schema, instance, options, current_datetime)
        }
        EdiBoundaryKind::Hl7 => format_edi::hl7::write(path, schema, instance),
        EdiBoundaryKind::Tradacoms => format_edi::tradacoms::write(path, schema, instance),
        EdiBoundaryKind::Idoc => {
            bail!("SAP IDoc output requires an embedded runtime layout")
        }
        EdiBoundaryKind::SwiftMt => {
            bail!("SWIFT MT output requires an embedded `swift_mt` layout")
        }
    }
    .with_context(|| format!("writing EDI output {}", path.display()))?;
    Ok(1)
}

fn write_x12_output(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
) -> Result<(), format_edi::EdiFormatError> {
    let separators = options
        .x12_separators
        .map(x12_separators)
        .unwrap_or_default();
    let version = options.x12_interchange_version.as_deref();
    match options.edi_autocomplete.as_ref() {
        Some(EdiAutocomplete::X12(config)) => format_edi::x12::write_with_syntax_and_autocomplete(
            path,
            schema,
            instance,
            separators,
            version,
            format_edi::x12::Autocomplete {
                current_datetime,
                request_acknowledgement: config.request_acknowledgement,
                transaction_set: config.transaction_set.as_deref(),
            },
        ),
        _ => format_edi::x12::write_with_syntax(path, schema, instance, separators, version),
    }
}

fn write_edifact_output(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
) -> Result<(), format_edi::EdiFormatError> {
    if let Some(EdiAutocomplete::Edifact(config)) = options.edi_autocomplete.as_ref() {
        format_edi::edifact::write_with_autocomplete(
            path,
            schema,
            instance,
            format_edi::edifact::Autocomplete {
                current_datetime,
                syntax_level: config.syntax_level.as_deref(),
                syntax_version: config.syntax_version.as_deref(),
                controlling_agency: config.controlling_agency.as_deref(),
                message_type: config.message_type.as_deref(),
            },
        )
    } else {
        format_edi::edifact::write(path, schema, instance)
    }
}

fn x12_separators(separators: mapping::X12Separators) -> format_edi::x12::Separators {
    format_edi::x12::Separators {
        element: separators.element,
        component: separators.component,
        segment: separators.segment,
        repetition: separators.repetition,
        release: separators.release,
    }
}

fn has_any_xlsx_layout(options: &FormatOptions) -> bool {
    has_legacy_xlsx_layout(options)
        || options.xlsx_composite.is_some()
        || options.xlsx_worksheet_set.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
}

fn has_legacy_xlsx_layout(options: &FormatOptions) -> bool {
    has_flat_xlsx_layout(options) || options.xlsx_update_existing
}

/// The records of a new-workbook transposed XLSX target. Engine validation
/// rejects these combinations first; this guards direct library callers.
fn transposed_xlsx_records<'a>(
    instance: &'a Instance,
    options: &FormatOptions,
) -> anyhow::Result<&'a [Instance]> {
    if options.xlsx_update_existing {
        bail!(
            "`xlsx_update_existing` cannot update a transposed XLSX layout; transposed targets write a new workbook"
        );
    }
    if options.has_header_row == Some(true)
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_headers.is_empty()
    {
        bail!(
            "transposed XLSX output cannot be combined with flat header, start-row, or column options"
        );
    }
    instance
        .as_repeated()
        .context("mapping did not produce a repeating record set for a transposed XLSX output")
}

/// Flat and transposed table options. Update-existing is not among them
/// because hierarchical targets accept it too.
fn has_flat_xlsx_layout(options: &FormatOptions) -> bool {
    options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_headers.is_empty()
        || !options.xlsx_rows.is_empty()
        || options.has_header_row.is_some()
}

fn extension_of(path: &Path) -> anyhow::Result<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .with_context(|| format!("{} has no file extension", path.display()))
}

fn extension_for_dispatch(path: &Path, options: &FormatOptions) -> anyhow::Result<String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_lowercase);
    match (extension, options.tabular_kind) {
        (Some(extension), _) if is_recognized_instance_extension(&extension) => Ok(extension),
        (_, Some(TabularBoundaryKind::Csv)) => Ok("csv".to_string()),
        (_, Some(TabularBoundaryKind::Xlsx)) => Ok("xlsx".to_string()),
        (Some(extension), None) => Ok(extension),
        (None, None) => bail!("{} has no file extension", path.display()),
    }
}

fn validate_tabular_fallback(
    path: &Path,
    options: &FormatOptions,
    side: &str,
) -> anyhow::Result<()> {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_lowercase)
        .as_deref()
        .is_some_and(is_recognized_instance_extension)
    {
        return Ok(());
    }
    match options.tabular_kind {
        Some(TabularBoundaryKind::Csv) if has_xlsx_specific_layout(options) => {
            bail!("CSV fallback identity cannot be combined with XLSX layout options for {side}")
        }
        Some(TabularBoundaryKind::Xlsx)
            if options.delimiter.is_some()
                || options.csv_quote.is_some()
                || options.csv_quote_disabled
                || options.csv_utf8_bom
                || options.csv_preserve_empty_strings =>
        {
            bail!("XLSX fallback identity cannot be combined with CSV dialect options for {side}")
        }
        Some(_) | None => Ok(()),
    }
}

fn has_xlsx_specific_layout(options: &FormatOptions) -> bool {
    options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_headers.is_empty()
        || options.xlsx_update_existing
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_worksheet_set.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
}

fn is_recognized_instance_extension(extension: &str) -> bool {
    matches!(
        extension,
        "csv"
            | "txt"
            | "xlsx"
            | "xml"
            | "json"
            | "json5"
            | "jsonl"
            | "ndjson"
            | "db"
            | "sqlite"
            | "sqlite3"
            | "edi"
            | "x12"
            | "edifact"
            | "hl7"
            | "idoc"
            | "fin"
            | "swift"
            | "pdf"
            | "xbrl"
    )
}

fn xml_write_options(options: &FormatOptions) -> format_xml::XmlWriteOptions {
    format_xml::XmlWriteOptions {
        scalar_output: options.xml_scalar_output.clone(),
        schema_hints: options.xml_schema_hints.clone(),
        ..Default::default()
    }
}

fn validate_xml_scalar_output_format(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> anyhow::Result<()> {
    if let Some(policy) = &options.xml_scalar_output {
        options
            .validate_xml_scalar_output_options()
            .map_err(anyhow::Error::msg)?;
        policy.validate_schema(schema)?;
        if path
            .extension()
            .is_some_and(|extension| !extension.eq_ignore_ascii_case("xml"))
        {
            bail!("explicit XML scalar output cannot be used with a non-XML output path");
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn owned_source_test_project() -> mapping::Project {
    mapping::Project {
        source: ir::SchemaNode::group(
            "Input",
            vec![ir::SchemaNode::scalar("Value", ir::ScalarType::String)],
        ),
        target: ir::SchemaNode::group(
            "Output",
            vec![ir::SchemaNode::scalar("Value", ir::ScalarType::String)],
        ),
        source_path: Some("input.json".into()),
        target_path: Some("output.json".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: mapping::Graph {
            nodes: [(
                9,
                mapping::Node::OwnedSourceField(mapping::OwnedSourceField {
                    source: mapping::OwnedSourceRef {
                        owner: mapping::SourceOwner::Primary,
                        path: vec!["Value".into()],
                    },
                    frame: None,
                }),
            )]
            .into(),
        },
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 9,
            }],
            ..mapping::Scope::default()
        },
    }
}

#[cfg(test)]
pub(crate) fn closed_owned_source_test_project() -> mapping::Project {
    let mut project = owned_source_test_project();
    let function = mapping::FunctionId::new(7);
    let owned = project.graph.nodes[&9].clone();
    project.user_functions.insert(
        function,
        mapping::UserFunction {
            library: "tests".into(),
            name: "closed_owned_value".into(),
            description: None,
            parameters: vec![],
            output_name: "value".into(),
            output_type: ir::ScalarType::String,
            body: mapping::Graph {
                nodes: [(9, owned)].into(),
            },
            output: 9,
        },
    );
    project.graph.nodes.insert(
        9,
        mapping::Node::UserFunctionCall {
            function,
            args: vec![],
        },
    );
    project
}

#[cfg(test)]
pub(crate) fn owned_collection_with_closed_source_function_test_project() -> mapping::Project {
    let mut project = closed_owned_source_test_project();
    project.root.iteration = mapping::ScopeIteration::OwnedSource {
        source: mapping::OwnedSourceRef {
            owner: mapping::SourceOwner::Primary,
            path: vec![],
        },
        output_path: None,
    };
    project
}

#[cfg(test)]
pub(crate) fn owned_host_test_directory(label: &str) -> anyhow::Result<std::path::PathBuf> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "ferrule-owned-host-{label}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory)?;
    Ok(directory)
}

#[cfg(test)]
mod owned_source_filesystem_admission_tests {
    use super::*;

    #[test]
    fn owned_refusal_precedes_input_open_and_publication_callback() {
        let project = closed_owned_source_test_project();
        let published = std::cell::Cell::new(0);
        let publish = || {
            published.set(published.get() + 1);
            true
        };
        let options = RunOptions {
            before_publish: Some(&publish),
            ..RunOptions::default()
        };
        let error = run_project_value_with_filesystem_policy(
            &project,
            Path::new("nonexistent-owned-input-parent/project.json"),
            &options,
            None,
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
        assert_eq!(published.get(), 0);
    }
}

#[cfg(test)]
pub(crate) fn owned_collection_test_project() -> mapping::Project {
    let mut project = owned_source_test_project();
    project.graph.nodes.insert(
        9,
        mapping::Node::Const {
            value: ir::Value::String("legacy".into()),
        },
    );
    project.root.iteration = mapping::ScopeIteration::OwnedSource {
        source: mapping::OwnedSourceRef {
            owner: mapping::SourceOwner::Primary,
            path: vec![],
        },
        output_path: None,
    };
    project
}

#[cfg(test)]
mod owned_collection_filesystem_admission_tests {
    use super::*;

    #[test]
    fn closed_source_function_refusal_with_owned_iteration_precedes_input_open_and_publication_callback()
     {
        let project = owned_collection_with_closed_source_function_test_project();
        let published = std::cell::Cell::new(0);
        let publish = || {
            published.set(published.get() + 1);
            true
        };
        let options = RunOptions {
            before_publish: Some(&publish),
            ..RunOptions::default()
        };
        let error = run_project_value_with_filesystem_policy(
            &project,
            Path::new("nonexistent-owned-input-parent/project.json"),
            &options,
            None,
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
        assert_eq!(published.get(), 0);
    }
}

#[cfg(test)]
pub(crate) fn owned_collection_host_positive_project() -> mapping::Project {
    let mut project = owned_collection_test_project();
    project.source.repeating = true;
    project.target.repeating = true;
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project
}

#[cfg(test)]
mod owned_basic_filesystem_transition_tests {
    use super::*;
    #[test]
    fn basic_field_and_collection_execute_with_publication_and_malformed_precedence()
    -> anyhow::Result<()> {
        for collection in [false, true] {
            let directory = owned_host_test_directory("filesystem")?;
            let input = directory.join("input.json");
            let output = directory.join("output.json");
            let path = directory.join("project.json");
            let project = if collection {
                owned_collection_host_positive_project()
            } else {
                owned_source_test_project()
            };
            let bytes: &[u8] = if collection {
                br#"[{"Value":"one"},{"Value":"two"}]"#
            } else {
                br#"{"Value":"supported"}"#
            };
            std::fs::write(&input, bytes)?;
            std::fs::write(&output, b"old-output")?;
            let calls = std::cell::Cell::new(0);
            let publish = || {
                calls.set(calls.get() + 1);
                true
            };
            let options = RunOptions::new().with_before_publish(&publish);
            let outcome =
                run_project_value_with_filesystem_policy(&project, &path, &options, None)?;
            let actual: serde_json::Value = serde_json::from_slice(&std::fs::read(&output)?)?;
            let expected = if collection {
                serde_json::json!([{"Value":"legacy"},{"Value":"legacy"}])
            } else {
                serde_json::json!({"Value":"supported"})
            };
            assert_eq!(actual, expected);
            assert_eq!(outcome.records_written, if collection { 2 } else { 1 });
            assert_eq!(calls.get(), 1);
            std::fs::write(&output, b"old-output")?;
            std::fs::write(&input, b"not JSON")?;
            calls.set(0);
            let error = run_project_value_with_filesystem_policy(&project, &path, &options, None)
                .unwrap_err();
            assert!(error.downcast_ref::<engine::EngineError>().is_none());
            assert_eq!(
                error.to_string(),
                format!(
                    "{} {}",
                    if collection {
                        "reading JSON input"
                    } else {
                        "reading input"
                    },
                    input.display()
                )
            );
            assert_eq!(calls.get(), 0);
            assert_eq!(std::fs::read(&output)?.as_slice(), b"old-output".as_slice());
            std::fs::write(&input, bytes)?;
            let cancel = || false;
            let error = run_project_value_with_filesystem_policy(
                &project,
                &path,
                &RunOptions::new().with_before_publish(&cancel),
                None,
            )
            .unwrap_err();
            assert_eq!(
                error.downcast_ref::<engine::EngineError>(),
                Some(&engine::EngineError::DebugCancelled)
            );
            assert_eq!(std::fs::read(&output)?.as_slice(), b"old-output".as_slice());
            std::fs::remove_file(input)?;
            std::fs::remove_file(output)?;
            std::fs::remove_dir(directory)?;
        }
        Ok(())
    }
}
