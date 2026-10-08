//! Additive, fail-closed eligibility for singular JSON5 companion boundaries.
//! This policy neither normalizes JSON5 nor changes the ordinary descriptor codec.

use std::fmt;

use ir::SchemaNode;

mod descriptor;
mod schema;
#[cfg(test)]
mod tests;

pub const MAX_SCHEMA_NODES: usize = 4096;
pub const MAX_LOGICAL_LEVELS: usize = 64;
pub const MAX_NAME_BYTES: usize = 4096;
pub const MAX_TOTAL_NAME_BYTES: usize = 1024 * 1024;
pub const MAX_DESCRIPTOR_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Json5ProfileResource {
    SchemaNodes,
    LogicalLevels,
    NameLength,
    NameBytes,
    DescriptorBytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Json5RequiredKind {
    TooManyNames,
    EmptyName,
    DuplicateName,
    UndeclaredName,
}

/// Bounded typed refusal; complete original codec/syntax causes remain available.
#[derive(Debug)]
pub enum Json5ProfileError {
    Limit {
        resource: Json5ProfileResource,
        requested: usize,
        max: usize,
    },
    RootObjectRequired,
    UnsupportedMetadata {
        field: &'static str,
    },
    DuplicateChildName {
        name: String,
    },
    InvalidRequiredName {
        kind: Json5RequiredKind,
        name: Option<String>,
    },
    DuplicateDescriptorField {
        field: String,
    },
    UnknownDescriptorField {
        field: String,
    },
    UnsupportedDescriptorMetadata {
        field: String,
    },
    InvalidDescriptorShape {
        field: &'static str,
    },
    DescriptorSyntax(serde_json::Error),
    Codec(crate::CodecError),
    DescriptorChanged,
}

impl fmt::Display for Json5ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit {
                resource,
                requested,
                max,
            } => write!(
                f,
                "JSON5 schema {resource:?} is {requested}; maximum is {max}"
            ),
            Self::RootObjectRequired => {
                f.write_str("JSON5 companions require a closed singular object root")
            }
            Self::UnsupportedMetadata { field } => write!(
                f,
                "JSON5 schema metadata {field} is outside the closed-object profile"
            ),
            Self::DuplicateChildName { name } => {
                write!(f, "JSON5 schema declares duplicate child {name:?}")
            }
            Self::InvalidRequiredName { kind, name } => {
                write!(f, "JSON5 required name is invalid ({kind:?}): {name:?}")
            }
            Self::UnsupportedDescriptorMetadata { field } => write!(
                f,
                "JSON5 descriptor metadata {field:?} is outside the profile"
            ),
            Self::DuplicateDescriptorField { field } => {
                write!(f, "JSON5 descriptor repeats decoded field {field:?}")
            }
            Self::UnknownDescriptorField { field } => {
                write!(f, "JSON5 descriptor has unknown field {field:?}")
            }
            Self::InvalidDescriptorShape { field } => {
                write!(f, "JSON5 descriptor has invalid {field} shape")
            }
            Self::DescriptorSyntax(error) => error.fmt(f),
            Self::Codec(error) => error.fmt(f),
            Self::DescriptorChanged => {
                f.write_str("JSON5 schema changed during the complete descriptor round trip")
            }
        }
    }
}

impl std::error::Error for Json5ProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DescriptorSyntax(error) => Some(error),
            Self::Codec(error) => Some(error),
            Self::Limit {
                resource: _,
                requested: _,
                max: _,
            }
            | Self::RootObjectRequired
            | Self::UnsupportedMetadata { field: _ }
            | Self::DuplicateChildName { name: _ }
            | Self::InvalidRequiredName { kind: _, name: _ }
            | Self::DuplicateDescriptorField { field: _ }
            | Self::UnknownDescriptorField { field: _ }
            | Self::UnsupportedDescriptorMetadata { field: _ }
            | Self::InvalidDescriptorShape { field: _ }
            | Self::DescriptorChanged => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Json5ProfileSummary {
    pub nodes: usize,
    pub logical_levels: usize,
    pub name_bytes: usize,
}

/// Census and validate every current typed schema field before any schema copy.
/// A policy depth cap does not promise that the ordinary codec can decode it.
pub fn validate_schema(schema: &SchemaNode) -> Result<Json5ProfileSummary, Json5ProfileError> {
    schema::validate(schema)
}

/// Produce an actually round-tripped ordinary or supported versioned descriptor.
/// Existing codec depth/representation failures are preserved as typed causes.
pub fn encode_schema(schema: &SchemaNode) -> Result<String, Json5ProfileError> {
    validate_schema(schema)?;
    let descriptor =
        crate::encode(schema, MAX_DESCRIPTOR_BYTES).map_err(Json5ProfileError::Codec)?;
    let decoded = decode_schema(&descriptor)?;
    if decoded != *schema {
        return Err(Json5ProfileError::DescriptorChanged);
    }
    Ok(descriptor)
}

/// Reject unknown raw metadata before the ordinary decoder can discard it.
/// Legacy callers of crate::decode retain their existing permissive behavior.
pub fn decode_schema(descriptor: &str) -> Result<SchemaNode, Json5ProfileError> {
    descriptor::validate(descriptor)?;
    let schema =
        crate::decode(descriptor, MAX_DESCRIPTOR_BYTES).map_err(Json5ProfileError::Codec)?;
    validate_schema(&schema)?;
    Ok(schema)
}

pub(super) fn limit(
    resource: Json5ProfileResource,
    requested: usize,
    max: usize,
) -> Json5ProfileError {
    Json5ProfileError::Limit {
        resource,
        requested,
        max,
    }
}

pub(super) fn add(
    total: &mut usize,
    amount: usize,
    resource: Json5ProfileResource,
    max: usize,
) -> Result<(), Json5ProfileError> {
    let requested = total.checked_add(amount).unwrap_or(usize::MAX);
    if requested > max {
        return Err(limit(resource, requested, max));
    }
    *total = requested;
    Ok(())
}
