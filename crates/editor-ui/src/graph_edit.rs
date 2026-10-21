//! Typed, invertible graph edits keyed by node and scope identity.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use mapping::{Binding, FunctionId, Graph, Node, NodeId, Project, Scope};

use crate::{EditorCommand, EditorTransaction, TargetId};

/// The graph and target scope tree that one mapping canvas edits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GraphDocument {
    /// The shared project graph with one target's root scope.
    Target(TargetId),
    /// One user-function body, which has no target scope.
    Function(FunctionId),
}

/// One step from a scope to a nested scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeStep {
    Child(usize),
    /// One segment of a concatenated iteration.
    Segment(usize),
    DynamicChild(usize),
}

/// A scope addressed structurally from its root scope.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopeLocation(Vec<ScopeStep>);

impl ScopeLocation {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn steps(&self) -> &[ScopeStep] {
        &self.0
    }

    fn join(&self, step: ScopeStep) -> Self {
        let mut steps = self.0.clone();
        steps.push(step);
        Self(steps)
    }

    fn resolve<'s>(&self, root: &'s Scope) -> Option<&'s Scope> {
        self.0.iter().try_fold(root, |scope, step| match *step {
            ScopeStep::Child(index) => scope.children.get(index),
            ScopeStep::Segment(index) => scope.concatenated()?.iter().nth(index),
            ScopeStep::DynamicChild(index) => {
                scope.dynamic_children.get(index).map(|child| &child.scope)
            }
        })
    }

    fn resolve_mut<'s>(&self, root: &'s mut Scope) -> Option<&'s mut Scope> {
        self.0.iter().try_fold(root, |scope, step| match *step {
            ScopeStep::Child(index) => scope.children.get_mut(index),
            ScopeStep::Segment(index) => scope.concatenated_mut()?.iter_mut().nth(index),
            ScopeStep::DynamicChild(index) => scope
                .dynamic_children
                .get_mut(index)
                .map(|child| &mut child.scope),
        })
    }
}

/// One node identity on both sides of an edit; `None` means absent.
#[derive(Clone, Debug)]
pub struct NodeChange {
    before: Option<Node>,
    after: Option<Node>,
}

impl NodeChange {
    pub fn before(&self) -> Option<&Node> {
        self.before.as_ref()
    }

    pub fn after(&self) -> Option<&Node> {
        self.after.as_ref()
    }
}

/// The ordered binding list of one scope on both sides of an edit.
#[derive(Clone, Debug)]
pub struct BindingsChange {
    before: Vec<Binding>,
    after: Vec<Binding>,
}

impl BindingsChange {
    pub fn before(&self) -> &[Binding] {
        &self.before
    }

    pub fn after(&self) -> &[Binding] {
        &self.after
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphEditError {
    /// The scope trees differ in shape, which a binding patch cannot express.
    ScopeShapeChanged {
        location: ScopeLocation,
    },
    /// The edited target or function no longer exists.
    MissingDocument(GraphDocument),
    /// A node's presence differs from the state the edit replaces.
    NodePresence {
        node: NodeId,
        expected: bool,
    },
    MissingScope {
        location: ScopeLocation,
    },
    /// A scope's bindings differ from the state the edit replaces.
    BindingsDiffer {
        location: ScopeLocation,
    },
    /// Binding changes were addressed to a document without a target scope.
    NoTargetScope,
    /// A requested node is not removed by the edit.
    NotRemoved {
        node: NodeId,
    },
    EmptyRemoval,
}

impl fmt::Display for GraphEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScopeShapeChanged { location } => {
                write!(f, "scope {location:?} changed shape during a graph edit")
            }
            Self::MissingDocument(document) => write!(f, "{document:?} no longer exists"),
            Self::NodePresence { node, expected } => {
                let state = if *expected { "present" } else { "absent" };
                write!(f, "mapping node {node} is not {state} as the edit requires")
            }
            Self::MissingScope { location } => write!(f, "scope {location:?} no longer exists"),
            Self::BindingsDiffer { location } => {
                write!(f, "bindings of scope {location:?} changed since the edit")
            }
            Self::NoTargetScope => f.write_str("function graphs have no target bindings"),
            Self::NotRemoved { node } => write!(f, "mapping node {node} is not removed"),
            Self::EmptyRemoval => f.write_str("a node removal must remove at least one node"),
        }
    }
}

impl std::error::Error for GraphEditError {}

/// Node and binding differences of one graph edit, keyed by identity.
///
/// Applying or reverting validates every affected identity before changing
/// anything, so a stale patch fails without partial effects.
#[derive(Clone, Debug, Default)]
pub struct GraphPatch {
    nodes: BTreeMap<NodeId, NodeChange>,
    bindings: BTreeMap<ScopeLocation, BindingsChange>,
}

