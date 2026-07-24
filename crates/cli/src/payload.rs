use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use ir::{Instance, SchemaNode};
use mapping::{EdiAutocomplete, EdiBoundaryKind, ExternalPayloadFormat, FormatOptions};

use super::{
    TraceSink, absolute_mapping_path, extension_for_dispatch, extension_of, formatted_edi_output,
    has_legacy_xlsx_layout, protobuf_layout, reject_edi_conflicts,
    reject_external_source_conflicts, reject_fixed_width_csv_options, reject_flextext_conflicts,
    reject_idoc_conflicts, reject_json_conflicts, reject_pdf_conflicts, reject_protobuf_conflicts,
    reject_swift_conflicts, reject_xbrl_conflicts, reject_xml_conflicts, require_valid,
    resolve_run_path, validate_tabular_fallback, x12_separators,
};

pub const MAX_PAYLOAD_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_PAYLOAD_RUN_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_PAYLOAD_ARTIFACTS: usize = 4096;
pub const MAX_PAYLOAD_PATH_BYTES: usize = 4096;
pub const MAX_PAYLOAD_NAME_BYTES: usize = 256;
const MAX_PAYLOAD_INPUTS: usize = 4096;

/// One bounded host-owned input document and its logical path identity.
#[derive(Debug, Clone, Copy)]
pub struct PayloadDocument<'a> {
    path: &'a Path,
    bytes: &'a [u8],
}

impl<'a> PayloadDocument<'a> {
    pub fn new(path: &'a Path, bytes: &'a [u8]) -> anyhow::Result<Self> {
        validate_logical_path(path, "payload")?;
        if bytes.len() > MAX_PAYLOAD_DOCUMENT_BYTES {
            bail!(
                "payload `{}` exceeds the {} MiB per-document limit",
                path.display(),
                MAX_PAYLOAD_DOCUMENT_BYTES / (1024 * 1024)
            );
        }
        Ok(Self { path, bytes })
    }

    pub fn path(self) -> &'a Path {
        self.path
    }

    pub fn bytes(self) -> &'a [u8] {
        self.bytes
    }
}

/// A payload assigned to one declared additional source.
#[derive(Debug, Clone, Copy)]
pub struct NamedPayloadInput<'a> {
    name: &'a str,
    document: PayloadDocument<'a>,
}

impl<'a> NamedPayloadInput<'a> {
    pub fn new(name: &'a str, document: PayloadDocument<'a>) -> anyhow::Result<Self> {
        if name.is_empty() {
            bail!("payload source name cannot be empty");
        }
        if name.len() > MAX_PAYLOAD_NAME_BYTES {
            bail!("payload source name exceeds the {MAX_PAYLOAD_NAME_BYTES}-byte UTF-8 limit");
        }
        Ok(Self { name, document })
    }

    pub fn name(self) -> &'a str {
        self.name
    }

    pub fn document(self) -> PayloadDocument<'a> {
        self.document
    }
}

/// Filesystem-free inputs and host context for one mapping execution.
pub struct PayloadRunOptions<'a> {
    primary: PayloadDocument<'a>,
    extra_sources: &'a [NamedPayloadInput<'a>],
    output_path: Option<&'a Path>,
    runtime_parameters: Option<&'a engine::RuntimeParameters>,
    trace_sink: Option<&'a dyn TraceSink>,
}

impl<'a> PayloadRunOptions<'a> {
    pub fn new(primary: PayloadDocument<'a>) -> Self {
        Self {
            primary,
            extra_sources: &[],
            output_path: None,
            runtime_parameters: None,
            trace_sink: None,
        }
    }

    pub fn with_extra_sources(mut self, extra_sources: &'a [NamedPayloadInput<'a>]) -> Self {
        self.extra_sources = extra_sources;
        self
    }

    pub fn with_output_path(mut self, path: &'a Path) -> Self {
        self.output_path = Some(path);
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

/// One serialized target document returned to the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadArtifact {
    pub target: String,
    pub records_written: usize,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

/// Ordered serialized results of a filesystem-free mapping run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadRunOutcome {
    pub records_written: usize,
    pub artifacts: Vec<PayloadArtifact>,
}

