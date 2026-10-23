use codegen::{ArtifactSet, CsvInputPolicy, DocumentProgram};

use crate::{EmitError, Options, rust_string};

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

/// Emit CSV acquisition and one selected document visitor with independent
/// reached clocks. The consumer decides when to serialize and publish a body.
pub fn emit_csv_to_mapforce_json5_documents(
    document: &DocumentProgram,
    options: &Options,
    source: &CsvInputPolicy,
    target: Option<&str>,
) -> Result<ArtifactSet, MapForceJson5DocumentEmitError> {
    let descriptor = codegen::prepare_csv_to_mapforce_json5_documents(document, source, target)
        .map_err(MapForceJson5DocumentEmitError::Boundary)?;
    emit_prepared(document, options, source, target, descriptor, None)
}

pub fn emit_csv_file_set_to_mapforce_json5_documents(
    document: &DocumentProgram,
    options: &Options,
    row_schema: &ir::SchemaNode,
    source: &CsvInputPolicy,
    target: Option<&str>,
) -> Result<ArtifactSet, MapForceJson5DocumentEmitError> {
    let descriptor = codegen::prepare_csv_file_set_to_mapforce_json5_documents(
        document, row_schema, source, target,
    )
    .map_err(MapForceJson5DocumentEmitError::Boundary)?;
    emit_prepared(
        document,
        options,
        source,
        target,
        descriptor,
        Some(row_schema),
    )
}

fn emit_prepared(
    document: &DocumentProgram,
    options: &Options,
    source: &CsvInputPolicy,
    target: Option<&str>,
    descriptor: String,
    row_schema: Option<&ir::SchemaNode>,
) -> Result<ArtifactSet, MapForceJson5DocumentEmitError> {
    let artifacts = crate::emit_with_document_clocks(document.program(), options, target)
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
        "\n/// Serialize the selected complete body only; does not open or accept a document.\npub fn serialize_mapforce_json5_document(value: &Instance) -> Result<String, codegen_runtime::MapForceJson5Error> {{\n    codegen_runtime::serialize_mapforce_json5_ascii_keys({}, value)\n}}\n\npub fn serialize_mapforce_json5_document_bytes(value: &Instance) -> Result<Vec<u8>, codegen_runtime::MapForceJson5Error> {{\n    serialize_mapforce_json5_document(value).map(String::into_bytes)\n}}\n",
        rust_string(&descriptor)
    );
    files
        .iter_mut()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .expect("ordinary Rust artifact")
        .contents
        .extend_from_slice(adapter.as_bytes());
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(MapForceJson5DocumentEmitError::Emit)
}
