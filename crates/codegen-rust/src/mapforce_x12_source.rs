use crate::{EmitError, Options};
use codegen::{ArtifactSet, MapForceX12SourceError, Program};
use ir::SchemaNode;
use mapping::FormatOptions;

#[derive(Debug)]
pub enum MapForceSourceEmitError {
    Boundary(MapForceX12SourceError),
    Writer(codegen::MapForceJsonWriterError),
    Emit(EmitError),
    Artifact,
}
impl std::fmt::Display for MapForceSourceEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Writer(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
            Self::Artifact => f.write_str("missing ordinary Rust mapping artifact"),
        }
    }
}

/// Bind native X12 source projection and strict JSON output; acquisition,
/// filename evaluation, clock phases and publication remain the host's work.
pub fn emit_with_mapforce_x12_json(
    program: &Program,
    options: &Options,
    schema: &SchemaNode,
    source_options: &FormatOptions,
    message: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let descriptor =
        codegen::prepare_mapforce_json_writer(program).map_err(MapForceSourceEmitError::Writer)?;
    let mut files =
        emit_with_mapforce_x12_source(program, options, schema, source_options, message)?
            .into_files();
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    mapping.contents.extend_from_slice(format!("\npub fn serialize_mapforce_json(value: &codegen_runtime::Instance) -> Result<String, codegen_runtime::MapForceJson5Error> {{\n    codegen_runtime::serialize_mapforce_json_ascii_keys({descriptor:?}, value)\n}}\n").as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}
impl std::error::Error for MapForceSourceEmitError {}

/// Add a bound source reader; hosts still own file acquisition, message iteration
/// and all filename/body/publication phases.
pub fn emit_with_mapforce_x12_source(
    program: &Program,
    options: &Options,
    schema: &SchemaNode,
    source_options: &FormatOptions,
    message: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let descriptor = codegen::prepare_mapforce_x12_source(program, schema, source_options, message)
        .map_err(MapForceSourceEmitError::Boundary)?;
    let mut files = crate::emit(program, options)
        .map_err(MapForceSourceEmitError::Emit)?
        .into_files();
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    mapping.contents.extend_from_slice(format!("\n/// Parse the complete physical X12 source, not a single-message body input.\npub fn parse_primary_x12_source(bytes: &[u8]) -> Result<codegen_runtime::Instance, codegen_runtime::X12BoundaryError> {{\n    codegen_runtime::parse_mapforce_x12_source({descriptor:?}, bytes)\n}}\n").as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}