/// Loads a project and executes it against host-owned input bytes.
pub fn run_project_payloads(
    project_path: &Path,
    options: &PayloadRunOptions<'_>,
) -> anyhow::Result<PayloadRunOutcome> {
    let project = super::load_project(project_path)?;
    run_project_value_payloads(&project, project_path, options)
}

/// Executes an in-memory project against host-owned input bytes.
pub fn run_project_value_payloads(
    project: &mapping::Project,
    project_path: &Path,
    options: &PayloadRunOptions<'_>,
) -> anyhow::Result<PayloadRunOutcome> {
    require_valid(project)?;
    validate_input_budget(options)?;

    let source = read_payload(options.primary, &project.source, &project.source_options)
        .context("reading primary input payload")?;
    let loaded_sources = load_extra_payloads(project, options.extra_sources)?;

    let runtime_project_path = absolute_mapping_path(project_path)?;
    let current_datetime = jiff::Zoned::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
        .to_string();
    let dynamic_loader = PayloadDynamicSourceLoader {
        sources: loaded_sources.dynamic,
    };
    let mut execution = engine::ExecutionContext::new(&runtime_project_path)
        .with_current_datetime(&current_datetime)
        .with_dynamic_source_loader(&dynamic_loader);
    if let Some(parameters) = options.runtime_parameters {
        execution = execution.with_parameters(parameters);
    }
    if let Some(trace_sink) = options.trace_sink {
        execution = execution.with_trace_sink(trace_sink);
    }
    let output = engine::run_outputs_with_sources_and_context(
        project,
        &source,
        loaded_sources.static_sources,
        &execution,
    )?;
    if output.extras.len() != project.extra_targets.len() {
        bail!("engine returned an unexpected number of additional target values");
    }
    validate_artifact_count(project, &output)?;

    let primary_destination = target_destination(
        project_path,
        options.output_path,
        project.target_path.as_deref(),
        project.root.output_path().is_some(),
        "primary target",
    )?;
    let mut artifacts = render_target(
        &project.target.name,
        &primary_destination,
        &project.target,
        &output.primary,
        &project.target_options,
        &current_datetime,
    )?;
    let records_written = artifacts
        .iter()
        .map(|artifact| artifact.records_written)
        .sum();

    for (target, output) in project.extra_targets.iter().zip(&output.extras) {
        let destination = target_destination(
            project_path,
            None,
            target.path.as_deref(),
            target.root.output_path().is_some(),
            &format!("extra target `{}`", target.name),
        )?;
        let rendered = render_target(
            &target.name,
            &destination,
            &target.schema,
            &output.instance,
            &target.options,
            &current_datetime,
        )
        .with_context(|| format!("rendering extra target `{}`", target.name))?;
        artifacts.extend(rendered);
    }

    validate_artifact_budget(&artifacts)?;
    validate_artifact_paths(&artifacts)?;
    Ok(PayloadRunOutcome {
        records_written,
        artifacts,
    })
}

fn validate_input_budget(options: &PayloadRunOptions<'_>) -> anyhow::Result<()> {
    if options.extra_sources.len() > MAX_PAYLOAD_INPUTS {
        bail!("payload run exceeds the limit of {MAX_PAYLOAD_INPUTS} additional input documents");
    }
    let total = options
        .extra_sources
        .iter()
        .try_fold(options.primary.bytes.len(), |total, input| {
            total.checked_add(input.document.bytes.len())
        })
        .context("payload input byte count overflowed")?;
    if total > MAX_PAYLOAD_RUN_BYTES {
        bail!(
            "payload inputs exceed the {} MiB total limit",
            MAX_PAYLOAD_RUN_BYTES / (1024 * 1024)
        );
    }
    Ok(())
}

struct LoadedPayloadSources {
    static_sources: Vec<(String, Instance)>,
    dynamic: BTreeMap<(String, String), Arc<Instance>>,
}

