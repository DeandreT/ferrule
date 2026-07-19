use ir::{SchemaKind, SchemaNode};

use crate::GeneratedSequence;

use super::{
    ProgramValidationError, RecursiveSequencePathRole, SequenceOwner, find_concrete_schema_group,
    follow_schema_from, schema_path_targets,
};

pub(super) fn validate(
    source: &SchemaNode,
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

    let mut groups = schema_path_targets(source, collection)
        .into_iter()
        .filter_map(|node| resolved_schema_node(source, node))
        .filter(|node| matches!(node.kind, SchemaKind::Group { .. }))
        .collect::<Vec<_>>();
    if groups.is_empty() {
        return invalid_path(owner, RecursiveSequencePathRole::Collection, collection);
    }

    groups.retain(|group| {
        follow_schema_from(source, group, children).is_some_and(|child| {
            child.repeating
                && resolved_schema_node(source, child)
                    .is_some_and(|resolved| std::ptr::eq(resolved, *group))
        })
    });
    if groups.is_empty() {
        return invalid_path(owner, RecursiveSequencePathRole::Children, children);
    }

    groups.retain(|group| {
        follow_schema_from(source, group, descent_value)
            .is_some_and(|node| matches!(node.kind, SchemaKind::Scalar { .. }))
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
        .filter_map(|group| follow_schema_from(source, group, values))
        .collect::<Vec<_>>();
    if value_roots.is_empty() {
        return invalid_path(owner, RecursiveSequencePathRole::Values, values);
    }
    if !value_roots.into_iter().any(|root| {
        follow_schema_from(source, root, value)
            .is_some_and(|node| matches!(node.kind, SchemaKind::Scalar { .. }))
    }) {
        return invalid_path(owner, RecursiveSequencePathRole::Value, value);
    }
    Ok(())
}

fn resolved_schema_node<'a>(root: &'a SchemaNode, node: &'a SchemaNode) -> Option<&'a SchemaNode> {
    node.recursive_ref
        .as_deref()
        .map(|anchor| find_concrete_schema_group(root, anchor))
        .unwrap_or(Some(node))
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
