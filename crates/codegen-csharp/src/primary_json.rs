use codegen::{ArtifactSet, DocumentProgram, JsonProjectionBoundaryError};
use mapping::JsonSourceShape;

use crate::{EmitError, literal};

#[derive(Debug)]
pub enum PrimaryJsonEmitError {
    Boundary(JsonProjectionBoundaryError),
    Emit(EmitError),
}
impl std::fmt::Display for PrimaryJsonEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for PrimaryJsonEmitError {}

/// Add lazy filename acquisition without selecting a transport or publishing output.
pub fn emit_with_primary_json_projection(
    document: &DocumentProgram,
    shape: &JsonSourceShape,
) -> Result<ArtifactSet, PrimaryJsonEmitError> {
    let descriptor = codegen::prepare_json_source_projection(document, shape)
        .map_err(PrimaryJsonEmitError::Boundary)?;
    let mut files = crate::emit_with_primary_filename(document)
        .map_err(PrimaryJsonEmitError::Emit)?
        .into_files();
    let code = format!(
        "\npublic static partial class GeneratedMapping\n{{\n    private static readonly global::Ferrule.Runtime.FerruleJsonSourceProjection PrimaryJsonProjection = global::Ferrule.Runtime.FerruleJsonSourceProjection.Parse({});\n\n    /// <summary>Lazy typed document acquisition. Callbacks own transport, parsing and cleanup.</summary>\n    public static global::System.Collections.Generic.IEnumerable<global::Ferrule.Runtime.FerruleInstance> AcquirePrimaryJson(\n        global::Ferrule.Runtime.FerruleExecutionContext execution,\n        global::System.Func<string, global::System.Collections.Generic.IEnumerable<string>> select,\n        global::System.Func<string, global::Ferrule.Runtime.FerruleJsonSourceDocument> load,\n        global::Ferrule.Runtime.FerruleJsonProjectionLimits limits, int maximum)\n    {{\n        global::System.ArgumentNullException.ThrowIfNull(execution);\n        return PrimaryJsonProjection.Acquire(() => EvaluatePrimaryFilename(execution), select, load, maximum, limits);\n    }}\n}}\n",
        literal::string(&descriptor)
    );
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
