use ir::SchemaKind;

use crate::GeneratedSequence;

use super::{
    ProgramValidationError, RecursiveSequencePathRole, SequenceOwner, sources::SourceCatalog,
};

pub(super) fn validate(
    sources: SourceCatalog<'_>,
    sequence: &GeneratedSequence,
    owner: &SequenceOwner,
) -> Result<(), ProgramValidationError> {
    let GeneratedSequence::RecursiveCollect {
        collection,
        children,
        descent_value,
        values,
        value,
        ..
    } = sequence
    else {
        return Ok(());
    };

    let mut groups = sources
        .path_targets(collection)
        .into_iter()
        .filter_map(|node| node.resolved())
        .filter(|node| matches!(node.node().kind, SchemaKind::Group { .. }))
        .collect::<Vec<_>>();
    if groups.is_empty() {
        return invalid_path(owner, RecursiveSequencePathRole::Collection, collection);
    }

    groups.retain(|group| {
        group.follow(children).is_some_and(|child| {
            child.node().repeating
                && child
                    .resolved()
                    .is_some_and(|resolved| std::ptr::eq(resolved.node(), group.node()))
        })
    });
    if groups.is_empty() {
        return invalid_path(owner, RecursiveSequencePathRole::Children, children);
    }

    groups.retain(|group| {
        group
            .follow(descent_value)
            .is_some_and(|node| matches!(node.node().kind, SchemaKind::Scalar { .. }))
    });
    if groups.is_empty() {
        return invalid_path(
            owner,
            RecursiveSequencePathRole::DescentValue,
            descent_value,
        );
    }

    let value_roots = groups
        .into_iter()
        .filter_map(|group| group.follow(values))
        .collect::<Vec<_>>();
    if value_roots.is_empty() {
        return invalid_path(owner, RecursiveSequencePathRole::Values, values);
    }
    if !value_roots.into_iter().any(|root| {
        root.follow(value)
            .is_some_and(|node| matches!(node.node().kind, SchemaKind::Scalar { .. }))
    }) {
        return invalid_path(owner, RecursiveSequencePathRole::Value, value);
    }
    Ok(())
}

fn invalid_path(
    owner: &SequenceOwner,
    role: RecursiveSequencePathRole,
    path: &[String],
) -> Result<(), ProgramValidationError> {
    Err(ProgramValidationError::InvalidRecursiveSequencePath {
        owner: owner.clone(),
        role,
        path: path.to_vec(),
    })
}
