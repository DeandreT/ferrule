use codegen::{ArtifactSet, CsvInputPolicy, DocumentProgram};

use crate::{EmitError, file, literal, runtime};

#[derive(Debug)]
pub enum MapForceJson5DocumentEmitError {
    Boundary(codegen::MapForceJson5DocumentError),
    Visitor(codegen::DocumentVisitorEmitError<EmitError>),
    Acquisition(crate::PrimaryCsvEmitError),
    Emit(EmitError),
}

impl std::fmt::Display for MapForceJson5DocumentEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Visitor(error) => error.fmt(f),
            Self::Acquisition(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for MapForceJson5DocumentEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Boundary(error) => error,
            Self::Visitor(error) => error,
            Self::Acquisition(error) => error,
            Self::Emit(error) => error,
        })
    }
}

/// Emit separate acquisition, reached-clock visitation and mapped JSON5
/// serialization. Physical publication stays with the consuming host.
pub fn emit_csv_to_mapforce_json5_documents(
    document: &DocumentProgram,
    source: &CsvInputPolicy,
    target: Option<&str>,
) -> Result<ArtifactSet, MapForceJson5DocumentEmitError> {
    let descriptor = codegen::prepare_csv_to_mapforce_json5_documents(document, source, target)
        .map_err(MapForceJson5DocumentEmitError::Boundary)?;
    emit_prepared(document, source, target, descriptor, None)
}

pub fn emit_csv_file_set_to_mapforce_json5_documents(
    document: &DocumentProgram,
    row_schema: &ir::SchemaNode,
    source: &CsvInputPolicy,
    target: Option<&str>,
) -> Result<ArtifactSet, MapForceJson5DocumentEmitError> {
    let descriptor = codegen::prepare_csv_file_set_to_mapforce_json5_documents(
        document, row_schema, source, target,
    )
    .map_err(MapForceJson5DocumentEmitError::Boundary)?;
    emit_prepared(document, source, target, descriptor, Some(row_schema))
}

fn emit_prepared(
    document: &DocumentProgram,
    source: &CsvInputPolicy,
    target: Option<&str>,
    descriptor: String,
    row_schema: Option<&ir::SchemaNode>,
) -> Result<ArtifactSet, MapForceJson5DocumentEmitError> {
    let artifacts = crate::emit_with_document_clocks(document.program(), target)
        .map_err(MapForceJson5DocumentEmitError::Visitor)?;
    let artifacts = crate::primary_filename::append(artifacts, document)
        .map_err(MapForceJson5DocumentEmitError::Emit)?;
    let mut files = (match row_schema {
        Some(schema) => crate::primary_csv::append_file_set(artifacts, schema, source),
        None => crate::primary_csv::append(artifacts, document, source),
    })
    .map_err(MapForceJson5DocumentEmitError::Acquisition)?
    .into_files();
    let adapter = format!(
        "\npublic static partial class GeneratedMapping\n{{\n    /// <summary>Serialize a complete body only; does not open or accept a document.</summary>\n    public static string SerializeMapForceJson5Document(global::Ferrule.Runtime.FerruleInstance value) =>\n        global::Ferrule.Runtime.FerruleJson.SerializeMapForceJson5AsciiKeys({}, value);\n\n    public static byte[] SerializeMapForceJson5DocumentBytes(global::Ferrule.Runtime.FerruleInstance value) =>\n        new global::System.Text.UTF8Encoding(false, true).GetBytes(SerializeMapForceJson5Document(value));\n}}\n",
        literal::string(&descriptor)
    );
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .expect("ordinary C# artifact")
        .contents
        .extend_from_slice(adapter.as_bytes());
    for (path, source) in runtime::JSON5_SOURCES {
        files.push(file(path, source).map_err(MapForceJson5DocumentEmitError::Emit)?);
    }
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceJson5DocumentEmitError::Emit)
}
