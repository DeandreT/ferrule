//! Validation of graph actions that change project parts outside any target
//! scope, and of auto-connect, which binds many target fields at once.

use std::collections::BTreeSet;

use mapping::{FailureIteration, FailureRule, FunctionId, Node, NodeId, Scope};

use crate::scope_command::empty_scope;
use crate::{GraphEditError, GraphPatch, PartChange, ProjectPart, ScopeLocation};

/// The one changed project part of a patch that changes no target scope
/// and no other part.
fn only_part(patch: &GraphPatch, part: ProjectPart) -> Result<(), GraphEditError> {
    let parts = patch.project_parts().parts().collect::<Vec<_>>();
    if !parts.contains(&part) {
        return Err(GraphEditError::ProjectPartNotChanged { part });
    }
    if parts.len() != 1 || !patch.scope_free() {
        return Err(GraphEditError::UnrelatedChanges);
    }
    Ok(())
}

fn not_changed(part: ProjectPart) -> GraphEditError {
    GraphEditError::ProjectPartNotChanged { part }
}

/// Every node a failure rule reads or owns.
fn rule_nodes(rule: &FailureRule) -> BTreeSet<NodeId> {
    let mut nodes = rule
        .selection
        .predicate()
        .into_iter()
        .chain(rule.message)
        .collect::<BTreeSet<_>>();
    if let FailureIteration::Sequence { sequence } = &rule.iteration {
        nodes.extend(sequence.inputs());
        nodes.extend(sequence.owned_items());
    }
    nodes
}

/// Whether the patch's node changes only add nodes that `owner` reads.
fn adds_only(patch: &GraphPatch, owner: &BTreeSet<NodeId>) -> bool {
    patch
        .nodes()
        .all(|(id, change)| change.before().is_none() && owner.contains(&id))
}

fn failure_rules(patch: &GraphPatch) -> Result<&PartChange<Vec<FailureRule>>, GraphEditError> {
    only_part(patch, ProjectPart::FailureRules)?;
    patch
        .project_parts()
        .failure_rules()
        .ok_or_else(|| not_changed(ProjectPart::FailureRules))
}

pub(crate) fn validate_add_failure_rule(
    patch: &GraphPatch,
    index: usize,
) -> Result<(), GraphEditError> {
    let change = failure_rules(patch)?;
    let (before, after) = (change.before(), change.after());
    let added =
        after.len() == before.len() + 1 && index == before.len() && after[..index] == *before;
    if !added {
        return Err(not_changed(ProjectPart::FailureRules));
    }
    if adds_only(patch, &rule_nodes(&after[index])) {
        Ok(())
    } else {
        Err(GraphEditError::UnrelatedChanges)
    }
}

pub(crate) fn validate_remove_failure_rule(
    patch: &GraphPatch,
    index: usize,
) -> Result<(), GraphEditError> {
    let change = failure_rules(patch)?;
    let (before, after) = (change.before(), change.after());
    let Some(removed) = before.get(index) else {
        return Err(not_changed(ProjectPart::FailureRules));
    };
    if before.len() != after.len() + 1
        || before[..index] != after[..index]
        || before[index + 1..] != after[index..]
    {
        return Err(not_changed(ProjectPart::FailureRules));
    }
    // Only the generated item the removed rule owns retires with it.
    let owned = match &removed.iteration {
        FailureIteration::Sequence { sequence } => sequence.owned_items().into_iter().collect(),
        FailureIteration::Source { .. } | FailureIteration::OwnedSource { .. } => BTreeSet::new(),
    };
    if patch
        .nodes()
        .all(|(id, change)| change.after().is_none() && owned.contains(&id))
    {
        Ok(())
    } else {
        Err(GraphEditError::UnrelatedChanges)
    }
}

pub(crate) fn validate_move_failure_rule(
    patch: &GraphPatch,
    from: usize,
    to: usize,
) -> Result<(), GraphEditError> {
    let change = failure_rules(patch)?;
    let mut swapped = change.before().clone();
    let moved = from.abs_diff(to) == 1 && from.max(to) < swapped.len() && {
        swapped.swap(from, to);
        swapped == *change.after()
    };
    if !moved {
        return Err(not_changed(ProjectPart::FailureRules));
    }
    if patch.nodes().next().is_none() {
        Ok(())
    } else {
        Err(GraphEditError::UnrelatedChanges)
    }
}

pub(crate) fn validate_edit_failure_rule(
    patch: &GraphPatch,
    index: usize,
) -> Result<(), GraphEditError> {
    let change = failure_rules(patch)?;
    let (before, after) = (change.before(), change.after());
    let edited = before.len() == after.len()
        && index < before.len()
        && before
            .iter()
            .zip(after)
            .enumerate()
            .all(|(position, (before, after))| (position == index) != (before == after));
    if !edited {
        return Err(not_changed(ProjectPart::FailureRules));
    }
    // An edit may add only an expression the edited rule now reads.
    if adds_only(patch, &rule_nodes(&after[index])) {
        Ok(())
    } else {
        Err(GraphEditError::UnrelatedChanges)
    }
}

