use codegen::{ArtifactSet, DocumentProgram, PrimaryDocumentBoundaryError};

use crate::{EmitError, Options, PrimaryDocumentEmitError};

/// Emits a strict-JSON primary filename plan. All selected documents are parsed
/// before one mapping execution; wildcard members retain their path identities.
pub fn emit_with_primary_json_documents(
    document: &DocumentProgram,
    options: &Options,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    codegen::validate_primary_json_documents(document)
        .map_err(PrimaryDocumentEmitError::Boundary)?;
    let shape = document
        .primary_route()
        .map_err(|error| {
            PrimaryDocumentEmitError::Boundary(PrimaryDocumentBoundaryError::Route(error))
        })?
        .shape;
    let mut files = crate::emit(document.program(), options)
        .map_err(PrimaryDocumentEmitError::Emit)?
        .into_files();
    let mut code = crate::primary_filename::evaluator(document);
    code.push_str(&crate::primary_filename::selection_constant(document));
    code.push_str(&crate::primary_xml_document::shape_constant(shape));
    code.push_str(
        r#"
/// Select and parse every strict JSON document, then map once and return the
/// complete JSON output. Publishing is the caller's responsibility.
pub fn execute_primary_json_document_bytes<H: codegen_runtime::PrimarySourceHost>(
    execution: &ExecutionContext<'_>, host: &H, maximum: usize,
) -> Result<Vec<u8>, codegen_runtime::PrimaryJsonDocumentError<H::Error>> {
    let mut budget = codegen_runtime::PrimaryJsonInputBudget::default();
    let documents = codegen_runtime::acquire_primary_documents(
        || evaluate_primary_filename(execution), PRIMARY_FILE_SELECTION, host, maximum,
        |_, data| {
            budget.charge(data.bytes.len())?;
            codegen_runtime::parse_json_bytes(SOURCE_JSON_SCHEMA, &data.bytes)
        },
    ).map_err(codegen_runtime::PrimaryJsonDocumentError::Acquisition)?;
    let source = codegen_runtime::assemble_primary_documents(PRIMARY_DOCUMENT_SHAPE, documents)
        .map_err(codegen_runtime::PrimaryJsonDocumentError::Assembly)?;
    let mapped = execute_outputs_with_context(&source, execution)
        .map_err(codegen_runtime::JsonBoundaryError::from)
        .map_err(codegen_runtime::PrimaryJsonDocumentError::Output)?;
    serialize_json_bytes_outputs(mapped).map(|outputs| outputs.primary)
        .map_err(codegen_runtime::PrimaryJsonDocumentError::Output)
}
"#,
    );
    let file = files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust mapping artifact");
    file.contents.extend_from_slice(code.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryDocumentEmitError::Emit)
}
