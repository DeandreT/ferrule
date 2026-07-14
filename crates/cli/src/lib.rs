//! Headless runner: loads a mapping project and runs it against an input
//! file (delimited/fixed-width text, XLSX, XML, JSON, SQLite, PDF, or X12
//! EDI, chosen by extension and format options) or a static HTTP(S) XML
//! source to produce an output file. Split out from `main.rs` so it's
//! testable without shelling out to the built binary.
//!
//! For SQLite (`.db`/`.sqlite`/`.sqlite3`) the table name is the project's
//! source/target schema root `name`. For EDI (`.edi`/`.x12`/`.edifact`)
//! the schema describes the segment/loop structure and picks the dialect
//! by its first segment (ISA = X12, UNB = EDIFACT) -- see `format_edi`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use ir::{Instance, SchemaNode};
use mapping::FormatOptions;

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
    reject_xbrl_boundaries(&project)?;

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

    let source_instance = read_instance(&input_path, &project.source, &project.source_options)?;

    let project_dir = project_path.parent().unwrap_or_else(|| Path::new("."));
    let mut extras = Vec::with_capacity(project.extra_sources.len());
    for extra in &project.extra_sources {
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
    let execution = engine::ExecutionContext::new(&runtime_project_path)
        .with_current_datetime(&current_datetime);
    let target_instance =
        engine::run_with_sources_and_context(&project, &source_instance, extras, &execution)?;

    let row_count = if project.target_options.pdf.is_some() {
        reject_pdf_conflicts(&project.target_options, "output")?;
        bail!("PDF output is not supported; `pdf` is input-only");
    } else if let Some(layout) = &project.target_options.flextext {
        reject_flextext_conflicts(&project.target_options, "output")?;
        format_flextext::write(&output_path, &project.target, &target_instance, layout)
            .with_context(|| format!("writing output {}", output_path.display()))?;
        1
    } else if let Some(options) = &project.target_options.protobuf {
        reject_protobuf_conflicts(&project.target_options, "output")?;
        let layout = format_protobuf::Layout::parse(&options.schema)
            .context("parsing embedded Protocol Buffers schema")?;
        format_protobuf::write(
            &output_path,
            &layout,
            &options.root_message,
            &target_instance,
        )
        .with_context(|| format!("writing output {}", output_path.display()))?;
        1
    } else if let Some(layout) = &project.target_options.fixed_width {
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
                if let Some(layout) = &project.target_options.xlsx_hierarchical {
                    if project.target_options.xlsx_grid.is_some()
                        || project.target_options.xlsx_composite.is_some()
                        || has_legacy_xlsx_layout(&project.target_options)
                    {
                        anyhow::bail!(
                            "`xlsx_hierarchical` cannot be combined with other XLSX layout options"
                        );
                    }
                    format_xlsx::write_hierarchical(
                        &output_path,
                        &project.target,
                        &target_instance,
                        layout,
                    )
                    .with_context(|| format!("writing output {}", output_path.display()))?
                } else {
                    if project.target_options.xlsx_grid.is_some() {
                        anyhow::bail!(
                            "grid XLSX output is not supported; `xlsx_grid` is input-only"
                        );
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
                    let rows = target_instance.as_repeated().context(
                        "mapping did not produce a repeating row set for an XLSX output",
                    )?;
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
            "pdf" => bail!("PDF output is not supported; PDF is input-only"),
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
    let stored_path = PathBuf::from(stored_path);
    if http_url(&stored_path).is_some() {
        if allow_http {
            return Ok(stored_path);
        }
        bail!("HTTP output URLs are not supported; pass a local --{argument} path");
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
    if options.xbrl.is_some() {
        bail!("XBRL source input is not executable; native XBRL reading is not supported");
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
        "edi" | "x12" | "edifact" => {
            let read = match format_edi::dialect_of(schema)? {
                format_edi::Dialect::X12 => format_edi::x12::read,
                format_edi::Dialect::Edifact => format_edi::edifact::read,
            };
            read(path, schema, options.lenient_segments)
                .with_context(|| format!("reading input {}", path.display()))?
        }
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
    if options.delimiter.is_some() || options.has_header_row.is_some() {
        bail!(
            "`fixed_width` cannot be combined with CSV `delimiter` or `has_header_row` options for {side}"
        );
    }
    Ok(())
}

fn reject_xbrl_boundaries(project: &mapping::Project) -> anyhow::Result<()> {
    if project.source_options.xbrl.is_some() {
        bail!("XBRL source input is not executable; native XBRL reading is not supported");
    }
    if let Some(source) = project
        .extra_sources
        .iter()
        .find(|source| source.options.xbrl.is_some())
    {
        bail!(
            "extra source `{}` is an XBRL boundary; native XBRL reading is not supported",
            source.name
        );
    }
    if project.target_options.xbrl.is_some() {
        bail!("XBRL target output is not executable; native XBRL writing is not supported");
    }
    Ok(())
}

fn reject_protobuf_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    if options.lenient_segments
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.http_get.is_some()
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
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.http_get.is_some()
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
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.http_get.is_some()
        || options.json_lines
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_any_xlsx_layout(options)
    {
        bail!("`pdf` cannot be combined with another format's options for {side}");
    }
    Ok(())
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
