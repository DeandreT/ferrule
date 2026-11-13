//! Explicit primary CSV acquisition followed by strict mapped-document output.

use std::fmt;

use codegen::{
    ArtifactSet, DocumentProgram, GenerationProfile, PrimaryMappedCsvDocumentBoundaryError,
    PrimaryMappedCsvDocumentPolicy,
};
use ir::{ScalarType, SchemaKind};

use crate::composition::ClassPart;
use crate::mapped_csv_document_output::MappedCsvDocumentEmitError;
use crate::{EmitError, Options, literal};

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

/// Acquire a directly named CSV document with a shared execution context, then
/// map once and return the complete strict mapped CSV document list.
pub fn emit_with_primary_mapped_csv_documents(
    document: &DocumentProgram,
    policy: &PrimaryMappedCsvDocumentPolicy,
) -> Result<ArtifactSet, PrimaryMappedCsvDocumentEmitError> {
    emit(document, policy, &Options::default())
}

pub(crate) fn emit(
    document: &DocumentProgram,
    policy: &PrimaryMappedCsvDocumentPolicy,
    options: &Options,
) -> Result<ArtifactSet, PrimaryMappedCsvDocumentEmitError> {
    codegen::validate_primary_mapped_csv_documents(document, policy)
        .map_err(PrimaryMappedCsvDocumentEmitError::Boundary)?;
    let mut library =
        crate::mapped_csv_document_output::library_validated(document.program(), &policy.output)?;
    crate::primary_filename::add_evaluator(&mut library, document);
    library.add_companion(
        "GeneratedMapping.PrimaryMappedCsvDocuments.cs",
        render(document, policy),
    );
    library.add_runtime(&[
        (
            "Runtime/FerruleCsvInput.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvInput.cs"),
        ),
        (
            "Runtime/FerruleCsvSource.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvSource.cs"),
        ),
        (
            "Runtime/FerruleCsvAcquisition.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvAcquisition.cs"),
        ),
        (
            "Runtime/FerrulePrimaryDocuments.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerrulePrimaryDocuments.cs"),
        ),
    ]);
    library
        .into_artifacts_with(options, GenerationProfile::PrimaryMappedCsvDocument)
        .map_err(Into::into)
}

fn render(document: &DocumentProgram, policy: &PrimaryMappedCsvDocumentPolicy) -> ClassPart {
    let SchemaKind::Group { children, .. } = &document.program().source.kind else {
        unreachable!("admitted primary CSV source")
    };
    let mut members = String::from(
        "    private static readonly global::Ferrule.Runtime.FerruleCsvReadField[] CsvInputFields =\n    [\n",
    );
    for field in children {
        let SchemaKind::Scalar { ty } = field.kind else {
            unreachable!("admitted primary CSV field")
        };
        let ty = match ty {
            ScalarType::String => "String",
            ScalarType::Int => "Int64",
            ScalarType::Float => "Double",
            ScalarType::Bool => "Bool",
        };
        members.push_str(&format!(
            "        new({}, global::Ferrule.Runtime.FerruleScalarType.{ty}),\n",
            literal::string(&field.name)
        ));
    }
    let input = &policy.input;
    members.push_str(&format!(
        "    ];\n    private static readonly global::Ferrule.Runtime.FerruleCsvReadOptions CsvInputOptions = new()\n    {{\n        Delimiter = {},\n        Quote = {},\n        QuoteDisabled = {},\n        HasHeaders = {},\n        PreserveEmptyStrings = {},\n    }};\n\n",
        character(input.delimiter),
        character(input.quote),
        input.quote_disabled,
        input.has_headers,
        input.preserve_empty_strings,
    ));
    members.push_str(
        "    /// <summary>The admitted plan selects exactly one directly named document.</summary>\n    public static readonly global::Ferrule.Runtime.FerrulePrimaryFileSelection PrimaryFileSelection = global::Ferrule.Runtime.FerrulePrimaryFileSelection.Direct;\n",
    );
    for (suffix, kind) in [("", "Text"), ("Bytes", "Bytes")] {
        members.push_str(&format!(
            r#"
    /// <summary>Acquire and parse all input rows before one contextual mapping execution; return the complete CSV document list.</summary>
    public static global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleCsv{kind}Document> ExecutePrimaryMappedCsvDocuments{suffix}(
        global::Ferrule.Runtime.FerruleExecutionContext execution,
        global::Ferrule.Runtime.IFerrulePrimarySourceHost host,
        global::Ferrule.Runtime.FerruleCsvByteDecodingPolicy decoding, int maximum)
    {{
        global::System.ArgumentNullException.ThrowIfNull(execution);
        global::System.ArgumentNullException.ThrowIfNull(host);
        var input = global::Ferrule.Runtime.FerrulePrimaryDocuments.AcquireDocuments(
            () => EvaluatePrimaryFilename(execution), PrimaryFileSelection, host,
            global::Ferrule.Runtime.FerruleDocumentShape.Rows, maximum,
            (request, data) => new global::Ferrule.Runtime.FerruleRepeated(
                global::System.Linq.Enumerable.Select(
                    global::Ferrule.Runtime.FerruleCsvSourceDocument.ParseBytes(
                        request, data.ResolvedPath, data.Bytes, CsvInputFields, decoding, CsvInputOptions).Rows(),
                    row => row.Value)));
        return ExecuteMappedCsvDocuments{suffix}(input, execution);
    }}
"#,
        ));
    }
    ClassPart::members(members)
}

fn character(value: Option<char>) -> String {
    value.map_or_else(
        || "null".into(),
        |value| format!("(char){}", u32::from(value)),
    )
}
