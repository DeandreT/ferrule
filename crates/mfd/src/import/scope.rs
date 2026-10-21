use std::collections::{BTreeMap, BTreeSet};

use super::schema::SchemaComponent;
use mapping::{
    Binding, IterationOutput, JoinId, JoinPlan, NodeId, Scope, ScopeConstruction, ScopeIteration,
    ScopeSequence, SequenceExpr, SequenceWindow, SortFilterOrder, SortKey,
};

#[derive(Clone)]
pub(super) struct TargetLeaf {
    chain: Vec<String>,
    field: String,
}

impl TargetLeaf {
    pub(super) fn from_path(path: &[String]) -> Option<Self> {
        let (field, chain) = path.split_last()?;
        Some(Self {
            chain: chain.to_vec(),
            field: field.clone(),
        })
    }

    pub(super) fn path(&self) -> Vec<String> {
        let mut path = self.chain.clone();
        path.push(self.field.clone());
        path
    }

    pub(super) fn chain(&self) -> &[String] {
        &self.chain
    }
}

/// The authored occurrence a scope binding stands for. Producer nodes are
/// interned, so duplicated target entries fed by one expression share a
/// node; only their authored input keys tell them apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum BindingOccurrence {
    /// A target entry declared next to same-named sibling entries of one
    /// parent entry occurrence. Each such input keeps its own binding;
    /// installing one input again is idempotent.
    Entry { parent: Vec<u32>, input: u32 },
    /// Any other binding, including entries whose duplicated ancestors import
    /// merges into one scope (type alternatives): identical `(field, node)`
    /// bindings collapse into one.
    Expression,
}

impl BindingOccurrence {
    /// The occurrence of an authored target input. An entry is a declared
    /// duplicate when its own marker, the last of its ancestors, owns exactly
    /// the input's target path; the remaining ancestors identify its parent.
    pub(super) fn authored(target: &SchemaComponent, input: u32) -> Self {
        let duplicate = target
            .input_ancestors
            .get(&input)
            .and_then(|ancestors| ancestors.split_last())
            .filter(|(marker, _)| {
                !target.input_keys.contains(marker)
                    && target.ports.get(&input).is_some_and(|path| {
                        target
                            .input_branch_paths
                            .iter()
                            .any(|(branch, owned)| branch == *marker && owned == path)
                    })
            });
        match duplicate {
            Some((_, parent)) => Self::Entry {
                parent: parent.to_vec(),
                input,
            },
            None => Self::Expression,
        }
    }
}

/// Authored duplicate entries installed for one target path and node, per
/// parent entry occurrence.
type Occurrences = BTreeMap<(Vec<String>, NodeId), BTreeMap<Vec<u32>, BTreeSet<u32>>>;

/// Builds the scope tree from iteration and binding connections. `anchors`
/// remembers, per scope chain, the absolute source path its iteration
/// starts from, so nested iterations can be expressed relative to it.
#[derive(Clone)]
pub(super) struct ScopeBuilder {
    pub(super) root: Scope,
    pub(super) anchors: BTreeMap<Vec<String>, Vec<String>>,
    /// `Scope` carries no occurrence identity, so this is what keeps
    /// duplicated entries fed by one expression apart.
    occurrences: Occurrences,
}

#[derive(Default)]
pub(super) struct IterationNodes {
    pub(super) filter: Option<NodeId>,
    pub(super) post_group_filter: Option<NodeId>,
    pub(super) group_by: Option<NodeId>,
    pub(super) group_starting_with: Option<NodeId>,
    pub(super) group_adjacent_by: Option<NodeId>,
    pub(super) group_ending_with: Option<NodeId>,
    pub(super) group_into_blocks: Option<NodeId>,
    pub(super) sort_by: Option<NodeId>,
    pub(super) sort_descending: bool,
    pub(super) sort_then_by: Vec<SortKey>,
    pub(super) sort_filter_order: SortFilterOrder,
    pub(super) windows: Vec<SequenceWindow>,
}

