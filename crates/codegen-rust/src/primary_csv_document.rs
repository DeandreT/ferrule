use codegen::{ArtifactSet, CsvDocumentBoundaryPolicy, DocumentProgram};

use crate::{CsvDocumentEmitError, EmitError, Options};

/// Add one complete primary CSV document entry point to the CSV document
/// adapters: the embedded filename plan and selection mode go to a host that
/// selects and loads; every selected document is parsed before the ordinary
/// mapper runs once; the caller publishes the returned bytes.
pub fn emit_with_primary_csv_documents(
    document: &DocumentProgram,
    options: &Options,
    policy: &CsvDocumentBoundaryPolicy,
) -> Result<ArtifactSet, CsvDocumentEmitError> {
    let mut files =
        crate::emit_with_csv_document_adapters(document.program(), options, policy)?.into_files();
    let mut code = crate::primary_filename::evaluator(document);
    code.push_str(&crate::primary_filename::selection_constant(document));
    code.push_str("\n/// Evaluate the primary filename, let `host` select and load, parse every\n/// selected document, then map once over all rows and return one complete CSV\n/// document. The first acquisition failure stops before later loads and before\n/// mapping. Publishing the bytes belongs to the caller.\npub fn execute_primary_csv_document_bytes<H: codegen_runtime::PrimarySourceHost>(\n    execution: &ExecutionContext<'_>,\n    host: &H,\n    decoding: codegen_runtime::CsvByteDecodingPolicy,\n    maximum: usize,\n) -> Result<Vec<u8>, codegen_runtime::PrimaryCsvDocumentError<H::Error>> {\n    let schema = csv_input_schema();\n    let input = codegen_runtime::acquire_primary_csv_rows(\n        || evaluate_primary_filename(execution),\n        PRIMARY_FILE_SELECTION,\n        host,\n        &schema,\n        csv_input_options(),\n        decoding,\n        maximum,\n    )\n    .map_err(codegen_runtime::PrimaryCsvDocumentError::Acquisition)?;\n    execute_csv_bytes_with_context(&input, execution)\n        .map_err(codegen_runtime::PrimaryCsvDocumentError::Output)\n}\n");
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust mapping artifact")
        .contents
        .extend_from_slice(code.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(CsvDocumentEmitError::Emit)
}