impl GraphPatch {
    /// Captures node and binding differences between two states of one graph
    /// and scope tree. `same_node` decides node content equality; scope trees
    /// must keep their shape. Other scope fields are not captured, so hosts
    /// that derive a patch from arbitrary edits must verify that applying it
    /// reproduces the edited state.
    pub fn between(
        before: (&Graph, &Scope),
        after: (&Graph, &Scope),
        same_node: impl Fn(&Node, &Node) -> bool,
    ) -> Result<Self, GraphEditError> {
        let ids = before
            .0
            .nodes
            .keys()
            .chain(after.0.nodes.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        let nodes = ids
            .into_iter()
            .filter_map(|id| {
                let change = NodeChange {
                    before: before.0.nodes.get(&id).cloned(),
                    after: after.0.nodes.get(&id).cloned(),
                };
                let unchanged = match (&change.before, &change.after) {
                    (Some(before), Some(after)) => same_node(before, after),
                    (before, after) => before.is_none() && after.is_none(),
                };
                (!unchanged).then_some((id, change))
            })
            .collect();
        let mut bindings = BTreeMap::new();
        diff_scopes(&ScopeLocation::root(), before.1, after.1, &mut bindings)?;
        Ok(Self { nodes, bindings })
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.bindings.is_empty()
    }

    /// Every node identity the edit adds, removes, or changes.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &NodeChange)> {
        self.nodes.iter().map(|(&id, change)| (id, change))
    }