fn load_extra_payloads(
    project: &mapping::Project,
    inputs: &[NamedPayloadInput<'_>],
) -> anyhow::Result<LoadedPayloadSources> {
    let mut static_payloads = BTreeMap::new();
    let mut dynamic_payloads = BTreeMap::new();
    for input in inputs {
        let source = project
            .extra_sources
            .iter()
            .find(|source| source.name == input.name)
            .with_context(|| format!("payload source `{}` is not declared", input.name))?;
        let instance = read_payload(input.document, &source.schema, &source.options)
            .with_context(|| format!("reading payload source `{}`", input.name))?;
        if source.dynamic_path.is_some() {
            let key = (
                input.name.to_string(),
                payload_identity(input.document.path)?,
            );
            if dynamic_payloads.insert(key, Arc::new(instance)).is_some() {
                bail!(
                    "payload source `{}` contains duplicate logical path `{}`",
                    input.name,
                    input.document.path.display()
                );
            }
        } else if static_payloads
            .insert(input.name.to_string(), instance)
            .is_some()
        {
            bail!(
                "static extra source `{}` requires exactly one payload document",
                input.name
            );
        }
    }

    let mut static_sources = Vec::new();
    for source in &project.extra_sources {
        if source.dynamic_path.is_some() {
            continue;
        }
        let value = static_payloads.remove(&source.name).with_context(|| {
            format!(
                "static extra source `{}` requires exactly one payload document",
                source.name
            )
        })?;
        static_sources.push((source.name.clone(), value));
    }
    Ok(LoadedPayloadSources {
        static_sources,
        dynamic: dynamic_payloads,
    })
}

struct PayloadDynamicSourceLoader {
    sources: BTreeMap<(String, String), Arc<Instance>>,
}

impl engine::DynamicSourceLoader for PayloadDynamicSourceLoader {
    fn load(&self, source_name: &str, path: &str) -> Result<Arc<Instance>, String> {
        self.sources
            .get(&(source_name.to_string(), normalize_payload_identity(path)))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "host did not supply payload source `{source_name}` at logical path `{path}`"
                )
            })
    }
}

fn payload_identity(path: &Path) -> anyhow::Result<String> {
    validate_logical_path(path, "payload").map(normalize_payload_identity)
}

fn normalize_payload_identity(path: &str) -> String {
    path.replace('\\', "/")
}

fn validate_logical_path<'a>(path: &'a Path, label: &str) -> anyhow::Result<&'a str> {
    let path = path
        .to_str()
        .with_context(|| format!("{label} path {} is not UTF-8", path.display()))?;
    if path.is_empty() {
        bail!("{label} path cannot be empty");
    }
    if path.len() > MAX_PAYLOAD_PATH_BYTES {
        bail!("{label} path exceeds the {MAX_PAYLOAD_PATH_BYTES}-byte UTF-8 limit");
    }
    Ok(path)
}

enum PayloadDestination {
    Static(PathBuf),
    DynamicBase(PathBuf),
}

fn target_destination(
    project_path: &Path,
    explicit: Option<&Path>,
    stored: Option<&str>,
    dynamic: bool,
    label: &str,
) -> anyhow::Result<PayloadDestination> {
    if dynamic {
        let base = explicit.map(Path::to_path_buf).unwrap_or_else(|| {
            project_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        });
        validate_logical_path(&base, label)?;
        return Ok(PayloadDestination::DynamicBase(base));
    }
    let path = resolve_run_path(
        project_path,
        explicit,
        stored,
        "output",
        "target_path",
        false,
    )
    .with_context(|| format!("resolving {label} payload path"))?;
    validate_logical_path(&path, label)?;
    Ok(PayloadDestination::Static(path))
}

