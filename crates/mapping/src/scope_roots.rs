//! Root expressions owned by scopes and failure rules, each with the context
//! it evaluates in.
//!
//! A scope evaluates its roots in several distinct contexts: once in the
//! enclosing context before any item exists, once per candidate or grouped
//! member, once per produced item, or only after another root selects an
//! item. Analyses that need only reachability can ignore the role; analyses
//! that reason about available frames must not merge roles, and none of them
//! can recover evaluation order from this visit order.

use crate::{
    FailureIteration, FailureRule, NodeId, NodeInputRole, NodeInputSlot, Scope, ScopeConstruction,
    SequenceExpr,
};

/// Where and how often a root expression is evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeRootRole {
    /// Once in the context enclosing the scope or rule, before any item
    /// exists: presence conditions, window bounds, block sizes, and
    /// generated-sequence inputs.
    Parent,
    /// Once per candidate item, before grouping and windows: filters, sort
    /// keys, item-wise grouping keys, and failure predicates.
    Item,
    /// Once per grouped member, after partitioning.
    GroupMember,
    /// Once per produced item: bindings, scalar values, output paths, tree
    /// roots, and the property names of dynamic child scopes.
    Output,
    /// Once per visited group of a recursive filter.
    RecursiveGroup,
    /// The private item identity of a generated sequence. It is readable only
    /// in that sequence's item context and is never evaluated outside it.
    GeneratedItem,
    /// Only after another root has selected an item, as a failure message.
    Lazy,
}

/// Which field of a scope or failure rule owns a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeRootSlot {
    Filter,
    PostGroupFilter,
    GroupBy,
    GroupAdjacentBy,
    GroupStartingWith,
    GroupEndingWith,
    GroupIntoBlocks,
    SortBy,
    SortThenBy(usize),
    OutputPath,
    /// The condition of an optional-presence iteration.
    Presence,
    /// A bound of the window at this index, in [`crate::SequenceWindow::nodes`] order.
    Window(usize),
    Binding(usize),
    DynamicBindingKey(usize),
    DynamicBindingValue(usize),
    ScalarValue,
    RecursiveFilterPredicate,
    AdjacencyTreeRoot,
    SequenceInput,
    /// The generated item identity returned by [`SequenceExpr::item`].
    SequenceItem,
    /// A reserved source item that a composed sequence owns besides its item.
    SequenceReservedItem,
    DynamicChildKey(usize),
    FailurePredicate,
    FailureMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopeRoot {
    pub node: NodeId,
    pub slot: ScopeRootSlot,
    pub role: ScopeRootRole,
}

/// How a nested scope is reached from its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NestedScopeKind {
    /// One ordered segment of a concatenated iteration.
    Segment(usize),
    /// A static child scope.
    Child(usize),
    /// A child scope stored under a computed property name.
    DynamicChild(usize),
}

impl Scope {
    /// Visits this scope's own root expressions in a fixed slot order. Nested
    /// scopes are not visited; see [`Scope::nested_scopes`].
    ///
    /// Analyses that stop at their first finding or report findings in order
    /// see roots in this order, so it is part of their precedence: controls,
    /// then the output path, presence and windows, secondary sort keys,
    /// bindings, construction, the sequence, and dynamic child keys.
    pub fn visit_roots(&self, visit: &mut impl FnMut(ScopeRoot)) {
        let mut root = |node, slot, role| visit(ScopeRoot { node, slot, role });
        use ScopeRootRole::{GroupMember, Item, Output, Parent, RecursiveGroup};
        use ScopeRootSlot as Slot;
        if let Some(node) = self.filter {
            root(node, Slot::Filter, Item);
        }
        if let Some(node) = self.post_group_filter {
            root(node, Slot::PostGroupFilter, GroupMember);
        }
        for (node, slot) in [
            (self.group_by, Slot::GroupBy),
            (self.group_adjacent_by, Slot::GroupAdjacentBy),
            (self.group_starting_with, Slot::GroupStartingWith),
            (self.group_ending_with, Slot::GroupEndingWith),
        ] {
            if let Some(node) = node {
                root(node, slot, Item);
            }
        }
        // A block size is an item count evaluated before partitioning.
        if let Some(node) = self.group_into_blocks {
            root(node, Slot::GroupIntoBlocks, Parent);
        }
        if let Some(node) = self.sort_by {
            root(node, Slot::SortBy, Item);
        }
        if let Some(node) = self.output_path() {
            root(node, Slot::OutputPath, Output);
        }
        if let Some(node) = self.presence() {
            root(node, Slot::Presence, Parent);
        }
        for (index, window) in self.windows.iter().copied().enumerate() {
            for node in window.nodes() {
                root(node, Slot::Window(index), Parent);
            }
        }
        for (index, key) in self.sort_then_by.iter().enumerate() {
            root(key.node, Slot::SortThenBy(index), Item);
        }
        for (index, binding) in self.bindings.iter().enumerate() {
            root(binding.node, Slot::Binding(index), Output);
        }
        for (index, binding) in self.dynamic_bindings.iter().enumerate() {
            root(binding.key, Slot::DynamicBindingKey(index), Output);
            root(binding.value, Slot::DynamicBindingValue(index), Output);
        }
        match &self.construction {
            ScopeConstruction::Scalar { value } => root(*value, Slot::ScalarValue, Output),
            ScopeConstruction::RecursiveFilter { plan } => root(
                plan.predicate(),
                Slot::RecursiveFilterPredicate,
                RecursiveGroup,
            ),
            ScopeConstruction::AdjacencyTree { plan } => {
                if let Some(node) = plan.root() {
                    root(node, Slot::AdjacencyTreeRoot, Output);
                }
            }
            ScopeConstruction::OwnedAdjacencyTree { plan } => {
                if let Some(node) = plan.root() {
                    root(node, Slot::AdjacencyTreeRoot, Output);
                }
            }
            ScopeConstruction::Constructed
            | ScopeConstruction::CopyCurrentSource
            | ScopeConstruction::XmlMixedContent { .. }
            | ScopeConstruction::PathHierarchy { .. }
            | ScopeConstruction::OwnedPathHierarchy { .. } => {}
        }
        if let Some(sequence) = self.sequence() {
            visit_sequence_roots(sequence, &mut root);
        }
        for (index, child) in self.dynamic_children.iter().enumerate() {
            root(child.key, Slot::DynamicChildKey(index), Output);
        }
    }

