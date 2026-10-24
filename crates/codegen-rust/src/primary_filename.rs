use codegen::{ArtifactSet, DocumentProgram, PrimaryFilenamePlan};
use mapping::{EmptyFileSelection, PrimaryFileSelection};

use crate::{EmitError, Options, rust_string};

/// Add source-free filename evaluation to the ordinary instance library.
/// This does not load files or expose a document-execution entry point.
pub fn emit_with_primary_filename(
    document: &DocumentProgram,
    options: &Options,
) -> Result<ArtifactSet, EmitError> {
    let mut files = crate::emit(document.program(), options)?.into_files();
    files
        .iter_mut()
        .find(|f| f.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust mapping artifact")
        .contents
        .extend_from_slice(evaluator(document).as_bytes());
    ArtifactSet::new(files).map_err(EmitError::from)
}

/// The `evaluate_primary_filename` item appended to a generated library.
pub(crate) fn evaluator(document: &DocumentProgram) -> String {
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
    format!(
        "\n/// Resolve filename text only. No path interpretation, source loading or mapping execution.\npub fn evaluate_primary_filename({parameter}: &ExecutionContext<'_>) -> Result<String, codegen_runtime::RuntimeError> {{\n    {expression}\n}}\n"
    )
}

/// The `PRIMARY_FILE_SELECTION` item: hosts receive the plan's selection mode
/// rather than choosing one.
pub(crate) fn selection_constant(document: &DocumentProgram) -> String {
    let selection = match document.primary_selection() {
        PrimaryFileSelection::Direct => "Direct".to_string(),
        PrimaryFileSelection::FileNameWildcard { empty } => format!(
            "FileNameWildcard {{ empty: codegen_runtime::EmptyFileSelection::{} }}",
            match empty {
                EmptyFileSelection::Refuse => "Refuse",
                EmptyFileSelection::MapNoDocuments => "MapNoDocuments",
            }
        ),
    };
    format!(
        "\n/// The plan's selection mode; hosts receive it rather than choosing one.\npub const PRIMARY_FILE_SELECTION: codegen_runtime::PrimaryFileSelection = codegen_runtime::PrimaryFileSelection::{selection};\n"
    )
}
