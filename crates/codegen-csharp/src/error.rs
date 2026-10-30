use std::fmt;

use codegen::{ArtifactPathError, ArtifactSetError, ProgramValidationError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    UnsupportedDecimalValue {
        node: mapping::NodeId,
    },
    UnsupportedFilterMapV1 {
        item: mapping::NodeId,
    },
    ProgramValidation(ProgramValidationError),
    CsvOutput(codegen::CsvOutputError),
    SchemaSerialization(String),
    /// The named boundary schema contains a JSON tuple frame, which the
    /// generated C# runtime cannot read or write yet.
    JsonTupleFrame {
        schema: String,
    },
    EmbeddedSchema(codegen::EmbeddedSchemaError),
    ArtifactPath(ArtifactPathError),
    ArtifactSet(ArtifactSetError),
    Manifest(codegen::ManifestError),
}

impl fmt::Display for EmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedDecimalValue { node } => write!(
                formatter,
                "decimal literal at node {node} has no admitted generated C# profile"
            ),
            Self::UnsupportedFilterMapV1 { item } => write!(
                formatter,
                "generated C# filter_map_v1 item {item} is not implemented"
            ),
            Self::ProgramValidation(error) => error.fmt(formatter),
            Self::CsvOutput(error) => error.fmt(formatter),
            Self::SchemaSerialization(message) => {
                write!(formatter, "cannot serialize embedded schema: {message}")
            }
            Self::JsonTupleFrame { schema } => write!(
                formatter,
                "schema {schema:?} contains a JSON tuple frame, which generated C# does not support"
            ),
            Self::EmbeddedSchema(error) => error.fmt(formatter),
            Self::ArtifactPath(error) => error.fmt(formatter),
            Self::ArtifactSet(error) => error.fmt(formatter),
            Self::Manifest(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for EmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ProgramValidation(error) => Some(error),
            Self::CsvOutput(error) => Some(error),
            Self::UnsupportedDecimalValue { .. }
            | Self::UnsupportedFilterMapV1 { .. }
            | Self::SchemaSerialization(_)
            | Self::JsonTupleFrame { .. } => None,
            Self::EmbeddedSchema(error) => Some(error),
            Self::ArtifactPath(error) => Some(error),
            Self::ArtifactSet(error) => Some(error),
            Self::Manifest(error) => Some(error),
        }
    }
}

impl From<ProgramValidationError> for EmitError {
    fn from(error: ProgramValidationError) -> Self {
        Self::ProgramValidation(error)
    }
}

impl From<codegen::EmbeddedSchemaError> for EmitError {
    fn from(error: codegen::EmbeddedSchemaError) -> Self {
        Self::EmbeddedSchema(error)
    }
}

impl From<ArtifactPathError> for EmitError {
    fn from(error: ArtifactPathError) -> Self {
        Self::ArtifactPath(error)
    }
}

impl From<ArtifactSetError> for EmitError {
    fn from(error: ArtifactSetError) -> Self {
        Self::ArtifactSet(error)
    }
}

impl From<codegen::CsvOutputError> for EmitError {
    fn from(error: codegen::CsvOutputError) -> Self {
        Self::CsvOutput(error)
    }
}
