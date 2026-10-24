use std::collections::BTreeSet;

use editor_ui::{
    CanvasPosition, GraphAction, GraphCommand, GraphDocument, GraphPatch, NodeProperty,
    ProjectParts, ScopeFields, ScopeLocation, ScopeSetting, TargetBinding, TargetId, WireSink,
};
use mapping::{Node, Project, Scope, ScopeConstruction};

use crate::{BindingRef, EditError, EditOperation, Expression, NodeId, ScopeRef, Target};

mod controls;
mod failure;
mod removal;

/// Plan without modifying the caller's project. A multi-command operation is
/// checked in order against one working model before any command is returned.
pub fn plan(project: &Project, operation: &EditOperation) -> Result<Vec<GraphCommand>, EditError> {
    let mut planner = Planner {
        project: project.clone(),
        commands: Vec::new(),
    };
    planner.run(operation)?;
    Ok(planner.commands)
}

struct Planner {
    project: Project,
    commands: Vec<GraphCommand>,
}

impl Planner {
    fn run(&mut self, op: &EditOperation) -> Result<(), EditError> {
        match op {
            EditOperation::Bind {
                binding,
                expression,
            } => {
                self.check_binding(binding)?;
                let node = self.expression(expression)?;
                let scope = self.scope(&binding.scope)?;
                let previous = scope
                    .bindings
                    .iter()
                    .find(|b| b.target_field == binding.field)
                    .map(|b| b.node);
                let mut after = self.project.clone();
                let target = scope_mut(&mut after, &binding.scope)?;
                match target
                    .bindings
                    .iter_mut()
                    .find(|b| b.target_field == binding.field)
                {
                    Some(binding) => binding.node = node,
                    None => target.bindings.push(mapping::Binding {
                        target_field: binding.field.clone(),
                        node,
                    }),
                }
                self.target_change(
                    &binding.scope,
                    after,
                    GraphAction::ConnectWire {
                        source: node,
                        sink: binding_sink(binding),
                    },
                )?;
                if let Some(previous) = previous {
                    self.cleanup(previous)?;
                }
            }
            EditOperation::RemoveBinding { binding } => {
                let previous = self.binding(binding)?;
                let mut after = self.project.clone();
                scope_mut(&mut after, &binding.scope)?
                    .bindings
                    .retain(|b| b.target_field != binding.field);
                self.target_change(
                    &binding.scope,
                    after,
                    GraphAction::RemoveBinding {
                        scope: location(&binding.scope),
                        binding: TargetBinding {
                            field: binding.field.clone(),
                            node: previous,
                        },
                    },
                )?;
            }
            EditOperation::RetargetBinding { binding, to, node } => {
                if binding.scope != to.scope {
                    return invalid("retargeting must stay within one scope");
                }
                self.check_binding(to)?;
                self.node(node.0)?;
                let previous = self.binding(binding)?;
                if binding.field != to.field
                    && self
                        .scope(&to.scope)?
                        .bindings
                        .iter()
                        .any(|b| b.target_field == to.field)
                {
                    return invalid("destination binding already exists");
                }
                let mut after = self.project.clone();
                let bindings = &mut scope_mut(&mut after, &to.scope)?.bindings;
                let entry = bindings
                    .iter_mut()
                    .find(|b| b.target_field == binding.field)
                    .ok_or_else(|| EditError::MissingBinding(binding.clone()))?;
                entry.target_field = to.field.clone();
                entry.node = node.0;
                self.target_change(
                    &to.scope,
                    after,
                    GraphAction::RetargetBinding {
                        scope: location(&to.scope),
                        from: TargetBinding {
                            field: binding.field.clone(),
                            node: previous,
                        },
                        to: TargetBinding {
                            field: to.field.clone(),
                            node: node.0,
                        },
                    },
                )?;
            }
            EditOperation::Connect {
                node,
                input,
                expression,
            } => {
                self.node(node.0)?;
                let source = self.expression(expression)?;
                let previous = self
                    .node(node.0)?
                    .evaluated_inputs()
                    .get(input.0 as usize)
                    .copied();
                if depends_on(&self.project, source, node.0) {
                    return Err(EditError::Cycle {
                        node: *node,
                        source_node: NodeId(source),
                    });
                }
                let mut after = self.project.clone();
                let target = after
                    .graph
                    .nodes
                    .get_mut(&node.0)
                    .ok_or(EditError::MissingNode(*node))?;
                if !editor_ui::set_scalar_input(target, input.0 as usize, source) {
                    return Err(EditError::Input {
                        node: *node,
                        input: *input,
                    });
                }
                self.graph_change(
                    after,
                    GraphAction::ConnectWire {
                        source,
                        sink: WireSink::NodeInput {
                            node: node.0,
                            input: input.0 as usize,
                        },
                    },
                )?;
                if let Some(previous) = previous {
                    self.cleanup(previous)?;
                }
            }
            EditOperation::Disconnect {
                node,
                input,
                replacement,
            } => self.disconnect(*node, input.0 as usize, *replacement)?,
            EditOperation::EditConstant { node, value } => {
                if !matches!(self.node(node.0)?, Node::Const { .. }) {
                    return invalid("node is not a constant");
                }
                self.edit_node(
                    *node,
                    Node::Const {
                        value: value.value()?,
                    },
                    NodeProperty::Constant,
                )?;
            }
            EditOperation::EditCall {
                node,
                function,
                args,
            } => {
                if !matches!(self.node(node.0)?, Node::Call { .. }) {
                    return invalid("node is not a builtin call");
                }
                self.check_call(function, args)?;
                for arg in args {
                    if depends_on(&self.project, arg.0, node.0) {
                        return Err(EditError::Cycle {
                            node: *node,
                            source_node: *arg,
                        });
                    }
                }
                let old = self.node(node.0)?.evaluated_inputs();
                self.edit_node(
                    *node,
                    Node::Call {
                        function: function.clone(),
                        args: args.iter().map(|id| id.0).collect(),
                    },
                    NodeProperty::FunctionCall,
                )?;
                for id in old {
                    self.cleanup(id)?;
                }
            }
            EditOperation::EditValueMap { node, value } => {
                let Node::ValueMap { input, .. } = self.node(node.0)? else {
                    return invalid("node is not a value map");
                };
                let replacement = Node::ValueMap {
                    input: *input,
                    input_type: value.input_type.as_ref().map(|ty| match ty {
                        crate::ScalarType::String {} => ir::ScalarType::String,
                        crate::ScalarType::Int {} => ir::ScalarType::Int,
                        crate::ScalarType::Float {} => ir::ScalarType::Float,
                        crate::ScalarType::Bool {} => ir::ScalarType::Bool,
                    }),
                    table: value
                        .table
                        .iter()
                        .map(|entry| Ok((entry.from.value()?, entry.to.value()?)))
                        .collect::<Result<_, EditError>>()?,
                    default: value
                        .default
                        .as_ref()
                        .map(|value| value.value())
                        .transpose()?,
                };
                self.edit_node(*node, replacement, NodeProperty::ValueMap)?;
            }
            EditOperation::RemoveNodes {
                scope,
                nodes,
                replacements,
            } => self.remove_nodes(scope, nodes, replacements)?,
            _ => self.control(op)?,
        }
        Ok(())
    }

