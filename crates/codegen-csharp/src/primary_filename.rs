use codegen::{ArtifactSet, DocumentProgram, PrimaryFilenamePlan};

use crate::{EmitError, literal};

/// Add source-free filename evaluation to the ordinary instance library.
/// This does not load files or expose a document-execution entry point.
pub fn emit_with_primary_filename(document: &DocumentProgram) -> Result<ArtifactSet, EmitError> {
    let mut files = crate::emit(document.program())?.into_files();
    let expression = match document.primary_filename() {
        PrimaryFilenamePlan::Literal { text, .. } => literal::string(text),
        PrimaryFilenamePlan::Parameter { node, name } => format!(
            "execution.ResolvePrimaryFilename({node}u, {})",
            literal::string(name)
        ),
    };
    let source = format!(
        "\npublic static partial class GeneratedMapping\n{{\n    /// <summary>Resolve filename text only; performs no path interpretation, loading or mapping execution.</summary>\n    public static string EvaluatePrimaryFilename(global::Ferrule.Runtime.FerruleExecutionContext execution)\n    {{\n        global::System.ArgumentNullException.ThrowIfNull(execution);\n        return {expression};\n    }}\n}}\n"
    );
    let artifact = files
        .iter_mut()
        .find(|f| f.path.as_str() == "GeneratedMapping.cs")
        .expect("ordinary C# mapping artifact");
    let original =
        std::str::from_utf8(&artifact.contents).expect("ordinary generated mapping is UTF-8");
    artifact.contents = original
        .replacen(
            "public static class GeneratedMapping\n{",
            "public static partial class GeneratedMapping\n{",
            1,
        )
        .into_bytes();
    artifact.contents.extend_from_slice(source.as_bytes());
    ArtifactSet::new(files).map_err(EmitError::from)
}