/// How a list of named items changes: one appended item, one removed item,
/// or one item replaced in place, each named before and after.
pub(crate) enum NamedEdit<'a> {
    Add(&'a str),
    Remove(&'a str),
    Replace { from: &'a str, to: &'a str },
}

impl NamedEdit<'_> {
    fn describes<T: PartialEq>(
        &self,
        before: &[T],
        after: &[T],
        name: impl Fn(&T) -> &str,
    ) -> bool {
        match *self {
            Self::Add(added) => {
                after.len() == before.len() + 1
                    && after[..before.len()] == *before
                    && after.last().is_some_and(|item| name(item) == added)
            }
            Self::Remove(removed) => {
                let mut named = before
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| name(item) == removed);
                match (named.next(), named.next()) {
                    (Some((index, _)), None) => {
                        before.len() == after.len() + 1
                            && before[..index] == after[..index]
                            && before[index + 1..] == after[index..]
                    }
                    _ => false,
                }
            }
            Self::Replace { from, to } => {
                let mut differing = before.iter().zip(after).filter(|(b, a)| b != a);
                before.len() == after.len()
                    && differing
                        .next()
                        .is_some_and(|(b, a)| name(b) == from && name(a) == to)
                    && differing.next().is_none()
            }
        }
    }
}

pub(crate) fn validate_named_sources(
    patch: &GraphPatch,
    edit: NamedEdit<'_>,
) -> Result<(), GraphEditError> {
    only_part(patch, ProjectPart::NamedSources)?;
    let described = patch.project_parts().named_sources().is_some_and(|change| {
        edit.describes(change.before(), change.after(), |source| &source.name)
    });
    if !described {
        return Err(not_changed(ProjectPart::NamedSources));
    }
    if patch.nodes().next().is_none() {
        Ok(())
    } else {
        Err(GraphEditError::UnrelatedChanges)
    }
}

pub(crate) fn validate_named_targets(
    patch: &GraphPatch,
    edit: NamedEdit<'_>,
) -> Result<(), GraphEditError> {
    only_part(patch, ProjectPart::NamedTargets)?;
    let described = patch.project_parts().named_targets().is_some_and(|change| {
        edit.describes(change.before(), change.after(), |target| &target.name)
    });
    if !described {
        return Err(not_changed(ProjectPart::NamedTargets));
    }
    if patch.nodes().next().is_none() {
        Ok(())
    } else {
        Err(GraphEditError::UnrelatedChanges)
    }
}

pub(crate) fn validate_source_options(patch: &GraphPatch) -> Result<(), GraphEditError> {
    only_part(patch, ProjectPart::SourceOptions)?;
    if patch.nodes().next().is_none() {
        Ok(())
    } else {
        Err(GraphEditError::UnrelatedChanges)
    }
}

pub(crate) fn validate_create_function(
    patch: &GraphPatch,
    function: FunctionId,
) -> Result<(), GraphEditError> {
    let part = ProjectPart::Function(function);
    only_part(patch, part)?;
    let created = patch
        .project_parts()
        .function(function)
        .is_some_and(|change| change.before().is_none() && change.after().is_some());
    if !created {
        return Err(not_changed(part));
    }
    if patch.nodes().next().is_none() {
        Ok(())
    } else {
        Err(GraphEditError::UnrelatedChanges)
    }
}

/// `scope` without its bindings and child scopes.
fn shallow(scope: &Scope) -> Scope {
    Scope {
        bindings: Vec::new(),
        children: Vec::new(),
        ..scope.clone()
    }
}

/// The bindings an empty scope created for new bindings holds, counting
/// those of the empty scopes created inside it.
fn new_bound_scope(scope: &Scope) -> Option<usize> {
    if shallow(scope) != empty_scope(&scope.target_field) {
        return None;
    }
    scope
        .children
        .iter()
        .try_fold(scope.bindings.len(), |count, child| {
            Some(count + new_bound_scope(child)?)
        })
}

/// The number of bindings `after` appends to `before`, when it keeps every
/// scope in order, only appends bindings to them, and adds only scopes
/// created for new bindings.
fn appended_bindings(before: &[Scope], after: &[Scope]) -> Option<usize> {
    let mut kept = before.iter().peekable();
    let mut appended = 0;
    for scope in after {
        appended += match kept.next_if(|kept| kept.target_field == scope.target_field) {
            Some(kept) => {
                if shallow(kept) != shallow(scope) || !scope.bindings.starts_with(&kept.bindings) {
                    return None;
                }
                scope.bindings.len() - kept.bindings.len()
                    + appended_bindings(&kept.children, &scope.children)?
            }
            None => new_bound_scope(scope)?,
        };
    }
    kept.next().is_none().then_some(appended)
}

pub(crate) fn validate_auto_connect(
    patch: &GraphPatch,
    scope: &ScopeLocation,
    connected: usize,
) -> Result<(), GraphEditError> {
    let within = |location: &ScopeLocation| location == scope || location.is_below(scope);
    if !patch.project_parts().is_empty()
        || patch.settings().next().is_some()
        || patch.scopes().any(|(location, _)| !within(location))
        || patch.children().any(|(location, _)| !within(location))
    {
        return Err(GraphEditError::UnrelatedChanges);
    }
    // Only source field reads are added.
    if !patch.nodes().all(|(_, change)| {
        change.before().is_none() && matches!(change.after(), Some(Node::SourceField { .. }))
    }) {
        return Err(GraphEditError::UnrelatedChanges);
    }
    let appended = patch
        .scopes()
        .map(|(_, change)| {
            change
                .after()
                .starts_with(change.before())
                .then(|| change.after().len() - change.before().len())
        })
        .chain(
            patch
                .children()
                .map(|(_, change)| appended_bindings(change.before(), change.after())),
        )
        .try_fold(0, |total, count| Some(total + count?));
    if connected > 0 && appended == Some(connected) {
        Ok(())
    } else {
        Err(GraphEditError::BindingNotChanged {
            location: scope.clone(),
        })
    }
}
