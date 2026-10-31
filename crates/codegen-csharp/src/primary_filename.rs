use codegen::{ArtifactSet, DocumentProgram, PrimaryFilenamePlan};

use crate::composition::Library;
use crate::{EmitError, literal};

/// Add source-free filename evaluation to the ordinary instance library.
/// This does not load files or expose a document-execution entry point.
pub fn emit_with_primary_filename(document: &DocumentProgram) -> Result<ArtifactSet, EmitError> {
    library(document)?.into_artifacts()
}

/// The ordinary library with the filename evaluator declared in a further
/// partial declaration of the mapping source.
pub(crate) fn library(document: &DocumentProgram) -> Result<Library, EmitError> {
    let mut library = crate::library(document.program())?;
    add_evaluator(&mut library, document);
    Ok(library)
}

/// Declare `EvaluatePrimaryFilename` in a further partial mapping declaration.
pub(crate) fn add_evaluator(library: &mut Library, document: &DocumentProgram) {
    add_filename_evaluator(
        library,
        document.primary_filename(),
        "EvaluatePrimaryFilename",
    );
    if let Some(filename) = document.output_filename() {
        add_filename_evaluator(library, filename, "EvaluateOutputFilename");
    }
}

fn add_filename_evaluator(library: &mut Library, filename: &PrimaryFilenamePlan, name: &str) {
    let expression = match filename {
        PrimaryFilenamePlan::Literal { text, .. } => literal::string(text),
        PrimaryFilenamePlan::Parameter { node, name } => format!(
            "execution.ResolvePrimaryFilename({node}u, {})",
            literal::string(name)
        ),
    };
    library.extend_mapping(format!(
        "    /// <summary>Resolve filename text only; performs no path interpretation, loading or mapping execution.</summary>\n    public static string {name}(global::Ferrule.Runtime.FerruleExecutionContext execution)\n    {{\n        global::System.ArgumentNullException.ThrowIfNull(execution);\n        return {expression};\n    }}\n"
    ));
}