fn render_target(
    name: &str,
    destination: &PayloadDestination,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
) -> anyhow::Result<Vec<PayloadArtifact>> {
    let files = match (destination, instance) {
        (PayloadDestination::Static(_), Instance::DocumentSet(_)) => {
            bail!("mapping produced dynamically named documents for a static output path")
        }
        (PayloadDestination::DynamicBase(_), value)
            if !matches!(value, Instance::DocumentSet(_)) =>
        {
            bail!("dynamic target mapping did not produce a document set")
        }
        (PayloadDestination::Static(path), instance) => vec![(path.clone(), instance)],
        (PayloadDestination::DynamicBase(base), Instance::DocumentSet(documents)) => {
            let paths = super::output_documents::validate_document_paths(documents)?;
            documents
                .iter()
                .zip(paths)
                .map(|(document, path)| (base.join(path), document.value()))
                .collect()
        }
        (PayloadDestination::DynamicBase(_), _) => unreachable!("guarded above"),
    };
    files
        .into_iter()
        .map(|(path, instance)| {
            validate_logical_path(&path, "output artifact")?;
            let (bytes, records_written) =
                render_payload(&path, schema, instance, options, current_datetime)
                    .with_context(|| format!("rendering target payload {}", path.display()))?;
            Ok(PayloadArtifact {
                target: name.to_string(),
                records_written,
                path,
                bytes,
            })
        })
        .collect()
}

fn validate_artifact_count(
    project: &mapping::Project,
    output: &engine::ExecutionOutputs,
) -> anyhow::Result<()> {
    let mut count = target_artifact_count(&output.primary, project.root.output_path().is_some())?;
    for (target, output) in project.extra_targets.iter().zip(&output.extras) {
        count = count
            .checked_add(target_artifact_count(
                &output.instance,
                target.root.output_path().is_some(),
            )?)
            .context("payload artifact count overflowed")?;
    }
    if count > MAX_PAYLOAD_ARTIFACTS {
        bail!("payload run exceeds the limit of {MAX_PAYLOAD_ARTIFACTS} output artifacts");
    }
    Ok(())
}

fn target_artifact_count(instance: &Instance, dynamic: bool) -> anyhow::Result<usize> {
    match (dynamic, instance) {
        (false, Instance::DocumentSet(_)) => {
            bail!("mapping produced dynamically named documents for a static output path")
        }
        (false, _) => Ok(1),
        (true, Instance::DocumentSet(documents)) => Ok(documents.len()),
        (true, _) => bail!("dynamic target mapping did not produce a document set"),
    }
}

fn validate_artifact_budget(artifacts: &[PayloadArtifact]) -> anyhow::Result<()> {
    let mut total = 0usize;
    for artifact in artifacts {
        if artifact.bytes.len() > MAX_PAYLOAD_DOCUMENT_BYTES {
            bail!(
                "output artifact `{}` exceeds the {} MiB per-document limit",
                artifact.path.display(),
                MAX_PAYLOAD_DOCUMENT_BYTES / (1024 * 1024)
            );
        }
        total = total
            .checked_add(artifact.bytes.len())
            .context("payload output byte count overflowed")?;
    }
    if total > MAX_PAYLOAD_RUN_BYTES {
        bail!(
            "payload outputs exceed the {} MiB total limit",
            MAX_PAYLOAD_RUN_BYTES / (1024 * 1024)
        );
    }
    Ok(())
}

fn validate_artifact_paths(artifacts: &[PayloadArtifact]) -> anyhow::Result<()> {
    let mut paths = Vec::with_capacity(artifacts.len());
    let mut unique = BTreeSet::new();
    for artifact in artifacts {
        let path = lexical_normalize(&artifact.path);
        if !unique.insert(path.clone()) {
            bail!(
                "multiple output artifacts use path `{}`",
                artifact.path.display()
            );
        }
        paths.push((&artifact.path, path));
    }
    for (index, (display, path)) in paths.iter().enumerate() {
        for (other_display, other) in paths.iter().skip(index + 1) {
            if path.starts_with(other) || other.starts_with(path) {
                bail!(
                    "output artifact paths `{}` and `{}` overlap as file and directory",
                    display.display(),
                    other_display.display()
                );
            }
        }
    }
    Ok(())
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn utf8<'a>(document: PayloadDocument<'a>, label: &str) -> anyhow::Result<&'a str> {
    std::str::from_utf8(document.bytes)
        .with_context(|| format!("{label} payload `{}` is not UTF-8", document.path.display()))
}

