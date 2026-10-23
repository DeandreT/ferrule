use crate::{EmbeddedSchemaError, Program};

#[derive(Debug)]
pub enum MapForceJsonWriterError {
    Shape(&'static str),
    Descriptor(EmbeddedSchemaError),
}
impl std::fmt::Display for MapForceJsonWriterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape(reason) => write!(f, "MapForce JSON writer: {reason}"),
            Self::Descriptor(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for MapForceJsonWriterError {}

/// The native strict JSON writer shares the shape/Decimal domain of the explicit
/// JSON5 writer, but never emits unquoted property identifiers.
pub fn prepare_mapforce_json_writer(program: &Program) -> Result<String, MapForceJsonWriterError> {
    codegen_schema::mapforce_json5_profile::validate(&program.target)
        .map_err(MapForceJsonWriterError::Shape)?;
    crate::serialize_embedded_schema(&program.target, crate::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
        .map_err(MapForceJsonWriterError::Descriptor)
}
