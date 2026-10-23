use crate::EmitError;
use codegen::{ArtifactSet, MapForceX12SourceError, Program};
use ir::SchemaNode;
use mapping::FormatOptions;

#[derive(Debug)]
pub enum MapForceSourceEmitError {
    Boundary(MapForceX12SourceError),
    Emit(EmitError),
    Artifact,
}
impl std::fmt::Display for MapForceSourceEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
            Self::Artifact => f.write_str("missing ordinary C# mapping artifact"),
        }
    }
}
impl std::error::Error for MapForceSourceEmitError {}

/// Add a bound, byte-oriented source reader without choosing acquisition,
/// document iteration, filename phases or output publication.
pub fn emit_with_mapforce_x12_source(
    program: &Program,
    schema: &SchemaNode,
    options: &FormatOptions,
    message: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let descriptor = codegen::prepare_mapforce_x12_source(program, schema, options, message)
        .map_err(MapForceSourceEmitError::Boundary)?;
    let mut files = crate::emit(program)
        .map_err(MapForceSourceEmitError::Emit)?
        .into_files();
    for (path, text) in crate::x12_output::X12_SOURCES {
        if !files.iter().any(|file| file.path.as_str() == path) {
            files.push(crate::file(path, text).map_err(MapForceSourceEmitError::Emit)?);
        }
    }
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    let text =
        std::str::from_utf8(&mapping.contents).map_err(|_| MapForceSourceEmitError::Artifact)?;
    let mut text = text.replacen(
        "public static class GeneratedMapping",
        "public static partial class GeneratedMapping",
        1,
    );
    text.push_str(&format!("\npublic static partial class GeneratedMapping\n{{\n    /// <summary>Parse the complete physical X12 source into its lexical value view, not a body input.</summary>\n    public static global::Ferrule.Runtime.FerruleInstance ParsePrimaryX12Source(byte[] bytes) => global::Ferrule.Runtime.FerruleX12.ParseMapForceSourceBytes({}, bytes);\n}}\n", crate::literal::string(&descriptor)));
    mapping.contents = text.into_bytes();
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}
