use codegen::{ArtifactSet, DocumentProgram, PrimaryFilenamePlan};

use crate::{EmitError, Options, rust_string};

/// Add source-free filename evaluation to the ordinary instance library.
/// This does not load files or expose a document-execution entry point.
pub fn emit_with_primary_filename(
    document: &DocumentProgram,
    options: &Options,
) -> Result<ArtifactSet, EmitError> {
    append(crate::emit(document.program(), options)?, document)
}

pub(crate) fn append(
    artifacts: ArtifactSet,
    document: &DocumentProgram,
) -> Result<ArtifactSet, EmitError> {
    let mut files = artifacts.into_files();
    let expression = match document.primary_filename() {
        PrimaryFilenamePlan::Literal { text, .. } => {
            format!("Ok({}.to_string())", rust_string(text))
        }
        PrimaryFilenamePlan::Parameter { node, name } => format!(
            "execution.resolve_primary_filename({node}, {})",
            rust_string(name)
        ),
    };
    let parameter = if matches!(
        document.primary_filename(),
        PrimaryFilenamePlan::Literal { .. }
    ) {
        "_execution"
    } else {
        "execution"
    };
    let source = format!(
        "\n/// Resolve filename text only. No path interpretation, source loading or mapping execution.\npub fn evaluate_primary_filename({parameter}: &ExecutionContext<'_>) -> Result<String, codegen_runtime::RuntimeError> {{\n    {expression}\n}}\n"
    );
    files
        .iter_mut()
        .find(|f| f.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust mapping artifact")
        .contents
        .extend_from_slice(source.as_bytes());
    ArtifactSet::new(files).map_err(EmitError::from)
}