    pub fn removed_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes_where(|change| change.before.is_some() && change.after.is_none())
    }

    pub fn added_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes_where(|change| change.before.is_none() && change.after.is_some())
    }

    pub fn changed_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes_where(|change| change.before.is_some() && change.after.is_some())
    }

    /// Every scope whose binding list the edit replaces.
    pub fn scopes(&self) -> impl Iterator<Item = (&ScopeLocation, &BindingsChange)> {
        self.bindings.iter()
    }

    pub fn apply_to(
        &self,
        graph: &mut Graph,
        scope: Option<&mut Scope>,
    ) -> Result<(), GraphEditError> {
        self.exchange(graph, scope, Side::After)
    }

    pub fn revert_on(
        &self,
        graph: &mut Graph,
        scope: Option<&mut Scope>,
    ) -> Result<(), GraphEditError> {
        self.exchange(graph, scope, Side::Before)
    }

    fn nodes_where(&self, keep: fn(&NodeChange) -> bool) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes
            .iter()
            .filter(move |(_, change)| keep(change))
            .map(|(&id, _)| id)
    }

    fn exchange(
        &self,
        graph: &mut Graph,
        scope: Option<&mut Scope>,
        target: Side,
    ) -> Result<(), GraphEditError> {
        for (&node, change) in &self.nodes {
            let expected = target.replaced_node(change).is_some();
            if graph.nodes.contains_key(&node) != expected {
                return Err(GraphEditError::NodePresence { node, expected });
            }
        }
        if !self.bindings.is_empty() {
            let root = scope.as_deref().ok_or(GraphEditError::NoTargetScope)?;
            for (location, change) in &self.bindings {
                let current =
                    location
                        .resolve(root)
                        .ok_or_else(|| GraphEditError::MissingScope {
                            location: location.clone(),
                        })?;
                if !same_bindings(&current.bindings, target.replaced_bindings(change)) {
                    return Err(GraphEditError::BindingsDiffer {
                        location: location.clone(),
                    });
                }
            }
        }

        // Every identity was validated above; the mutations below cannot fail.
        for (&node, change) in &self.nodes {
            match target.node(change) {
                Some(content) => graph.nodes.insert(node, content.clone()),
                None => graph.nodes.remove(&node),
            };
        }
        if let Some(root) = scope {
            for (location, change) in &self.bindings {
                if let Some(current) = location.resolve_mut(root) {
                    current.bindings = target.bindings(change).to_vec();
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Side {
    Before,
    After,
}

impl Side {
    fn node(self, change: &NodeChange) -> Option<&Node> {
        match self {
            Self::Before => change.before.as_ref(),
            Self::After => change.after.as_ref(),
        }
    }

    fn replaced_node(self, change: &NodeChange) -> Option<&Node> {
        self.opposite().node(change)
    }

    fn bindings(self, change: &BindingsChange) -> &[Binding] {
        match self {
            Self::Before => &change.before,
            Self::After => &change.after,
        }
    }

    fn replaced_bindings(self, change: &BindingsChange) -> &[Binding] {
        self.opposite().bindings(change)
    }

    fn opposite(self) -> Self {
        match self {
            Self::Before => Self::After,
            Self::After => Self::Before,
        }
    }
}

fn same_bindings(left: &[Binding], right: &[Binding]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.target_field == right.target_field && left.node == right.node)
}

fn diff_scopes(
    location: &ScopeLocation,
    before: &Scope,
    after: &Scope,
    changes: &mut BTreeMap<ScopeLocation, BindingsChange>,
) -> Result<(), GraphEditError> {
    if !same_bindings(&before.bindings, &after.bindings) {
        changes.insert(
            location.clone(),
            BindingsChange {
                before: before.bindings.clone(),
                after: after.bindings.clone(),
            },
        );
    }
    let shape_changed = || GraphEditError::ScopeShapeChanged {
        location: location.clone(),
    };
    if before.children.len() != after.children.len()
        || before.dynamic_children.len() != after.dynamic_children.len()
    {
        return Err(shape_changed());
    }
    for (index, (before, after)) in before.children.iter().zip(&after.children).enumerate() {
        diff_scopes(
            &location.join(ScopeStep::Child(index)),
            before,
            after,
            changes,
        )?;
    }
    match (before.concatenated(), after.concatenated()) {
        (None, None) => {}
        (Some(before), Some(after)) if before.len() == after.len() => {
            for (index, (before, after)) in before.iter().zip(after.iter()).enumerate() {
                diff_scopes(
                    &location.join(ScopeStep::Segment(index)),
                    before,
                    after,
                    changes,
                )?;
            }
        }
        _ => return Err(shape_changed()),
    }
    for (index, (before, after)) in before
        .dynamic_children
        .iter()
        .zip(&after.dynamic_children)
        .enumerate()
    {
        diff_scopes(
            &location.join(ScopeStep::DynamicChild(index)),
            &before.scope,
            &after.scope,
            changes,
        )?;
    }
    Ok(())
}

/// Removal of graph nodes from one document together with every dependent
/// change the removal implies: consumer inputs rewired to hidden unconnected
/// nodes, cleared raise messages, dropped target bindings, and orphaned
/// inputs.
#[derive(Clone, Debug)]
pub struct GraphNodeRemoval {
    document: GraphDocument,
    requested: BTreeSet<NodeId>,
    patch: GraphPatch,
}

impl GraphNodeRemoval {
    pub fn new(
        document: GraphDocument,
        requested: BTreeSet<NodeId>,
        patch: GraphPatch,
    ) -> Result<Self, GraphEditError> {
        if requested.is_empty() {
            return Err(GraphEditError::EmptyRemoval);
        }
        if let Some(&node) = requested.iter().find(|node| {
            !patch
                .nodes
                .get(node)
                .is_some_and(|change| change.before.is_some() && change.after.is_none())
        }) {
            return Err(GraphEditError::NotRemoved { node });
        }
        Ok(Self {
            document,
            requested,
            patch,
        })
    }

    pub fn document(&self) -> &GraphDocument {
        &self.document
    }

    /// The nodes the user asked to remove; [`Self::patch`] also lists the
    /// dependent identities.
    pub fn requested(&self) -> &BTreeSet<NodeId> {
        &self.requested
    }

    pub fn patch(&self) -> &GraphPatch {
        &self.patch
    }

    pub fn label(&self) -> &'static str {
        if self.requested.len() == 1 {
            "Remove node"
        } else {
            "Remove nodes"
        }
    }

    pub fn apply_to(&self, project: &mut Project) -> Result<(), GraphEditError> {
        let (graph, scope) = document_parts(project, &self.document)?;
        self.patch.apply_to(graph, scope)
    }

    pub fn revert_on(&self, project: &mut Project) -> Result<(), GraphEditError> {
        let (graph, scope) = document_parts(project, &self.document)?;
        self.patch.revert_on(graph, scope)
    }
}

impl EditorCommand for GraphNodeRemoval {
    type Error = GraphEditError;

    fn label(&self) -> &str {
        Self::label(self)
    }

    fn apply(self, transaction: &mut EditorTransaction<'_>) -> Result<(), Self::Error> {
        self.apply_to(transaction.project_mut())
    }
}

fn document_parts<'p>(
    project: &'p mut Project,
    document: &GraphDocument,
) -> Result<(&'p mut Graph, Option<&'p mut Scope>), GraphEditError> {
    let missing = || GraphEditError::MissingDocument(document.clone());
    match document {
        GraphDocument::Target(TargetId::Primary) => {
            Ok((&mut project.graph, Some(&mut project.root)))
        }
        GraphDocument::Target(TargetId::Named(name)) => {
            let target = project
                .extra_targets
                .iter_mut()
                .find(|target| target.name == *name)
                .ok_or_else(missing)?;
            Ok((&mut project.graph, Some(&mut target.root)))
        }
        GraphDocument::Function(id) => {
            let function = project.user_functions.get_mut(id).ok_or_else(missing)?;
            Ok((&mut function.body, None))
        }
    }
}

#[cfg(test)]
#[path = "graph_edit_tests.rs"]
mod tests;
