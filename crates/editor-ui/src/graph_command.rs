//! Graph edit commands: one user action on one graph document, the
//! identities it affects, and the patch that applies or inverts it.

use std::collections::BTreeSet;
use std::fmt;

use mapping::{DynamicBinding, Graph, JoinId, Node, NodeId, Project, Scope};

use crate::graph_edit::same_bindings;
use crate::scope_command::{self, ListEdit};
use crate::{
    EditorCommand, EditorTransaction, GraphDocument, GraphEditError, GraphPatch, ScopeLocation,
    ScopeSetting, ScopeSettingValue, TargetId,
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

/// The kind of node whose properties one edit changes, named in the action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeProperty {
    Constant,
    RuntimeParameter,
    /// Ordinary and owner-qualified item positions.
    Position,
    FunctionCall,
    Aggregate,
    /// Ordinary and primary-root source field paths.
    SourceField,
    ValueMap,
    Other,
}

impl NodeProperty {
    pub fn of(node: &Node) -> Self {
        match node {
            Node::Const { .. } => Self::Constant,
            Node::RuntimeParameter { .. } | Node::RuntimeParameterDefault { .. } => {
                Self::RuntimeParameter
            }
            Node::Position { .. } | Node::OwnedPosition(_) => Self::Position,
            Node::Call { .. } => Self::FunctionCall,
            Node::Aggregate { .. } => Self::Aggregate,
            Node::SourceField { .. } | Node::SourceRootField { .. } => Self::SourceField,
            Node::ValueMap { .. } => Self::ValueMap,
            _ => Self::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Constant => "Edit constant",
            Self::RuntimeParameter => "Edit runtime parameter",
            Self::Position => "Edit position",
            Self::FunctionCall => "Edit function call",
            Self::Aggregate => "Edit aggregate",
            Self::SourceField => "Edit source field",
            Self::ValueMap => "Edit value map",
            Self::Other => "Edit node properties",
        }
    }
}

/// One target binding: a target field and the node it reads.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetBinding {
    pub field: String,
    pub node: NodeId,
}

impl TargetBinding {
    pub fn of(binding: &mapping::Binding) -> Self {
        Self {
            field: binding.target_field.clone(),
            node: binding.node,
        }
    }

    fn matches(&self, binding: &mapping::Binding) -> bool {
        binding.target_field == self.field && binding.node == self.node
    }
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
    /// Changes the properties of `node`, adding or retiring only the hidden
    /// inputs that change requires.
    EditNode {
        node: NodeId,
        property: NodeProperty,
    },
    /// Appends `binding` to the bindings of `scope`.
    AddBinding {
        scope: ScopeLocation,
        binding: TargetBinding,
    },
    /// Removes `binding` from `scope`, keeping the order of the others.
    RemoveBinding {
        scope: ScopeLocation,
        binding: TargetBinding,
    },
    /// Replaces the target field or node of one binding in place.
    RetargetBinding {
        scope: ScopeLocation,
        from: TargetBinding,
        to: TargetBinding,
    },
    /// Changes one setting of `scope`. Computed properties and iterations
    /// other than source-backed ones are changed by their own actions.
    EditScope {
        scope: ScopeLocation,
        setting: ScopeSetting,
    },
    /// Appends an empty static child scope for target field `field` to the
    /// child scopes of `parent`.
    AddScope {
        parent: ScopeLocation,
        field: String,
    },
    /// Adds empty child scopes below `scope`, keeping every existing scope.
    ExpandScope { scope: ScopeLocation },
    /// Removes the static child scope at `scope`, with the generated items
    /// its scopes own.
    RemoveScope { scope: ScopeLocation },
    /// Appends `property` to the computed properties of `scope`.
    AddComputedProperty {
        scope: ScopeLocation,
        property: DynamicBinding,
    },
    /// Removes `property` from `scope`, keeping the order of the others.
    RemoveComputedProperty {
        scope: ScopeLocation,
        property: DynamicBinding,
    },
    /// Replaces the name or value expression of one computed property.
    EditComputedProperty {
        scope: ScopeLocation,
        from: DynamicBinding,
        to: DynamicBinding,
    },
    /// Makes the non-iterating `scope` iterate the new join `join`.
    CreateJoin { scope: ScopeLocation, join: JoinId },
    /// Changes the equality conditions of `join`, keeping its inputs.
    EditJoinKeys { scope: ScopeLocation, join: JoinId },
    /// Binds a new read of `join` to one target field of `scope`.
    AddJoinedOutput {
        scope: ScopeLocation,
        join: JoinId,
        binding: TargetBinding,
    },
    /// Makes `scope` iterate a new filter/map sequence, adding only the
    /// bound and item nodes that sequence owns.
    UseFilterMap { scope: ScopeLocation },
    /// Changes the stages of the filter/map sequence `scope` iterates.
    EditFilterMap { scope: ScopeLocation },
    /// Binds the XML type of `scope` to the new declared-type constant `node`.
    SetXmlType { scope: ScopeLocation, node: NodeId },
    /// Removes every XML type binding of `scope`, leaving inference to
    /// choose the type.
    InferXmlType { scope: ScopeLocation },
    /// Adds `duplicate`, a copy of the sequence consumer `original`, with
    /// the private expressions it owns.
    DuplicateSequenceConsumer { original: NodeId, duplicate: NodeId },
}

