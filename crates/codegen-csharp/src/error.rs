use std::fmt;

use codegen::{ArtifactPathError, ArtifactSetError, ProgramValidationError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    ProgramValidation(ProgramValidationError),
    SchemaSerialization(String),
    ArtifactPath(ArtifactPathError),
    ArtifactSet(ArtifactSetError),
}

impl fmt::Display for EmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProgramValidation(error) => error.fmt(formatter),
            Self::SchemaSerialization(message) => {
                write!(formatter, "cannot serialize embedded schema: {message}")
            }
            Self::ArtifactPath(error) => error.fmt(formatter),
            Self::ArtifactSet(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for EmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ProgramValidation(error) => Some(error),
            Self::SchemaSerialization(_) => None,
            Self::ArtifactPath(error) => Some(error),
            Self::ArtifactSet(error) => Some(error),
        }
    }
}

impl From<ProgramValidationError> for EmitError {
    fn from(error: ProgramValidationError) -> Self {
        Self::ProgramValidation(error)
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