    fn node(&self, id: u32) -> Result<&Node, EditError> {
        self.project
            .graph
            .nodes
            .get(&id)
            .ok_or(EditError::MissingNode(NodeId(id)))
    }
    fn scope(&self, at: &ScopeRef) -> Result<&Scope, EditError> {
        scope(&self.project, at)
    }
    fn binding(&self, at: &BindingRef) -> Result<u32, EditError> {
        let mut matches = self
            .scope(&at.scope)?
            .bindings
            .iter()
            .filter(|binding| binding.target_field == at.field);
        let first = matches
            .next()
            .ok_or_else(|| EditError::MissingBinding(at.clone()))?;
        if matches.next().is_some() {
            return invalid("ambiguous duplicate binding");
        }
        Ok(first.node)
    }
    fn check_binding(&self, at: &BindingRef) -> Result<(), EditError> {
        let root = target_root(&self.project, &at.scope)?;
        let mut current = root;
        let mut schema = match &at.scope.target {
            Target::Primary {} => &self.project.target,
            Target::Named { name } => {
                &self
                    .project
                    .extra_targets
                    .iter()
                    .find(|t| t.name == *name)
                    .ok_or_else(|| EditError::MissingScope(at.scope.clone()))?
                    .schema
            }
        };
        check_constructed(current)?;
        for index in &at.scope.children {
            current = current
                .children
                .get(index.0 as usize)
                .ok_or_else(|| EditError::MissingScope(at.scope.clone()))?;
            check_constructed(current)?;
            schema = schema
                .child(&current.target_field)
                .ok_or_else(|| EditError::MissingScope(at.scope.clone()))?;
        }
        let field = schema
            .child(&at.field)
            .ok_or_else(|| EditError::MissingBinding(at.clone()))?;
        if matches!(field.kind, ir::SchemaKind::Group { .. }) {
            return invalid("a binding must target a scalar field");
        }
        if current
            .bindings
            .iter()
            .filter(|b| b.target_field == at.field)
            .count()
            > 1
        {
            return invalid("ambiguous duplicate binding");
        }
        Ok(())
    }
    fn target_schema(&self, at: &ScopeRef) -> Result<&ir::SchemaNode, EditError> {
        let mut schema = match &at.target {
            Target::Primary {} => &self.project.target,
            Target::Named { name } => {
                &self
                    .project
                    .extra_targets
                    .iter()
                    .find(|target| target.name == *name)
                    .ok_or_else(|| EditError::MissingScope(at.clone()))?
                    .schema
            }
        };
        let mut current = target_root(&self.project, at)?;
        check_constructed(current)?;
        for index in &at.children {
            current = current
                .children
                .get(index.0 as usize)
                .ok_or_else(|| EditError::MissingScope(at.clone()))?;
            check_constructed(current)?;
            schema = schema
                .child(&current.target_field)
                .ok_or_else(|| EditError::MissingScope(at.clone()))?;
        }
        Ok(schema)
    }
    fn fresh(&self, id: NodeId) -> Result<(), EditError> {
        if occupied(&self.project).contains(&id.0) {
            Err(EditError::OccupiedNode(id))
        } else {
            Ok(())
        }
    }
    fn check_call(&self, function: &str, args: &[NodeId]) -> Result<(), EditError> {
        let Some(definition) = functions::builtin(function) else {
            return Err(EditError::UnknownFunction {
                function: function.into(),
            });
        };
        if !definition.accepts_arity(args.len()) {
            return Err(EditError::Arity {
                function: function.into(),
                count: args.len(),
            });
        }
        for arg in args {
            self.node(arg.0)?;
        }
        Ok(())
    }
    fn check_source(&self, path: &[String], frame: Option<&[String]>) -> Result<(), EditError> {
        let absolute: Vec<_> = frame.into_iter().flatten().chain(path).cloned().collect();
        let mut schema = &self.project.source;
        let mut segments = absolute.as_slice();
        if let Some(first) = segments.first() {
            let mut named = self
                .project
                .extra_sources
                .iter()
                .filter(|source| source.name == *first);
            if let Some(source) = named.next() {
                if named.next().is_some() || schema.child(first).is_some() {
                    return invalid("ambiguous named source path");
                }
                schema = &source.schema;
                segments = &segments[1..];
            }
        }
        for (walked, segment) in (absolute.len() - segments.len() + 1..).zip(segments) {
            schema = schema.child(segment).ok_or_else(|| {
                EditError::Invalid(format!(
                    "source field does not exist: {}",
                    absolute.join("/")
                ))
            })?;
            if frame.is_some_and(|frame| frame.len() == walked) && !schema.repeating {
                return invalid("source frame is not repeating");
            }
        }
        if matches!(schema.kind, ir::SchemaKind::Group { .. }) {
            return invalid("source field is not scalar");
        }
        Ok(())
    }
    fn expression(&mut self, expression: &Expression) -> Result<u32, EditError> {
        let (id, node) = match expression {
            Expression::Existing { node } => {
                self.node(node.0)?;
                return Ok(node.0);
            }
            Expression::Constant { id, value } => (
                *id,
                Node::Const {
                    value: value.value()?,
                },
            ),
            Expression::Source { id, path, frame } => {
                self.check_source(path, frame.as_deref())?;
                (
                    *id,
                    Node::SourceField {
                        path: path.clone(),
                        frame: frame.clone(),
                    },
                )
            }
            Expression::OwnedSource {
                id,
                owner,
                path,
                frame,
            } => {
                let (owner, mut schema) = match owner {
                    crate::SourceOwner::Primary {} => {
                        (mapping::SourceOwner::Primary, &self.project.source)
                    }
                    crate::SourceOwner::Named { name } => {
                        let mut sources = self
                            .project
                            .extra_sources
                            .iter()
                            .filter(|source| source.name == *name);
                        let source = sources.next().ok_or_else(|| {
                            EditError::Invalid(format!("source {name} does not exist"))
                        })?;
                        if sources.next().is_some() || source.dynamic_path.is_some() {
                            return invalid("source ownership is ambiguous or dynamic");
                        }
                        (
                            mapping::SourceOwner::Named { name: name.clone() },
                            &source.schema,
                        )
                    }
                };
                if frame.as_ref().is_some_and(|frame| !path.starts_with(frame)) {
                    return invalid("owned source frame must contain the field path");
                }
                for (index, segment) in path.iter().enumerate() {
                    schema = schema.child(segment).ok_or_else(|| {
                        EditError::Invalid(format!(
                            "owned source field {} does not exist",
                            path.join("/")
                        ))
                    })?;
                    if frame.as_ref().is_some_and(|frame| frame.len() == index + 1)
                        && !schema.repeating
                    {
                        return invalid("owned source frame is not repeating");
                    }
                }
                if matches!(schema.kind, ir::SchemaKind::Group { .. }) {
                    return invalid("owned source field is not scalar");
                }
                (
                    *id,
                    Node::OwnedSourceField(mapping::OwnedSourceField {
                        source: mapping::OwnedSourceRef {
                            owner,
                            path: path.clone(),
                        },
                        frame: frame.clone(),
                    }),
                )
            }
            Expression::Call { id, function, args } => {
                self.check_call(function, args)?;
                (
                    *id,
                    Node::Call {
                        function: function.clone(),
                        args: args.iter().map(|id| id.0).collect(),
                    },
                )
            }
        };
        self.fresh(id)?;
        let mut after = self.project.clone();
        after.graph.nodes.insert(id.0, node);
        self.graph_change(
            after,
            GraphAction::InsertNode {
                node: id.0,
                position: CanvasPosition { x: 0.0, y: 0.0 },
            },
        )?;
        Ok(id.0)
    }
    fn edit_node(
        &mut self,
        id: NodeId,
        node: Node,
        property: NodeProperty,
    ) -> Result<(), EditError> {
        let mut after = self.project.clone();
        after.graph.nodes.insert(id.0, node);
        self.graph_change(
            after,
            GraphAction::EditNode {
                node: id.0,
                property,
            },
        )
    }
    fn graph_change(&mut self, after: Project, action: GraphAction) -> Result<(), EditError> {
        let patch = GraphPatch::of_nodes(&self.project.graph, &after.graph, same_node);
        self.commit(GraphDocument::Target(TargetId::Primary), action, patch)
    }
    fn target_change(
        &mut self,
        at: &ScopeRef,
        after: Project,
        action: GraphAction,
    ) -> Result<(), EditError> {
        let mut patch = GraphPatch::between_reshaped(
            (&self.project.graph, target_root(&self.project, at)?),
            (&after.graph, target_root(&after, at)?),
            same_node,
        )?;
        if matches!(action, GraphAction::EditScope { .. }) {
            patch = patch.with_scope_settings(
                &location(at),
                &ScopeFields::capture(self.scope(at)?),
                &ScopeFields::capture(scope(&after, at)?),
            )?;
        }
        self.commit(document(at), action, patch)
    }
    fn project_change(&mut self, after: Project, action: GraphAction) -> Result<(), EditError> {
        let patch = GraphPatch::of_nodes(&self.project.graph, &after.graph, same_node)
            .with_project_parts(ProjectParts::between(&self.project, &after, same_node));
        self.commit(GraphDocument::Project, action, patch)
    }
    fn commit(
        &mut self,
        document: GraphDocument,
        action: GraphAction,
        patch: GraphPatch,
    ) -> Result<(), EditError> {
        if patch.is_empty() {
            return Ok(());
        }
        let command = GraphCommand::new(document, action, patch)?;
        command.apply_to_matching(&mut self.project, same_node)?;
        self.commands.push(command);
        Ok(())
    }
}

