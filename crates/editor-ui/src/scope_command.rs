//! Validation of graph actions that restructure target scopes or change
//! scope state outside the scope editor's settings: child scope
//! construction, computed properties, joins, filter/map sequences, XML type
//! bindings, and sequence consumer duplication.

use std::collections::BTreeSet;

use ir::{Value, XML_TYPE_FIELD};
use mapping::{Binding, DynamicBinding, JoinId, Node, NodeId, Scope, SequenceExpr};

use crate::{
    GraphEditError, GraphPatch, Iteration, ScopeLocation, ScopeSetting, ScopeSettingValue,
    ScopeStep, TargetBinding,
};

/// The one changed child list of a patch that changes nothing else.
fn only_children<'p>(
    patch: &'p GraphPatch,
    location: &ScopeLocation,
) -> Result<(&'p [Scope], &'p [Scope]), GraphEditError> {
    let change =
        patch
            .children_of(location)
            .ok_or_else(|| GraphEditError::ScopeTreeNotChanged {
                location: location.clone(),
            })?;
    if patch.nodes().next().is_some()
        || patch.scopes().next().is_some()
        || patch.settings().next().is_some()
        || patch.children().count() != 1
    {
        return Err(GraphEditError::UnrelatedChanges);
    }
    Ok((change.before(), change.after()))
}

fn empty_scope(field: &str) -> Scope {
    Scope {
        target_field: field.to_owned(),
        ..Scope::default()
    }
}

/// Whether `scope` is an empty scope whose children are all empty scopes.
fn fresh(scope: &Scope) -> bool {
    let shallow = Scope {
        children: Vec::new(),
        ..scope.clone()
    };
    shallow == empty_scope(&scope.target_field) && scope.children.iter().all(fresh)
}

/// Whether `after` keeps every scope of `before` in order, unchanged except
/// for added descendants, and adds only empty scopes beside them.
fn extends(before: &[Scope], after: &[Scope]) -> bool {
    let mut kept = before.iter().peekable();
    for scope in after {
        match kept.next_if(|kept| kept.target_field == scope.target_field) {
            Some(kept) => {
                let shallow = |scope: &Scope| Scope {
                    children: Vec::new(),
                    ..scope.clone()
                };
                if shallow(kept) != shallow(scope) || !extends(&kept.children, &scope.children) {
                    return false;
                }
            }
            None if fresh(scope) => {}
            None => return false,
        }
    }
    kept.next().is_none()
}

pub(crate) fn validate_add_scope(
    patch: &GraphPatch,
    parent: &ScopeLocation,
    field: &str,
) -> Result<(), GraphEditError> {
    let (before, after) = only_children(patch, parent)?;
    let added = after.len() == before.len() + 1
        && after[..before.len()] == *before
        && after.last() == Some(&empty_scope(field));
    if added {
        Ok(())
    } else {
        Err(GraphEditError::ScopeTreeNotChanged {
            location: parent.clone(),
        })
    }
}

pub(crate) fn validate_expand_scope(
    patch: &GraphPatch,
    scope: &ScopeLocation,
) -> Result<(), GraphEditError> {
    let not_expanded = || GraphEditError::ScopeTreeNotChanged {
        location: scope.clone(),
    };
    if patch.nodes().next().is_some()
        || patch.scopes().next().is_some()
        || patch.settings().next().is_some()
    {
        return Err(GraphEditError::UnrelatedChanges);
    }
    if patch.children().next().is_none() {
        return Err(not_expanded());
    }
    for (location, change) in patch.children() {
        if location != scope && !location.is_below(scope) {
            return Err(GraphEditError::UnrelatedChanges);
        }
        if !extends(change.before(), change.after()) {
            return Err(not_expanded());
        }
    }
    Ok(())
}

/// Generated item nodes owned by the sequences of `scope` and every scope
/// nested in it.
fn owned_items(scope: &Scope, items: &mut BTreeSet<NodeId>) {
    if let Some(sequence) = scope.sequence() {
        items.extend(sequence.owned_items());
    }
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            owned_items(segment, items);
        }
    }
    for child in &scope.children {
        owned_items(child, items);
    }
    for child in &scope.dynamic_children {
        owned_items(&child.scope, items);
    }
}

pub(crate) fn validate_remove_scope(
    patch: &GraphPatch,
    scope: &ScopeLocation,
) -> Result<(), GraphEditError> {
    let not_removed = || GraphEditError::ScopeTreeNotChanged {
        location: scope.clone(),
    };
    let Some((parent, ScopeStep::Child(index))) = scope.parent() else {
        return Err(not_removed());
    };
    let change = patch.children_of(&parent).ok_or_else(not_removed)?;
    if patch.scopes().next().is_some()
        || patch.settings().next().is_some()
        || patch.children().count() != 1
    {
        return Err(GraphEditError::UnrelatedChanges);
    }
    let (before, after) = (change.before(), change.after());
    let Some(removed) = before.get(index) else {
        return Err(not_removed());
    };
    if before.len() != after.len() + 1
        || before[..index] != after[..index]
        || before[index + 1..] != after[index..]
    {
        return Err(not_removed());
    }
    // Only the generated items the removed scopes own retire with them.
    let mut items = BTreeSet::new();
    owned_items(removed, &mut items);
    if patch
        .nodes()
        .all(|(id, change)| change.after().is_none() && items.contains(&id))
    {
        Ok(())
    } else {
        Err(GraphEditError::UnrelatedChanges)
    }
}

