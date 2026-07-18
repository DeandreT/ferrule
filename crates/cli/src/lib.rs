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

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use ir::{Instance, SchemaNode};
use mapping::{EdiBoundaryKind, ExternalPayloadFormat, FormatOptions};

const DEFAULT_HTTP_TIMEOUT_SECONDS: u64 = 30;
const MAX_HTTP_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_HTTP_RESPONSE_HEADER_BYTES: usize = 64 * 1024;
const MAX_HTTP_REDIRECTS: u32 = 5;

/// Result of running a project after resolving its input and output paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub records_written: usize,
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub extra_outputs: Vec<WrittenOutput>,
}

/// One additional target file written during a project run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenOutput {
    pub name: String,
    pub records_written: usize,
    pub path: PathBuf,
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
    let project = load_project(project_path)?;
    require_valid(&project)?;

    let input_path = resolve_run_path(
        project_path,
        input_path,
        project.source_path.as_deref(),
        "input",
        "source_path",
        true,
    )?;
    let output_path = resolve_run_path(
        project_path,
        output_path,
        project.target_path.as_deref(),
        "output",
        "target_path",
        false,
    )?;
    let extra_output_paths = project
        .extra_targets
        .iter()
        .map(|target| {
            let stored = target
                .path
                .as_deref()
                .filter(|path| !path.trim().is_empty())
                .with_context(|| {
                    format!("extra target `{}` has no stored output path", target.name)
                })?;
            resolve_stored_path(project_path, stored, false)
                .with_context(|| format!("resolving extra target `{}` output", target.name))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let source_instance = read_instance(&input_path, &project.source, &project.source_options)?;

    let project_dir = project_path.parent().unwrap_or_else(|| Path::new("."));
    let mut extras = Vec::with_capacity(project.extra_sources.len());
    for extra in project
        .extra_sources
        .iter()
        .filter(|extra| extra.dynamic_path.is_none())
    {
        let path = PathBuf::from(&extra.path);
        let path = if path.is_absolute() || http_url(&path).is_some() {
            path
        } else {
            project_dir.join(path)
        };
        extras.push((
            extra.name.clone(),
            read_instance(&path, &extra.schema, &extra.options)
                .with_context(|| format!("loading extra source `{}`", extra.name))?,
        ));
    }

    let runtime_project_path = std::fs::canonicalize(project_path)
        .with_context(|| format!("resolving project path {}", project_path.display()))?;
    let current_datetime = jiff::Zoned::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
        .to_string();
    let dynamic_loader = ProjectDynamicSourceLoader::new(project_dir, &project.extra_sources);
    let execution = engine::ExecutionContext::new(&runtime_project_path)
        .with_current_datetime(&current_datetime)
        .with_dynamic_source_loader(&dynamic_loader);
    let outputs = engine::run_outputs_with_sources_and_context(
        &project,
        &source_instance,
        extras,
        &execution,
    )?;

    let engine::ExecutionOutputs {
        primary,
        extras: target_outputs,
    } = outputs;
    let row_count = write_output(
        &output_path,
        &project.target,
        &primary,
        &project.target_options,
    )?;
    if target_outputs.len() != project.extra_targets.len() {
        bail!("engine returned an unexpected number of additional target values");
    }
    let mut extra_outputs = Vec::with_capacity(project.extra_targets.len());
    for ((target, path), output) in project
        .extra_targets
        .iter()
        .zip(extra_output_paths)
        .zip(target_outputs)
    {
        let records_written =
            write_output(&path, &target.schema, &output.instance, &target.options)
                .with_context(|| format!("writing extra target `{}`", target.name))?;
        extra_outputs.push(WrittenOutput {
            name: output.name,
            records_written,
            path,
        });
    }

    Ok(RunOutcome {
        records_written: row_count,
        input_path,
        output_path,
        extra_outputs,
    })
}

struct ProjectDynamicSourceLoader<'a> {
    project_dir: &'a Path,
    sources: &'a [mapping::NamedSource],
    cache: RefCell<BTreeMap<(String, String), Arc<Instance>>>,
}

impl<'a> ProjectDynamicSourceLoader<'a> {
    fn new(project_dir: &'a Path, sources: &'a [mapping::NamedSource]) -> Self {
        Self {
            project_dir,
            sources,
            cache: RefCell::new(BTreeMap::new()),
        }
    }
}