impl GraphAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::RemoveNodes { requested } if requested.len() == 1 => "Remove node",
            Self::RemoveNodes { .. } => "Remove nodes",
            Self::ConnectWire { .. } => "Connect wire",
            Self::DisconnectWire { .. } => "Disconnect wire",
            Self::InsertNode { .. } => "Insert node",
            Self::EditNode { property, .. } => property.label(),
            Self::AddBinding { .. } => "Add binding",
            Self::RemoveBinding { .. } => "Remove binding",
            Self::RetargetBinding { .. } => "Retarget binding",
            Self::EditScope { setting, .. } => setting.label(),
            Self::AddScope { .. } => "Add child scope",
            Self::ExpandScope { .. } => "Expand scope subtree",
            Self::RemoveScope { .. } => "Remove scope",
            Self::AddComputedProperty { .. } => "Add computed property",
            Self::RemoveComputedProperty { .. } => "Remove computed property",
            Self::EditComputedProperty { .. } => "Edit computed property",
            Self::CreateJoin { .. } => "Create join",
            Self::EditJoinKeys { .. } => "Edit join keys",
            Self::AddJoinedOutput { .. } => "Add joined output",
            Self::UseFilterMap { .. } => "Use filter/map",
            Self::EditFilterMap { .. } => "Edit filter/map",
            Self::SetXmlType { .. } => "Set XML type",
            Self::InferXmlType { .. } => "Use XML type inference",
            Self::DuplicateSequenceConsumer { .. } => "Duplicate sequence consumer",
        }
    }

    /// Whether consecutive commands performing this same action, such as
    /// keystrokes in one property field, may form one history step.
    pub fn coalesces(&self) -> bool {
        matches!(
            self,
            Self::EditNode { .. } | Self::EditScope { .. } | Self::EditFilterMap { .. }
        )
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
                let additive = patch.only_nodes()
                    && patch.nodes().all(|(_, change)| change.before().is_none());
                if additive {
                    Ok(())
                } else {
                    Err(GraphEditError::InsertionNotAdditive)
                }
            }
            Self::EditNode { node, property } => {
                let edited = patch.node(*node).is_some_and(|change| {
                    change
                        .before()
                        .zip(change.after())
                        .is_some_and(|(before, after)| {
                            NodeProperty::of(before) == *property
                                || NodeProperty::of(after) == *property
                        })
                });
                if !edited {
                    return Err(GraphEditError::NodeNotEdited { node: *node });
                }
                // Only hidden inputs the edit adds or retires may change beside it.
                let local = patch.only_nodes()
                    && patch.nodes().all(|(id, change)| {
                        id == *node || change.before().is_none() || change.after().is_none()
                    });
                if local {
                    Ok(())
                } else {
                    Err(GraphEditError::UnrelatedChanges)
                }
            }
            Self::AddBinding { scope, binding } => validate_binding_edit(patch, scope, |b, a| {
                a.len() == b.len() + 1
                    && same_bindings(b, &a[..b.len()])
                    && a.last().is_some_and(|last| binding.matches(last))
            }),
            Self::RemoveBinding { scope, binding } => {
                validate_binding_edit(patch, scope, |b, a| {
                    b.len() == a.len() + 1
                        && (0..b.len()).any(|index| {
                            binding.matches(&b[index])
                                && same_bindings(&b[..index], &a[..index])
                                && same_bindings(&b[index + 1..], &a[index..])
                        })
                })
            }
            Self::RetargetBinding { scope, from, to } => {
                validate_binding_edit(patch, scope, |b, a| {
                    let mut differing = b.iter().zip(a).filter(|(b, a)| {
                        !same_bindings(std::slice::from_ref(b), std::slice::from_ref(a))
                    });
                    from != to
                        && b.len() == a.len()
                        && differing
                            .next()
                            .is_some_and(|(b, a)| from.matches(b) && to.matches(a))
                        && differing.next().is_none()
                })
            }
            Self::EditScope { scope, setting } => {
                let not_changed = || GraphEditError::SettingNotChanged {
                    location: scope.clone(),
                    setting: *setting,
                };
                let change = patch.setting(scope, *setting).ok_or_else(not_changed)?;
                let named_elsewhere = |value: &ScopeSettingValue| match value {
                    ScopeSettingValue::Iteration(iteration) => !iteration.is_source_backed(),
                    ScopeSettingValue::ComputedProperties(_) => true,
                    _ => false,
                };
                if named_elsewhere(change.before()) || named_elsewhere(change.after()) {
                    return Err(not_changed());
                }
                if patch.nodes().next().is_none()
                    && patch.scopes().next().is_none()
                    && patch.children().next().is_none()
                    && patch.settings().count() == 1
                {
                    Ok(())
                } else {
                    Err(GraphEditError::UnrelatedChanges)
                }
            }
            Self::AddScope { parent, field } => {
                scope_command::validate_add_scope(patch, parent, field)
            }
            Self::ExpandScope { scope } => scope_command::validate_expand_scope(patch, scope),
            Self::RemoveScope { scope } => scope_command::validate_remove_scope(patch, scope),
            Self::AddComputedProperty { scope, property } => {
                scope_command::validate_computed_properties(patch, scope, ListEdit::Add(property))
            }
            Self::RemoveComputedProperty { scope, property } => {
                scope_command::validate_computed_properties(
                    patch,
                    scope,
                    ListEdit::Remove(property),
                )
            }
            Self::EditComputedProperty { scope, from, to } => {
                scope_command::validate_computed_properties(
                    patch,
                    scope,
                    ListEdit::Replace { from, to },
                )
            }
            Self::CreateJoin { scope, join } => {
                scope_command::validate_create_join(patch, scope, *join)
            }
            Self::EditJoinKeys { scope, join } => {
                scope_command::validate_join_keys(patch, scope, *join)
            }
            Self::AddJoinedOutput {
                scope,
                join,
                binding,
            } => scope_command::validate_joined_output(patch, scope, *join, binding),
            Self::UseFilterMap { scope } => scope_command::validate_use_filter_map(patch, scope),
            Self::EditFilterMap { scope } => scope_command::validate_edit_filter_map(patch, scope),
            Self::SetXmlType { scope, node } => {
                scope_command::validate_set_xml_type(patch, scope, *node)
            }
            Self::InferXmlType { scope } => scope_command::validate_infer_xml_type(patch, scope),
            Self::DuplicateSequenceConsumer {
                original,
                duplicate,
            } => scope_command::validate_duplicate(patch, *original, *duplicate),
        }
    }
}

