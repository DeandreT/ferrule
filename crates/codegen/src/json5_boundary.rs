//! Shared admission for explicitly selected singular JSON5 companions.
//! No parser, emitted method or CLI selection is introduced here.

use std::fmt;

use codegen_schema::json5_profile::{self, Json5ProfileError};
use mapping::FormatOptions;

use crate::{Program, ProgramValidationError, validate_program};

mod options;
mod scopes;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Json5BoundarySide {
    Source,
    Target,
}

#[derive(Debug)]
pub enum Json5BoundaryPolicyError {
    Schema {
        side: Json5BoundarySide,
        error: Json5ProfileError,
    },
    FormatOption {
        side: Json5BoundarySide,
        field: &'static str,
    },
    ProgramField {
        field: &'static str,
    },
    TargetScope {
        field: &'static str,
    },
    Validation(ProgramValidationError),
}

impl fmt::Display for Json5BoundaryPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema { side, error } => write!(f, "JSON5 {side:?} schema: {error}"),
            Self::FormatOption { side, field } => {
                write!(f, "JSON5 {side:?} format option {field} is unsupported")
            }
            Self::ProgramField { field } => write!(
                f,
                "JSON5 program field {field} is outside the singular closed-object profile"
            ),
            Self::TargetScope { field } => write!(
                f,
                "JSON5 target scope {field} is outside the static closed-object profile"
            ),
            Self::Validation(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Json5BoundaryPolicyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Schema { side: _, error } => Some(error),
            Self::Validation(error) => Some(error),
            Self::FormatOption { side: _, field: _ }
            | Self::ProgramField { field: _ }
            | Self::TargetScope { field: _ } => None,
        }
    }
}

/// Complete actual descriptor bytes proved before a future opt-in emitter publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Json5BoundaryProfile {
    pub source_descriptor: String,
    pub target_descriptor: String,
}

/// Retained JSON identities may vary; neither identity enables an adapter.
/// Callers must explicitly select the future companion independently of options.
pub fn validate_json5_format_options(
    options: &FormatOptions,
    side: Json5BoundarySide,
) -> Result<(), Json5BoundaryPolicyError> {
    options::validate(options, side)
}

/// Prove the bounded shape, existing program semantics and actual codec round trips.
/// No new graph-expression budget or ordinary validation behavior is imposed.
pub fn prepare_json5_boundary(
    program: &Program,
) -> Result<Json5BoundaryProfile, Json5BoundaryPolicyError> {
    // Future Program fields require an explicit admission decision.
    let Program {
        xml_boundary,
        source,
        extra_sources,
        target,
        expressions,
        user_functions,
        failure_rules,
        root,
        extra_targets,
    } = program;
    if xml_boundary.is_some() {
        return Err(Json5BoundaryPolicyError::ProgramField {
            field: "xml_boundary",
        });
    }
    if !extra_sources.is_empty() {
        return Err(Json5BoundaryPolicyError::ProgramField {
            field: "extra_sources",
        });
    }
    if !extra_targets.is_empty() {
        return Err(Json5BoundaryPolicyError::ProgramField {
            field: "extra_targets",
        });
    }
    let schema_error = |side, error| Json5BoundaryPolicyError::Schema { side, error };
    json5_profile::validate_schema(source)
        .map_err(|error| schema_error(Json5BoundarySide::Source, error))?;
    json5_profile::validate_schema(target)
        .map_err(|error| schema_error(Json5BoundarySide::Target, error))?;
    scopes::validate(root, target)?;
    // These existing graph domains remain governed by full ordinary validation.
    let _ordinary_expression_domains = (expressions, user_functions, failure_rules);
    validate_program(program).map_err(Json5BoundaryPolicyError::Validation)?;
    let source_descriptor = json5_profile::encode_schema(source)
        .map_err(|error| schema_error(Json5BoundarySide::Source, error))?;
    let target_descriptor = json5_profile::encode_schema(target)
        .map_err(|error| schema_error(Json5BoundarySide::Target, error))?;
    Ok(Json5BoundaryProfile {
        source_descriptor,
        target_descriptor,
    })
}

pub fn validate_json5_boundary(program: &Program) -> Result<(), Json5BoundaryPolicyError> {
    prepare_json5_boundary(program).map(|_| ())
}
