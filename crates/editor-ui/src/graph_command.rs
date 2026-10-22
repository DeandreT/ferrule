//! Graph edit commands: one user action on one graph document, the
//! identities it affects, and the patch that applies or inverts it.

use std::collections::BTreeSet;
use std::fmt;

use mapping::{Graph, Node, NodeId, Project, Scope};

use crate::{
    EditorCommand, EditorTransaction, GraphDocument, GraphEditError, GraphPatch, ScopeLocation,
    TargetId,
};

/// The receiving end of one wire.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WireSink {
    /// One input slot of a graph node, numbered as its canvas pins are.
    NodeInput { node: NodeId, input: usize },
    /// The binding of one target field in one scope.
    Binding { scope: ScopeLocation, field: String },
}

impl fmt::Display for WireSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NodeInput { node, input } => write!(f, "input {input} of mapping node {node}"),
            Self::Binding { scope, field } => {
                write!(f, "target field {field} in scope {:?}", scope.steps())
            }
        }
    }
}

/// A position on a mapping canvas, in canvas coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasPosition {
    pub x: f32,
    pub y: f32,
}

/// The user action a [`GraphCommand`] performs, named by the identities it
/// targets. Dependent changes, such as orphaned inputs a removal or a
/// replaced wire drops, appear only in the command's patch.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphAction {
    /// Removes the requested nodes.
    RemoveNodes { requested: BTreeSet<NodeId> },
    /// Feeds `source` into `sink`, replacing any previous value.
    ConnectWire { source: NodeId, sink: WireSink },
    /// Leaves `sink` without a wired value.
    DisconnectWire { sink: WireSink },
    /// Adds `node`, with any helper nodes it owns, placed at `position`.
    InsertNode {
        node: NodeId,
        position: CanvasPosition,
    },
}

impl GraphAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::RemoveNodes { requested } if requested.len() == 1 => "Remove node",
            Self::RemoveNodes { .. } => "Remove nodes",
            Self::ConnectWire { .. } => "Connect wire",
            Self::DisconnectWire { .. } => "Disconnect wire",
            Self::InsertNode { .. } => "Insert node",
        }
    }

    /// Checks that `patch` performs this action on its named identities.
    fn validate(&self, patch: &GraphPatch) -> Result<(), GraphEditError> {
        match self {
            Self::RemoveNodes { requested } => {
                if requested.is_empty() {
                    return Err(GraphEditError::EmptyRemoval);
                }
                match requested.iter().find(|&&node| {
                    !patch
                        .node(node)
                        .is_some_and(|change| change.before().is_some() && change.after().is_none())
                }) {
                    Some(&node) => Err(GraphEditError::NotRemoved { node }),
                    None => Ok(()),
                }
            }
            Self::ConnectWire { source, sink } => {
                let source_present = patch
                    .node(*source)
                    .is_none_or(|change| change.after().is_some());
                if source_present && feeds(patch, sink, *source) {
                    Ok(())
                } else {
                    Err(GraphEditError::NotConnected {
                        source: *source,
                        sink: sink.clone(),
                    })
                }
            }
            Self::DisconnectWire { sink } => {
                let disconnected = match sink {
                    WireSink::NodeInput { node, .. } => patch.node(*node).is_some_and(|change| {
                        change.before().is_some() && change.after().is_some()
                    }),
                    WireSink::Binding { scope, field } => {
                        patch.scope(scope).is_some_and(|change| {
                            binding_of(change.before(), field).is_some()
                                && binding_of(change.after(), field).is_none()
                        })
                    }
                };
                if disconnected {
                    Ok(())
                } else {
                    Err(GraphEditError::NotDisconnected { sink: sink.clone() })
                }
            }
            Self::InsertNode { node, .. } => {
                if !patch
                    .node(*node)
                    .is_some_and(|change| change.before().is_none() && change.after().is_some())
                {
                    return Err(GraphEditError::NotInserted { node: *node });
                }
                let additive = patch.scopes().next().is_none()
                    && patch.nodes().all(|(_, change)| change.before().is_none());
                if additive {
                    Ok(())
                } else {
                    Err(GraphEditError::InsertionNotAdditive)
                }
            }
        }
    }
}

/// Whether the patch makes `sink` read `source`: a changed node input whose
/// new content depends on `source`, or a binding newly pointing at it. Hosts
/// verify the exact input slot, which only their canvases number.
fn feeds(patch: &GraphPatch, sink: &WireSink, source: NodeId) -> bool {
    match sink {
        WireSink::NodeInput { node, .. } => patch.node(*node).is_some_and(|change| {
            change.before().is_some()
                && change
                    .after()
                    .is_some_and(|after| after.dependencies().contains(&source))
        }),
        WireSink::Binding { scope, field } => patch.scope(scope).is_some_and(|change| {
            binding_of(change.after(), field) == Some(source)
                && binding_of(change.before(), field) != Some(source)
        }),
    }
}

fn binding_of(bindings: &[mapping::Binding], field: &str) -> Option<NodeId> {
    bindings
        .iter()
        .find(|binding| binding.target_field == field)
        .map(|binding| binding.node)
}

/// One graph edit command: a validated action on one document and the
/// patch that applies or inverts it atomically.
#[derive(Clone, Debug)]
pub struct GraphCommand {
    document: GraphDocument,
    action: GraphAction,
    patch: GraphPatch,
}

impl GraphCommand {
    /// Pairs an action with the patch that performs it, rejecting patches
    /// that do not change the action's identities as it describes.
    pub fn new(
        document: GraphDocument,
        action: GraphAction,
        patch: GraphPatch,
    ) -> Result<Self, GraphEditError> {
        action.validate(&patch)?;
        Ok(Self {
            document,
            action,
            patch,
        })
    }

    pub fn document(&self) -> &GraphDocument {
        &self.document
    }

    pub fn action(&self) -> &GraphAction {
        &self.action
    }

    pub fn patch(&self) -> &GraphPatch {
        &self.patch
    }

    pub fn label(&self) -> &'static str {
        self.action.label()
    }

    pub fn apply_to(&self, project: &mut Project) -> Result<(), GraphEditError> {
        let (graph, scope) = document_parts(project, &self.document)?;
        self.patch.apply_to(graph, scope)
    }

    pub fn revert_on(&self, project: &mut Project) -> Result<(), GraphEditError> {
        let (graph, scope) = document_parts(project, &self.document)?;
        self.patch.revert_on(graph, scope)
    }

    /// Applies the command only if every node it replaces still matches its
    /// recorded content under `same_node`.
    pub fn apply_to_matching(
        &self,
        project: &mut Project,
        same_node: impl Fn(&Node, &Node) -> bool,
    ) -> Result<(), GraphEditError> {
        let (graph, scope) = document_parts(project, &self.document)?;
        self.patch.apply_to_matching(graph, scope, same_node)
    }

    /// Reverts the command with the content check of
    /// [`Self::apply_to_matching`].
    pub fn revert_on_matching(
        &self,
        project: &mut Project,
        same_node: impl Fn(&Node, &Node) -> bool,
    ) -> Result<(), GraphEditError> {
        let (graph, scope) = document_parts(project, &self.document)?;
        self.patch.revert_on_matching(graph, scope, same_node)
    }
}

impl EditorCommand for GraphCommand {
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
#[path = "graph_command_tests.rs"]
mod tests;