    /// Scopes evaluated as part of this scope: concatenated segments, static
    /// children, then dynamic children.
    pub fn nested_scopes(&self) -> impl Iterator<Item = (NestedScopeKind, &Scope)> {
        self.concatenated()
            .into_iter()
            .flat_map(|segments| segments.iter().enumerate())
            .map(|(index, scope)| (NestedScopeKind::Segment(index), scope))
            .chain(
                self.children
                    .iter()
                    .enumerate()
                    .map(|(index, scope)| (NestedScopeKind::Child(index), scope)),
            )
            .chain(
                self.dynamic_children
                    .iter()
                    .enumerate()
                    .map(|(index, child)| (NestedScopeKind::DynamicChild(index), &child.scope)),
            )
    }

    /// Visits this scope's roots, then every nested scope's roots, depth first.
    pub fn visit_tree_roots(&self, visit: &mut impl FnMut(ScopeRoot)) {
        self.visit_roots(visit);
        for (_, nested) in self.nested_scopes() {
            nested.visit_tree_roots(visit);
        }
    }
}

impl FailureRule {
    /// Visits this rule's predicate, message, and generated-sequence roots.
    pub fn visit_roots(&self, visit: &mut impl FnMut(ScopeRoot)) {
        let mut root = |node, slot, role| visit(ScopeRoot { node, slot, role });
        if let FailureIteration::Sequence { sequence } = &self.iteration {
            visit_sequence_roots(sequence, &mut root);
        }
        if let Some(node) = self.selection.predicate() {
            root(node, ScopeRootSlot::FailurePredicate, ScopeRootRole::Item);
        }
        if let Some(node) = self.message {
            root(node, ScopeRootSlot::FailureMessage, ScopeRootRole::Lazy);
        }
    }
}

fn visit_sequence_roots(
    sequence: &SequenceExpr,
    root: &mut impl FnMut(NodeId, ScopeRootSlot, ScopeRootRole),
) {
    // Inputs at every composition level, then the outermost level's item
    // identities, as in `SequenceExpr::owned_items`.
    sequence.visit_level_inputs(&mut |declared| {
        let slot = match declared.input.slot {
            NodeInputSlot::SequenceInput(_) => ScopeRootSlot::SequenceInput,
            NodeInputSlot::SequenceReservedItem if declared.level == 0 => {
                ScopeRootSlot::SequenceReservedItem
            }
            NodeInputSlot::SequenceItem if declared.level == 0 => ScopeRootSlot::SequenceItem,
            _ => return,
        };
        let role = match declared.input.role {
            NodeInputRole::Parent => ScopeRootRole::Parent,
            _ => ScopeRootRole::GeneratedItem,
        };
        root(declared.input.node, slot, role);
    });
}

/// Every executable root of a project: each target's scope tree, dynamic
/// source paths, and failure rules.
pub(crate) fn project_roots(project: &crate::Project) -> Vec<NodeId> {
    let mut roots = Vec::new();
    let mut push = |root: ScopeRoot| roots.push(root.node);
    project.root.visit_tree_roots(&mut push);
    for target in &project.extra_targets {
        target.root.visit_tree_roots(&mut push);
    }
    roots.extend(
        project
            .extra_sources
            .iter()
            .filter_map(|source| source.dynamic_path.as_ref())
            .map(|path| path.node),
    );
    for rule in &project.failure_rules {
        rule.visit_roots(&mut |root| roots.push(root.node));
    }
    roots
}

#[cfg(test)]
#[path = "tests/scope_roots.rs"]
mod tests;
