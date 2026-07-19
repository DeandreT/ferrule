use std::collections::BTreeMap;

use ir::{SchemaKind, SchemaNode};
use mapping::NodeId;

use super::{Expression, ProgramValidationError};

pub(super) fn validate(
    source: &SchemaNode,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        let Expression::Lookup {
            collection,
            key,
            value,
            ..
        } = expression
        else {
            continue;
        };
        let candidates = direct_path_targets(source, collection);
        if !candidates.iter().any(|candidate| candidate.repeating) {
            return Err(ProgramValidationError::InvalidLookupCollection {
                node,
                collection: collection.clone(),
            });
        }
        let scalar_below_collection = |path: &[String]| {
            candidates.iter().any(|candidate| {
                candidate.repeating
                    && follow_direct(source, candidate, path).is_some_and(|leaf| {
                        matches!(leaf.kind, SchemaKind::Scalar { .. })
                            && (path.is_empty() || !leaf.repeating)
                    })
            })
        };
        if !scalar_below_collection(key) {
            return Err(ProgramValidationError::InvalidLookupKeyPath {
                node,
                collection: collection.clone(),
                key: key.clone(),
            });
        }
        if !scalar_below_collection(value) {
            return Err(ProgramValidationError::InvalidLookupValuePath {
                node,
                collection: collection.clone(),
                value: value.clone(),
            });
        }
    }
    Ok(())
}

fn direct_path_targets<'a>(root: &'a SchemaNode, path: &[String]) -> Vec<&'a SchemaNode> {
    fn visit<'a>(
        root: &'a SchemaNode,
        current: &'a SchemaNode,
        path: &[String],
        targets: &mut Vec<&'a SchemaNode>,
    ) {
        if let Some(target) = follow_direct(root, current, path) {
            targets.push(target);
        }
        if let SchemaKind::Group { children, .. } = &current.kind {
            for child in children {
                visit(root, child, path, targets);
            }
        }
    }

    let mut targets = Vec::new();
    visit(root, root, path, &mut targets);
    targets
}

/// Lookup paths follow plain fields and cannot implicitly traverse another
/// repeated boundary. The terminal collection itself may be repeating.
fn follow_direct<'a>(
    root: &'a SchemaNode,
    current: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let mut current = current;
    for (index, segment) in path.iter().enumerate() {
        if let Some(anchor) = &current.recursive_ref {
            current = find_concrete_group(root, anchor)?;
        }
        current = current.child(segment)?;
        if current.repeating && index + 1 != path.len() {
            return None;
        }
    }
    Some(current)
}

fn find_concrete_group<'a>(current: &'a SchemaNode, anchor: &str) -> Option<&'a SchemaNode> {
    if current.recursive_ref.is_none()
        && current.name == anchor
        && matches!(current.kind, SchemaKind::Group { .. })
    {
        return Some(current);
    }
    let SchemaKind::Group { children, .. } = &current.kind else {
        return None;
    };
    children
        .iter()
        .find_map(|child| find_concrete_group(child, anchor))
}
