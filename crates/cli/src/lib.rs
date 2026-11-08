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

use anyhow::{Context, bail};
use format_output::{
    formatted_edi_output, output_format, transposed_xlsx_records,
    validate_xml_scalar_output_format, write_output, xml_write_options,
};
use format_selection::{
    Direction, JsonMode, Origin, SelectedFormat, reject_inactive_root_xml_read_options,
    validate_csv_metadata_identity, xml_read_options,
};
use http_input::{http_url, read_http_xml, sanitize_url};
use ir::{Instance, SchemaNode};
use mapping::{
    EdiBoundaryKind, EdiConfigDependency, ExternalPayloadFormat, FormatOptions, ProtobufOptions,
    TabularBoundaryKind,
};

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
mod format_output;
mod http_input;
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

    Ok(project_directory(project_path).join(stored_path))
}

fn project_directory(project_path: &Path) -> &Path {
    project_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
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
        if !options.edi_value_constraints.is_empty() {
            let mut instance =
                format_edi::document::read_source(path, schema, options, EdiBoundaryKind::Idoc)
                    .with_context(|| format!("reading SAP IDoc input {}", path.display()))?;
            format_edi::apply_implied_decimals(&mut instance, &options.edi_implied_decimals)
                .with_context(|| format!("applying EDI numeric formats to {}", path.display()))?;
            return Ok(instance);
        }
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
            if !options.edi_value_constraints.is_empty() {
                return format_edi::document::read_source(
                    path,
                    schema,
                    options,
                    inferred_edi_kind(schema)?,
                )
                .with_context(|| format!("reading input {}", path.display()));
            }
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

mod format_selection;

fn read_edi_input(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
) -> anyhow::Result<Instance> {
    if !options.edi_value_constraints.is_empty() {
        let mut instance = format_edi::document::read_source(path, schema, options, kind)
            .with_context(|| format!("reading input {}", path.display()))?;
        format_edi::apply_implied_decimals(&mut instance, &options.edi_implied_decimals)
            .with_context(|| format!("applying EDI numeric formats to {}", path.display()))?;
        return Ok(instance);
    }
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

fn inferred_edi_kind(schema: &SchemaNode) -> Result<EdiBoundaryKind, format_edi::EdiFormatError> {
    Ok(match format_edi::dialect_of(schema)? {
        format_edi::Dialect::X12 => EdiBoundaryKind::X12,
        format_edi::Dialect::Edifact => EdiBoundaryKind::Edifact,
        format_edi::Dialect::Hl7 => EdiBoundaryKind::Hl7,
        format_edi::Dialect::Tradacoms => EdiBoundaryKind::Tradacoms,
    })
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

#[cfg(test)]
pub(crate) fn owned_source_test_project() -> mapping::Project {
    mapping::Project {
        execution_contract: None,
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
pub(crate) fn owned_host_test_directory(label: &str) -> anyhow::Result<test_support::TempDir> {
    Ok(test_support::TempDir::new(&format!("owned-host-{label}"))?)
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
pub(crate) fn owned_collection_host_positive_project() -> mapping::Project {
    let mut project = owned_collection_test_project();
    project.source.repeating = true;
    project.target.repeating = true;
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project
}

#[cfg(test)]
mod owned_basic_filesystem_transition_tests;
#[cfg(test)]
mod owned_collection_filesystem_admission_tests;
#[cfg(test)]
mod owned_source_filesystem_admission_tests;
