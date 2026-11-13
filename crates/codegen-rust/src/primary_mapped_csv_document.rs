use std::fmt;

use codegen::{
    ArtifactSet, CsvInputPolicy, DocumentProgram, PrimaryMappedCsvDocumentBoundaryError,
    PrimaryMappedCsvDocumentPolicy,
};
use ir::SchemaKind;

use crate::{
    EmitError, Options, mapped_csv_document_output::MappedCsvDocumentEmitError, rust_string,
};

#[derive(Debug)]
pub enum PrimaryMappedCsvDocumentEmitError {
    Boundary(PrimaryMappedCsvDocumentBoundaryError),
    Emit(EmitError),
}

impl fmt::Display for PrimaryMappedCsvDocumentEmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PrimaryMappedCsvDocumentEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Boundary(error) => error,
            Self::Emit(error) => error,
        })
    }
}

impl From<EmitError> for PrimaryMappedCsvDocumentEmitError {
    fn from(error: EmitError) -> Self {
        Self::Emit(error)
    }
}

impl From<MappedCsvDocumentEmitError> for PrimaryMappedCsvDocumentEmitError {
    fn from(error: MappedCsvDocumentEmitError) -> Self {
        match error {
            MappedCsvDocumentEmitError::Boundary(error) => {
                Self::Boundary(PrimaryMappedCsvDocumentBoundaryError::Output(error))
            }
            MappedCsvDocumentEmitError::Emit(error) => Self::Emit(error),
        }
    }
}

/// Acquire one directly named CSV input, then map once and return the complete
/// strict mapped CSV document list. Publishing belongs to the caller.
pub fn emit_with_primary_mapped_csv_documents(
    document: &DocumentProgram,
    options: &Options,
    policy: &PrimaryMappedCsvDocumentPolicy,
) -> Result<ArtifactSet, PrimaryMappedCsvDocumentEmitError> {
    codegen::validate_primary_mapped_csv_documents(document, policy)
        .map_err(PrimaryMappedCsvDocumentEmitError::Boundary)?;
    let mut files = crate::mapped_csv_document_output::emit_validated(
        document.program(),
        options,
        &policy.output,
    )?
    .into_files();
    let mut code = crate::primary_filename::evaluator(document);
    code.push_str(&crate::primary_filename::selection_constant(document));
    code.push_str(&input_settings(&document.program().source, &policy.input));
    code.push_str(ERROR);
    for (suffix, kind) in [("", "Text"), ("_bytes", "Bytes")] {
        code.push_str(&format!(
            r#"
/// Acquire and parse the complete direct input, then map once with the same
/// execution context and return every output document, or one typed failure.
pub fn execute_primary_mapped_csv_documents{suffix}<H: codegen_runtime::PrimarySourceHost>(
    execution: &ExecutionContext<'_>,
    host: &H,
    decoding: codegen_runtime::CsvByteDecodingPolicy,
    maximum: usize,
) -> Result<Vec<codegen_runtime::Csv{kind}Document>, PrimaryMappedCsvDocumentError<H::Error>> {{
    let schema = csv_input_schema();
    let options = csv_input_options();
    let documents = codegen_runtime::acquire_primary_documents(
        || evaluate_primary_filename(execution),
        PRIMARY_FILE_SELECTION,
        host,
        maximum,
        |request, data| {{
            let parsed = codegen_runtime::CsvSourceDocument::parse_bytes(
                request,
                data.resolved_path.as_deref(),
                &data.bytes,
                &schema,
                &options,
                decoding,
            )?;
            Ok::<_, codegen_runtime::CsvSourceError>(Instance::Repeated(
                parsed.rows().map(|row| row.value().clone()).collect(),
            ))
        }},
    ).map_err(PrimaryMappedCsvDocumentError::Acquisition)?;
    let source = codegen_runtime::assemble_primary_documents(
        codegen_runtime::PrimaryDocumentShape::Rows,
        documents,
    ).map_err(PrimaryMappedCsvDocumentError::Assembly)?;
    execute_mapped_csv_documents{suffix}_with_context(&source, execution)
        .map_err(PrimaryMappedCsvDocumentError::Output)
}}
"#,
        ));
    }
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust mapping artifact")
        .contents
        .extend_from_slice(code.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(Into::into)
}

fn input_settings(schema: &ir::SchemaNode, policy: &CsvInputPolicy) -> String {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        unreachable!("admitted CSV source")
    };
    let mut code = String::from("\nfn csv_input_schema() -> codegen_runtime::SchemaNode {\n");
    code.push_str(&format!(
        "    codegen_runtime::SchemaNode::group({}, vec![\n",
        rust_string(&schema.name)
    ));
    for field in children {
        let SchemaKind::Scalar { ty } = field.kind else {
            unreachable!("admitted CSV field")
        };
        code.push_str(&format!(
            "        codegen_runtime::SchemaNode::scalar({}, codegen_runtime::ScalarType::{ty:?}),\n",
            rust_string(&field.name)
        ));
    }
    code.push_str(&format!(
        "    ])\n}}\n\nfn csv_input_options() -> codegen_runtime::CsvReadOptions {{\n    codegen_runtime::CsvReadOptions {{\n        delimiter: {:?},\n        quote: {:?},\n        quote_disabled: {},\n        has_headers: {},\n        preserve_empty_strings: {},\n        repair_dependency: None,\n    }}\n}}\n",
        policy.delimiter,
        policy.quote,
        policy.quote_disabled,
        policy.has_headers,
        policy.preserve_empty_strings,
    ));
    code
}

const ERROR: &str = r#"
/// Stage-preserving failures of primary acquisition and mapped CSV output.
#[derive(Debug)]
pub enum PrimaryMappedCsvDocumentError<E> {
    Acquisition(codegen_runtime::PrimaryAcquisitionError<
        codegen_runtime::RuntimeError, E, codegen_runtime::CsvSourceError>),
    Assembly(codegen_runtime::PrimaryAssemblyError),
    Output(codegen_runtime::CsvDocumentOutputError),
}

impl<E: std::fmt::Display> std::fmt::Display for PrimaryMappedCsvDocumentError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Acquisition(error) => error.fmt(f),
            Self::Assembly(error) => error.fmt(f),
            Self::Output(error) => error.fmt(f),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for PrimaryMappedCsvDocumentError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Acquisition(error) => error,
            Self::Assembly(error) => error,
            Self::Output(error) => error,
        })
    }
}
"#;
