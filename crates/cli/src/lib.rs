//! Headless runner: loads a mapping project and runs it against an input
//! file (delimited/fixed-width text, XLSX, XML, JSON, SQLite, or X12 EDI,
//! chosen by extension and format options) to produce an output file. Split
//! out from `main.rs` so it's testable
//! without shelling out to the built binary.
//!
//! For SQLite (`.db`/`.sqlite`/`.sqlite3`) the table name is the project's
//! source/target schema root `name`. For EDI (`.edi`/`.x12`/`.edifact`)
//! the schema describes the segment/loop structure and picks the dialect
//! by its first segment (ISA = X12, UNB = EDIFACT) -- see `format_edi`.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use ir::{Instance, SchemaNode};
use mapping::FormatOptions;

/// Result of running a project after resolving its input and output paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub records_written: usize,
    pub input_path: PathBuf,
    pub output_path: PathBuf,
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
    )?;
    let output_path = resolve_run_path(
        project_path,
        output_path,
        project.target_path.as_deref(),
        "output",
        "target_path",
    )?;

    let source_instance = read_instance(&input_path, &project.source, &project.source_options)?;

    let project_dir = project_path.parent().unwrap_or_else(|| Path::new("."));
    let mut extras = Vec::with_capacity(project.extra_sources.len());
    for extra in &project.extra_sources {
        let path = PathBuf::from(&extra.path);
        let path = if path.is_absolute() {
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
    let execution = engine::ExecutionContext::new(&runtime_project_path)
        .with_current_datetime(&current_datetime);
    let target_instance =
        engine::run_with_sources_and_context(&project, &source_instance, extras, &execution)?;

    let row_count = if let Some(layout) = &project.target_options.fixed_width {
        reject_fixed_width_csv_options(&project.target_options, "output")?;
        let rows = target_instance
            .as_repeated()
            .context("mapping did not produce a repeating row set for a fixed-width output")?;
        format_csv::write_fixed_width(&output_path, &project.target, rows, layout)
            .with_context(|| format!("writing output {}", output_path.display()))?;
        rows.len()
    } else {
        match extension_of(&output_path)?.as_str() {
            "csv" | "txt" => {
                let rows = target_instance
                    .as_repeated()
                    .context("mapping did not produce a repeating row set for a CSV output")?;
                format_csv::write(
                    &output_path,
                    &project.target,
                    rows,
                    project.target_options.delimiter,
                    project.target_options.has_header_row.unwrap_or(true),
                )
                .with_context(|| format!("writing output {}", output_path.display()))?;
                rows.len()
            }
            "xlsx" => {
                if project.target_options.xlsx_grid.is_some() {
                    anyhow::bail!("grid XLSX output is not supported; `xlsx_grid` is input-only");
                }
                if project.target_options.xlsx_composite.is_some() {
                    anyhow::bail!(
                        "composite XLSX output is not supported; `xlsx_composite` is input-only"
                    );
                }
                if !project.target_options.xlsx_rows.is_empty() {
                    anyhow::bail!(
                        "transposed XLSX output is not supported; `xlsx_rows` is input-only"
                    );
                }
                let rows = target_instance
                    .as_repeated()
                    .context("mapping did not produce a repeating row set for an XLSX output")?;
                format_xlsx::write(
                    &output_path,
                    &project.target,
                    rows,
                    project.target_options.xlsx_sheet.as_deref(),
                    project.target_options.xlsx_start_row.unwrap_or(1),
                    &project.target_options.xlsx_columns,
                    project.target_options.has_header_row.unwrap_or(true),
                )
                .with_context(|| format!("writing output {}", output_path.display()))?;
                rows.len()
            }
            "xml" => {
                format_xml::write(&output_path, &project.target, &target_instance)
                    .with_context(|| format!("writing output {}", output_path.display()))?;
                1
            }
            "json" | "jsonl" | "ndjson" => {
                let json_lines = project.target_options.json_lines
                    || matches!(extension_of(&output_path)?.as_str(), "jsonl" | "ndjson");
                if json_lines {
                    format_json::write_lines(&output_path, &project.target, &target_instance)
                } else {
                    format_json::write(&output_path, &project.target, &target_instance)
                }
                .with_context(|| format!("writing output {}", output_path.display()))?;
                target_instance.as_repeated().map_or(1, <[Instance]>::len)
            }
            "db" | "sqlite" | "sqlite3" => {
                let rows = target_instance
                    .as_repeated()
                    .context("mapping did not produce a repeating row set for a database output")?;
                format_db::write(&output_path, &project.target, rows)
                    .with_context(|| format!("writing output {}", output_path.display()))?;
                rows.len()
            }
            "edi" | "x12" | "edifact" => {
                let write = match format_edi::dialect_of(&project.target)? {
                    format_edi::Dialect::X12 => format_edi::x12::write,
                    format_edi::Dialect::Edifact => format_edi::edifact::write,
                };
                write(&output_path, &project.target, &target_instance)
                    .with_context(|| format!("writing output {}", output_path.display()))?;
                1
            }
            other => bail!("unsupported output file extension: .{other}"),
        }
    };

    Ok(RunOutcome {
        records_written: row_count,
        input_path,
        output_path,
    })
}

fn resolve_run_path(
    project_path: &Path,
    explicit_path: Option<&Path>,
    stored_path: Option<&str>,
    argument: &str,
    project_field: &str,
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
    let stored_path = PathBuf::from(stored_path);
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

/// Reads any supported instance file into an [`Instance`], shaped by `schema`.
/// A configured fixed-width layout takes precedence over the extension. CSV,
/// fixed-width text, flat/transposed/grid XLSX, and single-table database
/// inputs arrive wrapped in [`Instance::Repeated`]; composite XLSX and database
/// schemas produce their grouped shapes directly.
fn read_instance(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> anyhow::Result<Instance> {
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
            if let Some(layout) = &options.xlsx_grid {
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
        "edi" | "x12" | "edifact" => {
            let read = match format_edi::dialect_of(schema)? {
                format_edi::Dialect::X12 => format_edi::x12::read,
                format_edi::Dialect::Edifact => format_edi::edifact::read,
            };
            read(path, schema, options.lenient_segments)
                .with_context(|| format!("reading input {}", path.display()))?
        }
        other => bail!("unsupported input file extension: .{other}"),
    };
    Ok(instance)
}

fn reject_fixed_width_csv_options(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    if options.delimiter.is_some() || options.has_header_row.is_some() {
        bail!(
            "`fixed_width` cannot be combined with CSV `delimiter` or `has_header_row` options for {side}"
        );
    }
    Ok(())
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
