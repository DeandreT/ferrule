use std::collections::BTreeMap;

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

/// Builds the scope tree from iteration and binding connections. `anchors`
/// remembers, per scope chain, the absolute source path its iteration
/// starts from, so nested iterations can be expressed relative to it.
#[derive(Clone)]
pub(super) struct ScopeBuilder {
    pub(super) root: Scope,
    pub(super) anchors: BTreeMap<Vec<String>, Vec<String>>,
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

    pub(super) fn add_binding(&mut self, target: TargetLeaf, node: NodeId) {
        let scope = self.ensure_scope(&target.chain);
        if scope
            .bindings
            .iter()
            .any(|binding| binding.target_field == target.field && binding.node == node)
        {
            return;
        }
        scope.bindings.push(Binding {
            target_field: target.field,
            node,
        });
    }
}
