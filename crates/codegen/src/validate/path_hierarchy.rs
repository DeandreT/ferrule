use ir::{SchemaKind, SchemaNode};

use crate::TargetScope;

use super::{ProgramValidationError, ScopeSchemas};

pub(super) struct Construction<'a> {
    pub collection: &'a [String],
    pub separator: &'a str,
    pub directories: &'a str,
    pub files: &'a str,
    pub name: &'a str,
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
        separator,
        directories,
        files,
        name,
    } = construction;
    if collection.is_empty()
        || collection.iter().any(String::is_empty)
        || separator.is_empty()
        || directories.is_empty()
        || files.is_empty()
        || name.is_empty()
        || directories == files
    {
        return Err(ProgramValidationError::InvalidPathHierarchyConstruction {
            target_path: target_path.to_vec(),
        });
    }
    if schemas
        .sources
        .schema_at(schemas.current_source, collection)
        .is_none_or(|collection| {
            !collection.node().repeating
                || !matches!(collection.node().kind, SchemaKind::Scalar { .. })
        })
    {
        return Err(ProgramValidationError::InvalidPathHierarchyCollection {
            target_path: target_path.to_vec(),
            collection: collection.to_vec(),
        });
    }
    if !matches!(target.kind, SchemaKind::Group { .. }) {
        return Err(
            ProgramValidationError::PathHierarchyConstructionRequiresGroupTarget {
                target_path: target_path.to_vec(),
            },
        );
    }
    if target
        .child(name)
        .is_none_or(|name| name.repeating || !matches!(name.kind, SchemaKind::Scalar { .. }))
    {
        return Err(ProgramValidationError::InvalidPathHierarchyName {
            target_path: target_path.to_vec(),
            field: name.to_string(),
        });
    }
    if target.child(files).is_none_or(|files| {
        !files.repeating
            || !matches!(files.kind, SchemaKind::Group { .. })
            || files.child(name).is_none_or(|name| {
                name.repeating || !matches!(name.kind, SchemaKind::Scalar { .. })
            })
    }) {
        return Err(ProgramValidationError::InvalidPathHierarchyFiles {
            target_path: target_path.to_vec(),
            field: files.to_string(),
            name: name.to_string(),
        });
    }
    if target.child(directories).is_none_or(|directories| {
        !directories.repeating
            || directories.recursive_ref.as_deref() != Some(target.name.as_str())
            || !matches!(directories.kind, SchemaKind::Group { .. })
    }) {
        return Err(ProgramValidationError::InvalidPathHierarchyDirectories {
            target_path: target_path.to_vec(),
            field: directories.to_string(),
        });
    }
    if !scope.bindings.is_empty() || !scope.children.is_empty() {
        return Err(
            ProgramValidationError::PathHierarchyConstructionHasContent {
                target_path: target_path.to_vec(),
            },
        );
    }
    if scope.iteration.is_some() {
        return Err(
            ProgramValidationError::PathHierarchyConstructionHasIteration {
                target_path: target_path.to_vec(),
            },
        );
    }
    Ok(())
}
