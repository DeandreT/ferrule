//! Headless runner: loads a mapping project and runs it against an input
//! file (CSV, XML, JSON, or SQLite, chosen by extension) to produce an
//! output file. Split out from `main.rs` so it's testable without shelling
//! out to the built binary.
//!
//! For SQLite (`.db`/`.sqlite`/`.sqlite3`) the table name is the project's
//! source/target schema root `name`.

use std::path::Path;

use anyhow::{Context, bail};
use ir::Instance;

/// Loads the project at `project_path`, runs it against `input_path`, and
/// writes the result to `output_path`. Returns the number of top-level
/// records written (rows for a CSV output, 1 for an XML document).
pub fn run_project(
    project_path: &Path,
    input_path: &Path,
    output_path: &Path,
) -> anyhow::Result<usize> {
    let project_json = std::fs::read_to_string(project_path)
        .with_context(|| format!("reading project file {}", project_path.display()))?;
    let project: mapping::Project = serde_json::from_str(&project_json)
        .with_context(|| format!("parsing project file {}", project_path.display()))?;

    let source_instance = match extension_of(input_path)? {
        "csv" => {
            let rows = format_csv::read(input_path, &project.source)
                .with_context(|| format!("reading input {}", input_path.display()))?;
            Instance::Repeated(rows)
        }
        "xml" => format_xml::read(input_path, &project.source)
            .with_context(|| format!("reading input {}", input_path.display()))?,
        "json" => format_json::read(input_path, &project.source)
            .with_context(|| format!("reading input {}", input_path.display()))?,
        "db" | "sqlite" | "sqlite3" => {
            let rows = format_db::read(input_path, &project.source)
                .with_context(|| format!("reading input {}", input_path.display()))?;
            Instance::Repeated(rows)
        }
        other => bail!("unsupported input file extension: .{other}"),
    };

    let target_instance = engine::run(&project, &source_instance)?;

    let row_count = match extension_of(output_path)? {
        "csv" => {
            let rows = target_instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for a CSV output")?;
            format_csv::write(output_path, &project.target, rows)
                .with_context(|| format!("writing output {}", output_path.display()))?;
            rows.len()
        }
        "xml" => {
            format_xml::write(output_path, &project.target, &target_instance)
                .with_context(|| format!("writing output {}", output_path.display()))?;
            1
        }
        "json" => {
            format_json::write(output_path, &project.target, &target_instance)
                .with_context(|| format!("writing output {}", output_path.display()))?;
            target_instance.as_repeated().map_or(1, <[Instance]>::len)
        }
        "db" | "sqlite" | "sqlite3" => {
            let rows = target_instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for a database output")?;
            format_db::write(output_path, &project.target, rows)
                .with_context(|| format!("writing output {}", output_path.display()))?;
            rows.len()
        }
        other => bail!("unsupported output file extension: .{other}"),
    };

    Ok(row_count)
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

/// Introspects a SQLite table as a `SchemaNode`, printed as pretty JSON --
/// the database counterpart to [`import_xsd`].
pub fn import_db(db_path: &Path, table: &str) -> anyhow::Result<String> {
    let schema = format_db::introspect(db_path, table)
        .with_context(|| format!("introspecting {} in {}", table, db_path.display()))?;
    Ok(serde_json::to_string_pretty(&schema)?)
}

fn extension_of(path: &Path) -> anyhow::Result<&str> {
    path.extension()
        .and_then(|e| e.to_str())
        .with_context(|| format!("{} has no file extension", path.display()))
}
