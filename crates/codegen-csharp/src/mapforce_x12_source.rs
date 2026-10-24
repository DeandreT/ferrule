use crate::EmitError;
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
            Self::Artifact => f.write_str("missing ordinary C# mapping artifact"),
        }
    }
}

/// Bind native X12 source projection and strict JSON output without choosing
/// filename timing or physical input/output operations.
pub fn emit_with_mapforce_x12_json(
    program: &Program,
    schema: &SchemaNode,
    options: &FormatOptions,
    message: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let descriptor =
        codegen::prepare_mapforce_json_writer(program).map_err(MapForceSourceEmitError::Writer)?;
    let mut files = emit_with_mapforce_x12_source(program, schema, options, message)?.into_files();
    for (path, text) in crate::runtime::JSON5_SOURCES {
        if !files.iter().any(|file| file.path.as_str() == path) {
            files.push(crate::file(path, text).map_err(MapForceSourceEmitError::Emit)?);
        }
    }
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    mapping.contents.extend_from_slice(format!("\npublic static partial class GeneratedMapping\n{{\n    public static string SerializeMapForceJson(global::Ferrule.Runtime.FerruleInstance value) => global::Ferrule.Runtime.FerruleJson.SerializeMapForceJsonAsciiKeys({}, value);\n}}\n",crate::literal::string(&descriptor)).as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}
impl std::error::Error for MapForceSourceEmitError {}

/// Compose the original source parameter, lazy acquisition and eager JSON body
/// preparation. Transport and input cleanup remain host-owned.
pub fn emit_with_primary_eager_x12_json(
    document: &codegen::DocumentProgram,
    schema: &SchemaNode,
    options: &FormatOptions,
    message: &[String],
    filename_target: &str,
    driver: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let descriptor =
        codegen::prepare_mapforce_x12_source(document.program(), schema, options, message)
            .map_err(MapForceSourceEmitError::Boundary)?;
    let artifacts = emit_with_eager_x12_json_message(
        document.program(),
        schema,
        options,
        message,
        filename_target,
        driver,
    )?;
    let mut files = crate::primary_filename::append(artifacts, document)
        .map_err(MapForceSourceEmitError::Emit)?
        .into_files();
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    let descriptor = crate::literal::string(&descriptor);
    mapping.contents.extend_from_slice(format!(r#"
public static partial class GeneratedMapping
{{
    public static global::Ferrule.Runtime.FerruleParsedX12Source ParsePrimaryX12Document(string identity, byte[] bytes) => global::Ferrule.Runtime.FerruleX12.ParseMapForceSourceDocument({descriptor}, identity, bytes);
    /// <summary>Loaders finish parsing and input cleanup before returning a document.</summary>
    public static global::System.Collections.Generic.IEnumerable<global::Ferrule.Runtime.FerruleDocument> AcquirePrimaryX12(
        global::Ferrule.Runtime.FerruleExecutionContext execution,
        global::System.Func<string, global::System.Collections.Generic.IEnumerable<string>> select,
        global::System.Func<string, global::Ferrule.Runtime.FerruleParsedX12Source> load,
        int maximum)
    {{
        global::System.ArgumentNullException.ThrowIfNull(execution);
        return global::Ferrule.Runtime.FerruleX12SourceAcquisition.Acquire({descriptor}, () => EvaluatePrimaryFilename(execution), select, load, maximum);
    }}
}}
"#).as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}

/// Prepare an eager JSON message before evaluating its filename. Acquisition,
/// serialization and physical output remain separate host operations.
pub fn emit_with_eager_x12_json_message(
    program: &Program,
    schema: &SchemaNode,
    options: &FormatOptions,
    message: &[String],
    filename_target: &str,
    driver: &[String],
) -> Result<ArtifactSet, MapForceSourceEmitError> {
    let plan = codegen::prepare_eager_x12_json_message(program, filename_target, driver)
        .map_err(MapForceSourceEmitError::Writer)?;
    let mut files = emit_with_mapforce_x12_json(program, schema, options, message)?.into_files();
    let entry = crate::mapping::eager_json_message_entry(program, &plan);
    let mapping = files
        .iter_mut()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .ok_or(MapForceSourceEmitError::Artifact)?;
    mapping.contents.extend_from_slice(entry.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceSourceEmitError::Emit)
}

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