/// Checks that the patch changes only the bindings of `scope`, and as
/// `expected` describes from their old and new lists.
fn validate_binding_edit(
    patch: &GraphPatch,
    scope: &ScopeLocation,
    expected: impl Fn(&[mapping::Binding], &[mapping::Binding]) -> bool,
) -> Result<(), GraphEditError> {
    let Some(change) = patch.scope(scope) else {
        return Err(GraphEditError::BindingNotChanged {
            location: scope.clone(),
        });
    };
    if patch.nodes().next().is_some()
        || patch.settings().next().is_some()
        || patch.children().next().is_some()
        || patch.scopes().count() != 1
    {
        return Err(GraphEditError::UnrelatedChanges);
    }
    if expected(change.before(), change.after()) {
        Ok(())
    } else {
        Err(GraphEditError::BindingNotChanged {
            location: scope.clone(),
        })
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

    /// The one command equivalent to this command followed by `next`, when
    /// both perform the same coalescing action on one document and the
    /// combined patch still performs it.
    pub fn then(&self, next: &Self, same_node: impl Fn(&Node, &Node) -> bool) -> Option<Self> {
        if self.document != next.document || self.action != next.action || !self.action.coalesces()
        {
            return None;
        }
        let patch = self.patch.then(&next.patch, same_node)?;
        Self::new(self.document.clone(), self.action.clone(), patch).ok()
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
