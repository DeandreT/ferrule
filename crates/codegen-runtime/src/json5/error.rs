use std::fmt;

use codegen_schema::json5_profile::Json5ProfileError;
use format_json::JsonFormatError;
use format_json::json5_boundary::Json5SyntaxError;

use crate::RuntimeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Json5BoundaryResource {
    OriginalDocumentBytes,
    OutputDocumentBytes,
}

/// Additive errors retain the actual typed cause and every original limit field.
#[derive(Debug)]
pub enum Json5BoundaryError {
    Limit {
        resource: Json5BoundaryResource,
        requested: usize,
        max: usize,
    },
    Encoding(std::str::Utf8Error),
    SourceSchema(Json5ProfileError),
    TargetSchema(Json5ProfileError),
    Syntax(Json5SyntaxError),
    Input(JsonFormatError),
    Mapping(RuntimeError),
    Output(JsonFormatError),
    OutputSerialization(serde_json::Error),
}

impl fmt::Display for Json5BoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit {
                resource,
                requested,
                max,
            } => write!(f, "JSON5 {resource:?} is {requested}; maximum is {max}"),
            Self::Encoding(error) => write!(f, "JSON5 input is not UTF-8: {error}"),
            Self::SourceSchema(error) => write!(f, "JSON5 source schema: {error}"),
            Self::TargetSchema(error) => write!(f, "JSON5 target schema: {error}"),
            Self::Syntax(error) => write!(f, "JSON5 syntax: {error}"),
            Self::Input(error) => write!(f, "JSON5 input: {error}"),
            Self::Mapping(error) => error.fmt(f),
            Self::Output(error) => write!(f, "JSON5 output: {error}"),
            Self::OutputSerialization(error) => {
                write!(f, "JSON5 strict output serialization: {error}")
            }
        }
    }
}

impl std::error::Error for Json5BoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Limit {
                resource: _,
                requested: _,
                max: _,
            } => None,
            Self::Encoding(error) => Some(error),
            Self::SourceSchema(error) | Self::TargetSchema(error) => Some(error),
            Self::Syntax(error) => Some(error),
            Self::Input(error) | Self::Output(error) => Some(error),
            Self::Mapping(error) => Some(error),
            Self::OutputSerialization(error) => Some(error),
        }
    }
}
