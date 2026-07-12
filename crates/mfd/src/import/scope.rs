use std::collections::BTreeMap;

use mapping::{Binding, NodeId, Scope, SequenceExpr};

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
}

/// Builds the scope tree from iteration and binding connections. `anchors`
/// remembers, per scope chain, the absolute source path its iteration
/// starts from, so nested iterations can be expressed relative to it.
pub(super) struct ScopeBuilder {
    pub(super) root: Scope,
    pub(super) anchors: BTreeMap<Vec<String>, Vec<String>>,
}

pub(super) struct IterationNodes {
    pub(super) filter: Option<NodeId>,
    pub(super) group_by: Option<NodeId>,
    pub(super) group_into_blocks: Option<NodeId>,
    pub(super) sort_by: Option<NodeId>,
    pub(super) sort_descending: bool,
    pub(super) take: Option<NodeId>,
}

impl ScopeBuilder {
    fn ensure_scope(&mut self, chain: &[String]) -> &mut Scope {
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
    fn enclosing_anchor(&self, chain: &[String]) -> Vec<String> {
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
        scope.source = Some(relative);
        scope.filter = nodes.filter;
        scope.group_by = nodes.group_by;
        scope.group_into_blocks = nodes.group_into_blocks;
        scope.sort_by = nodes.sort_by;
        scope.sort_descending = nodes.sort_descending;
        scope.take = nodes.take;
    }

    pub(super) fn add_sequence(
        &mut self,
        target_path: &[String],
        sequence: SequenceExpr,
        nodes: IterationNodes,
    ) {
        let scope = self.ensure_scope(target_path);
        scope.sequence = Some(sequence);
        scope.filter = nodes.filter;
        scope.group_by = nodes.group_by;
        scope.group_into_blocks = nodes.group_into_blocks;
        scope.sort_by = nodes.sort_by;
        scope.sort_descending = nodes.sort_descending;
        scope.take = nodes.take;
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
