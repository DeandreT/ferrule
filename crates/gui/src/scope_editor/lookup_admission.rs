//! The same receiving-context gate for every Inspector expression assignment.
use egui::Ui;
use mapping::{Graph, NodeId, Scope};
use std::collections::{BTreeMap, BTreeSet};

use crate::path_picker::SourcePathCatalog;

pub(crate) const READ_ONLY: &str = "This saved scope uses an owned Lookup in an unsupported context. Its expressions are retained read-only.";
const REJECTED: &str =
    "This change would place an owned Lookup in an unsupported context. The scope is unchanged.";

#[derive(Clone, Copy, Default)]
pub(super) struct Policy<'a> {
    paths: Option<&'a SourcePathCatalog>,
    receiving: bool,
}

impl<'a> Policy<'a> {
    pub(super) fn for_scope(paths: &'a SourcePathCatalog, scope: &Scope, ancestors: bool) -> Self {
        Self {
            paths: Some(paths),
            receiving: ancestors && paths.lookup_binding_supported(scope, &[]),
        }
    }

    pub(super) fn allows(self, graph: &Graph, node: NodeId) -> bool {
        crate::source_ownership::lookup_assignment_allowed(graph, node, self.receiving, |lookup| {
            self.paths
                .is_some_and(|paths| paths.lookup_editable(graph, lookup))
        })
    }

    fn scope_allowed(self, graph: &Graph, scope: &Scope) -> bool {
        let mut allowed = true;
        scope.visit_roots(&mut |root| allowed &= self.allows(graph, root.node));
        allowed
    }
}

fn rejected_assignments(
    graph: &Graph,
    scope: &Scope,
    paths: &SourcePathCatalog,
    ancestors: bool,
) -> BTreeSet<(Vec<mapping::NestedScopeKind>, mapping::ScopeRootSlot)> {
    let mut rejected = BTreeSet::new();
    let mut pending = vec![(scope, Vec::new(), ancestors)];
    while let Some((scope, path, ancestors)) = pending.pop() {
        let policy = Policy::for_scope(paths, scope, ancestors);
        scope.visit_roots(&mut |root| {
            if !policy.allows(graph, root.node) {
                rejected.insert((path.clone(), root.slot));
            }
        });
        for (kind, child) in scope.nested_scopes() {
            let mut child_path = path.clone();
            child_path.push(kind);
            pending.push((child, child_path, policy.receiving));
        }
    }
    rejected
}

#[derive(Clone, PartialEq, Eq)]
struct GroupingContext {
    iteration: mapping::ScopeIteration,
    construction: mapping::ScopeConstruction,
    output: mapping::IterationOutput,
    keys: [Option<NodeId>; 5],
}

impl GroupingContext {
    fn of(scope: &Scope) -> Self {
        Self {
            iteration: scope.iteration.clone(),
            construction: scope.construction.clone(),
            output: scope.iteration_output,
            keys: [
                scope.group_by,
                scope.group_adjacent_by,
                scope.group_starting_with,
                scope.group_ending_with,
                scope.group_into_blocks,
            ],
        }
    }
}

/// A Lookup already under a grouped descendant is already a rejected picker
/// slot. Its full grouping ancestry must still be retained: adding a group at
/// its parent promotes that entire parent's subtree, including legacy siblings.
fn grouped_lookup_contexts(
    graph: &Graph,
    scope: &Scope,
) -> BTreeMap<(Vec<mapping::NestedScopeKind>, mapping::ScopeRootSlot), Vec<GroupingContext>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![(scope, Vec::new(), Vec::new(), false)];
    while let Some((scope, path, mut contexts, grouped)) = pending.pop() {
        let grouped = grouped || scope.has_grouping();
        contexts.push(GroupingContext::of(scope));
        if grouped {
            scope.visit_roots(&mut |root| {
                if !crate::source_ownership::lookup_dependencies(graph, root.node).is_empty() {
                    found.insert((path.clone(), root.slot), contexts.clone());
                }
            });
        }
        for (kind, child) in scope.nested_scopes() {
            let mut path = path.clone();
            path.push(kind);
            pending.push((child, path, contexts.clone(), grouped));
        }
    }
    found
}

/// One prospective edit boundary shared by control defaults and committed edits.
/// Existing rejected slots may stay unchanged; grouped Lookup ancestry may not
/// change merely because those slots were rejected before the edit as well.
pub(super) fn permits_change(
    graph: &Graph,
    before: &Scope,
    after: &Scope,
    paths: &SourcePathCatalog,
    ancestors: bool,
) -> bool {
    rejected_assignments(graph, after, paths, ancestors)
        .is_subset(&rejected_assignments(graph, before, paths, ancestors))
        && grouped_lookup_contexts(graph, before) == grouped_lookup_contexts(graph, after)
}

/// Retains imported invalid metadata; rejects only edits introducing a new
/// invalid receiving context. No whole-project validation or repair is involved.
pub(crate) fn edit<R: Default>(
    ui: &mut Ui,
    scope: &mut Scope,
    graph: &Graph,
    paths: &SourcePathCatalog,
    ancestors: bool,
    show: impl FnOnce(&mut Ui, &mut Scope) -> R,
) -> R {
    let editable = Policy::for_scope(paths, scope, ancestors).scope_allowed(graph, scope);
    if !editable {
        let mut display = scope.clone();
        ui.add_enabled_ui(false, |ui| show(ui, &mut display));
        if ui.is_enabled() {
            ui.weak(READ_ONLY);
        }
        return R::default();
    }
    let before = crate::graph_viewer::frame_has_edit_input(ui.ctx()).then(|| scope.clone());
    let result = show(ui, scope);
    if let Some(before) = before
        && !permits_change(graph, &before, scope, paths, ancestors)
    {
        *scope = before;
        ui.weak(REJECTED);
        return R::default();
    }
    result
}
