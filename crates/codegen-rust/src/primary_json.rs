use codegen::{ArtifactSet, DocumentProgram, JsonProjectionBoundaryError};
use mapping::JsonSourceShape;

use crate::{EmitError, Options, rust_string};

#[derive(Debug)]
pub enum PrimaryJsonEmitError {
    Boundary(JsonProjectionBoundaryError),
    Emit(EmitError),
    Writer(codegen::MapForceX12WriterError),
}
impl std::fmt::Display for PrimaryJsonEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
            Self::Writer(error) => error.fmt(f),
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
    let mut files = emit_with_primary_json_projection(document, options, shape)?.into_files();
    let source = format!(
        "\npub fn serialize_mapforce_x12_document(value: &Instance, execution: &ExecutionContext<'_>) -> Result<Vec<u8>, codegen_runtime::X12BoundaryError> {{\n    codegen_runtime::serialize_mapforce_x12_document({0}, value, execution)\n}}\npub fn write_mapforce_x12_document(value: &Instance, execution: &ExecutionContext<'_>, output: &mut dyn std::io::Write) -> Result<(), codegen_runtime::MapForceX12WriteError> {{\n    codegen_runtime::write_mapforce_x12_document({0}, value, execution, output)\n}}\n",
        rust_string(&descriptor)
    );
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
    let mut files = crate::emit_with_primary_filename(document, options)
        .map_err(PrimaryJsonEmitError::Emit)?
        .into_files();
    let mut code = format!(
        "\nfn primary_json_projection() -> &'static codegen_runtime::JsonSourceShape {{\n    static SHAPE: std::sync::OnceLock<codegen_runtime::JsonSourceShape> = std::sync::OnceLock::new();\n    SHAPE.get_or_init(|| codegen_runtime::parse_json_projection_shape({}).expect(\"generation-validated JSON projection\"))\n}}\n\n/// Lazily acquire one typed document at a time. Host callbacks own I/O and parsing.\npub fn acquire_primary_json<'a, E: 'a, I: Iterator<Item = Result<String, E>> + 'a>(\n    execution: &'a ExecutionContext<'a>,\n    select: impl FnOnce(&str) -> Result<I, E> + 'a,\n    load: impl FnMut(&str) -> Result<codegen_runtime::JsonSourceDocument, E> + 'a,\n    limits: codegen_runtime::JsonProjectionLimits, maximum: usize,\n) -> Result<codegen_runtime::JsonSourceAcquisition<'a, codegen_runtime::RuntimeError, E>, codegen_runtime::JsonAcquisitionError<codegen_runtime::RuntimeError, E>> {{\n    codegen_runtime::JsonSourceAcquisition::new(move || evaluate_primary_filename(execution), select, load, primary_json_projection(), limits, maximum)\n}}\n",
        rust_string(&descriptor)
    );
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
