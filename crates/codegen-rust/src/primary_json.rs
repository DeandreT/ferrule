use codegen::{ArtifactSet, DocumentProgram, JsonProjectionBoundaryError};
use mapping::JsonSourceShape;

use crate::{EmitError, Options, rust_string};

#[derive(Debug)]
pub enum PrimaryJsonEmitError {
    Boundary(JsonProjectionBoundaryError),
    Emit(EmitError),
    Writer(codegen::MapForceX12WriterError),
    Visitor(codegen::DocumentVisitorEmitError<EmitError>),
}
impl std::fmt::Display for PrimaryJsonEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
            Self::Writer(error) => error.fmt(f),
            Self::Visitor(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for PrimaryJsonEmitError {}

pub fn emit_with_mapforce_x12_writer(
    document: &DocumentProgram,
    options: &Options,
    shape: &JsonSourceShape,
    target_options: &mapping::FormatOptions,
    minimums: &[mapping::MapForceEdiMinimum],
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor =
        codegen::prepare_mapforce_x12_writer(document.program(), target_options, minimums)
            .map_err(PrimaryJsonEmitError::Writer)?;
    emit_with_writer(document, options, shape, descriptor)
}

/// Emit grouped documents with incremental delivery and an individual X12 writer.
/// Filename clocks precede each body; writer clocks remain separate host calls.
pub fn emit_grouped_with_mapforce_x12_writer(
    document: &DocumentProgram,
    options: &Options,
    shape: &JsonSourceShape,
    target_options: &mapping::FormatOptions,
    minimums: &[mapping::MapForceEdiMinimum],
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor =
        codegen::prepare_mapforce_x12_document_writer(document, target_options, minimums)
            .map_err(PrimaryJsonEmitError::Writer)?;
    let projection = codegen::prepare_json_source_projection(document, shape)
        .map_err(PrimaryJsonEmitError::Boundary)?;
    let base = crate::document_visitor::emit_with_static_error_document_clocks(
        document.program(),
        options,
    )
    .map_err(PrimaryJsonEmitError::Visitor)?;
    let base =
        crate::primary_filename::append(base, document).map_err(PrimaryJsonEmitError::Emit)?;
    let projected = append_projection(base, projection, document.json_grouping_preparation())?;
    append_writer(projected, descriptor)
}

fn emit_with_writer(
    document: &DocumentProgram,
    options: &Options,
    shape: &JsonSourceShape,
    descriptor: String,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let projected = emit_with_primary_json_projection(document, options, shape)?;
    append_writer(projected, descriptor)
}

fn append_writer(
    artifacts: ArtifactSet,
    descriptor: String,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let mut files = artifacts.into_files();
    let mut source = format!(
        "\npub fn serialize_mapforce_x12_document(value: &Instance, execution: &ExecutionContext<'_>) -> Result<Vec<u8>, codegen_runtime::X12BoundaryError> {{\n    codegen_runtime::serialize_mapforce_x12_document({0}, value, execution)\n}}\npub fn write_mapforce_x12_document(value: &Instance, execution: &ExecutionContext<'_>, output: &mut dyn std::io::Write) -> Result<(), codegen_runtime::MapForceX12WriteError> {{\n    codegen_runtime::write_mapforce_x12_document({0}, value, execution, output)\n}}\n",
        rust_string(&descriptor)
    );
    source.push_str(&format!("\npub fn serialize_mapforce_x12_document_with_clock(value: &Instance, execution: &ExecutionContext<'_>, clock: &mut dyn FnMut(codegen_runtime::MapForceX12ClockField) -> Result<String, String>) -> Result<Vec<u8>, codegen_runtime::X12BoundaryError> {{\n    codegen_runtime::serialize_mapforce_x12_document_with_clock({0}, value, execution, clock)\n}}\npub fn write_mapforce_x12_document_with_clock(value: &Instance, execution: &ExecutionContext<'_>, clock: &mut dyn FnMut(codegen_runtime::MapForceX12ClockField) -> Result<String, String>, output: &mut dyn std::io::Write) -> Result<(), codegen_runtime::MapForceX12WriteError> {{\n    codegen_runtime::write_mapforce_x12_document_with_clock({0}, value, execution, clock, output)\n}}\n",rust_string(&descriptor)));
    files
        .iter_mut()
        .find(|f| f.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust artifact")
        .contents
        .extend_from_slice(source.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryJsonEmitError::Emit)
}

/// Add lazy primary filename acquisition and typed ordered JSON projection.
/// The host owns transport, parsing and cleanup; this does not publish output.
pub fn emit_with_primary_json_projection(
    document: &DocumentProgram,
    options: &Options,
    shape: &JsonSourceShape,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor = codegen::prepare_json_source_projection(document, shape)
        .map_err(PrimaryJsonEmitError::Boundary)?;
    let base =
        crate::emit_with_primary_filename(document, options).map_err(PrimaryJsonEmitError::Emit)?;
    append_projection(base, descriptor, document.json_grouping_preparation())
}

fn append_projection(
    base: ArtifactSet,
    descriptor: String,
    preparation: Option<&mapping::JsonGroupingPreparation>,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let mut files = base.into_files();
    let preparation_call = if preparation.is_some() {
        ".and_then(|source| source.with_grouping_preparation(primary_json_grouping()))"
    } else {
        ""
    };
    let mut code = format!(
        "\nfn primary_json_projection() -> &'static codegen_runtime::JsonSourceShape {{\n    static SHAPE: std::sync::OnceLock<codegen_runtime::JsonSourceShape> = std::sync::OnceLock::new();\n    SHAPE.get_or_init(|| codegen_runtime::parse_json_projection_shape({}).expect(\"generation-validated JSON projection\"))\n}}\n\n/// Lazily acquire one typed document at a time. Host callbacks own I/O and parsing.\npub fn acquire_primary_json<'a, E: 'a, I: Iterator<Item = Result<String, E>> + 'a>(\n    execution: &'a ExecutionContext<'a>,\n    select: impl FnOnce(&str) -> Result<I, E> + 'a,\n    load: impl FnMut(&str) -> Result<codegen_runtime::JsonSourceDocument, E> + 'a,\n    limits: codegen_runtime::JsonProjectionLimits, maximum: usize,\n) -> Result<codegen_runtime::JsonSourceAcquisition<'a, codegen_runtime::RuntimeError, E>, codegen_runtime::JsonAcquisitionError<codegen_runtime::RuntimeError, E>> {{\n    codegen_runtime::JsonSourceAcquisition::new(move || evaluate_primary_filename(execution), select, load, primary_json_projection(), limits, maximum)\n}}\n",
        rust_string(&descriptor)
    );
    if let Some(preparation) = preparation {
        let encoded = serde_json::to_string(preparation)
            .map_err(JsonProjectionBoundaryError::Descriptor)
            .map_err(PrimaryJsonEmitError::Boundary)?;
        code = code.replacen(
            "primary_json_projection(), limits, maximum)\n",
            &format!("primary_json_projection(), limits, maximum){preparation_call}\n"),
            1,
        );
        code.push_str(&format!("\nfn primary_json_grouping() -> &'static codegen_runtime::JsonGroupingPreparation {{\n    static PLAN: std::sync::OnceLock<codegen_runtime::JsonGroupingPreparation> = std::sync::OnceLock::new();\n    PLAN.get_or_init(|| codegen_runtime::parse_json_grouping_preparation({}).expect(\"generation-validated grouping preparation\"))\n}}\npub fn prepare_primary_json_document(document: Instance, limits: codegen_runtime::JsonProjectionLimits) -> Result<Instance, codegen_runtime::JsonGroupingError> {{\n    codegen_runtime::prepare_json_grouping_document(primary_json_grouping(), primary_json_projection(), document, limits)\n}}\n",rust_string(&encoded)));
    }
    code.push_str("\n/// Explicit wire-compatible byte decoding. Lone UTF-16 escapes become U+FFFD;\n/// separately qualify string operations and encoding. Strict parsing is unchanged.\npub fn parse_primary_json_source(identity: &str, bytes: &[u8]) -> Result<codegen_runtime::JsonSourceDocument, codegen_runtime::NativeJsonSourceError> {\n    codegen_runtime::parse_native_json_source(identity, bytes, primary_json_projection(), codegen_runtime::JsonSourceLimits::default())\n}\n");
    code.push_str("\n/// Sample the body clock only at reached current-dateTime nodes.\n/// Source acquisition and filename clocks remain host-owned separate calls.\npub fn execute_primary_json_with_body_clock<'a>(source: &'a Instance, execution: &ExecutionContext<'a>, clock: &'a dyn codegen_runtime::DocumentBodyClock) -> Result<Instance, RuntimeError> {\n    let context = ScopeContext::with_execution_context(source, execution).with_document_body_clock(Some(clock));\n    Ok(execute_outputs_from_context(&context)?.primary)\n}\n");
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust artifact")
        .contents
        .extend_from_slice(code.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryJsonEmitError::Emit)
}