/// The one changed setting of a patch that changes nothing else.
fn only_setting<'p>(
    patch: &'p GraphPatch,
    scope: &ScopeLocation,
    setting: ScopeSetting,
    unchanged: GraphEditError,
) -> Result<(&'p ScopeSettingValue, &'p ScopeSettingValue), GraphEditError> {
    let change = patch.setting(scope, setting).ok_or(unchanged)?;
    if patch.nodes().next().is_some()
        || patch.scopes().next().is_some()
        || patch.children().next().is_some()
        || patch.settings().count() != 1
    {
        return Err(GraphEditError::UnrelatedChanges);
    }
    Ok((change.before(), change.after()))
}

/// How one ordered list changes: one appended item, one removed item, or one
/// item replaced in place.
pub(crate) enum ListEdit<'a, T> {
    Add(&'a T),
    Remove(&'a T),
    Replace { from: &'a T, to: &'a T },
}

impl<T: PartialEq> ListEdit<'_, T> {
    pub(crate) fn describes(&self, before: &[T], after: &[T]) -> bool {
        match *self {
            Self::Add(item) => {
                after.len() == before.len() + 1
                    && after[..before.len()] == *before
                    && after.last() == Some(item)
            }
            Self::Remove(item) => {
                before.len() == after.len() + 1
                    && (0..before.len()).any(|index| {
                        before[index] == *item
                            && before[..index] == after[..index]
                            && before[index + 1..] == after[index..]
                    })
            }
            Self::Replace { from, to } => {
                let mut differing = before.iter().zip(after).filter(|(b, a)| b != a);
                from != to
                    && before.len() == after.len()
                    && differing.next() == Some((from, to))
                    && differing.next().is_none()
            }
        }
    }
}

pub(crate) fn validate_computed_properties(
    patch: &GraphPatch,
    scope: &ScopeLocation,
    edit: ListEdit<'_, DynamicBinding>,
) -> Result<(), GraphEditError> {
    let unchanged = || GraphEditError::ComputedPropertiesNotChanged {
        location: scope.clone(),
    };
    match only_setting(patch, scope, ScopeSetting::ComputedProperties, unchanged())? {
        (
            ScopeSettingValue::ComputedProperties(before),
            ScopeSettingValue::ComputedProperties(after),
        ) if edit.describes(before, after) => Ok(()),
        _ => Err(unchanged()),
    }
}

/// The iteration change of `scope` when nothing else changes but the nodes
/// `nodes_allowed` permits.
fn iteration_change<'p>(
    patch: &'p GraphPatch,
    scope: &ScopeLocation,
    nodes_allowed: bool,
) -> Result<(&'p Iteration, &'p Iteration), GraphEditError> {
    let unchanged = || GraphEditError::IterationNotChanged {
        location: scope.clone(),
    };
    let change = patch
        .setting(scope, ScopeSetting::Iteration)
        .ok_or_else(unchanged)?;
    if (!nodes_allowed && patch.nodes().next().is_some())
        || patch.scopes().next().is_some()
        || patch.children().next().is_some()
        || patch.settings().count() != 1
    {
        return Err(GraphEditError::UnrelatedChanges);
    }
    match (change.before(), change.after()) {
        (ScopeSettingValue::Iteration(before), ScopeSettingValue::Iteration(after)) => {
            Ok((before, after))
        }
        _ => Err(unchanged()),
    }
}

pub(crate) fn validate_create_join(
    patch: &GraphPatch,
    scope: &ScopeLocation,
    join: JoinId,
) -> Result<(), GraphEditError> {
    match iteration_change(patch, scope, false)? {
        (Iteration::None, Iteration::Joined { id, .. }) if *id == join => Ok(()),
        _ => Err(GraphEditError::IterationNotChanged {
            location: scope.clone(),
        }),
    }
}

pub(crate) fn validate_join_keys(
    patch: &GraphPatch,
    scope: &ScopeLocation,
    join: JoinId,
) -> Result<(), GraphEditError> {
    match iteration_change(patch, scope, false)? {
        (
            Iteration::Joined { id, plan: before },
            Iteration::Joined {
                id: edited,
                plan: after,
            },
        ) if *id == join && *edited == join && before.sources().eq(after.sources()) => Ok(()),
        _ => Err(GraphEditError::IterationNotChanged {
            location: scope.clone(),
        }),
    }
}

/// The one node a patch adds, when it changes no other node.
fn one_added_node(patch: &GraphPatch, node: NodeId) -> Option<&Node> {
    let mut nodes = patch.nodes();
    let (id, change) = nodes.next()?;
    if id != node || nodes.next().is_some() || change.before().is_some() {
        return None;
    }
    change.after()
}

