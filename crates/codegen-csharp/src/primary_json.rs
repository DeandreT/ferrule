use codegen::{ArtifactSet, DocumentProgram, JsonProjectionBoundaryError};
use mapping::JsonSourceShape;

use crate::{EmitError, literal};

#[derive(Debug)]
pub enum PrimaryJsonEmitError {
    Boundary(JsonProjectionBoundaryError),
    Emit(EmitError),
    Writer(codegen::MapForceX12WriterError),
    Visitor(codegen::DocumentVisitorEmitError<EmitError>),
}
impl std::fmt::Display for PrimaryJsonEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
            Self::Writer(error) => error.fmt(f),
            Self::Visitor(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for PrimaryJsonEmitError {}

pub fn emit_with_mapforce_x12_writer(
    document: &DocumentProgram,
    shape: &JsonSourceShape,
    target_options: &mapping::FormatOptions,
    minimums: &[mapping::MapForceEdiMinimum],
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor =
        codegen::prepare_mapforce_x12_writer(document.program(), target_options, minimums)
            .map_err(PrimaryJsonEmitError::Writer)?;
    emit_with_writer(document, shape, descriptor)
}

/// Emit grouped documents with incremental delivery and an individual X12 writer.
/// Filename clocks precede each body; writer clocks remain separate host calls.
pub fn emit_grouped_with_mapforce_x12_writer(
    document: &DocumentProgram,
    shape: &JsonSourceShape,
    target_options: &mapping::FormatOptions,
    minimums: &[mapping::MapForceEdiMinimum],
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor =
        codegen::prepare_mapforce_x12_document_writer(document, target_options, minimums)
            .map_err(PrimaryJsonEmitError::Writer)?;
    let projection = codegen::prepare_json_source_projection(document, shape)
        .map_err(PrimaryJsonEmitError::Boundary)?;
    let base = crate::emit_with_static_error_document_clocks(document.program())
        .map_err(PrimaryJsonEmitError::Visitor)?;
    let base =
        crate::primary_filename::append(base, document).map_err(PrimaryJsonEmitError::Emit)?;
    let projected = append_projection(base, projection, document.json_grouping_preparation())?;
    append_writer(projected, descriptor)
}

fn emit_with_writer(
    document: &DocumentProgram,
    shape: &JsonSourceShape,
    descriptor: String,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let projected = emit_with_primary_json_projection(document, shape)?;
    append_writer(projected, descriptor)
}

fn append_writer(
    artifacts: ArtifactSet,
    descriptor: String,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let mut files = artifacts.into_files();
    for (path, contents) in crate::x12_output::X12_SOURCES {
        if !files.iter().any(|file| file.path.as_str() == path) {
            files
                .push(crate::file(path, contents.to_string()).map_err(PrimaryJsonEmitError::Emit)?);
        }
    }
    for (path, contents) in [
        (
            "Runtime/FerruleMapForceEdiScalar.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleMapForceEdiScalar.cs"),
        ),
        (
            "Runtime/X12/FerruleX12.MapForceWriter.cs",
            include_str!(
                "../../../runtime/csharp/Ferrule.Runtime/X12/FerruleX12.MapForceWriter.cs"
            ),
        ),
    ] {
        files.push(crate::file(path, contents.to_string()).map_err(PrimaryJsonEmitError::Emit)?);
    }
    let mut code = format!(
        "\npublic static partial class GeneratedMapping\n{{\n    public static byte[] SerializeMapForceX12Document(global::Ferrule.Runtime.FerruleInstance value, global::Ferrule.Runtime.FerruleExecutionContext execution) => global::Ferrule.Runtime.FerruleX12.SerializeMapForceDocumentBytes({0}, value, execution);\n    public static void WriteMapForceX12Document(global::Ferrule.Runtime.FerruleInstance value, global::Ferrule.Runtime.FerruleExecutionContext execution, global::System.IO.Stream output) => global::Ferrule.Runtime.FerruleX12.WriteMapForceDocument({0}, value, execution, output);\n}}\n",
        literal::string(&descriptor)
    );
    code.push_str(&format!("\npublic static partial class GeneratedMapping\n{{\n    public static byte[] SerializeMapForceX12DocumentWithClock(global::Ferrule.Runtime.FerruleInstance value, global::Ferrule.Runtime.FerruleExecutionContext execution, global::System.Func<global::Ferrule.Runtime.FerruleMapForceX12ClockField, string> clock) => global::Ferrule.Runtime.FerruleX12.SerializeMapForceDocumentBytesWithClock({0}, value, execution, clock);\n    public static void WriteMapForceX12DocumentWithClock(global::Ferrule.Runtime.FerruleInstance value, global::Ferrule.Runtime.FerruleExecutionContext execution, global::System.Func<global::Ferrule.Runtime.FerruleMapForceX12ClockField, string> clock, global::System.IO.Stream output) => global::Ferrule.Runtime.FerruleX12.WriteMapForceDocumentWithClock({0}, value, execution, clock, output);\n}}\n",literal::string(&descriptor)));
    files
        .iter_mut()
        .find(|f| f.path.as_str() == "GeneratedMapping.cs")
        .expect("ordinary C# artifact")
        .contents
        .extend_from_slice(code.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryJsonEmitError::Emit)
}