impl engine::DynamicSourceLoader for ProjectDynamicSourceLoader<'_> {
    fn load(&self, source_name: &str, path: &str) -> Result<Arc<Instance>, String> {
        let key = (source_name.to_string(), path.to_string());
        if let Some(instance) = self.cache.borrow().get(&key).cloned() {
            return Ok(instance);
        }
        let source = self
            .sources
            .iter()
            .find(|source| source.name == source_name)
            .ok_or_else(|| format!("dynamic source `{source_name}` is not declared"))?;
        let path = PathBuf::from(path);
        let resolved = if path.is_absolute() || http_url(&path).is_some() {
            path
        } else {
            self.project_dir.join(path)
        };
        let instance = Arc::new(
            read_instance(&resolved, &source.schema, &source.options)
                .map_err(|error| error.to_string())?,
        );
        self.cache.borrow_mut().insert(key, Arc::clone(&instance));
        Ok(instance)
    }
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

fn load_project(project_path: &Path) -> anyhow::Result<mapping::Project> {
    let project_json = std::fs::read_to_string(project_path)
        .with_context(|| format!("reading project file {}", project_path.display()))?;
    serde_json::from_str(&project_json)
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

/// Converts a MapForce `.mfd` design into a ferrule project file. Returns
/// the warnings for constructs that could not be converted.
pub fn import_mfd(mfd_path: &Path, out_path: &Path) -> anyhow::Result<Vec<String>> {
    let imported =
        mfd::import(mfd_path).with_context(|| format!("importing {}", mfd_path.display()))?;
    let json = serde_json::to_string_pretty(&imported.project)?;
    std::fs::write(out_path, json).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(imported.warnings)
}

/// Converts a ferrule project file into a MapForce `.mfd` design (plus
/// generated XSDs next to it). Returns warnings for skipped constructs.
pub fn export_mfd(project_path: &Path, out_path: &Path) -> anyhow::Result<Vec<String>> {
    let project = load_project(project_path)?;
    let warnings = mfd::export(&project, out_path)
        .with_context(|| format!("writing {}", out_path.display()))?;
    Ok(warnings)
}

/// Introspects a SQLite table as a `SchemaNode`, printed as pretty JSON --
/// the database counterpart to [`import_xsd`].
pub fn import_db(db_path: &Path, table: &str) -> anyhow::Result<String> {
    let schema = format_db::introspect(db_path, table)
        .with_context(|| format!("introspecting {} in {}", table, db_path.display()))?;
    Ok(serde_json::to_string_pretty(&schema)?)
}

fn write_output(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> anyhow::Result<usize> {
    if options.local_xml_file_set && !options.xml_document {
        bail!("`local_xml_file_set` requires `xml_document` for output");
    }
    if options.local_xml_file_set {
        bail!("`local_xml_file_set` is input-only");
    }
    if options.xbrl.is_some() {
        reject_xbrl_conflicts(options, "output")?;
        let xbrl = options
            .xbrl
            .as_ref()
            .context("missing XBRL target options")?;
        format_xbrl::write(path, schema, instance, xbrl)
            .with_context(|| format!("writing XBRL output {}", path.display()))?;
        return Ok(1);
    }
    if options.idoc.is_some() {
        reject_idoc_conflicts(options, "output")?;
        bail!("SAP IDoc output is not supported; `idoc` is input-only");
    }
    if options.swift_mt.is_some() {
        reject_swift_conflicts(options, "output")?;
        bail!("SWIFT MT output is not supported; `swift_mt` is input-only");
    }
    if options.pdf.is_some() {
        reject_pdf_conflicts(options, "output")?;
        bail!("PDF output is not supported; `pdf` is input-only");
    }
    if let Some(layout) = &options.flextext {
        reject_flextext_conflicts(options, "output")?;
        format_flextext::write(path, schema, instance, layout)
            .with_context(|| format!("writing output {}", path.display()))?;
        return Ok(1);
    }
    if let Some(protobuf) = &options.protobuf {
        reject_protobuf_conflicts(options, "output")?;
        let layout = format_protobuf::Layout::parse(&protobuf.schema)
            .context("parsing embedded Protocol Buffers schema")?;
        format_protobuf::write(path, &layout, &protobuf.root_message, instance)
            .with_context(|| format!("writing output {}", path.display()))?;
        return Ok(1);
    }
    if let Some(kind) = options.edi_kind {
        reject_edi_conflicts(options, "output")?;
        return write_edi_output(path, schema, instance, options, kind);
    }
    if options.xml_document {
        reject_xml_conflicts(options, "output")?;
        format_xml::write(path, schema, instance)
            .with_context(|| format!("writing XML output {}", path.display()))?;
        return Ok(1);
    }
    if options.json_document || options.json_lines {
        reject_json_conflicts(options, "output")?;
        let write = if options.json_lines {
            format_json::write_lines
        } else {
            format_json::write
        };
        write(path, schema, instance)
            .with_context(|| format!("writing JSON output {}", path.display()))?;
        return Ok(instance.as_repeated().map_or(1, <[Instance]>::len));
    }
    if let Some(layout) = &options.fixed_width {
        reject_fixed_width_csv_options(options, "output")?;
        let rows = instance
            .as_repeated()
            .context("mapping did not produce a repeating row set for a fixed-width output")?;
        format_csv::write_fixed_width(path, schema, rows, layout)
            .with_context(|| format!("writing output {}", path.display()))?;
        return Ok(rows.len());
    }

    match extension_of(path)?.as_str() {
        "csv" | "txt" => {
            let rows = instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for a CSV output")?;
            format_csv::write(
                path,
                schema,
                rows,
                options.delimiter,
                options.has_header_row.unwrap_or(true),
            )
            .with_context(|| format!("writing output {}", path.display()))?;
            Ok(rows.len())
        }
        "xlsx" => {
            if let Some(layout) = &options.xlsx_hierarchical {
                if options.xlsx_grid.is_some()
                    || options.xlsx_composite.is_some()
                    || has_legacy_xlsx_layout(options)
                {
                    bail!("`xlsx_hierarchical` cannot be combined with other XLSX layout options");
                }
                return format_xlsx::write_hierarchical(path, schema, instance, layout)
                    .with_context(|| format!("writing output {}", path.display()));
            }
            if options.xlsx_grid.is_some() {
                bail!("grid XLSX output is not supported; `xlsx_grid` is input-only");
            }
            if options.xlsx_composite.is_some() {
                bail!("composite XLSX output is not supported; `xlsx_composite` is input-only");
            }
            if !options.xlsx_rows.is_empty() {
                bail!("transposed XLSX output is not supported; `xlsx_rows` is input-only");
            }
            let rows = instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for an XLSX output")?;
            let write = if options.xlsx_update_existing {
                format_xlsx::update
            } else {
                format_xlsx::write
            };
            write(
                path,
                schema,
                rows,
                options.xlsx_sheet.as_deref(),
                options.xlsx_start_row.unwrap_or(1),
                &options.xlsx_columns,
                options.has_header_row.unwrap_or(true),
            )
            .with_context(|| format!("writing output {}", path.display()))?;
            Ok(rows.len())
        }
        "xml" => {
            format_xml::write(path, schema, instance)
                .with_context(|| format!("writing output {}", path.display()))?;
            Ok(1)
        }
        "json" | "jsonl" | "ndjson" => {
            let json_lines =
                options.json_lines || matches!(extension_of(path)?.as_str(), "jsonl" | "ndjson");
            if json_lines {
                format_json::write_lines(path, schema, instance)
            } else {
                format_json::write(path, schema, instance)
            }
            .with_context(|| format!("writing output {}", path.display()))?;
            Ok(instance.as_repeated().map_or(1, <[Instance]>::len))
        }
        "db" | "sqlite" | "sqlite3" => {
            format_db::write_instance(path, schema, instance)
                .with_context(|| format!("writing output {}", path.display()))?;
            Ok(instance.as_repeated().map_or(1, <[Instance]>::len))
        }
        "edi" | "x12" | "edifact" | "hl7" => {
            let write = match format_edi::dialect_of(schema)? {
                format_edi::Dialect::X12 => format_edi::x12::write,
                format_edi::Dialect::Edifact => format_edi::edifact::write,
                format_edi::Dialect::Hl7 => bail!("HL7 output is not yet supported"),
                format_edi::Dialect::Tradacoms => {
                    bail!("TRADACOMS output is not yet supported")
                }
            };
            write(path, schema, instance)
                .with_context(|| format!("writing output {}", path.display()))?;
            Ok(1)
        }
        "pdf" => bail!("PDF output is not supported; PDF is input-only"),
        other => bail!("unsupported output file extension: .{other}"),
    }
}

/// Reads any supported instance file into an [`Instance`], shaped by `schema`.
/// A configured PDF, FlexText, or fixed-width layout takes precedence over the
/// extension. CSV, fixed-width text, flat/transposed/grid XLSX, and single-table
/// database inputs arrive wrapped in [`Instance::Repeated`]; composite XLSX,
/// PDF, and database schemas produce their grouped shapes directly.
fn read_instance(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> anyhow::Result<Instance> {
    if options.local_xml_file_set && !options.xml_document {
        bail!("`local_xml_file_set` requires `xml_document` for input");
    }
    if options.xbrl.is_some() {
        reject_xbrl_conflicts(options, "input")?;
        let xbrl = options
            .xbrl
            .as_ref()
            .context("missing XBRL source options")?;
        return format_xbrl::read_with_options(path, schema, xbrl)
            .with_context(|| format!("reading XBRL input {}", path.display()));
    }

    if let Some(layout) = &options.idoc {
        reject_idoc_conflicts(options, "input")?;
        return format_edi::idoc::read(path, schema, layout, options.lenient_segments)
            .with_context(|| format!("reading SAP IDoc input {}", path.display()));
    }

    if let Some(layout) = &options.swift_mt {
        reject_swift_conflicts(options, "input")?;
        return format_edi::swift::read(path, schema, layout, options.lenient_segments)
            .with_context(|| format!("reading SWIFT MT input {}", path.display()));
    }

    if let Some(boundary) = &options.external_source {
        reject_external_source_conflicts(options, "input")?;
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

    if let Some(pdf) = &options.pdf {
        reject_pdf_conflicts(options, "input")?;
        return format_pdf::read(path, pdf)
            .with_context(|| format!("reading input {}", path.display()));
    }

    if let Some(layout) = &options.flextext {
        reject_flextext_conflicts(options, "input")?;
        return format_flextext::read(path, schema, layout)
            .with_context(|| format!("reading input {}", path.display()));
    }

    if options.protobuf.is_some() {
        reject_protobuf_conflicts(options, "input")?;
        bail!("Protocol Buffers input is not supported; `protobuf` is output-only");
    }

    if let Some(kind) = options.edi_kind {
        reject_edi_conflicts(options, "input")?;
        return read_edi_input(path, schema, options, kind);
    }

    if options.xml_document {
        reject_xml_conflicts(options, "input")?;
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
            return read_http_xml(url, schema, options);
        }
        return format_xml::read(path, schema)
            .with_context(|| format!("reading XML input {}", path.display()));
    }

    if options.json_document || options.json_lines {
        reject_json_conflicts(options, "input")?;
        let read = if options.json_lines {
            format_json::read_lines
        } else {
            format_json::read
        };
        return read(path, schema)
            .with_context(|| format!("reading JSON input {}", path.display()));
    }

    if let Some(url) = http_url(path) {
        return read_http_xml(url, schema, options);
    }

    if let Some(layout) = &options.fixed_width {
        reject_fixed_width_csv_options(options, "input")?;
        let rows = format_csv::read_fixed_width(path, schema, layout)
            .with_context(|| format!("reading input {}", path.display()))?;
        return Ok(Instance::Repeated(rows));
    }

    let instance = match extension_of(path)?.as_str() {
        "csv" | "txt" => {
            let rows = format_csv::read(
                path,
                schema,
                options.delimiter,
                options.has_header_row.unwrap_or(true),
            )
            .with_context(|| format!("reading input {}", path.display()))?;
            Instance::Repeated(rows)
        }
        "xlsx" => {
            if options.xlsx_hierarchical.is_some() {
                anyhow::bail!(
                    "hierarchical XLSX input is not supported; `xlsx_hierarchical` is output-only"
                );
            } else if let Some(layout) = &options.xlsx_grid {
                if options.xlsx_composite.is_some() || has_legacy_xlsx_layout(options) {
                    anyhow::bail!(
                        "`xlsx_grid` cannot be combined with `xlsx_composite` or legacy XLSX sheet, row, column, transposed, or header options"
                    );
                }
                let rows = format_xlsx::read_grid(path, schema, layout)
                    .with_context(|| format!("reading input {}", path.display()))?;
                Instance::Repeated(rows)
            } else if let Some(layout) = &options.xlsx_composite {
                if has_legacy_xlsx_layout(options) {
                    anyhow::bail!(
                        "`xlsx_composite` cannot be combined with legacy XLSX sheet, row, column, or header options"
                    );
                }
                format_xlsx::read_composite(path, schema, layout)
                    .with_context(|| format!("reading input {}", path.display()))?
            } else {
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
        "xml" => format_xml::read(path, schema)
            .with_context(|| format!("reading input {}", path.display()))?,
        "json" | "jsonl" | "ndjson" => {
            let json_lines =
                options.json_lines || matches!(extension_of(path)?.as_str(), "jsonl" | "ndjson");
            if json_lines {
                format_json::read_lines(path, schema)
            } else {
                format_json::read(path, schema)
            }
            .with_context(|| format!("reading input {}", path.display()))?
        }
        "db" | "sqlite" | "sqlite3" => format_db::read_instance(path, schema)
            .with_context(|| format!("reading input {}", path.display()))?,
        "edi" | "x12" | "edifact" | "hl7" => {
            let read = match format_edi::dialect_of(schema)? {
                format_edi::Dialect::X12 => format_edi::x12::read,
                format_edi::Dialect::Edifact => format_edi::edifact::read,
                format_edi::Dialect::Hl7 => format_edi::hl7::read,
                format_edi::Dialect::Tradacoms => format_edi::tradacoms::read,
            };
            read(path, schema, options.lenient_segments)
                .with_context(|| format!("reading input {}", path.display()))?
        }
        "idoc" => bail!("SAP IDoc input requires an embedded `idoc` layout"),
        "fin" | "swift" => bail!("SWIFT MT input requires an embedded `swift_mt` layout"),
        "pdf" => bail!("PDF input requires embedded `pdf` extraction options"),
        other => bail!("unsupported input file extension: .{other}"),
    };
    Ok(instance)
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
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_seconds)))
        .max_redirects(MAX_HTTP_REDIRECTS)
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
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_HTTP_RESPONSE_BYTES)
        .read_to_vec()
        .map_err(|error| http_body_error(error, &display_url, timeout_seconds))?;
    let text = String::from_utf8(bytes).map_err(|_| {
        anyhow::anyhow!("HTTP GET {display_url} returned a response that is not UTF-8")
    })?;
    format_xml::from_str(&text, schema)
        .with_context(|| format!("parsing XML response from HTTP GET {display_url}"))
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

fn reject_fixed_width_csv_options(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    if options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.xml_document
        || options.local_xml_file_set
    {
        bail!(
            "`fixed_width` cannot be combined with `delimiter`, `has_header_row`, \
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
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
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
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.idoc.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
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
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
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
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
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
            "`json_document`/`json_lines` cannot be combined with another format's options for {side}"
        );
    }
    Ok(())
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
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.pdf.is_some()
        || options.json_document
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
        ExternalPayloadFormat::Xml => !options.json_document && !options.json_lines,
    };
    if !owns_identity
        || options.lenient_segments
        || options.edi_kind.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.delimiter.is_some()
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

fn read_edi_input(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
) -> anyhow::Result<Instance> {
    let instance = match kind {
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
    };
    instance.with_context(|| format!("reading input {}", path.display()))
}

fn write_edi_output(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
) -> anyhow::Result<usize> {
    match kind {
        EdiBoundaryKind::X12 => format_edi::x12::write_with_syntax(
            path,
            schema,
            instance,
            options
                .x12_separators
                .map(x12_separators)
                .unwrap_or_default(),
            options.x12_interchange_version.as_deref(),
        ),
        EdiBoundaryKind::Edifact => format_edi::edifact::write(path, schema, instance),
        EdiBoundaryKind::Hl7 => bail!("HL7 output is not yet supported"),
        EdiBoundaryKind::Tradacoms => bail!("TRADACOMS output is not yet supported"),
        EdiBoundaryKind::Idoc => bail!("SAP IDoc output is not supported; IDoc is input-only"),
        EdiBoundaryKind::SwiftMt => {
            bail!("SWIFT MT output is not supported; SWIFT MT is input-only")
        }
    }
    .with_context(|| format!("writing EDI output {}", path.display()))?;
    Ok(1)
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
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
}

fn has_legacy_xlsx_layout(options: &FormatOptions) -> bool {
    options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_rows.is_empty()
        || options.has_header_row.is_some()
}

fn extension_of(path: &Path) -> anyhow::Result<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .with_context(|| format!("{} has no file extension", path.display()))
}
