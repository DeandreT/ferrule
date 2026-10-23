//! Explicit CSV acquisition, reached document clocks and mapped JSON5 output.
//! These contracts remain separate: emission performs no physical I/O.

use std::fmt;

use crate::{CsvInputBoundaryError, CsvInputPolicy, DocumentProgram, DocumentVisitorBoundaryError};

#[derive(Debug)]
pub enum MapForceJson5DocumentError {
    Source(CsvInputBoundaryError),
    Visitor(DocumentVisitorBoundaryError),
    Schema(codegen_schema::CodecError),
    Shape(&'static str),
}

impl fmt::Display for MapForceJson5DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => error.fmt(f),
            Self::Visitor(error) => error.fmt(f),
            Self::Schema(error) => error.fmt(f),
            Self::Shape(reason) => write!(f, "mapped JSON5 document: {reason}"),
        }
    }
}

impl std::error::Error for MapForceJson5DocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::Visitor(error) => Some(error),
            Self::Schema(error) => Some(error),
            Self::Shape(_) => None,
        }
    }
}

/// Return the unchanged selected target descriptor after complete admission.
pub fn prepare_csv_to_mapforce_json5_documents(
    document: &DocumentProgram,
    source: &CsvInputPolicy,
    target: Option<&str>,
) -> Result<String, MapForceJson5DocumentError> {
    let program = document.program();
    if program.xml_boundary.is_some() {
        return Err(MapForceJson5DocumentError::Shape(
            "XML document adapters are a separate boundary",
        ));
    }
    crate::validate_primary_csv_acquisition(program, source)
        .map_err(MapForceJson5DocumentError::Source)?;
    let index = crate::validate_document_clocks(program, target)
        .map_err(MapForceJson5DocumentError::Visitor)?;
    let schema = index.map_or(&program.target, |index| {
        &program.extra_targets[index].target
    });
    if schema.repeating {
        return Err(MapForceJson5DocumentError::Shape(
            "the document root must not repeat",
        ));
    }
    let descriptor = codegen_schema::encode(schema, crate::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
        .map_err(MapForceJson5DocumentError::Schema)?;
    codegen_schema::mapforce_json5_profile::validate(schema)
        .map_err(MapForceJson5DocumentError::Shape)?;
    Ok(descriptor)
}
