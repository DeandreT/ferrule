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
    let json_output = document.output_filename().is_some();
    let filename = if json_output {
        format!(
            r#"{{ var outputFilename = EvaluateOutputFilename(execution);
        var inputFilename = EvaluatePrimaryFilename(execution);
        foreach (var (node, text) in new (uint, string)[] {{ ({input_node}u, inputFilename), ({output_node}u, outputFilename) }})
            if (text.Length == 0 || text.IndexOfAny(['*', '?']) >= 0 || global::System.Linq.Enumerable.Any(text, char.IsControl))
                throw new FilenameEndpointException(node);
        return inputFilename; }}"#,
            input_node = match document.primary_filename() {
                codegen::PrimaryFilenamePlan::Literal { node, .. }
                | codegen::PrimaryFilenamePlan::Parameter { node, .. } => node,
            },
            output_node = match document.output_filename().expect("output filename") {
                codegen::PrimaryFilenamePlan::Literal { node, .. }
                | codegen::PrimaryFilenamePlan::Parameter { node, .. } => node,
            }
        )
    } else {
        "EvaluatePrimaryFilename(execution)".into()
    };
    let output_setup = if json_output {
        String::new()
    } else {
        format!("var output = new global::Ferrule.Runtime.FerruleEdi({target}, {output}, {kind});")
    };
    let serialize: String = if json_output {
        "SerializeJsonBytesOutputs(ExecuteOutputs(source, execution)).Primary".into()
    } else {
        "output.Serialize(ExecuteOutputs(source, execution).Primary, execution.CurrentDateTime)"
            .into()
    };
    let endpoint_error = if json_output {
        r#"public sealed class FilenameEndpointException(uint node) : global::System.Exception($"filename endpoint node {node} requires nonempty text without wildcards or control characters")
    {
        public uint Node { get; } = node;
    }"#
    } else {
        ""
    };
    let code = format!(
        r#"
    public static readonly global::Ferrule.Runtime.FerrulePrimaryFileSelection PrimaryFileSelection = {selection};
    public const global::Ferrule.Runtime.FerruleDocumentShape PrimaryDocumentShape = {shape};
    {endpoint_error}
    public static byte[] ExecutePrimaryEdiDocumentBytes(global::Ferrule.Runtime.FerruleExecutionContext execution,
        global::Ferrule.Runtime.IFerrulePrimarySourceHost host, int maximum)
    {{
        global::System.ArgumentNullException.ThrowIfNull(execution);
        global::System.ArgumentNullException.ThrowIfNull(host);
        var input = new global::Ferrule.Runtime.FerruleEdi({source}, {input}, {kind}, input: true);
        {output_setup}
        var budget = new global::Ferrule.Runtime.FerruleEdiInputBudget();
        var source = global::Ferrule.Runtime.FerrulePrimaryDocuments.AcquireDocuments(() => {filename},
            PrimaryFileSelection, host, PrimaryDocumentShape, maximum, data => {{ budget.Charge(data.Bytes.Length); return input.Parse(data.Bytes); }});
        return {serialize};
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
