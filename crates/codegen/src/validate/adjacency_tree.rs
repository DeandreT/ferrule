use ir::{ScalarType, SchemaKind, SchemaNode};

use crate::TargetScope;

use super::{ProgramValidationError, ScopeSchemas};

pub(super) struct Construction<'a> {
    pub collection: &'a [String],
    pub key: &'a [String],
    pub parent: &'a [String],
    pub target_key: &'a str,
    pub target_children: &'a str,
}

pub(super) fn validate(
    scope: &TargetScope,
    construction: Construction<'_>,
    schemas: ScopeSchemas<'_>,
    target: &SchemaNode,
    target_path: &[String],
) -> Result<(), ProgramValidationError> {
    let Construction {
        collection,
        key,
        parent,
        target_key,
        target_children,
    } = construction;
    if invalid_path(collection)
        || invalid_path(key)
        || invalid_path(parent)
        || key == parent
        || target_key.is_empty()
        || target_children.is_empty()
        || target_key == target_children
    {
        return Err(ProgramValidationError::InvalidAdjacencyTreeConstruction {
            target_path: target_path.to_vec(),
        });
    }
    let Some(collection_schema) = schemas
        .sources
        .schema_at(schemas.current_source, collection)
        .filter(|collection| {
            collection.node().repeating
                && matches!(collection.node().kind, SchemaKind::Group { .. })
        })
    else {
        return Err(ProgramValidationError::InvalidAdjacencyTreeCollection {
            target_path: target_path.to_vec(),
            collection: collection.to_vec(),
        });
    };
    validate_rows(
        collection_schema,
        Output {
            key,
            parent,
            target_key,
            target_children,
        },
        target_path,
    )?;
    validate_output(
        scope,
        Output {
            key,
            parent,
            target_key,
            target_children,
        },
        target,
        target_path,
    )
}

/// Row-relative fields and generated target names; none selects a source.
pub(super) struct Output<'a> {
    pub key: &'a [String],
    pub parent: &'a [String],
    pub target_key: &'a str,
    pub target_children: &'a str,
}

impl Output<'_> {
    fn is_valid(&self) -> bool {
        !invalid_path(self.key)
            && !invalid_path(self.parent)
            && self.key != self.parent
            && !self.target_key.is_empty()
            && !self.target_children.is_empty()
            && self.target_key != self.target_children
    }
}

/// Validates an owned row collection against its exact endpoint schema.
/// Unknown or unresolved owners and paths are left to the ownership structure
/// diagnostic; key and parent stay relative to each selected row.
pub(super) fn validate_owned(
    scope: &TargetScope,
    collection: &mapping::OwnedSourceRef,
    output: Output<'_>,
    schemas: ScopeSchemas<'_>,
    target: &SchemaNode,
    target_path: &[String],
) -> Result<(), ProgramValidationError> {
    if !output.is_valid() {
        return Err(ProgramValidationError::InvalidAdjacencyTreeConstruction {
            target_path: target_path.to_vec(),
        });
    }
    if let Some(rows) = schemas.sources.owned_schema_at(collection) {
        if !rows.node().repeating || !matches!(rows.node().kind, SchemaKind::Group { .. }) {
            return Err(ProgramValidationError::InvalidAdjacencyTreeCollection {
                target_path: target_path.to_vec(),
                collection: collection.path.clone(),
            });
        }
        validate_rows(
            rows,
            Output {
                key: output.key,
                parent: output.parent,
                target_key: output.target_key,
                target_children: output.target_children,
            },
            target_path,
        )?;
    }
    validate_output(scope, output, target, target_path)
}

fn validate_rows(
    rows: super::SchemaCursor<'_>,
    output: Output<'_>,
    target_path: &[String],
) -> Result<(), ProgramValidationError> {
    validate_string_field(
        rows.follow(output.key).map(super::SchemaCursor::node),
        target_path,
        output.key,
        "key",
    )?;
    validate_string_field(
        rows.follow(output.parent).map(super::SchemaCursor::node),
        target_path,
        output.parent,
        "parent",
    )
}

fn validate_output(
    scope: &TargetScope,
    output: Output<'_>,
    target: &SchemaNode,
    target_path: &[String],
) -> Result<(), ProgramValidationError> {
    let Output {
        target_key,
        target_children,
        ..
    } = output;
    if !matches!(target.kind, SchemaKind::Group { .. }) {
        return Err(
            ProgramValidationError::AdjacencyTreeConstructionRequiresGroupTarget {
                target_path: target_path.to_vec(),
            },
        );
    }
    if target.child(target_key).is_none_or(|key| {
        key.repeating
            || !matches!(
                key.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            )
    }) {
        return Err(ProgramValidationError::InvalidAdjacencyTreeTargetKey {
            target_path: target_path.to_vec(),
            field: target_key.to_string(),
        });
    }
    if target.child(target_children).is_none_or(|children| {
        !children.repeating
            || !matches!(children.kind, SchemaKind::Group { .. })
            || children.recursive_ref.as_deref() != Some(target.name.as_str())
    }) {
        return Err(ProgramValidationError::InvalidAdjacencyTreeTargetChildren {
            target_path: target_path.to_vec(),
            field: target_children.to_string(),
        });
    }
    if !scope.bindings.is_empty() || !scope.children.is_empty() {
        return Err(
            ProgramValidationError::AdjacencyTreeConstructionHasContent {
                target_path: target_path.to_vec(),
            },
        );
    }
    if scope.iteration.is_some() {
        return Err(
            ProgramValidationError::AdjacencyTreeConstructionHasIteration {
                target_path: target_path.to_vec(),
            },
        );
    }
    Ok(())
}

fn invalid_path(path: &[String]) -> bool {
    path.is_empty() || path.iter().any(String::is_empty)
}

fn validate_string_field(
    field: Option<&SchemaNode>,
    target_path: &[String],
    path: &[String],
    role: &'static str,
) -> Result<(), ProgramValidationError> {
    if field.is_none_or(|field| {
        field.repeating
            || !matches!(
                field.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            )
    }) {
        return Err(ProgramValidationError::InvalidAdjacencyTreeField {
            target_path: target_path.to_vec(),
            role,
            path: path.to_vec(),
        });
    }
    Ok(())
}