fn read_payload(
    document: PayloadDocument<'_>,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> anyhow::Result<Instance> {
    if options.local_xml_file_set {
        bail!(
            "`local_xml_file_set` requires multiple filesystem documents and is unavailable for a single payload"
        );
    }
    if options.xbrl.is_some() {
        reject_xbrl_conflicts(options, "input")?;
        let xbrl = options
            .xbrl
            .as_ref()
            .context("missing XBRL source options")?;
        return format_xbrl::from_str_with_options(utf8(document, "XBRL")?, schema, xbrl)
            .context("parsing XBRL input payload");
    }
    if let Some(layout) = &options.idoc {
        reject_idoc_conflicts(options, "input")?;
        return format_edi::idoc::from_bytes(
            document.bytes,
            schema,
            layout,
            options.lenient_segments,
        )
        .context("parsing SAP IDoc input payload");
    }
    if let Some(layout) = &options.swift_mt {
        reject_swift_conflicts(options, "input")?;
        return format_edi::swift::from_bytes(
            document.bytes,
            schema,
            layout,
            options.lenient_segments,
        )
        .context("parsing SWIFT MT input payload");
    }
    if let Some(boundary) = &options.external_source {
        reject_external_source_conflicts(options, "input")?;
        return match boundary.payload() {
            ExternalPayloadFormat::Json => format_json::from_str(utf8(document, "JSON")?, schema)
                .context("parsing captured JSON input payload"),
            ExternalPayloadFormat::Xml => format_xml::from_str(utf8(document, "XML")?, schema)
                .context("parsing captured XML input payload"),
        };
    }
    if let Some(pdf) = &options.pdf {
        reject_pdf_conflicts(options, "input")?;
        return format_pdf::from_bytes(document.bytes, pdf).context("parsing PDF input payload");
    }
    if let Some(layout) = &options.flextext {
        reject_flextext_conflicts(options, "input")?;
        return format_flextext::from_str(utf8(document, "FlexText")?, schema, layout)
            .context("parsing FlexText input payload");
    }
    if let Some(protobuf) = &options.protobuf {
        reject_protobuf_conflicts(options, "input")?;
        let layout =
            protobuf_layout(protobuf).context("parsing embedded Protocol Buffers schema")?;
        return format_protobuf::from_slice(&layout, &protobuf.root_message, document.bytes)
            .context("parsing Protocol Buffers input payload");
    }
    if let Some(kind) = options.edi_kind {
        reject_edi_conflicts(options, "input")?;
        let text = utf8(document, "EDI")?;
        let mut instance = match kind {
            EdiBoundaryKind::X12 => format_edi::x12::from_str_with_separators(
                text,
                schema,
                options.lenient_segments,
                options.x12_separators.map(x12_separators),
            ),
            EdiBoundaryKind::Edifact => {
                format_edi::edifact::from_str(text, schema, options.lenient_segments)
            }
            EdiBoundaryKind::Hl7 => {
                format_edi::hl7::from_str(text, schema, options.lenient_segments)
            }
            EdiBoundaryKind::Tradacoms => {
                format_edi::tradacoms::from_str(text, schema, options.lenient_segments)
            }
            EdiBoundaryKind::Idoc | EdiBoundaryKind::SwiftMt => {
                bail!("EDI boundary `{kind:?}` requires an embedded runtime layout")
            }
        }?;
        format_edi::apply_implied_decimals(&mut instance, &options.edi_implied_decimals)
            .context("applying EDI numeric formats to input payload")?;
        return Ok(instance);
    }
    if options.xml_document {
        reject_xml_conflicts(options, "input")?;
        let text = utf8(document, "XML")?;
        return match &options.wsdl {
            Some(wsdl) => format_xml::from_wsdl_message_str(text, schema, wsdl.operation()),
            None => format_xml::from_str(text, schema),
        }
        .context("parsing XML input payload");
    }
    if options.json_document || options.json_lines {
        reject_json_conflicts(options, "input")?;
        let text = utf8(document, "JSON")?;
        return if options.json_lines {
            format_json::from_lines(text, schema)
        } else {
            format_json::from_str(text, schema)
        }
        .context("parsing JSON input payload");
    }
    if let Some(layout) = &options.fixed_width {
        reject_fixed_width_csv_options(options, "input")?;
        let rows = format_csv::from_str_fixed_width(utf8(document, "fixed-width")?, schema, layout)
            .context("parsing fixed-width input payload")?;
        return Ok(Instance::Repeated(rows));
    }

    validate_tabular_fallback(document.path, options, "input")?;
    match extension_for_dispatch(document.path, options)?.as_str() {
        "csv" | "txt" => format_csv::from_str(
            utf8(document, "CSV")?,
            schema,
            options.delimiter,
            options.has_header_row.unwrap_or(true),
        )
        .map(Instance::Repeated)
        .context("parsing CSV input payload"),
        "xlsx" => read_xlsx_payload(document.bytes, schema, options),
        "xml" => format_xml::from_str(utf8(document, "XML")?, schema)
            .context("parsing XML input payload"),
        "json" | "jsonl" | "ndjson" => {
            let lines = options.json_lines
                || matches!(extension_of(document.path)?.as_str(), "jsonl" | "ndjson");
            if lines {
                format_json::from_lines(utf8(document, "JSON")?, schema)
            } else {
                format_json::from_str(utf8(document, "JSON")?, schema)
            }
            .context("parsing JSON input payload")
        }
        "db" | "sqlite" | "sqlite3" => {
            bail!("SQLite input requires a persistent database and is unavailable as a payload")
        }
        "edi" | "x12" | "edifact" | "hl7" => {
            let text = utf8(document, "EDI")?;
            match format_edi::dialect_of(schema)? {
                format_edi::Dialect::X12 => {
                    format_edi::x12::from_str(text, schema, options.lenient_segments)
                }
                format_edi::Dialect::Edifact => {
                    format_edi::edifact::from_str(text, schema, options.lenient_segments)
                }
                format_edi::Dialect::Hl7 => {
                    format_edi::hl7::from_str(text, schema, options.lenient_segments)
                }
                format_edi::Dialect::Tradacoms => {
                    format_edi::tradacoms::from_str(text, schema, options.lenient_segments)
                }
            }
            .context("parsing EDI input payload")
        }
        "idoc" => bail!("SAP IDoc input requires an embedded `idoc` layout"),
        "fin" | "swift" => bail!("SWIFT MT input requires an embedded `swift_mt` layout"),
        "pdf" => bail!("PDF input requires embedded `pdf` extraction options"),
        other => bail!("unsupported input payload extension: .{other}"),
    }
}

