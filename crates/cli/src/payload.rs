mod serialization;

pub(crate) use serialization::render_payload;
use serialization::{Limits, RenderedArtifact, render_target};

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use ir::{Instance, SchemaNode};
use mapping::{EdiBoundaryKind, ExternalPayloadFormat, FormatOptions};

use super::{
    TraceSink, absolute_mapping_path, extension_for_dispatch, extension_of, has_legacy_xlsx_layout,
    json5_selected, protobuf_layout, reject_edi_conflicts, reject_external_source_conflicts,
    reject_fixed_width_csv_options, reject_flextext_conflicts, reject_idoc_conflicts,
    reject_json_conflicts, reject_pdf_conflicts, reject_protobuf_conflicts, reject_swift_conflicts,
    reject_xbrl_conflicts, reject_xml_conflicts, require_valid, resolve_run_path,
    validate_tabular_fallback, x12_separators,
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
    target: Option<engine::TargetSelection<'a>>,
    runtime_parameters: Option<&'a engine::RuntimeParameters>,
    execution_purpose: engine::ExecutionPurpose,
    trace_sink: Option<&'a dyn TraceSink>,
    debug_hook: Option<&'a dyn engine::DebugHook>,
}

impl<'a> PayloadRunOptions<'a> {
    pub fn new(primary: PayloadDocument<'a>) -> Self {
        Self {
            primary,
            extra_sources: &[],
            output_path: None,
            target: None,
            runtime_parameters: None,
            execution_purpose: engine::ExecutionPurpose::Run,
            trace_sink: None,
            debug_hook: None,
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

    pub fn with_target(mut self, target: engine::TargetSelection<'a>) -> Self {
        self.target = Some(target);
        self
    }

    pub fn with_runtime_parameters(mut self, parameters: &'a engine::RuntimeParameters) -> Self {
        self.runtime_parameters = Some(parameters);
        self
    }

    /// Enables design-time preview inputs explicitly. Ordinary payload runs
    /// ignore saved preview values and retain the normal host/default contract.
    pub fn with_execution_purpose(mut self, purpose: engine::ExecutionPurpose) -> Self {
        self.execution_purpose = purpose;
        self
    }

    pub fn with_trace_sink(mut self, trace_sink: &'a dyn TraceSink) -> Self {
        self.trace_sink = Some(trace_sink);
        self
    }

    /// Supplies a synchronous host control point before ordinary target writes.
    /// Cancelling returns an error before any payload artifacts are rendered.
    pub fn with_debug_hook(mut self, debug_hook: &'a dyn engine::DebugHook) -> Self {
        self.debug_hook = Some(debug_hook);
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
    let required_sources = options
        .target
        .map(|selection| engine::required_sources_for_target(project, selection))
        .transpose()?;
    let required_static_names = required_sources.as_ref().map(|sources| {
        sources
            .static_sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<BTreeSet<_>>()
    });
    let required_dynamic_names = required_sources.as_ref().map(|sources| {
        sources
            .dynamic_sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<BTreeSet<_>>()
    });
    let loaded_sources = load_extra_payloads(
        project,
        options.extra_sources,
        required_static_names.as_ref(),
        required_dynamic_names.as_ref(),
    )?;

    let runtime_project_path = absolute_mapping_path(project_path)?;
    let current_datetime = jiff::Zoned::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
        .to_string();
    let dynamic_loader = PayloadDynamicSourceLoader {
        sources: loaded_sources.dynamic,
    };
    let mut execution = engine::ExecutionContext::new(&runtime_project_path)
        .with_purpose(options.execution_purpose)
        .with_current_datetime(&current_datetime)
        .with_dynamic_source_loader(&dynamic_loader);
    if let Some(parameters) = options.runtime_parameters {
        execution = execution.with_parameters(parameters);
    }
    if let Some(trace_sink) = options.trace_sink {
        execution = execution.with_trace_sink(trace_sink);
    }
    if let Some(debug_hook) = options.debug_hook {
        execution = execution.with_debug_hook(debug_hook);
    }
    let (records_written, artifacts) = match options.target {
        Some(selection) => {
            let output = engine::run_selected_target_with_sources_and_context(
                project,
                &source,
                loaded_sources.static_sources,
                &execution,
                selection,
            )?;
            render_selected_target(
                project,
                project_path,
                options.output_path,
                &output,
                &current_datetime,
                Limits::PRODUCTION,
            )?
        }
        None => {
            let output = engine::run_outputs_with_sources_and_context(
                project,
                &source,
                loaded_sources.static_sources,
                &execution,
            )?;
            render_all_targets(
                project,
                project_path,
                options.output_path,
                &output,
                &current_datetime,
                Limits::PRODUCTION,
            )?
        }
    };

    let artifacts = serialization::finalize(artifacts, Limits::PRODUCTION)?;
    Ok(PayloadRunOutcome {
        records_written,
        artifacts,
    })
}

fn render_all_targets(
    project: &mapping::Project,
    project_path: &Path,
    output_path: Option<&Path>,
    output: &engine::ExecutionOutputs,
    current_datetime: &str,
    limits: Limits,
) -> anyhow::Result<(usize, Vec<RenderedArtifact>)> {
    if output.extras.len() != project.extra_targets.len() {
        bail!("engine returned an unexpected number of additional target values");
    }
    validate_artifact_count(project, output)?;

    let primary_destination = target_destination(
        project_path,
        output_path,
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
        current_datetime,
        limits,
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
            current_datetime,
            limits,
        )
        .with_context(|| format!("rendering extra target `{}`", target.name))?;
        artifacts.extend(rendered);
    }
    Ok((records_written, artifacts))
}

fn render_selected_target(
    project: &mapping::Project,
    project_path: &Path,
    output_path: Option<&Path>,
    output: &engine::SelectedTargetOutput,
    current_datetime: &str,
    limits: Limits,
) -> anyhow::Result<(usize, Vec<RenderedArtifact>)> {
    let (name, stored, scope, schema, instance, options, label) = match output {
        engine::SelectedTargetOutput::Primary(instance) => (
            project.target.name.as_str(),
            project.target_path.as_deref(),
            &project.root,
            &project.target,
            instance,
            &project.target_options,
            "primary target".to_string(),
        ),
        engine::SelectedTargetOutput::Named(output) => {
            let target = project
                .extra_targets
                .iter()
                .find(|target| target.name == output.name)
                .context("engine returned an unknown selected target")?;
            (
                target.name.as_str(),
                target.path.as_deref(),
                &target.root,
                &target.schema,
                &output.instance,
                &target.options,
                format!("extra target `{}`", target.name),
            )
        }
    };
    let count = target_artifact_count(instance, scope.output_path().is_some())?;
    if count > MAX_PAYLOAD_ARTIFACTS {
        bail!("payload run exceeds the limit of {MAX_PAYLOAD_ARTIFACTS} output artifacts");
    }
    let destination = target_destination(
        project_path,
        output_path,
        stored,
        scope.output_path().is_some(),
        &label,
    )?;
    let artifacts = render_target(
        name,
        &destination,
        schema,
        instance,
        options,
        current_datetime,
        limits,
    )
    .with_context(|| format!("rendering {label}"))?;
    let records_written = artifacts
        .iter()
        .map(|artifact| artifact.records_written)
        .sum();
    Ok((records_written, artifacts))
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
    required_static: Option<&BTreeSet<&str>>,
    required_dynamic: Option<&BTreeSet<&str>>,
) -> anyhow::Result<LoadedPayloadSources> {
    let mut static_payloads = BTreeMap::new();
    let mut dynamic_payloads = BTreeMap::new();
    for input in inputs {
        let source = project
            .extra_sources
            .iter()
            .find(|source| source.name == input.name)
            .with_context(|| format!("payload source `{}` is not declared", input.name))?;
        if source.dynamic_path.is_none()
            && required_static.is_some_and(|required| !required.contains(source.name.as_str()))
        {
            continue;
        }
        if source.dynamic_path.is_some()
            && required_dynamic.is_some_and(|required| !required.contains(source.name.as_str()))
        {
            continue;
        }
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
        if source.dynamic_path.is_some()
            || required_static.is_some_and(|required| !required.contains(source.name.as_str()))
        {
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

pub(crate) fn validate_logical_path<'a>(path: &'a Path, label: &str) -> anyhow::Result<&'a str> {
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

pub(crate) enum PayloadDestination {
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

/// Visit logical documents without materializing their serialized buffers.
pub(crate) fn visit_target_documents(
    destination: &PayloadDestination,
    instance: &Instance,
    mut visit: impl FnMut(PathBuf, &Instance) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    match (destination, instance) {
        (PayloadDestination::Static(_), Instance::DocumentSet(_)) => {
            bail!("mapping produced dynamically named documents for a static output path")
        }
        (PayloadDestination::DynamicBase(_), value)
            if !matches!(value, Instance::DocumentSet(_)) =>
        {
            bail!("dynamic target mapping did not produce a document set")
        }
        (PayloadDestination::Static(path), instance) => {
            validate_logical_path(path, "output artifact")?;
            visit(path.clone(), instance)?;
        }
        (PayloadDestination::DynamicBase(base), Instance::DocumentSet(documents)) => {
            let paths = super::output_documents::validate_document_paths(documents)?;
            for (document, path) in documents.iter().zip(paths) {
                let path = base.join(path);
                validate_logical_path(&path, "output artifact")?;
                visit(path, document.value())?;
            }
        }
        (PayloadDestination::DynamicBase(_), _) => unreachable!("guarded above"),
    }
    Ok(())
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

pub(crate) fn target_artifact_count(instance: &Instance, dynamic: bool) -> anyhow::Result<usize> {
    match (dynamic, instance) {
        (false, Instance::DocumentSet(_)) => {
            bail!("mapping produced dynamically named documents for a static output path")
        }
        (false, _) => Ok(1),
        (true, Instance::DocumentSet(documents)) => Ok(documents.len()),
        (true, _) => bail!("dynamic target mapping did not produce a document set"),
    }
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

pub(crate) fn read_payload(
    document: PayloadDocument<'_>,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> anyhow::Result<Instance> {
    options
        .validate_xml_schema_hint_options(false)
        .map_err(anyhow::Error::msg)?;
    super::validate_csv_metadata_identity(document.path, options, "input")?;
    super::reject_inactive_root_xml_read_options(document.path, schema, options, "input")?;
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
        let mut instance =
            format_edi::idoc::from_bytes(document.bytes, schema, layout, options.lenient_segments)
                .context("parsing SAP IDoc input payload")?;
        format_edi::apply_implied_decimals(&mut instance, &options.edi_implied_decimals)
            .context("applying EDI numeric formats to SAP IDoc input payload")?;
        return Ok(instance);
    }
    if let Some(layout) = &options.swift_mt {
        reject_swift_conflicts(options, "input")?;
        let mut instance =
            format_edi::swift::from_bytes(document.bytes, schema, layout, options.lenient_segments)
                .context("parsing SWIFT MT input payload")?;
        format_edi::apply_implied_decimals(&mut instance, &options.edi_implied_decimals)
            .context("applying EDI numeric formats to SWIFT MT input payload")?;
        return Ok(instance);
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
            None => {
                format_xml::from_str_with_options(text, schema, &super::xml_read_options(options))
            }
        }
        .context("parsing XML input payload");
    }
    if options.json_document || options.json5 || options.json_lines {
        reject_json_conflicts(options, "input")?;
        let json5 = json5_selected(document.path, options)?;
        let text = utf8(document, if json5 { "JSON5" } else { "JSON" })?;
        return if options.json_lines {
            format_json::from_lines(text, schema)
        } else if json5 {
            format_json::from_json5_str(text, schema)
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
        "csv" | "txt" => format_csv::from_str_with_options(
            utf8(document, "CSV")?,
            schema,
            &format_csv::CsvReadOptions::from(options),
        )
        .map(Instance::Repeated)
        .context("parsing CSV input payload"),
        "xlsx" => read_xlsx_payload(document.bytes, schema, options),
        "xml" => format_xml::from_str(utf8(document, "XML")?, schema)
            .context("parsing XML input payload"),
        "json" | "json5" | "jsonl" | "ndjson" => {
            let lines = options.json_lines
                || matches!(extension_of(document.path)?.as_str(), "jsonl" | "ndjson");
            let json5 = json5_selected(document.path, options)?;
            if lines && json5 {
                bail!("JSON5 cannot be combined with JSON Lines");
            }
            if lines {
                format_json::from_lines(utf8(document, "JSON")?, schema)
            } else if json5 {
                format_json::from_json5_str(utf8(document, "JSON5")?, schema)
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
