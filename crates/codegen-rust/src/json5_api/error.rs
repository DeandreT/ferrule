use std::fmt;

use codegen::{ArtifactSetError, Json5BoundaryPolicyError};

use crate::EmitError;

/// Opt-in errors do not alter the ordinary emitter's error enum or behavior.
#[derive(Debug)]
pub enum Json5EmitError {
    Policy(Json5BoundaryPolicyError),
    Ordinary(EmitError),
    ArtifactSet(ArtifactSetError),
    MissingLibrary,
}

impl fmt::Display for Json5EmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => error.fmt(f),
            Self::Ordinary(error) => error.fmt(f),
            Self::ArtifactSet(error) => error.fmt(f),
            Self::MissingLibrary => {
                f.write_str("ordinary Rust artifacts contain no src/lib.rs for JSON5 companions")
            }
        }
    }
}

impl std::error::Error for Json5EmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Policy(error) => Some(error),
            Self::Ordinary(error) => Some(error),
            Self::ArtifactSet(error) => Some(error),
            Self::MissingLibrary => None,
        }
    }
}