fn read_xlsx_payload(
    bytes: &[u8],
    schema: &SchemaNode,
    options: &FormatOptions,
) -> anyhow::Result<Instance> {
    if let Some(layout) = &options.xlsx_hierarchical {
        if options.xlsx_grid.is_some()
            || options.xlsx_composite.is_some()
            || options.xlsx_worksheet_set.is_some()
            || has_legacy_xlsx_layout(options)
        {
            bail!("`xlsx_hierarchical` cannot be combined with other XLSX layout options");
        }
        return format_xlsx::from_bytes_hierarchical(bytes, schema, layout)
            .context("parsing hierarchical XLSX input payload");
    }
    if let Some(layout) = &options.xlsx_grid {
        if options.xlsx_composite.is_some()
            || options.xlsx_worksheet_set.is_some()
            || has_legacy_xlsx_layout(options)
        {
            bail!("`xlsx_grid` conflicts with other XLSX layout options");
        }
        return format_xlsx::from_bytes_grid(bytes, schema, layout)
            .map(Instance::Repeated)
            .context("parsing grid XLSX input payload");
    }
    if let Some(layout) = &options.xlsx_worksheet_set {
        if options.xlsx_composite.is_some() || has_legacy_xlsx_layout(options) {
            bail!("`xlsx_worksheet_set` conflicts with other XLSX layout options");
        }
        return format_xlsx::from_bytes_worksheet_set(bytes, schema, layout)
            .context("parsing worksheet-set XLSX input payload");
    }
    if let Some(layout) = &options.xlsx_composite {
        if has_legacy_xlsx_layout(options) {
            bail!("`xlsx_composite` conflicts with legacy XLSX layout options");
        }
        return format_xlsx::from_bytes_composite(bytes, schema, layout)
            .context("parsing composite XLSX input payload");
    }
    if !options.xlsx_rows.is_empty() && !options.xlsx_headers.is_empty() {
        bail!("transposed XLSX input cannot be combined with flat header overrides");
    }
    let rows = if options.xlsx_rows.is_empty() {
        format_xlsx::from_bytes(
            bytes,
            schema,
            options.xlsx_sheet.as_deref(),
            options.xlsx_start_row.unwrap_or(1),
            &options.xlsx_columns,
            options.has_header_row.unwrap_or(true),
        )
    } else {
        format_xlsx::from_bytes_transposed(
            bytes,
            schema,
            options.xlsx_sheet.as_deref(),
            &options.xlsx_rows,
        )
    }
    .context("parsing XLSX input payload")?;
    Ok(Instance::Repeated(rows))
}

