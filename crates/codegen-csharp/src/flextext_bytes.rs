//! Closed, primary-to-primary FlexText byte profile. The layout, not a path or
//! filename, is authoritative. Unsupported commands never fall back to text.
use crate::{EmitError, composition::ClassPart};
use codegen::{ArtifactSet, Program};
use mapping::{FlexCommand, FormatOptions};

#[derive(Debug)]
pub enum FlexTextEmitError {
    Unsupported,
    Emit(EmitError),
}

impl std::fmt::Display for FlexTextEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("expected primary FlexText string Store layouts"),
            Self::Emit(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for FlexTextEmitError {}

/// Emit the string Store profile: strict UTF-8, optional BOM, LF/CRLF output,
/// no trimming, named boundaries, external resources or inferred layouts.
pub fn emit_with_flextext_bytes(
    program: &Program,
    input: &FormatOptions,
    output: &FormatOptions,
) -> Result<ArtifactSet, FlexTextEmitError> {
    let store = |options: &FormatOptions, schema: &ir::SchemaNode| {
        let layout = options.flextext.as_ref()?;
        let FlexCommand::Store {
            name,
            ty: ir::ScalarType::String,
            trim: None,
        } = layout.command()
        else {
            return None;
        };
        if options
            != &(FormatOptions {
                flextext: Some(layout.clone()),
                ..Default::default()
            })
            || schema != &layout.schema()
            || name.len() > 16384
        {
            return None;
        }
        Some((
            crate::literal::string(name),
            layout.write_bom(),
            crate::literal::string(layout.output_line_ending().as_str()),
        ))
    };
    if !program.extra_sources.is_empty()
        || !program.extra_targets.is_empty()
        || program.root.produces_documents()
    {
        return Err(FlexTextEmitError::Unsupported);
    }
    let (source, _, _) = store(input, &program.source).ok_or(FlexTextEmitError::Unsupported)?;
    let (target, bom, ending) =
        store(output, &program.target).ok_or(FlexTextEmitError::Unsupported)?;
    let mut library = crate::library(program).map_err(FlexTextEmitError::Emit)?;
    library.add_runtime(&[(
        "Runtime/FerruleFlexTextStore.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFlexTextStore.cs"),
    )]);
    let methods = format!(
        r#"
    public static byte[] ExecuteFlexTextBytesOutputsWithHost(byte[] source, global::Ferrule.Runtime.FerruleExecutionContext? context = null)
    {{
        var input = global::Ferrule.Runtime.FerruleFlexTextStore.Read(source, {source});
        var mapped = context is null ? ExecuteOutputs(input) : ExecuteOutputs(input, context);
        return global::Ferrule.Runtime.FerruleFlexTextStore.Write(mapped.Primary, {target}, {bom}, {ending});
    }}

    public static byte[] ExecuteFlexTextBytesSelectedTargetWithHost(byte[] source, global::Ferrule.Runtime.FerruleTargetSelection selection, global::Ferrule.Runtime.FerruleExecutionContext? context = null)
    {{
        var result = (SelectedTargetOutput.Primary)ExecuteSelectedTargetWithHost(global::Ferrule.Runtime.FerruleFlexTextStore.Read(source, {source}), selection, [], context);
        return global::Ferrule.Runtime.FerruleFlexTextStore.Write(result.Instance, {target}, {bom}, {ending});
    }}
"#
    );
    library.add_companion(
        "GeneratedMapping.FlexTextBytes.cs",
        ClassPart::members(methods),
    );
    library.into_artifacts().map_err(FlexTextEmitError::Emit)
}
