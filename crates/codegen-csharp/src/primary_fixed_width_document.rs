use crate::composition::ClassPart;
use crate::{Options, PrimaryDocumentEmitError};
use codegen::{ArtifactSet, DocumentProgram, GenerationProfile};

pub fn emit_with_primary_fixed_width_documents(
    document: &DocumentProgram,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    emit(document, &Options::default())
}

pub(crate) fn emit(
    document: &DocumentProgram,
    options: &Options,
) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
    let policy = codegen::validate_primary_fixed_width_documents(document)
        .map_err(PrimaryDocumentEmitError::Boundary)?;
    let mut library = crate::library(document.program()).map_err(PrimaryDocumentEmitError::Emit)?;
    crate::primary_filename::add_evaluator(&mut library, document);
    let layout = |layout: &mapping::FixedWidthLayout| {
        format!(
            "new(new long[] {{ {} }}, {}, {}, {})",
            layout
                .field_widths()
                .iter()
                .map(|w| format!("{}L", w.get()))
                .collect::<Vec<_>>()
                .join(", "),
            u32::from(layout.fill_char()),
            layout.record_delimiters(),
            layout.treat_empty_as_absent()
        )
    };
    let fields = |schema: &ir::SchemaNode, read: bool| {
        let ir::SchemaKind::Group { children, .. } = &schema.kind else {
            unreachable!("validated flat group")
        };
        children.iter().map(|field| {
            let ir::SchemaKind::Scalar { ty } = field.kind else { unreachable!("validated scalar") };
            let ty = match ty { ir::ScalarType::String => "String", ir::ScalarType::Int => "Int64", ir::ScalarType::Float => "Double", ir::ScalarType::Bool => "Bool" };
            format!("new global::Ferrule.Runtime.FerruleCsv{}Field({}, global::Ferrule.Runtime.FerruleScalarType.{ty})", if read {"Read"} else {""}, crate::literal::string(&field.name))
        }).collect::<Vec<_>>().join(",\n")
    };
    let code = format!(
        r#"
    public static readonly global::Ferrule.Runtime.FerrulePrimaryFileSelection PrimaryFileSelection = {selection};
    private static readonly global::Ferrule.Runtime.FerruleFixedWidthLayout FixedWidthInputLayout = {input};
    private static readonly global::Ferrule.Runtime.FerruleFixedWidthLayout FixedWidthOutputLayout = {output};
    private static readonly global::Ferrule.Runtime.FerruleCsvReadField[] FixedWidthInputFields = [{input_fields}];
    private static readonly global::Ferrule.Runtime.FerruleCsvField[] FixedWidthOutputFields = [{output_fields}];
    public static global::Ferrule.Runtime.FerruleRepeated ParseFixedWidthBytes(string path, string? resolved, byte[] bytes) =>
        global::Ferrule.Runtime.FerruleFixedWidth.ParseBytes(bytes, FixedWidthInputFields, FixedWidthInputLayout, path, resolved);
    public static byte[] SerializeFixedWidthBytes(global::Ferrule.Runtime.FerruleInstance source) =>
        global::Ferrule.Runtime.FerruleFixedWidth.SerializeBytes(source, FixedWidthOutputFields, FixedWidthOutputLayout);
    public static byte[] ExecutePrimaryFixedWidthDocumentBytes(global::Ferrule.Runtime.FerruleExecutionContext execution,
        global::Ferrule.Runtime.IFerrulePrimarySourceHost host, int maximum)
    {{
        global::System.ArgumentNullException.ThrowIfNull(execution);
        global::System.ArgumentNullException.ThrowIfNull(host);
        var budget = new global::Ferrule.Runtime.FerruleFixedWidthInputBudget();
        var source = global::Ferrule.Runtime.FerrulePrimaryDocuments.AcquireDocuments(() => EvaluatePrimaryFilename(execution),
            PrimaryFileSelection, host, global::Ferrule.Runtime.FerruleDocumentShape.Rows, maximum,
            (request, data) => {{ budget.Charge(data.Bytes.Length); return ParseFixedWidthBytes(request, data.ResolvedPath, data.Bytes); }});
        return SerializeFixedWidthBytes(ExecuteOutputs(source, execution).Primary);
    }}
"#,
        selection = crate::primary_csv_document::selection_literal(document),
        input = layout(&policy.source),
        output = layout(&policy.target),
        input_fields = fields(&document.program().source, true),
        output_fields = fields(&document.program().target, false)
    );
    library.add_companion(
        "GeneratedMapping.PrimaryFixedWidthDocument.cs",
        ClassPart::members(code),
    );
    library.add_runtime(&crate::primary_xml_document::PRIMARY_DOCUMENT_CSV_RUNTIME);
    library.add_runtime(&crate::primary_csv_document::PRIMARY_DOCUMENT_RUNTIME);
    library.add_runtime(&[
        (
            "Runtime/FerruleCsv.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsv.cs"),
        ),
        (
            "Runtime/FerruleFixedWidth.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFixedWidth.cs"),
        ),
    ]);
    library
        .into_artifacts_with(options, GenerationProfile::PrimaryFixedWidthDocument)
        .map_err(PrimaryDocumentEmitError::Emit)
}