fn render_payload(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
) -> anyhow::Result<(Vec<u8>, usize)> {
    if options.local_xml_file_set {
        bail!("`local_xml_file_set` is input-only");
    }
    if options.xbrl.is_some() {
        reject_xbrl_conflicts(options, "output")?;
        let xbrl = options
            .xbrl
            .as_ref()
            .context("missing XBRL target options")?;
        let text = format_xbrl::to_string(schema, instance, xbrl)
            .context("rendering XBRL output payload")?;
        return Ok((text.into_bytes(), 1));
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
        let text = format_flextext::to_string(schema, instance, layout)
            .context("rendering FlexText output payload")?;
        return Ok((text.into_bytes(), 1));
    }
    if let Some(protobuf) = &options.protobuf {
        reject_protobuf_conflicts(options, "output")?;
        let layout =
            protobuf_layout(protobuf).context("parsing embedded Protocol Buffers schema")?;
        let bytes = format_protobuf::to_vec(&layout, &protobuf.root_message, instance)
            .context("rendering Protocol Buffers output payload")?;
        return Ok((bytes, 1));
    }
    if let Some(kind) = options.edi_kind {
        reject_edi_conflicts(options, "output")?;
        let formatted = formatted_edi_output(instance, options)?;
        let text = render_edi_payload(schema, &formatted, options, kind, current_datetime)?;
        return Ok((text.into_bytes(), 1));
    }
    if options.xml_document {
        reject_xml_conflicts(options, "output")?;
        return format_xml::to_string(schema, instance)
            .map(|text| (text.into_bytes(), 1))
            .context("rendering XML output payload");
    }
    if options.json_document || options.json_lines {
        reject_json_conflicts(options, "output")?;
        let text = if options.json_lines {
            format_json::to_lines(schema, instance)
        } else {
            format_json::to_string(schema, instance)
        }
        .context("rendering JSON output payload")?;
        return Ok((
            text.into_bytes(),
            instance.as_repeated().map_or(1, <[Instance]>::len),
        ));
    }
    if let Some(layout) = &options.fixed_width {
        reject_fixed_width_csv_options(options, "output")?;
        let rows = instance
            .as_repeated()
            .context("mapping did not produce a repeating row set for a fixed-width output")?;
        let text = format_csv::to_string_fixed_width(schema, rows, layout)
            .context("rendering fixed-width output payload")?;
        return Ok((text.into_bytes(), rows.len()));
    }

    validate_tabular_fallback(path, options, "output")?;
    match extension_for_dispatch(path, options)?.as_str() {
        "csv" | "txt" => {
            let rows = instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for a CSV output")?;
            let text = format_csv::to_string(
                schema,
                rows,
                options.delimiter,
                options.has_header_row.unwrap_or(true),
            )
            .context("rendering CSV output payload")?;
            Ok((text.into_bytes(), rows.len()))
        }
        "xlsx" => render_xlsx_payload(schema, instance, options),
        "xml" => format_xml::to_string(schema, instance)
            .map(|text| (text.into_bytes(), 1))
            .context("rendering XML output payload"),
        "json" | "jsonl" | "ndjson" => {
            let lines =
                options.json_lines || matches!(extension_of(path)?.as_str(), "jsonl" | "ndjson");
            let text = if lines {
                format_json::to_lines(schema, instance)
            } else {
                format_json::to_string(schema, instance)
            }
            .context("rendering JSON output payload")?;
            Ok((
                text.into_bytes(),
                instance.as_repeated().map_or(1, <[Instance]>::len),
            ))
        }
        "db" | "sqlite" | "sqlite3" => {
            bail!("SQLite output requires a persistent database and is unavailable as a payload")
        }
        "edi" | "x12" | "edifact" | "hl7" => {
            let formatted = formatted_edi_output(instance, options)?;
            let text = match format_edi::dialect_of(schema)? {
                format_edi::Dialect::X12 => render_edi_payload(
                    schema,
                    &formatted,
                    options,
                    EdiBoundaryKind::X12,
                    current_datetime,
                ),
                format_edi::Dialect::Edifact => render_edi_payload(
                    schema,
                    &formatted,
                    options,
                    EdiBoundaryKind::Edifact,
                    current_datetime,
                ),
                format_edi::Dialect::Hl7 => {
                    format_edi::hl7::to_string(schema, &formatted).map_err(anyhow::Error::new)
                }
                format_edi::Dialect::Tradacoms => {
                    format_edi::tradacoms::to_string(schema, &formatted).map_err(anyhow::Error::new)
                }
            }?;
            Ok((text.into_bytes(), 1))
        }
        "pdf" => bail!("PDF output is not supported; PDF is input-only"),
        other => bail!("unsupported output payload extension: .{other}"),
    }
}

