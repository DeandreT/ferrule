//! Read-only execution survey over the local (gitignored) MapForce samples.
//!
//! Run with:
//! `cargo test -p mfd --test samples_execution_survey -- --ignored --nocapture`.
//! Set `FERRULE_EXECUTION_SURVEY_JSON=/path/to/report.json` for a versioned
//! machine-readable report and `FERRULE_EXECUTION_SURVEY_DETAILS=1` for every
//! per-file outcome.
//!
//! The harness resolves every input beneath the sample directory, rejects
//! network and data-dependent sources, and writes projects and outputs only to
//! a unique temporary workspace. Reference comparison is deliberately narrow:
//! an existing primary `outputinstance` must be declared explicitly and must
//! not also be an input or an update-in-place workbook.

use std::collections::BTreeSet;
use std::error::Error;
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use ir::{Instance, SchemaKind, SchemaNode};
use mapping::{EdiBoundaryKind, ExternalPayloadFormat, FormatOptions, Project};

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

fn extension(path: &Path) -> Result<String, String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| format!("path `{}` has no usable extension", path.display()))
}

fn is_http(value: &str) -> bool {
    value.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

fn portable_path(value: &str) -> PathBuf {
    PathBuf::from(value.replace('\\', "/"))
}

fn resolve_sample_input(samples_root: &Path, stored: &str) -> Result<PathBuf, String> {
    if stored.trim().is_empty() {
        return Err("input instance path is empty".to_string());
    }
    if is_http(stored) {
        return Err("network input is disabled by the read-only execution survey".to_string());
    }
    let stored = portable_path(stored);
    let candidate = if stored.is_absolute() {
        stored
    } else {
        samples_root.join(stored)
    };
    let resolved = std::fs::canonicalize(&candidate).map_err(|error| {
        format!(
            "local input `{}` is unavailable: {error}",
            candidate.display()
        )
    })?;
    let canonical_root = std::fs::canonicalize(samples_root)
        .map_err(|error| format!("resolving sample root failed: {error}"))?;
    if !resolved.starts_with(&canonical_root) {
        return Err(format!(
            "local input `{}` escapes the read-only sample directory",
            candidate.display()
        ));
    }
    if !resolved.is_file() {
        return Err(format!(
            "local input `{}` is not a file",
            resolved.display()
        ));
    }
    Ok(resolved)
}

fn read_instance(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> Result<Instance, String> {
    if let Some(xbrl) = &options.xbrl {
        return format_xbrl::read_with_options(path, schema, xbrl)
            .map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.idoc {
        return format_edi::idoc::read(path, schema, layout, options.lenient_segments)
            .map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.swift_mt {
        return format_edi::swift::read(path, schema, layout, options.lenient_segments)
            .map_err(|error| error.to_string());
    }
    if let Some(boundary) = &options.external_source {
        return match boundary.payload() {
            ExternalPayloadFormat::Json => {
                format_json::read(path, schema).map_err(|error| error.to_string())
            }
            ExternalPayloadFormat::Xml => {
                format_xml::read(path, schema).map_err(|error| error.to_string())
            }
        };
    }
    if let Some(layout) = &options.pdf {
        return format_pdf::read(path, layout).map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.flextext {
        return format_flextext::read(path, schema, layout).map_err(|error| error.to_string());
    }
    if options.protobuf.is_some() {
        return Err("Protocol Buffers input is not supported".to_string());
    }
    if let Some(layout) = &options.fixed_width {
        return format_csv::read_fixed_width(path, schema, layout)
            .map(Instance::Repeated)
            .map_err(|error| error.to_string());
    }
    if options.xml_document {
        return format_xml::read(path, schema).map_err(|error| error.to_string());
    }

    match extension(path)?.as_str() {
        "csv" | "txt" => format_csv::read(
            path,
            schema,
            options.delimiter,
            options.has_header_row.unwrap_or(true),
        )
        .map(Instance::Repeated)
        .map_err(|error| error.to_string()),
        "xlsx" => read_xlsx(path, schema, options),
        "xml" => format_xml::read(path, schema).map_err(|error| error.to_string()),
        "json" | "jsonl" | "ndjson" if options.json_lines => {
            format_json::read_lines(path, schema).map_err(|error| error.to_string())
        }
        "json" | "jsonl" | "ndjson" => {
            format_json::read(path, schema).map_err(|error| error.to_string())
        }
        "db" | "sqlite" | "sqlite3" => {
            format_db::read_instance(path, schema).map_err(|error| error.to_string())
        }
        "edi" | "x12" | "edifact" | "hl7" => read_edi(path, schema, options),
        "idoc" => Err("SAP IDoc input has no embedded layout".to_string()),
        "fin" | "swift" => Err("SWIFT MT input has no embedded layout".to_string()),
        "pdf" => Err("PDF input has no embedded extraction layout".to_string()),
        other => Err(format!("unsupported input file extension `.{other}`")),
    }
}

fn read_xlsx(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> Result<Instance, String> {
    if options.xlsx_hierarchical.is_some() {
        return Err("hierarchical XLSX input is not supported".to_string());
    }
    if let Some(layout) = &options.xlsx_grid {
        return format_xlsx::read_grid(path, schema, layout)
            .map(Instance::Repeated)
            .map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.xlsx_composite {
        return format_xlsx::read_composite(path, schema, layout)
            .map_err(|error| error.to_string());
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
    };
    rows.map(Instance::Repeated)
        .map_err(|error| error.to_string())
}

fn read_edi(path: &Path, schema: &SchemaNode, options: &FormatOptions) -> Result<Instance, String> {
    match format_edi::dialect_of(schema).map_err(|error| error.to_string())? {
        format_edi::Dialect::X12 => format_edi::x12::read_with_separators(
            path,
            schema,
            options.lenient_segments,
            options.x12_separators.map(x12_separators),
        ),
        format_edi::Dialect::Edifact => {
            format_edi::edifact::read(path, schema, options.lenient_segments)
        }
        format_edi::Dialect::Hl7 => format_edi::hl7::read(path, schema, options.lenient_segments),
        format_edi::Dialect::Tradacoms => {
            format_edi::tradacoms::read(path, schema, options.lenient_segments)
        }
    }
    .map_err(|error| error.to_string())
}

fn write_instance(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> Result<(), String> {
    if let Some(xbrl) = &options.xbrl {
        return format_xbrl::write(path, schema, instance, xbrl).map_err(|error| error.to_string());
    }
    if options.idoc.is_some() {
        return Err("SAP IDoc output is not supported".to_string());
    }
    if options.swift_mt.is_some() {
        return Err("SWIFT MT output is not supported".to_string());
    }
    if options.pdf.is_some() {
        return Err("PDF output is not supported".to_string());
    }
    if let Some(layout) = &options.flextext {
        return format_flextext::write(path, schema, instance, layout)
            .map_err(|error| error.to_string());
    }
    if let Some(protobuf) = &options.protobuf {
        let layout =
            format_protobuf::Layout::parse(&protobuf.schema).map_err(|error| error.to_string())?;
        return format_protobuf::write(path, &layout, &protobuf.root_message, instance)
            .map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.fixed_width {
        let rows = instance
            .as_repeated()
            .ok_or_else(|| "fixed-width output is not a repeating row set".to_string())?;
        return format_csv::write_fixed_width(path, schema, rows, layout)
            .map_err(|error| error.to_string());
    }
    if options.xml_document {
        return format_xml::write(path, schema, instance).map_err(|error| error.to_string());
    }

    match extension(path)?.as_str() {
        "csv" | "txt" => {
            let rows = instance
                .as_repeated()
                .ok_or_else(|| "CSV output is not a repeating row set".to_string())?;
            format_csv::write(
                path,
                schema,
                rows,
                options.delimiter,
                options.has_header_row.unwrap_or(true),
            )
            .map_err(|error| error.to_string())
        }
        "xlsx" => write_xlsx(path, schema, instance, options),
        "xml" => format_xml::write(path, schema, instance).map_err(|error| error.to_string()),
        "json" | "jsonl" | "ndjson" if options.json_lines => {
            format_json::write_lines(path, schema, instance).map_err(|error| error.to_string())
        }
        "json" | "jsonl" | "ndjson" => {
            format_json::write(path, schema, instance).map_err(|error| error.to_string())
        }
        "db" | "sqlite" | "sqlite3" => {
            format_db::write_instance(path, schema, instance).map_err(|error| error.to_string())
        }
        "edi" | "x12" | "edifact" => {
            match format_edi::dialect_of(schema).map_err(|error| error.to_string())? {
                format_edi::Dialect::X12 => format_edi::x12::write_with_syntax(
                    path,
                    schema,
                    instance,
                    options
                        .x12_separators
                        .map(x12_separators)
                        .unwrap_or_default(),
                    options.x12_interchange_version.as_deref(),
                ),
                format_edi::Dialect::Edifact => format_edi::edifact::write(path, schema, instance),
                format_edi::Dialect::Hl7 => return Err("HL7 output is not supported".to_string()),
                format_edi::Dialect::Tradacoms => {
                    return Err("TRADACOMS output is not supported".to_string());
                }
            }
            .map_err(|error| error.to_string())
        }
        "hl7" => Err("HL7 output is not supported".to_string()),
        other => Err(format!("unsupported output file extension `.{other}`")),
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

fn write_xlsx(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> Result<(), String> {
    if options.xlsx_update_existing {
        return Err(
            "update-in-place XLSX output is excluded from the read-only survey".to_string(),
        );
    }
    if let Some(layout) = &options.xlsx_hierarchical {
        return format_xlsx::write_hierarchical(path, schema, instance, layout)
            .map(|_| ())
            .map_err(|error| error.to_string());
    }
    if options.xlsx_grid.is_some()
        || options.xlsx_composite.is_some()
        || !options.xlsx_rows.is_empty()
    {
        return Err("the selected XLSX input layout cannot be used for output".to_string());
    }
    let rows = instance
        .as_repeated()
        .ok_or_else(|| "XLSX output is not a repeating row set".to_string())?;
    format_xlsx::write(
        path,
        schema,
        rows,
        options.xlsx_sheet.as_deref(),
        options.xlsx_start_row.unwrap_or(1),
        &options.xlsx_columns,
        options.has_header_row.unwrap_or(true),
    )
    .map_err(|error| error.to_string())
}

fn inferred_extension(options: &FormatOptions) -> Option<&'static str> {
    if options.xbrl.is_some() {
        Some("xbrl")
    } else if options.protobuf.is_some() {
        Some("bin")
    } else if options.flextext.is_some() || options.fixed_width.is_some() {
        Some("txt")
    } else if options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || options.xlsx_update_existing
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
    {
        Some("xlsx")
    } else if options.delimiter.is_some() || options.has_header_row.is_some() {
        Some("csv")
    } else if options.json_lines {
        Some("jsonl")
    } else if options.json_document {
        Some("json")
    } else if options.xml_document {
        Some("xml")
    } else {
        match options.edi_kind {
            Some(EdiBoundaryKind::X12) => Some("x12"),
            Some(EdiBoundaryKind::Edifact) => Some("edifact"),
            Some(EdiBoundaryKind::Hl7) => Some("hl7"),
            Some(EdiBoundaryKind::Tradacoms) => Some("edi"),
            Some(EdiBoundaryKind::Idoc) => Some("idoc"),
            Some(EdiBoundaryKind::SwiftMt) => Some("fin"),
            None => None,
        }
    }
}

fn output_path(
    sample_dir: &Path,
    stored: Option<&str>,
    options: &FormatOptions,
    label: &str,
) -> Result<PathBuf, String> {
    let file_name = stored
        .filter(|value| !value.trim().is_empty() && !is_http(value))
        .and_then(|value| portable_path(value).file_name().map(|name| name.to_owned()));
    if let Some(file_name) = file_name {
        return Ok(sample_dir.join(file_name));
    }
    let extension = inferred_extension(options)
        .ok_or_else(|| format!("{label} has no stored output path or retained format marker"))?;
    Ok(sample_dir.join(format!("{label}.{extension}")))
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
    Ok(reference)
}

struct LoadedSources {
    primary: Instance,
    extras: Vec<(String, Instance)>,
    paths: BTreeSet<PathBuf>,
}

fn load_sources(project: &Project, samples_root: &Path) -> Result<LoadedSources, String> {
    let source_path = project
        .source_path
        .as_deref()
        .ok_or_else(|| "primary source has no input instance path".to_string())?;
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
            return Err(format!(
                "extra source `{}` has a data-dependent path",
                source.name
            ));
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

#[derive(Debug, Default, PartialEq, Eq)]
struct FileInstanceConnections {
    target_path: bool,
}

fn connected_file_instances(mfd_path: &Path) -> Result<FileInstanceConnections, String> {
    let text = std::fs::read_to_string(mfd_path)
        .map_err(|error| format!("reading MFD target metadata failed: {error}"))?;
    let document = roxmltree::Document::parse(&text)
        .map_err(|error| format!("parsing MFD target metadata failed: {error}"))?;
    let xml_components = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("library") == Some("xml"))
        .collect::<Vec<_>>();
    let mut target_keys = BTreeSet::new();
    for component in xml_components {
        let source = component
            .descendants()
            .any(|node| node.has_tag_name("document") && node.attribute("inputinstance").is_some());
        for entry in component.descendants().filter(|entry| {
            entry.has_tag_name("entry") && entry.attribute("name") == Some("FileInstance")
        }) {
            if !source
                && let Some(key) = entry
                    .attribute("inpkey")
                    .and_then(|key| key.parse::<u32>().ok())
            {
                target_keys.insert(key);
            }
        }
    }
    let mut connections = FileInstanceConnections::default();
    for edge in document
        .descendants()
        .filter(|node| node.has_tag_name("edge"))
    {
        let to = edge
            .attribute("to")
            .or_else(|| edge.attribute("vertexkey"))
            .and_then(|key| key.parse::<u32>().ok());
        connections.target_path |= to.is_some_and(|key| target_keys.contains(&key));
    }
    Ok(connections)
}

fn write_outputs(
    project: &Project,
    outputs: &engine::ExecutionOutputs,
    sample_dir: &Path,
    samples_root: &Path,
) -> Result<PathBuf, String> {
    let primary_path = output_path(
        sample_dir,
        project.target_path.as_deref(),
        &project.target_options,
        "primary-output",
    )?;
    prepare_database_output(
        samples_root,
        project.target_path.as_deref(),
        &primary_path,
        &project.target,
    )?;
    write_instance(
        &primary_path,
        &project.target,
        &outputs.primary,
        &project.target_options,
    )
    .map_err(|error| format!("writing primary output failed: {error}"))?;
    if outputs.extras.len() != project.extra_targets.len() {
        return Err("engine returned an unexpected number of additional targets".to_string());
    }
    for (index, (target, output)) in project
        .extra_targets
        .iter()
        .zip(&outputs.extras)
        .enumerate()
    {
        let path = output_path(
            sample_dir,
            target.path.as_deref(),
            &target.options,
            &format!("extra-output-{index}"),
        )?;
        prepare_database_output(samples_root, target.path.as_deref(), &path, &target.schema)?;
        write_instance(&path, &target.schema, &output.instance, &target.options)
            .map_err(|error| format!("writing extra target `{}` failed: {error}", target.name))?;
    }
    Ok(primary_path)
}

fn prepare_database_output(
    samples_root: &Path,
    stored: Option<&str>,
    output: &Path,
    schema: &SchemaNode,
) -> Result<(), String> {
    if !matches!(extension(output)?.as_str(), "db" | "sqlite" | "sqlite3") {
        return Ok(());
    }
    let relational = matches!(
        &schema.kind,
        SchemaKind::Group { children, .. }
            if children
                .iter()
                .any(|child| matches!(child.kind, SchemaKind::Group { .. }))
    );
    let Some(stored) = stored else {
        return if relational {
            Err("relational SQLite output has no stored database template".to_string())
        } else {
            Ok(())
        };
    };
    match resolve_sample_input(samples_root, stored) {
        Ok(template) => std::fs::copy(&template, output)
            .map(|_| ())
            .map_err(|error| {
                format!(
                    "copying SQLite output template `{}` failed: {error}",
                    template.display()
                )
            }),
        Err(reason) if relational => Err(format!(
            "relational SQLite output requires its stored database template: {reason}"
        )),
        Err(_) => Ok(()),
    }
}

fn survey_file(
    index: usize,
    mfd_path: &Path,
    samples_root: &Path,
    workspace: &SurveyWorkspace,
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

    if imported.project.source_options.local_xml_file_set {
        match connected_file_instances(mfd_path) {
            Ok(FileInstanceConnections {
                target_path: true, ..
            }) => {
                outcome.execution = StageOutcome::skipped(
                    "target FileInstance is connected: per-source output filenames are not represented yet",
                );
                return outcome;
            }
            Ok(_) => {}
            Err(reason) => {
                outcome.execution = StageOutcome::skipped(reason);
                return outcome;
            }
        }
    }

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

    let sources = match load_sources(&imported.project, samples_root) {
        Ok(sources) => sources,
        Err(reason) => {
            outcome.execution = StageOutcome::skipped(reason);
            return outcome;
        }
    };
    let execution =
        engine::ExecutionContext::new(&project_path).with_current_datetime(FIXED_CURRENT_DATETIME);
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
    outcome.execution = StageOutcome::passed();

    let output_path = match write_outputs(&imported.project, &outputs, &sample_dir, samples_root) {
        Ok(path) => path,
        Err(reason) => {
            outcome.output_write = StageOutcome::skipped(reason);
            return outcome;
        }
    };
    outcome.output = Some(output_path.display().to_string());
    outcome.output_write = StageOutcome::passed();

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
    if expected == outputs.primary {
        outcome.reference_match = StageOutcome::passed();
    } else {
        outcome.reference_match = StageOutcome::failed(format!(
            "runtime value differs from explicit reference `{}`",
            reference.display()
        ));
    }
    outcome
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
            "reference_policy": "existing explicit outputinstance, not reused as input or update template",
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
fn retained_tabular_options_select_a_pathless_output_format() {
    let csv = FormatOptions {
        delimiter: Some(';'),
        has_header_row: Some(false),
        ..FormatOptions::default()
    };
    assert_eq!(inferred_extension(&csv), Some("csv"));

    let xlsx = FormatOptions {
        has_header_row: Some(true),
        xlsx_sheet: Some("Summary".into()),
        ..FormatOptions::default()
    };
    assert_eq!(inferred_extension(&xlsx), Some("xlsx"));
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
    let outcomes = paths
        .iter()
        .enumerate()
        .map(|(index, path)| survey_file(index, path, &samples_root, &workspace))
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
        "explicit references: {} available; {} matched; {} mismatched",
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