fn invalid<T>(message: &str) -> Result<T, EditError> {
    Err(EditError::Invalid(message.to_owned()))
}
fn location(at: &ScopeRef) -> ScopeLocation {
    ScopeLocation::of_children(
        &at.children
            .iter()
            .map(|id| id.0 as usize)
            .collect::<Vec<_>>(),
    )
}
fn document(at: &ScopeRef) -> GraphDocument {
    GraphDocument::Target(match &at.target {
        Target::Primary {} => TargetId::Primary,
        Target::Named { name } => TargetId::Named(name.clone()),
    })
}
fn binding_sink(at: &BindingRef) -> WireSink {
    WireSink::Binding {
        scope: location(&at.scope),
        field: at.field.clone(),
    }
}
fn target_root<'a>(project: &'a Project, at: &ScopeRef) -> Result<&'a Scope, EditError> {
    match &at.target {
        Target::Primary {} => Ok(&project.root),
        Target::Named { name } => {
            let mut targets = project.extra_targets.iter().filter(|t| t.name == *name);
            let target = targets
                .next()
                .ok_or_else(|| EditError::MissingScope(at.clone()))?;
            if targets.next().is_some() {
                return invalid("ambiguous target name");
            }
            Ok(&target.root)
        }
    }
}
fn scope<'a>(project: &'a Project, at: &ScopeRef) -> Result<&'a Scope, EditError> {
    let mut scope = target_root(project, at)?;
    for index in &at.children {
        scope = scope
            .children
            .get(index.0 as usize)
            .ok_or_else(|| EditError::MissingScope(at.clone()))?;
    }
    Ok(scope)
}
fn scope_mut<'a>(project: &'a mut Project, at: &ScopeRef) -> Result<&'a mut Scope, EditError> {
    let mut scope = match &at.target {
        Target::Primary {} => &mut project.root,
        Target::Named { name } => {
            &mut project
                .extra_targets
                .iter_mut()
                .find(|t| t.name == *name)
                .ok_or_else(|| EditError::MissingScope(at.clone()))?
                .root
        }
    };
    for index in &at.children {
        scope = scope
            .children
            .get_mut(index.0 as usize)
            .ok_or_else(|| EditError::MissingScope(at.clone()))?;
    }
    Ok(scope)
}
fn check_constructed(scope: &Scope) -> Result<(), EditError> {
    if scope.construction != ScopeConstruction::Constructed {
        return invalid("scope construction is read-only");
    }
    Ok(())
}
// Comparing floats by bits is essential: PartialEq loses negative zero and NaNs.
pub(crate) fn same_node(a: &Node, b: &Node) -> bool {
    match (a, b) {
        (Node::Const { value: a }, Node::Const { value: b }) => same_value(a, b),
        (
            Node::ValueMap {
                input: ai,
                input_type: at,
                table: av,
                default: ad,
            },
            Node::ValueMap {
                input: bi,
                input_type: bt,
                table: bv,
                default: bd,
            },
        ) => {
            ai == bi
                && at == bt
                && av.len() == bv.len()
                && av
                    .iter()
                    .zip(bv)
                    .all(|((ak, av), (bk, bv))| same_value(ak, bk) && same_value(av, bv))
                && match (ad, bd) {
                    (Some(a), Some(b)) => same_value(a, b),
                    (None, None) => true,
                    _ => false,
                }
        }
        _ => match (serde_json::to_string(a), serde_json::to_string(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        },
    }
}
fn same_value(a: &ir::Value, b: &ir::Value) -> bool {
    match (a, b) {
        (ir::Value::Float(a), ir::Value::Float(b)) => a.to_bits() == b.to_bits(),
        _ => a == b,
    }
}
fn depends_on(project: &Project, start: u32, target: u32) -> bool {
    let mut pending = vec![start];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if id == target {
            return true;
        }
        if seen.insert(id)
            && let Some(node) = project.graph.nodes.get(&id)
        {
            pending.extend(node.dependencies());
        }
    }
    false
}
fn occupied(project: &Project) -> BTreeSet<u32> {
    let mut ids = project.graph.nodes.keys().copied().collect::<BTreeSet<_>>();
    for node in project.graph.nodes.values() {
        ids.extend(node.dependencies());
    }
    project.root.visit_tree_roots(&mut |root| {
        ids.insert(root.node);
    });
    for target in &project.extra_targets {
        target.root.visit_tree_roots(&mut |root| {
            ids.insert(root.node);
        });
    }
    for rule in &project.failure_rules {
        rule.visit_roots(&mut |root| {
            ids.insert(root.node);
        });
    }
    ids.extend(
        project
            .extra_sources
            .iter()
            .filter_map(|s| s.dynamic_path.as_ref().map(|p| p.node)),
    );
    ids
}