fn render_xlsx_payload(
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> anyhow::Result<(Vec<u8>, usize)> {
    if options.xlsx_update_existing {
        bail!(
            "update-existing XLSX output requires a persistent workbook and is unavailable as a payload"
        );
    }
    if let Some(layout) = &options.xlsx_hierarchical {
        if options.xlsx_grid.is_some()
            || options.xlsx_composite.is_some()
            || options.xlsx_worksheet_set.is_some()
            || has_legacy_xlsx_layout(options)
        {
            bail!("`xlsx_hierarchical` cannot be combined with other XLSX layout options");
        }
        let (bytes, worksheets) = format_xlsx::to_bytes_hierarchical(schema, instance, layout)
            .context("rendering hierarchical XLSX output payload")?;
        return Ok((bytes, worksheets));
    }
    if options.xlsx_grid.is_some() {
        bail!("grid XLSX output is not supported; `xlsx_grid` is input-only");
    }
    if options.xlsx_worksheet_set.is_some() {
        bail!("worksheet-set XLSX output is not supported; `xlsx_worksheet_set` is input-only");
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
    let bytes = format_xlsx::to_bytes_with_options(
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
    .context("rendering XLSX output payload")?;
    Ok((bytes, rows.len()))
}

fn render_edi_payload(
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
    current_datetime: &str,
) -> anyhow::Result<String> {
    match kind {
        EdiBoundaryKind::X12 => {
            let separators = options
                .x12_separators
                .map(x12_separators)
                .unwrap_or_default();
            let version = options.x12_interchange_version.as_deref();
            match options.edi_autocomplete.as_ref() {
                Some(EdiAutocomplete::X12(config)) => {
                    format_edi::x12::to_string_with_syntax_and_autocomplete(
                        schema,
                        instance,
                        separators,
                        version,
                        format_edi::x12::Autocomplete {
                            current_datetime,
                            request_acknowledgement: config.request_acknowledgement,
                            transaction_set: config.transaction_set.as_deref(),
                        },
                    )
                }
                _ => format_edi::x12::to_string_with_syntax(schema, instance, separators, version),
            }
        }
        EdiBoundaryKind::Edifact => {
            if let Some(EdiAutocomplete::Edifact(config)) = options.edi_autocomplete.as_ref() {
                format_edi::edifact::to_string_with_autocomplete(
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
                format_edi::edifact::to_string(schema, instance)
            }
        }
        EdiBoundaryKind::Hl7 => format_edi::hl7::to_string(schema, instance),
        EdiBoundaryKind::Tradacoms => format_edi::tradacoms::to_string(schema, instance),
        EdiBoundaryKind::Idoc => {
            bail!("SAP IDoc output is not supported; IDoc is input-only")
        }
        EdiBoundaryKind::SwiftMt => {
            bail!("SWIFT MT output is not supported; SWIFT MT is input-only")
        }
    }
    .context("rendering EDI output payload")
}
