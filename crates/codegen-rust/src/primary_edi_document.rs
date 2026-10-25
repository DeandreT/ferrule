use crate::{EmitError, Options, PrimaryDocumentEmitError};
use codegen::{ArtifactSet, DocumentProgram};

pub fn emit_with_primary_edi_documents(
    document: &DocumentProgram,
    options: &Options,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    let policy = codegen::validate_primary_edi_documents(document)
        .map_err(PrimaryDocumentEmitError::Boundary)?;
    let mut files = crate::emit(document.program(), options)
        .map_err(PrimaryDocumentEmitError::Emit)?
        .into_files();
    let source = serde_json::to_string(&document.program().source).expect("schema");
    let target = serde_json::to_string(&document.program().target).expect("schema");
    let input = serde_json::to_string(&policy.source).expect("options");
    let output = serde_json::to_string(&policy.target).expect("options");
    let kind = format!("{:?}", policy.kind);
    let mut code = crate::primary_filename::evaluator(document);
    code.push_str(&crate::primary_filename::selection_constant(document));
    code.push_str(&crate::primary_xml_document::shape_constant(
        document.primary_route().expect("validated route").shape,
    ));
    code.push_str(&format!(r#"
pub fn execute_primary_edi_document_bytes<H: codegen_runtime::PrimarySourceHost>(execution: &ExecutionContext<'_>, host: &H, maximum: usize)
    -> Result<Vec<u8>, codegen_runtime::PrimaryEdiDocumentError<H::Error>> {{
    use codegen_runtime::{{EdiBoundaryError, EdiDocumentBoundary, PrimaryEdiDocumentError as Error, PrimaryEdiKind}};
    let input = EdiDocumentBoundary::input({source:?}, {input:?}, PrimaryEdiKind::{kind}).map_err(Error::Boundary)?;
    let output = EdiDocumentBoundary::new({target:?}, {output:?}, PrimaryEdiKind::{kind}).map_err(Error::Boundary)?;
    let mut budget = codegen_runtime::EdiInputBudget::default();
    let documents = codegen_runtime::acquire_primary_documents(|| evaluate_primary_filename(execution), PRIMARY_FILE_SELECTION, host, maximum,
        |_, data| {{ budget.charge(data.bytes.len())?; input.parse(&data.bytes) }})
        .map_err(Error::Acquisition)?;
    let source = codegen_runtime::assemble_primary_documents(PRIMARY_DOCUMENT_SHAPE, documents).map_err(Error::Assembly)?;
    let mapped = execute_outputs_with_context(&source, execution).map_err(EdiBoundaryError::Execution).map_err(Error::Boundary)?;
    output.serialize(&mapped.primary, execution.current_datetime().unwrap_or("")).map_err(Error::Boundary)
}}
"#));
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("library")
        .contents
        .extend_from_slice(code.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryDocumentEmitError::Emit)
}