impl ScopeBuilder {
    pub(super) fn new(anchors: BTreeMap<Vec<String>, Vec<String>>) -> Self {
        Self {
            root: Scope::default(),
            anchors,
            occurrences: BTreeMap::new(),
        }
    }

    pub(super) fn scope(&self, chain: &[String]) -> Option<&Scope> {
        let mut scope = &self.root;
        for element in chain {
            scope = scope
                .children
                .iter()
                .find(|child| child.target_field == *element)?;
        }
        Some(scope)
    }

    pub(super) fn ensure_scope(&mut self, chain: &[String]) -> &mut Scope {
        let mut scope = &mut self.root;
        for element in chain {
            let idx = match scope
                .children
                .iter()
                .position(|c| c.target_field == *element)
            {
                Some(idx) => idx,
                None => {
                    scope.children.push(Scope {
                        target_field: element.clone(),
                        ..Scope::default()
                    });
                    scope.children.len() - 1
                }
            };
            scope = &mut scope.children[idx];
        }
        scope
    }

    /// The nearest enclosing anchor for a chain, if any iteration exists
    /// above it.
    pub(super) fn enclosing_anchor(&self, chain: &[String]) -> Vec<String> {
        for len in (0..chain.len()).rev() {
            if let Some(anchor) = self.anchors.get(&chain[..len]) {
                return anchor.clone();
            }
        }
        Vec::new()
    }

    pub(super) fn add_iteration(
        &mut self,
        target_path: &[String],
        source_abs: &[String],
        nodes: IterationNodes,
        output: IterationOutput,
    ) {
        let anchor = self.enclosing_anchor(target_path);
        let relative: Vec<String> = if source_abs.starts_with(&anchor) {
            source_abs[anchor.len()..].to_vec()
        } else {
            source_abs.to_vec()
        };
        self.anchors
            .insert(target_path.to_vec(), source_abs.to_vec());
        let scope = self.ensure_scope(target_path);
        scope.set_source(Some(relative));
        scope.filter = nodes.filter;
        scope.post_group_filter = nodes.post_group_filter;
        scope.group_by = nodes.group_by;
        scope.group_starting_with = nodes.group_starting_with;
        scope.group_adjacent_by = nodes.group_adjacent_by;
        scope.group_ending_with = nodes.group_ending_with;
        scope.group_into_blocks = nodes.group_into_blocks;
        scope.sort_by = nodes.sort_by;
        scope.sort_descending = nodes.sort_descending;
        scope.sort_then_by = nodes.sort_then_by;
        scope.sort_filter_order = nodes.sort_filter_order;
        scope.windows = nodes.windows;
        scope.iteration_output = output;
    }

    pub(super) fn add_copy_iteration(&mut self, target_path: &[String], source_abs: &[String]) {
        self.add_iteration(
            target_path,
            source_abs,
            IterationNodes::default(),
            IterationOutput::Repeated,
        );
        self.ensure_scope(target_path).construction = ScopeConstruction::CopyCurrentSource;
    }

    pub(super) fn add_sequence(
        &mut self,
        target_path: &[String],
        sequence: SequenceExpr,
        nodes: IterationNodes,
        output: IterationOutput,
    ) {
        let scope = self.ensure_scope(target_path);
        scope.set_sequence(Some(sequence));
        scope.filter = nodes.filter;
        scope.post_group_filter = nodes.post_group_filter;
        scope.group_by = nodes.group_by;
        scope.group_starting_with = nodes.group_starting_with;
        scope.group_adjacent_by = nodes.group_adjacent_by;
        scope.group_ending_with = nodes.group_ending_with;
        scope.group_into_blocks = nodes.group_into_blocks;
        scope.sort_by = nodes.sort_by;
        scope.sort_descending = nodes.sort_descending;
        scope.sort_then_by = nodes.sort_then_by;
        scope.sort_filter_order = nodes.sort_filter_order;
        scope.windows = nodes.windows;
        scope.iteration_output = output;
    }

