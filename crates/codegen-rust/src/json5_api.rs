use codegen::{ArtifactSet, Json5BoundaryProfile, Program, prepare_json5_boundary};

use crate::{Options, emit, rust_string};

mod error;
pub use error::Json5EmitError;

/// Append four explicitly selected singular closed-object JSON5 companions.
/// Ordinary emit, its artifact bytes, manifest, signatures and errors are intact.
pub fn emit_with_json5(
    program: &Program,
    options: &Options,
) -> Result<ArtifactSet, Json5EmitError> {
    let profile = prepare_json5_boundary(program).map_err(Json5EmitError::Policy)?;
    let mut files = emit(program, options)
        .map_err(Json5EmitError::Ordinary)?
        .into_files();
    let Some(library) = files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
    else {
        return Err(Json5EmitError::MissingLibrary);
    };
    library
        .contents
        .extend_from_slice(render(&profile).as_bytes());
    ArtifactSet::new(files).map_err(Json5EmitError::ArtifactSet)
}

fn render(profile: &Json5BoundaryProfile) -> String {
    let mut source = format!(
        "\nconst JSON5_SOURCE_SCHEMA: &str = {};\nconst JSON5_TARGET_SCHEMA: &str = {};\n\n",
        rust_string(&profile.source_descriptor),
        rust_string(&profile.target_descriptor)
    );
    source.push_str(ADAPTERS);
    source
}

const ADAPTERS: &str = r#"/// Normalize one complete JSON5 object, map it, and return bounded strict JSON.
pub fn execute_json5(source: &str) -> Result<String, codegen_runtime::Json5BoundaryError> {
    codegen_runtime::map_json5_with(JSON5_SOURCE_SCHEMA, JSON5_TARGET_SCHEMA, source, execute)
}

/// Use the caller's fixed execution context with the same complete boundary.
pub fn execute_json5_with_context(
    source: &str,
    execution: &ExecutionContext<'_>,
) -> Result<String, codegen_runtime::Json5BoundaryError> {
    codegen_runtime::map_json5_with(JSON5_SOURCE_SCHEMA, JSON5_TARGET_SCHEMA, source, |input| execute_with_context(input, execution))
}

/// Validate original UTF-8 bytes and return complete owned strict JSON bytes.
pub fn execute_json5_bytes(source: &[u8]) -> Result<Vec<u8>, codegen_runtime::Json5BoundaryError> {
    codegen_runtime::map_json5_bytes_with(JSON5_SOURCE_SCHEMA, JSON5_TARGET_SCHEMA, source, execute)
}

/// Apply the same UTF-8 boundary with the caller's fixed execution context.
pub fn execute_json5_bytes_with_context(
    source: &[u8],
    execution: &ExecutionContext<'_>,
) -> Result<Vec<u8>, codegen_runtime::Json5BoundaryError> {
    codegen_runtime::map_json5_bytes_with(JSON5_SOURCE_SCHEMA, JSON5_TARGET_SCHEMA, source, |input| execute_with_context(input, execution))
}
"#;