/// The binding change of `scope` when no setting or child scope changes.
fn bindings_change<'p>(
    patch: &'p GraphPatch,
    scope: &ScopeLocation,
) -> Option<(&'p [Binding], &'p [Binding])> {
    let change = patch.scope(scope)?;
    (patch.scopes().count() == 1
        && patch.settings().next().is_none()
        && patch.children().next().is_none())
    .then(|| (change.before(), change.after()))
}

pub(crate) fn validate_joined_output(
    patch: &GraphPatch,
    scope: &ScopeLocation,
    join: JoinId,
    binding: &TargetBinding,
) -> Result<(), GraphEditError> {
    let reads_join = matches!(
        one_added_node(patch, binding.node),
        Some(Node::JoinField { join: read, .. } | Node::JoinPosition { join: read }) if *read == join
    );
    let appended = bindings_change(patch, scope).is_some_and(|(before, after)| {
        ListEdit::Add(&Binding {
            target_field: binding.field.clone(),
            node: binding.node,
        })
        .describes(before, after)
    });
    if reads_join && appended {
        Ok(())
    } else {
        Err(GraphEditError::BindingNotChanged {
            location: scope.clone(),
        })
    }
}

pub(crate) fn validate_use_filter_map(
    patch: &GraphPatch,
    scope: &ScopeLocation,
) -> Result<(), GraphEditError> {
    let (before, after) = iteration_change(patch, scope, true)?;
    let Iteration::Generated(sequence @ SequenceExpr::FilterMapV1(_)) = after else {
        return Err(GraphEditError::IterationNotChanged {
            location: scope.clone(),
        });
    };
    if !before.is_source_backed() {
        return Err(GraphEditError::IterationNotChanged {
            location: scope.clone(),
        });
    }
    // The new sequence may add only its own bounds and item nodes.
    let owned = sequence
        .inputs()
        .into_iter()
        .chain(sequence.owned_items())
        .collect::<BTreeSet<_>>();
    if patch
        .nodes()
        .all(|(id, change)| change.before().is_none() && owned.contains(&id))
    {
        Ok(())
    } else {
        Err(GraphEditError::UnrelatedChanges)
    }
}

pub(crate) fn validate_edit_filter_map(
    patch: &GraphPatch,
    scope: &ScopeLocation,
) -> Result<(), GraphEditError> {
    match iteration_change(patch, scope, false)? {
        (
            Iteration::Generated(SequenceExpr::FilterMapV1(_)),
            Iteration::Generated(SequenceExpr::FilterMapV1(_)),
        ) => Ok(()),
        _ => Err(GraphEditError::IterationNotChanged {
            location: scope.clone(),
        }),
    }
}

pub(crate) fn validate_set_xml_type(
    patch: &GraphPatch,
    scope: &ScopeLocation,
    node: NodeId,
) -> Result<(), GraphEditError> {
    let declared = matches!(
        one_added_node(patch, node),
        Some(Node::Const {
            value: Value::String(_)
        })
    );
    let marker = Binding {
        target_field: XML_TYPE_FIELD.to_owned(),
        node,
    };
    let bound = bindings_change(patch, scope).is_some_and(|(before, after)| {
        let unbound = !before
            .iter()
            .any(|binding| binding.target_field == XML_TYPE_FIELD);
        (unbound && ListEdit::Add(&marker).describes(before, after))
            || before.iter().any(|from| {
                from.target_field == XML_TYPE_FIELD
                    && ListEdit::Replace { from, to: &marker }.describes(before, after)
            })
    });
    if declared && bound {
        Ok(())
    } else {
        Err(GraphEditError::XmlTypeNotChanged {
            location: scope.clone(),
        })
    }
}

pub(crate) fn validate_infer_xml_type(
    patch: &GraphPatch,
    scope: &ScopeLocation,
) -> Result<(), GraphEditError> {
    if patch.nodes().next().is_some() {
        return Err(GraphEditError::UnrelatedChanges);
    }
    let cleared = bindings_change(patch, scope).is_some_and(|(before, after)| {
        let kept = before
            .iter()
            .filter(|binding| binding.target_field != XML_TYPE_FIELD)
            .cloned()
            .collect::<Vec<_>>();
        kept.len() < before.len() && kept == after
    });
    if cleared {
        Ok(())
    } else {
        Err(GraphEditError::XmlTypeNotChanged {
            location: scope.clone(),
        })
    }
}

pub(crate) fn validate_duplicate(
    patch: &GraphPatch,
    original: NodeId,
    duplicate: NodeId,
) -> Result<(), GraphEditError> {
    let consumer = patch
        .node(duplicate)
        .and_then(|change| change.before().is_none().then_some(change.after()?))
        .is_some_and(|node| {
            matches!(
                node,
                Node::SequenceExists { .. }
                    | Node::SequenceItemAt { .. }
                    | Node::SequenceAggregate { .. }
            )
        });
    if !consumer || patch.node(original).is_some() {
        return Err(GraphEditError::NotDuplicated { duplicate });
    }
    if patch.only_nodes() && patch.nodes().all(|(_, change)| change.before().is_none()) {
        Ok(())
    } else {
        Err(GraphEditError::UnrelatedChanges)
    }
}
