use crate::{EmitError, Options, PrimaryDocumentEmitError};
use codegen::{ArtifactSet, DocumentProgram};

/// Emits native fixed-width readers, writers and one combined primary row run.
pub fn emit_with_primary_fixed_width_documents(
    document: &DocumentProgram,
    options: &Options,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    let policy = codegen::validate_primary_fixed_width_documents(document)
        .map_err(PrimaryDocumentEmitError::Boundary)?;
    let mut files = crate::emit(document.program(), options)
        .map_err(PrimaryDocumentEmitError::Emit)?
        .into_files();
    let descriptor = |value| serde_json::to_string(value).expect("validated descriptor serializes");
    let mut code = crate::primary_filename::evaluator(document);
    code.push_str(&crate::primary_filename::selection_constant(document));
    let source = descriptor(&document.program().source);
    let target = descriptor(&document.program().target);
    let input_layout = serde_json::to_string(&policy.source).expect("validated layout");
    let output_layout = serde_json::to_string(&policy.target).expect("validated layout");
    code.push_str(&format!(r#"
const FIXED_WIDTH_SOURCE: &str = {source:?};
const FIXED_WIDTH_TARGET: &str = {target:?};
const FIXED_WIDTH_INPUT_LAYOUT: &str = {input_layout:?};
const FIXED_WIDTH_OUTPUT_LAYOUT: &str = {output_layout:?};
pub fn parse_fixed_width_bytes(path: &str, resolved: Option<&str>, bytes: &[u8]) -> Result<Instance, codegen_runtime::FixedWidthBoundaryError> {{
    codegen_runtime::parse_fixed_width_document(FIXED_WIDTH_SOURCE, FIXED_WIDTH_INPUT_LAYOUT, path, resolved, bytes)
}}
pub fn serialize_fixed_width_bytes(source: &Instance) -> Result<Vec<u8>, codegen_runtime::FixedWidthBoundaryError> {{
    codegen_runtime::serialize_fixed_width_document(FIXED_WIDTH_TARGET, FIXED_WIDTH_OUTPUT_LAYOUT, source)
}}
pub fn execute_primary_fixed_width_document_bytes<H: codegen_runtime::PrimarySourceHost>(execution: &ExecutionContext<'_>, host: &H, maximum: usize)
    -> Result<Vec<u8>, codegen_runtime::PrimaryFixedWidthDocumentError<H::Error>> {{
    let mut budget = codegen_runtime::FixedWidthInputBudget::default();
    let documents = codegen_runtime::acquire_primary_documents(|| evaluate_primary_filename(execution), PRIMARY_FILE_SELECTION, host, maximum,
        |request, data| {{
            budget.charge(data.bytes.len())?;
            parse_fixed_width_bytes(request, data.resolved_path.as_deref(), &data.bytes)
        }}).map_err(codegen_runtime::PrimaryFixedWidthDocumentError::Acquisition)?;
    let source = codegen_runtime::assemble_primary_documents(codegen_runtime::PrimaryDocumentShape::Rows, documents)
        .map_err(codegen_runtime::PrimaryFixedWidthDocumentError::Assembly)?;
    let mapped = execute_outputs_with_context(&source, execution)
        .map_err(codegen_runtime::FixedWidthBoundaryError::Execution)
        .map_err(codegen_runtime::PrimaryFixedWidthDocumentError::Output)?;
    serialize_fixed_width_bytes(&mapped.primary).map_err(codegen_runtime::PrimaryFixedWidthDocumentError::Output)
}}
"#));
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("mapping library")
        .contents
        .extend_from_slice(code.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryDocumentEmitError::Emit)
}
