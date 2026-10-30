//! Absolute source frames in which graph nodes are evaluated.
//!
//! Aggregate and lookup collections are relative to the current source item.
//! They usually match one source port by suffix, but sources that repeat one
//! structure in several places, such as XLSX grids on several worksheets,
//! need the frame of the scope that evaluates the node to choose the port.

use std::collections::BTreeMap;

use mapping::{NodeId, NodeInputRole, Project, Scope, ScopeIteration};

pub(super) struct NodeFrames(BTreeMap<NodeId, Option<Vec<String>>>);

impl NodeFrames {
    pub(super) fn new(project: &Project) -> Self {
        let mut frames = Self(BTreeMap::new());
        frames.visit(project, &project.root, Some(Vec::new()));
        frames
    }

    /// The one absolute frame `node` is evaluated in, when every scope that
    /// reaches it shares that frame.
    pub(super) fn frame(&self, node: NodeId) -> Option<&[String]> {
        self.0.get(&node)?.as_deref()
    }

    fn visit(&mut self, project: &Project, scope: &Scope, parent: Option<Vec<String>>) {
        let frame = match &scope.iteration {
            ScopeIteration::Source(path)
            | ScopeIteration::DynamicDocuments { source: path, .. } => parent.map(|mut frame| {
                frame.extend(path.iter().cloned());
                frame
            }),
            ScopeIteration::None | ScopeIteration::OptionalPresence { .. } => parent,
            _ => None,
        };
        let roots = scope
            .bindings
            .iter()
            .map(|binding| binding.node)
            .chain(scope.filter);
        for node in roots {
            self.record(project, node, frame.as_ref());
        }
        for child in &scope.children {
            self.visit(project, child, frame.clone());
        }
    }

    /// Records `frame` for `node` and the inputs it evaluates in the same
    /// context. A node reached from different or unknown frames has none.
    fn record(&mut self, project: &Project, node: NodeId, frame: Option<&Vec<String>>) {
        let mut pending = vec![node];
        while let Some(node) = pending.pop() {
            match self.0.get(&node) {
                Some(None) => continue,
                Some(Some(known)) if Some(known) == frame => continue,
                Some(Some(_)) => {
                    self.0.insert(node, None);
                }
                None => {
                    self.0.insert(node, frame.cloned());
                }
            }
            if let Some(graph_node) = project.graph.nodes.get(&node) {
                pending.extend(graph_node.inputs_where(|input| {
                    matches!(input.role, NodeInputRole::Parent | NodeInputRole::Lazy)
                }));
            }
        }
    }
}
