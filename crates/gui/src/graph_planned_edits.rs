//! Canvas edits re-applied through typed patches, so hosting documents can
//! record them as commands instead of snapshots.

use editor_ui::{GraphAction, GraphPatch, ScopeLocation, WireSink};
use mapping::{Graph, Node, NodeId, Scope};

use super::GraphViewer;
use crate::project_state::model_key;

/// One edit a canvas applied through its typed patch. The hosting document
/// supplies the graph identity when recording it.
#[derive(Clone, Debug)]
pub(crate) struct PlannedGraphEdit {
    pub(crate) action: GraphAction,
    pub(crate) patch: GraphPatch,
}

impl GraphViewer<'_> {
    /// Runs one in-place canvas edit. When `action` names the edit from the
    /// edited state, the edit is re-applied through its typed patch and kept
    /// for the hosting document's history.
    pub(super) fn plan_edit<T>(
        &mut self,
        edit: impl FnOnce(&mut Self) -> T,
        action: impl FnOnce(&Self, &T) -> Option<GraphAction>,
    ) -> T {
        // Planning clones only on an explicit edit action, never per frame.
        let graph = self.graph.clone();
        let scope = self.root_scope.clone();
        let result = edit(self);
        let Some(action) = action(self, &result) else {
            return result;
        };
        let edited_graph = std::mem::replace(self.graph, graph);
        let edited_scope = std::mem::replace(self.root_scope, scope);
        if let Some(patch) = self.adopt_planned_state(edited_graph, edited_scope)
            && !patch.is_empty()
        {
            self.planned_edits.push(PlannedGraphEdit { action, patch });
        }
        result
    }

    /// Adopts a planned graph and scope state through its typed patch,
    /// verifying that the patch reproduces the plan exactly. A plan that a
    /// patch cannot reproduce is adopted directly and left to snapshot
    /// history.
    pub(super) fn adopt_planned_state(&mut self, graph: Graph, scope: Scope) -> Option<GraphPatch> {
        let same_node = |left: &Node, right: &Node| model_key(left) == model_key(right);
        let patch = GraphPatch::between(
            (&*self.graph, &*self.root_scope),
            (&graph, &scope),
            same_node,
        );
        if let Ok(patch) = patch
            && patch
                .apply_to(self.graph, Some(&mut *self.root_scope))
                .is_ok()
            && model_key(&*self.graph) == model_key(&graph)
            && model_key(&*self.root_scope) == model_key(&scope)
        {
            return Some(patch);
        }
        *self.graph = graph;
        *self.root_scope = scope;
        None
    }

    /// The binding sink for a target field reached through a static chain.
    pub(super) fn binding_sink(&self, chain: &[String], field: &str) -> Option<WireSink> {
        Some(WireSink::Binding {
            scope: ScopeLocation::of_child_chain(self.root_scope, chain)?,
            field: field.to_owned(),
        })
    }

    /// The value `sink` currently reads, at the exact slot the canvas shows.
    pub(super) fn sink_value(&self, sink: &WireSink) -> Option<NodeId> {
        match sink {
            WireSink::NodeInput { node, input } => self.input_at(*node, *input),
            WireSink::Binding { scope, field } => {
                let mut current = &*self.root_scope;
                for step in scope.steps() {
                    let editor_ui::ScopeStep::Child(index) = *step else {
                        return None;
                    };
                    current = current.children.get(index)?;
                }
                current
                    .bindings
                    .iter()
                    .find(|binding| binding.target_field == *field)
                    .map(|binding| binding.node)
            }
        }
    }

    /// Whether `sink` reads no wired value: no binding, no optional input,
    /// or a hidden unconnected input.
    pub(super) fn sink_disconnected(&self, sink: &WireSink) -> bool {
        self.sink_value(sink)
            .is_none_or(|value| matches!(self.graph.nodes.get(&value), Some(Node::Unconnected)))
    }
}
