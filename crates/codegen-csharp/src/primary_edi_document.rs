use crate::composition::ClassPart;
use crate::{Options, PrimaryDocumentEmitError};
use codegen::{ArtifactSet, DocumentProgram, GenerationProfile};

pub fn emit_with_primary_edi_documents(
    document: &DocumentProgram,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    emit(document, &Options::default())
}

pub(crate) fn emit(
    document: &DocumentProgram,
    options: &Options,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    let policy = codegen::validate_primary_edi_documents(document)
        .map_err(PrimaryDocumentEmitError::Boundary)?;
    let mut library = crate::library(document.program()).map_err(PrimaryDocumentEmitError::Emit)?;
    crate::primary_filename::add_evaluator(&mut library, document);
    let source =
        crate::literal::string(&serde_json::to_string(&document.program().source).expect("schema"));
    let target =
        crate::literal::string(&serde_json::to_string(&document.program().target).expect("schema"));
    let input = crate::literal::string(&serde_json::to_string(&policy.source).expect("options"));
    let output = crate::literal::string(&serde_json::to_string(&policy.target).expect("options"));
    let kind = crate::literal::string(&format!("{:?}", policy.kind));
    let code = format!(
        r#"
    public static readonly global::Ferrule.Runtime.FerrulePrimaryFileSelection PrimaryFileSelection = {selection};
    public const global::Ferrule.Runtime.FerruleDocumentShape PrimaryDocumentShape = {shape};
    public static byte[] ExecutePrimaryEdiDocumentBytes(global::Ferrule.Runtime.FerruleExecutionContext execution,
        global::Ferrule.Runtime.IFerrulePrimarySourceHost host, int maximum)
    {{
        global::System.ArgumentNullException.ThrowIfNull(execution);
        global::System.ArgumentNullException.ThrowIfNull(host);
        var input = new global::Ferrule.Runtime.FerruleEdi({source}, {input}, {kind}, input: true);
        var output = new global::Ferrule.Runtime.FerruleEdi({target}, {output}, {kind});
        var budget = new global::Ferrule.Runtime.FerruleEdiInputBudget();
        var source = global::Ferrule.Runtime.FerrulePrimaryDocuments.AcquireDocuments(() => EvaluatePrimaryFilename(execution),
            PrimaryFileSelection, host, PrimaryDocumentShape, maximum, data => {{ budget.Charge(data.Bytes.Length); return input.Parse(data.Bytes); }});
        return output.Serialize(ExecuteOutputs(source, execution).Primary, execution.CurrentDateTime);
    }}
"#,
        selection = crate::primary_csv_document::selection_literal(document),
        shape = crate::primary_xml_document::shape_literal(
            document.primary_route().expect("validated route").shape
        )
    );
    library.add_companion(
        "GeneratedMapping.PrimaryEdiDocument.cs",
        ClassPart::members(code),
    );
    library.add_runtime(&crate::primary_xml_document::PRIMARY_DOCUMENT_CSV_RUNTIME);
    library.add_runtime(&crate::primary_csv_document::PRIMARY_DOCUMENT_RUNTIME);
    crate::edi_runtime::add_runtime(&mut library);
    library
        .into_artifacts_with(options, GenerationProfile::PrimaryEdiDocument)
        .map_err(PrimaryDocumentEmitError::Emit)
}
