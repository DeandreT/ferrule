use std::fmt;

use codegen::{
    ArtifactSet, CompanionSelection, CsvMappedDocumentOutputError, CsvMappedDocumentOutputPolicy,
    GenerationProfile, Program,
};

use crate::{EmitError, Options, composition::ClassPart, literal};

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

/// Emit only typed inputs and the explicit constructed-row CSV document family.
pub fn emit_with_mapped_csv_document_output(
    program: &Program,
    policy: &CsvMappedDocumentOutputPolicy,
) -> Result<ArtifactSet, MappedCsvDocumentEmitError> {
    emit(program, policy, &Options::default())
}

pub(crate) fn emit(
    program: &Program,
    policy: &CsvMappedDocumentOutputPolicy,
    options: &Options,
) -> Result<ArtifactSet, MappedCsvDocumentEmitError> {
    codegen::validate_csv_mapped_document_output(program, policy)
        .map_err(MappedCsvDocumentEmitError::Boundary)?;
    library_validated(program, policy)?
        .into_artifacts_with(options, GenerationProfile::MappedCsvDocumentOutput)
        .map_err(Into::into)
}

/// Build only after the caller has admitted the mapped-output boundary.
pub(crate) fn library_validated(
    program: &Program,
    policy: &CsvMappedDocumentOutputPolicy,
) -> Result<crate::composition::Library, MappedCsvDocumentEmitError> {
    let row = policy
        .row_schema(&program.target)
        .map_err(MappedCsvDocumentEmitError::Boundary)?;
    let mut library = crate::library_with_companions(program, CompanionSelection::TypedOnly)?;
    let mut members = crate::csv_output::render_row_settings(&row, &policy.target);
    for (suffix, kind, serializer) in [
        ("", "Text", "SerializeMapped"),
        ("Bytes", "Bytes", "SerializeMappedBytes"),
    ] {
        for context in [false, true] {
            let argument = if context {
                ", global::Ferrule.Runtime.FerruleExecutionContext executionContext"
            } else {
                ""
            };
            let call = if context { ", executionContext" } else { "" };
            members.push_str(&format!(
                "    public static global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleCsv{kind}Document> ExecuteMappedCsvDocuments{suffix}(global::Ferrule.Runtime.FerruleInstance source{argument})\n    {{\n        var output = Execute(source{call});\n        return global::Ferrule.Runtime.FerruleCsvDocuments.{serializer}(output, {}, CsvOutputFields, CsvOutputPolicy);\n    }}\n\n",
                literal::string(&policy.rows_field)
            ));
        }
    }
    library.add_companion(
        "GeneratedMapping.MappedCsvDocuments.cs",
        ClassPart::members(members),
    );
    library.add_runtime(&[
        (
            "Runtime/FerruleCsv.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsv.cs"),
        ),
        (
            "Runtime/FerruleCsvDocuments.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvDocuments.cs"),
        ),
    ]);
    Ok(library)
}
