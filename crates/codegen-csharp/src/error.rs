use std::fmt;

use codegen::{ArtifactPathError, ArtifactSetError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    DuplicateNode { node: u32 },
    MissingExpression { node: u32 },
    NonFiniteFloat { node: u32 },
    InvalidDuplicateBinding { scope: usize, binding: usize },
    ArtifactPath(ArtifactPathError),
    ArtifactSet(ArtifactSetError),
}

impl fmt::Display for EmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNode { node } => {
                write!(formatter, "compiled mapping contains duplicate node {node}")
            }
            Self::MissingExpression { node } => {
                write!(formatter, "target binding references missing node {node}")
            }
            Self::NonFiniteFloat { node } => {
                write!(formatter, "graph node {node} contains a non-finite float")
            }
            Self::InvalidDuplicateBinding { scope, binding } => write!(
                formatter,
                "scope {scope} binding {binding} duplicates a non-repeating or incompatible target"
            ),
            Self::ArtifactPath(error) => error.fmt(formatter),
            Self::ArtifactSet(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for EmitError {}

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
