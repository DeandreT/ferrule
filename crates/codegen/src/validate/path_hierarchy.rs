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

/// Generated target structure; these names never select a source.
pub(super) struct Output<'a> {
    pub separator: &'a str,
    pub directories: &'a str,
    pub files: &'a str,
    pub name: &'a str,
}

impl Output<'_> {
    fn is_valid(&self) -> bool {
        !self.separator.is_empty()
            && !self.directories.is_empty()
            && !self.files.is_empty()
            && !self.name.is_empty()
            && self.directories != self.files
    }
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
    let output = Output {
        separator,
        directories,
        files,
        name,
    };
    if collection.is_empty() || collection.iter().any(String::is_empty) || !output.is_valid() {
        return Err(ProgramValidationError::InvalidPathHierarchyConstruction {
            target_path: target_path.to_vec(),
        });
    }
    if schemas
        .sources
        .schema_at(schemas.current_source, collection)
        .is_none_or(|collection| !collection.node().repeating || !collection.node().is_scalar())
    {
        return Err(ProgramValidationError::InvalidPathHierarchyCollection {
            target_path: target_path.to_vec(),
            collection: collection.to_vec(),
        });
    }
    validate_output(scope, output, target, target_path)
}

/// Validates an owned collection against its exact endpoint schema. Unknown or
/// unresolved owners and paths are left to the ownership structure diagnostic.
pub(super) fn validate_owned(
    scope: &TargetScope,
    collection: &mapping::OwnedSourceRef,
    output: Output<'_>,
    schemas: ScopeSchemas<'_>,
    target: &SchemaNode,
    target_path: &[String],
) -> Result<(), ProgramValidationError> {
    if !output.is_valid() {
        return Err(ProgramValidationError::InvalidPathHierarchyConstruction {
            target_path: target_path.to_vec(),
        });
    }
    if schemas
        .sources
        .owned_schema_at(collection)
        .is_some_and(|node| !node.node().repeating || !node.node().is_scalar())
    {
        return Err(ProgramValidationError::InvalidPathHierarchyCollection {
            target_path: target_path.to_vec(),
            collection: collection.path.clone(),
        });
    }
    validate_output(scope, output, target, target_path)
}

fn validate_output(
    scope: &TargetScope,
    output: Output<'_>,
    target: &SchemaNode,
    target_path: &[String],
) -> Result<(), ProgramValidationError> {
    let Output {
        directories,
        files,
        name,
        ..
    } = output;
    if !matches!(target.kind, SchemaKind::Group { .. }) {
        return Err(
            ProgramValidationError::PathHierarchyConstructionRequiresGroupTarget {
                target_path: target_path.to_vec(),
            },
        );
    }
    if target
        .child(name)
        .is_none_or(|name| name.repeating || !name.is_scalar())
    {
        return Err(ProgramValidationError::InvalidPathHierarchyName {
            target_path: target_path.to_vec(),
            field: name.to_string(),
        });
    }
    if target.child(files).is_none_or(|files| {
        !files.repeating
            || !matches!(files.kind, SchemaKind::Group { .. })
            || files
                .child(name)
                .is_none_or(|name| name.repeating || !name.is_scalar())
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