    pub(super) fn add_join(
        &mut self,
        target_path: &[String],
        id: JoinId,
        plan: JoinPlan,
        nodes: IterationNodes,
        output: IterationOutput,
    ) {
        let scope = self.ensure_scope(target_path);
        scope.iteration = ScopeIteration::InnerJoin { id, plan };
        scope.filter = nodes.filter;
        scope.post_group_filter = nodes.post_group_filter;
        scope.sort_by = nodes.sort_by;
        scope.sort_descending = nodes.sort_descending;
        scope.sort_then_by = nodes.sort_then_by;
        scope.sort_filter_order = nodes.sort_filter_order;
        scope.windows = nodes.windows;
        scope.iteration_output = output;
    }

    pub(super) fn add_concatenated(
        &mut self,
        target_path: &[String],
        first: Scope,
        rest: Vec<Scope>,
        output: IterationOutput,
    ) {
        let scope = self.ensure_scope(target_path);
        scope.iteration = ScopeIteration::Concatenate(ScopeSequence::new(first, rest));
        scope.iteration_output = output;
    }

    /// Installs one binding, canonical by authored occurrence rather than by
    /// expression: each duplicate entry of one parent occurrence keeps its
    /// own binding even when an identical `(field, node)` binding exists,
    /// while any other binding, or a repeated install of one input, reuses an
    /// existing one.
    pub(super) fn add_binding(
        &mut self,
        target: TargetLeaf,
        node: NodeId,
        occurrence: BindingOccurrence,
    ) {
        let required = match occurrence {
            BindingOccurrence::Entry { parent, input } => {
                let parents = self.occurrences.entry((target.path(), node)).or_default();
                if parents.entry(parent).or_default().insert(input) {
                    parents.values().map(BTreeSet::len).max().unwrap_or(1)
                } else {
                    1
                }
            }
            BindingOccurrence::Expression => 1,
        };
        let scope = self.ensure_scope(&target.chain);
        let existing = scope
            .bindings
            .iter()
            .filter(|binding| binding.target_field == target.field && binding.node == node)
            .count();
        if existing >= required {
            return;
        }
        scope.bindings.push(Binding {
            target_field: target.field,
            node,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf() -> TargetLeaf {
        TargetLeaf {
            chain: vec!["Row".into()],
            field: "Value".into(),
        }
    }

    fn entry(parent: &[u32], input: u32) -> BindingOccurrence {
        BindingOccurrence::Entry {
            parent: parent.to_vec(),
            input,
        }
    }

    fn installed(installs: impl IntoIterator<Item = (NodeId, BindingOccurrence)>) -> Vec<NodeId> {
        let mut scopes = ScopeBuilder::new(BTreeMap::new());
        for (node, occurrence) in installs {
            scopes.add_binding(leaf(), node, occurrence);
        }
        scopes
            .scope(&["Row".into()])
            .map(|scope| scope.bindings.iter().map(|binding| binding.node).collect())
            .unwrap_or_default()
    }

    #[test]
    fn one_authored_input_installed_twice_remains_one_binding() {
        assert_eq!(
            installed([(7, entry(&[30], 31)), (7, entry(&[30], 31))]),
            [7]
        );
        assert_eq!(
            installed([(7, BindingOccurrence::Expression), (7, entry(&[30], 31))]),
            [7]
        );
    }

    #[test]
    fn sibling_entries_sharing_one_node_keep_one_binding_each() {
        assert_eq!(
            installed([
                (7, entry(&[30], 31)),
                (8, entry(&[30], 32)),
                (7, entry(&[30], 33)),
                (7, entry(&[30], 31)),
                (7, BindingOccurrence::Expression),
            ]),
            [7, 8, 7]
        );
    }

    #[test]
    fn entries_of_distinct_parent_occurrences_keep_expression_canonicalization() {
        assert_eq!(
            installed([(7, entry(&[21], 22)), (7, entry(&[24], 25))]),
            [7]
        );
        assert_eq!(
            installed([
                (7, BindingOccurrence::Expression),
                (7, BindingOccurrence::Expression)
            ]),
            [7]
        );
    }
}