/// Add lazy filename acquisition without selecting a transport or publishing output.
pub fn emit_with_primary_json_projection(
    document: &DocumentProgram,
    shape: &JsonSourceShape,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor = codegen::prepare_json_source_projection(document, shape)
        .map_err(PrimaryJsonEmitError::Boundary)?;
    let base = crate::emit_with_primary_filename(document).map_err(PrimaryJsonEmitError::Emit)?;
    append_projection(base, descriptor, document.json_grouping_preparation())
}

fn append_projection(
    base: ArtifactSet,
    descriptor: String,
    preparation: Option<&mapping::JsonGroupingPreparation>,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let mut files = base.into_files();
    let mut code = format!(
        "\npublic static partial class GeneratedMapping\n{{\n    private static readonly global::Ferrule.Runtime.FerruleJsonSourceProjection PrimaryJsonProjection = global::Ferrule.Runtime.FerruleJsonSourceProjection.Parse({});\n\n    /// <summary>Lazy typed document acquisition. Callbacks own transport, parsing and cleanup.</summary>\n    public static global::System.Collections.Generic.IEnumerable<global::Ferrule.Runtime.FerruleInstance> AcquirePrimaryJson(\n        global::Ferrule.Runtime.FerruleExecutionContext execution,\n        global::System.Func<string, global::System.Collections.Generic.IEnumerable<string>> select,\n        global::System.Func<string, global::Ferrule.Runtime.FerruleJsonSourceDocument> load,\n        global::Ferrule.Runtime.FerruleJsonProjectionLimits limits, int maximum)\n    {{\n        global::System.ArgumentNullException.ThrowIfNull(execution);\n        return PrimaryJsonProjection.Acquire(() => EvaluatePrimaryFilename(execution), select, load, maximum, limits);\n    }}\n}}\n",
        literal::string(&descriptor)
    );
    if let Some(preparation) = preparation {
        let encoded = serde_json::to_string(preparation)
            .map_err(JsonProjectionBoundaryError::Descriptor)
            .map_err(PrimaryJsonEmitError::Boundary)?;
        code = code.replacen(
            "load, maximum, limits);\n",
            "load, maximum, limits, PrimaryJsonGrouping);\n",
            1,
        );
        code.push_str(&format!("\npublic static partial class GeneratedMapping\n{{\n    private static readonly global::Ferrule.Runtime.FerruleJsonGroupingPreparation PrimaryJsonGrouping = global::Ferrule.Runtime.FerruleJsonGroupingPreparation.Parse({});\n    public static global::Ferrule.Runtime.FerruleInstance PreparePrimaryJsonDocument(global::Ferrule.Runtime.FerruleInstance document, global::Ferrule.Runtime.FerruleJsonProjectionLimits? limits = null) => PrimaryJsonGrouping.Prepare(document, limits);\n}}\n",literal::string(&encoded)));
    }
    code.push_str(&format!("\npublic static partial class GeneratedMapping\n{{\n    public static global::Ferrule.Runtime.FerruleJsonSourceDocument ParsePrimaryJsonSource(string identity, byte[] bytes) => global::Ferrule.Runtime.FerruleNativeJsonSource.Parse(identity, bytes, {});\n}}\n",literal::string(&descriptor)));
    code.push_str("\npublic static partial class GeneratedMapping\n{\n    /// <summary>Sample only reached body current-dateTime nodes; source acquisition and filename clocks remain separate.</summary>\n    public static global::Ferrule.Runtime.FerruleInstance ExecutePrimaryJsonWithBodyClock(global::Ferrule.Runtime.FerruleInstance source, global::Ferrule.Runtime.FerruleExecutionContext execution, global::System.Func<string> clock)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(clock);\n        return ExecuteOutputs(global::Ferrule.Runtime.ScopeContext.FromSource(source, execution).WithDocumentBodyClock(clock)).Primary;\n    }\n}\n");
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .expect("ordinary C# artifact")
        .contents
        .extend_from_slice(code.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(PrimaryJsonEmitError::Emit)
}
