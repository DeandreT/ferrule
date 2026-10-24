use crate::composition::ClassPart;
use crate::primary_csv_document::{PRIMARY_DOCUMENT_RUNTIME, selection_literal};
use crate::primary_xml_document::{PRIMARY_DOCUMENT_CSV_RUNTIME, shape_literal};
use crate::{Options, PrimaryDocumentEmitError};
use codegen::{ArtifactSet, DocumentProgram, GenerationProfile, PrimaryDocumentBoundaryError};

/// Emits a strict-JSON primary filename plan with one shared mapping context.
pub fn emit_with_primary_json_documents(
    document: &DocumentProgram,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    emit(document, &Options::default())
}

pub(crate) fn emit(
    document: &DocumentProgram,
    options: &Options,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    let boundary = PrimaryDocumentEmitError::Boundary;
    codegen::validate_primary_json_documents(document).map_err(boundary)?;
    let shape = document
        .primary_route()
        .map_err(|error| boundary(PrimaryDocumentBoundaryError::Route(error)))?
        .shape;
    let mut library = crate::library(document.program()).map_err(PrimaryDocumentEmitError::Emit)?;
    crate::primary_filename::add_evaluator(&mut library, document);
    let code = format!(
        r#"
    public static readonly global::Ferrule.Runtime.FerrulePrimaryFileSelection PrimaryFileSelection = {selection};
    public const global::Ferrule.Runtime.FerruleDocumentShape PrimaryDocumentShape = {shape};

    /// <summary>Parse all strict JSON documents before one mapping execution.</summary>
    public static byte[] ExecutePrimaryJsonDocumentBytes(global::Ferrule.Runtime.FerruleExecutionContext execution,
        global::Ferrule.Runtime.IFerrulePrimarySourceHost host, int maximum)
    {{
        global::System.ArgumentNullException.ThrowIfNull(execution);
        global::System.ArgumentNullException.ThrowIfNull(host);
        var budget = new global::Ferrule.Runtime.FerrulePrimaryJsonInputBudget();
        var source = global::Ferrule.Runtime.FerrulePrimaryDocuments.AcquireDocuments(() => EvaluatePrimaryFilename(execution),
            PrimaryFileSelection, host, PrimaryDocumentShape, maximum, data => {{
                budget.Charge(data.Bytes.Length);
                return global::Ferrule.Runtime.FerruleJson.ParseEmbeddedBytes(SourceJsonSchema, data.Bytes);
            }});
        return SerializeJsonBytesOutputs(ExecuteOutputs(source, execution)).Primary;
    }}
"#,
        selection = selection_literal(document),
        shape = shape_literal(shape)
    );
    library.add_companion(
        "GeneratedMapping.PrimaryJsonDocument.cs",
        ClassPart::members(code),
    );
    library.add_runtime(&PRIMARY_DOCUMENT_CSV_RUNTIME);
    library.add_runtime(&PRIMARY_DOCUMENT_RUNTIME);
    library.add_runtime(&[(
        "Runtime/FerrulePrimaryJsonDocuments.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerrulePrimaryJsonDocuments.cs"),
    )]);
    library
        .into_artifacts_with(options, GenerationProfile::PrimaryJsonDocument)
        .map_err(PrimaryDocumentEmitError::Emit)
}
