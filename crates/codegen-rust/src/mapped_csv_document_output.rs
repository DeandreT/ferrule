use std::fmt;

use codegen::{
    ArtifactSet, CompanionSelection, CsvMappedDocumentOutputError, CsvMappedDocumentOutputPolicy,
    Program,
};

use crate::{EmitError, Options, rust_string};

#[derive(Debug)]
pub enum MappedCsvDocumentEmitError {
    Boundary(CsvMappedDocumentOutputError),
    Emit(EmitError),
}

impl fmt::Display for MappedCsvDocumentEmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for MappedCsvDocumentEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Boundary(error) => error,
            Self::Emit(error) => error,
        })
    }
}

impl From<EmitError> for MappedCsvDocumentEmitError {
    fn from(error: EmitError) -> Self {
        Self::Emit(error)
    }
}

/// Typed input, ordinary mapping execution, then one complete bounded list of
/// projected CSV documents. No filename acquisition or filesystem publication.
pub fn emit_with_mapped_csv_document_output(
    program: &Program,
    options: &Options,
    policy: &CsvMappedDocumentOutputPolicy,
) -> Result<ArtifactSet, MappedCsvDocumentEmitError> {
    codegen::validate_csv_mapped_document_output(program, policy)
        .map_err(MappedCsvDocumentEmitError::Boundary)?;
    emit_validated(program, options, policy)
}

/// Render only after the caller has admitted the mapped-output boundary.
pub(crate) fn emit_validated(
    program: &Program,
    options: &Options,
    policy: &CsvMappedDocumentOutputPolicy,
) -> Result<ArtifactSet, MappedCsvDocumentEmitError> {
    let row = policy
        .row_schema(&program.target)
        .map_err(MappedCsvDocumentEmitError::Boundary)?;
    let mut adapters = crate::csv_api::render_row_settings(&row, &policy.target)?;
    for (suffix, kind, serializer) in [
        ("", "Text", "serialize_mapped_csv_documents"),
        ("_bytes", "Bytes", "serialize_mapped_csv_documents_bytes"),
    ] {
        for context in [false, true] {
            let (context_suffix, argument, call) = if context {
                (
                    "_with_context",
                    ", execution: &ExecutionContext<'_>",
                    "execute_with_context(source, execution)",
                )
            } else {
                ("", "", "execute(source)")
            };
            adapters.push_str(&format!(
                "\n/// Map once and return all documents, or a typed failure without partial output.\npub fn execute_mapped_csv_documents{suffix}{context_suffix}(source: &Instance{argument}) -> Result<Vec<codegen_runtime::Csv{kind}Document>, codegen_runtime::CsvDocumentOutputError> {{\n    let output = {call}?;\n    codegen_runtime::{serializer}(&csv_output_schema(), &output, {}, &csv_output_options())\n}}\n",
                rust_string(&policy.rows_field)
            ));
        }
    }
    let mut files =
        crate::emit_with_companions(program, options, CompanionSelection::TypedOnly)?.into_files();
    for file in &mut files {
        if file.path.as_str() == "src/lib.rs" {
            file.contents.extend_from_slice(adapters.as_bytes());
        }
    }
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(Into::into)
}
