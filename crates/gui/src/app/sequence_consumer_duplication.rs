//! Atomic duplication of one scalar sequence consumer and its private expressions.

use super::*;
use egui_snarl::{InPinId, OutPinId};
use mapping::{FailureIteration, Node, ScopeConstruction, ScopeIteration, SequenceExpr};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DuplicationError {
    UnavailableContext {
        context: &'static str,
    },
    MissingNode {
        node: NodeId,
        role: &'static str,
    },
    UnsupportedSequence {
        kind: &'static str,
    },
    UnsupportedPrivateNode {
        node: NodeId,
    },
    DuplicatePrivateOwner {
        item: NodeId,
        count: usize,
    },
    InvalidPrivateOwner {
        item: NodeId,
    },
    PrivateOwnerOutsideContext {
        expression: NodeId,
        item: NodeId,
        consumer: NodeId,
        context: String,
    },
    ExpressionCycle {
        path: Vec<NodeId>,
    },
    AmbiguousPrivateContext {
        node: NodeId,
        owners: Vec<NodeId>,
    },
    FilterMapAdmission(mapping::FilterMapAdmissionError),
    FunctionAdmission {
        function: FunctionId,
        message: String,
    },
    AmbiguousParentContext {
        node: NodeId,
        consumer: NodeId,
        source_frames: Vec<Vec<String>>,
    },
    IdExhaustion {
        message: String,
        required: usize,
    },
    MissingCanvasFrame {
        canvas: String,
        node: NodeId,
    },
    InvalidFramePosition {
        node: NodeId,
        axis: &'static str,
        bits: u32,
    },
    InvalidCanvasInput {
        canvas: String,
        node: NodeId,
        input: usize,
    },
    DuplicateCanvasFrame {
        canvas: String,
        node: NodeId,
    },
}

impl std::fmt::Display for DuplicationError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "{self:?}")
    }
}

// Requests exist only between the current canvas's begin/take calls. Applying
// them outside the viewer borrow provides the complete current Project.
fn request_key() -> egui::Id {
    egui::Id::new("sequence_consumer_duplication_request")
}

pub(crate) fn request(context: &egui::Context, node: NodeId) {
    context.data_mut(|data| data.insert_temp(request_key(), node));
}

pub(crate) fn begin(context: &egui::Context) {
    let _ = take(context);
}

pub(crate) fn take(context: &egui::Context) -> Option<NodeId> {
    context.data_mut(|data| data.remove_temp(request_key()))
}

fn consumer_sequence(node: &Node) -> Option<&SequenceExpr> {
    match node {
        Node::SequenceExists { sequence, .. }
        | Node::SequenceItemAt { sequence, .. }
        | Node::SequenceAggregate { sequence, .. } => Some(sequence),
        _ => None,
    }
}

fn owner_counts(project: &Project) -> BTreeMap<NodeId, usize> {
    let mut owners = BTreeMap::new();
    let mut add = |sequence: &SequenceExpr| {
        for item in sequence.owned_items() {
            *owners.entry(item).or_default() += 1;
        }
    };
    let mut scopes = vec![&project.root];
    scopes.extend(project.extra_targets.iter().map(|target| &target.root));
    while let Some(scope) = scopes.pop() {
        if let Some(sequence) = scope.sequence() {
            add(sequence);
        }
        scopes.extend(scope.children.iter());
        scopes.extend(scope.dynamic_children.iter().map(|child| &child.scope));
        if let Some(segments) = scope.concatenated() {
            scopes.extend(segments.iter());
        }
    }
    for node in project.graph.nodes.values() {
        if let Some(sequence) = consumer_sequence(node) {
            add(sequence);
        }
    }
    for rule in &project.failure_rules {
        if let FailureIteration::Sequence { sequence } = &rule.iteration {
            add(sequence);
        }
    }
    owners
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Context {
    allowed: Option<NodeId>,
    private_frame: Option<NodeId>,
    empty_rule: bool,
}

impl Context {
    const PARENT: Self = Self {
        allowed: None,
        private_frame: None,
        empty_rule: false,
    };
    fn private(item: NodeId) -> Self {
        Self {
            allowed: Some(item),
            private_frame: Some(item),
            empty_rule: false,
        }
    }
    fn empty(self) -> Self {
        Self {
            allowed: None,
            empty_rule: true,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Visit {
    node: NodeId,
    context: Context,
    clone: bool,
}

struct Spec {
    context: Context,
    remap_inputs: Vec<bool>,
}

struct Planner<'a> {
    project: &'a Project,
    selected: NodeId,
    owners: BTreeMap<NodeId, usize>,
    specs: BTreeMap<NodeId, Spec>,
    functions: BTreeSet<FunctionId>,
    sensitive_parents: BTreeSet<NodeId>,
}

impl Planner<'_> {
    fn sequence(&self, sequence: &SequenceExpr) -> Result<(), DuplicationError> {
        let kind = match sequence {
            SequenceExpr::Generate { .. } => None,
            SequenceExpr::FilterMapV1(value)
                if matches!(value.source.as_ref(), SequenceExpr::Generate { .. }) =>
            {
                None
            }
            SequenceExpr::FilterMapV1(_) => Some("FilterMapV1 source"),
            SequenceExpr::Tokenize { .. } => Some("Tokenize"),
            SequenceExpr::TokenizeByLength { .. } => Some("TokenizeByLength"),
            SequenceExpr::TokenizeRegex { .. } => Some("TokenizeRegex"),
            SequenceExpr::RecursiveCollect { .. } => Some("RecursiveCollect"),
        };
        if let Some(kind) = kind {
            return Err(DuplicationError::UnsupportedSequence { kind });
        }
        for item in sequence.owned_items() {
            let count = self.owners.get(&item).copied().unwrap_or_default();
            if count != 1 {
                return Err(DuplicationError::DuplicatePrivateOwner { item, count });
            }
            match self.project.graph.nodes.get(&item) {
                None => {
                    return Err(DuplicationError::MissingNode {
                        node: item,
                        role: "private owner",
                    });
                }
                Some(Node::SourceField { path, frame: None }) if path.is_empty() => {}
                Some(_) => return Err(DuplicationError::InvalidPrivateOwner { item }),
            }
        }
        Ok(())
    }

    fn children(&self, visit: Visit, value: &Node) -> Vec<Visit> {
        let input = |node, context, clone| Visit {
            node,
            context,
            clone,
        };
        let boundary = |node| input(node, visit.context, false);
        let copying = visit.clone;
        let private = |item| {
            if copying {
                Context::private(item)
            } else {
                Context {
                    allowed: Some(item),
                    private_frame: None,
                    empty_rule: false,
                }
            }
        };
        match value {
            Node::SequenceExists {
                sequence,
                predicate,
            } => sequence
                .inputs()
                .into_iter()
                .map(boundary)
                .chain([input(*predicate, private(sequence.item()), copying)])
                .collect(),
            Node::SequenceAggregate {
                sequence,
                predicate,
                expression,
                arg,
                ..
            } => sequence
                .inputs()
                .into_iter()
                .map(boundary)
                .chain(
                    predicate
                        .iter()
                        .chain(expression)
                        .map(|&node| input(node, private(sequence.item()), copying)),
                )
                .chain(arg.iter().copied().map(boundary))
                .collect(),
            Node::SequenceItemAt { sequence, index } => sequence
                .inputs()
                .into_iter()
                .chain([*index])
                .map(|node| input(node, visit.context.empty(), false))
                .collect(),
            _ => value
                .dependencies()
                .into_iter()
                .map(|node| input(node, visit.context, visit.clone))
                .collect(),
        }
    }

    fn walk(&mut self) -> Result<(), DuplicationError> {
        let root = Visit {
            node: self.selected,
            context: Context::PARENT,
            clone: true,
        };
        let mut pending = vec![(root, 0_u8)];
        let mut active = Vec::new();
        let mut complete = BTreeMap::<Visit, bool>::new();
        while let Some((visit, phase)) = pending.pop() {
            if phase == 2 {
                complete.insert(visit, true);
                continue;
            }
            if phase == 0 && complete.contains_key(&visit) {
                continue;
            }
            let value =
                self.project
                    .graph
                    .nodes
                    .get(&visit.node)
                    .ok_or(DuplicationError::MissingNode {
                        node: visit.node,
                        role: if self.owners.contains_key(&visit.node) {
                            "private owner"
                        } else {
                            "private expression"
                        },
                    })?;
            let children = self.children(visit, value);
            if phase == 1 {
                let flags = children
                    .iter()
                    .map(|child| complete[child])
                    .collect::<Vec<_>>();
                if let Some(sequence) = consumer_sequence(value)
                    && !visit.clone
                {
                    let parent_flags = match value {
                        Node::SequenceItemAt { .. } => flags.iter().chain(None),
                        Node::SequenceAggregate { arg: Some(_), .. } => {
                            flags[..sequence.inputs().len()].iter().chain(flags.last())
                        }
                        _ => flags[..sequence.inputs().len()].iter().chain(None),
                    };
                    let dependent = visit.context.private_frame.is_some()
                        && parent_flags.into_iter().any(|flag| *flag);
                    active.pop();
                    if dependent {
                        // Inspecting a shared reducer is not copying it. Only a
                        // parent path dependent on the enclosing private frame
                        // requests a second, complete copy of that reducer.
                        pending.push((visit, 2));
                        pending.push((
                            Visit {
                                clone: true,
                                ..visit
                            },
                            0,
                        ));
                    } else {
                        complete.insert(visit, false);
                    }
                    continue;
                }
                let frame_read =
                    matches!(value, Node::Position { collection } if collection.is_empty());
                let own_read = self.owners.contains_key(&visit.node);
                let clone = visit.clone
                    || (visit.context.private_frame.is_some()
                        && (own_read || frame_read || flags.iter().any(|flag| *flag)));
                if clone {
                    if let Some(previous) = self.specs.get(&visit.node)
                        && (previous.context.private_frame != visit.context.private_frame
                            || previous.remap_inputs != flags)
                        && !own_read
                    {
                        let owners = [previous.context.private_frame, visit.context.private_frame]
                            .into_iter()
                            .flatten()
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect();
                        return Err(DuplicationError::AmbiguousPrivateContext {
                            node: visit.node,
                            owners,
                        });
                    }
                    if !matches!(
                        value,
                        Node::Const { .. }
                            | Node::SourceField { .. }
                            | Node::Position { .. }
                            | Node::Call { .. }
                            | Node::UserFunctionCall { .. }
                            | Node::If { .. }
                            | Node::Raise { .. }
                            | Node::SequenceExists { .. }
                            | Node::SequenceItemAt { .. }
                            | Node::SequenceAggregate { .. }
                    ) {
                        return Err(DuplicationError::UnsupportedPrivateNode { node: visit.node });
                    }
                    self.specs.insert(
                        visit.node,
                        Spec {
                            context: visit.context,
                            remap_inputs: flags,
                        },
                    );
                }
                if !clone
                    && (matches!(value, Node::SourceField { .. })
                        || (visit.context.private_frame.is_none()
                            && matches!(value, Node::Position { .. })))
                {
                    self.sensitive_parents.insert(visit.node);
                }
                complete.insert(visit, clone);
                active.pop();
                continue;
            }
            if self.owners.contains_key(&visit.node) && visit.context.allowed != Some(visit.node) {
                let context = match visit.context.allowed {
                    None if visit.context.empty_rule => "Empty".into(),
                    None => "parent".into(),
                    Some(owner) => format!("output owner{}", owner),
                };
                return Err(DuplicationError::PrivateOwnerOutsideContext {
                    expression: visit.node,
                    item: visit.node,
                    consumer: self.selected,
                    context,
                });
            }
            if let Some(index) = active.iter().position(|&node| node == visit.node) {
                return Err(DuplicationError::ExpressionCycle {
                    path: active[index..]
                        .iter()
                        .copied()
                        .chain([visit.node])
                        .collect(),
                });
            }
            if let Some(sequence) = consumer_sequence(value) {
                self.sequence(sequence)?;
                if visit.clone {
                    for item in sequence.owned_items() {
                        self.specs.entry(item).or_insert(Spec {
                            context: Context::private(item),
                            remap_inputs: Vec::new(),
                        });
                    }
                }
                if let SequenceExpr::FilterMapV1(value) = sequence {
                    self.functions.extend([value.predicate, value.mapper]);
                }
            }
            if let Node::UserFunctionCall { function, args } = value {
                let definition = self.project.user_functions.get(function).ok_or_else(|| {
                    DuplicationError::FunctionAdmission {
                        function: *function,
                        message: "missing function".into(),
                    }
                })?;
                if definition.parameters.len() != args.len() {
                    return Err(DuplicationError::FunctionAdmission {
                        function: *function,
                        message: "argument count differs from ordered parameters".into(),
                    });
                }
                self.functions.insert(*function);
            }
            if matches!(value, Node::Position { collection } if !collection.is_empty())
                || matches!(
                    value,
                    Node::CollectionFind { .. }
                        | Node::Aggregate { .. }
                        | Node::JoinField { .. }
                        | Node::JoinPosition { .. }
                        | Node::JoinAggregate { .. }
                )
            {
                return Err(DuplicationError::UnsupportedPrivateNode { node: visit.node });
            }
            if visit.clone {
                match value {
                    Node::SourceField { path, frame }
                        if !self.owners.contains_key(&visit.node)
                            || !path.is_empty()
                            || frame.is_some() =>
                    {
                        return Err(DuplicationError::UnsupportedPrivateNode { node: visit.node });
                    }
                    Node::Position { collection } if !collection.is_empty() => {
                        return Err(DuplicationError::UnsupportedPrivateNode { node: visit.node });
                    }
                    _ => {}
                }
            }
            active.push(visit.node);
            pending.push((visit, 1));
            pending.extend(children.into_iter().rev().map(|child| (child, 0)));
        }
        Ok(())
    }
}

fn remap_node(mut value: Node, spec: &Spec, ids: &BTreeMap<NodeId, NodeId>) -> Node {
    let mut flags = spec.remap_inputs.iter();
    let mut input = |node: &mut NodeId| {
        if *flags.next().expect("planned input") {
            *node = ids[node];
        }
    };
    let sequence = |sequence: &mut SequenceExpr, input: &mut dyn FnMut(&mut NodeId)| match sequence
    {
        SequenceExpr::Generate { from, to, item } => {
            if let Some(from) = from {
                input(from);
            }
            input(to);
            *item = ids[item];
        }
        SequenceExpr::FilterMapV1(value) => {
            let SequenceExpr::Generate { from, to, item } = value.source.as_mut() else {
                unreachable!("admitted source")
            };
            if let Some(from) = from {
                input(from);
            }
            input(to);
            *item = ids[item];
            value.item = ids[&value.item];
            for capture in &mut value.captures {
                input(&mut capture.node);
            }
        }
        _ => unreachable!("admitted sequence"),
    };
    match &mut value {
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => {
            for arg in args {
                input(arg);
            }
        }
        Node::If {
            condition,
            then,
            else_,
        } => {
            input(condition);
            input(then);
            input(else_);
        }
        Node::Raise {
            message: Some(message),
        } => {
            input(message);
        }
        Node::SequenceExists {
            sequence: source,
            predicate,
        } => {
            sequence(source, &mut input);
            input(predicate);
        }
        Node::SequenceItemAt {
            sequence: source,
            index,
        } => {
            sequence(source, &mut input);
            input(index);
        }
        Node::SequenceAggregate {
            sequence: source,
            predicate,
            expression,
            arg,
            ..
        } => {
            sequence(source, &mut input);
            for node in predicate.iter_mut().chain(expression).chain(arg) {
                input(node);
            }
        }
        _ => {}
    }
    value
}

#[derive(Debug)]
pub(super) struct DuplicationPlan {
    pub(super) ids: BTreeMap<NodeId, NodeId>,
    pub(super) nodes: BTreeMap<NodeId, Node>,
    pub(super) duplicate: NodeId,
}

fn plan(project: &Project, selected: NodeId) -> Result<DuplicationPlan, DuplicationError> {
    let value = project
        .graph
        .nodes
        .get(&selected)
        .ok_or(DuplicationError::MissingNode {
            node: selected,
            role: "consumer",
        })?;
    let source = consumer_sequence(value)
        .ok_or(DuplicationError::UnsupportedPrivateNode { node: selected })?;
    let mut planner = Planner {
        project,
        selected,
        owners: owner_counts(project),
        specs: BTreeMap::new(),
        functions: BTreeSet::new(),
        sensitive_parents: BTreeSet::new(),
    };
    planner.sequence(source)?;
    project
        .validate_filter_map_v1()
        .map_err(DuplicationError::FilterMapAdmission)?;
    planner.walk()?;
    validate_functions(project, &planner.functions)?;
    validate_parent_contexts(project, selected, &planner.sensitive_parents)?;
    let fresh = crate::graph_viewer::reserve_project_node_ids(project, planner.specs.len())
        .map_err(|message| DuplicationError::IdExhaustion {
            message,
            required: planner.specs.len(),
        })?;
    let ids = planner
        .specs
        .keys()
        .copied()
        .zip(fresh)
        .collect::<BTreeMap<_, _>>();
    let nodes = planner
        .specs
        .iter()
        .map(|(&node, spec)| {
            (
                ids[&node],
                remap_node(project.graph.nodes[&node].clone(), spec, &ids),
            )
        })
        .collect();
    Ok(DuplicationPlan {
        duplicate: ids[&selected],
        ids,
        nodes,
    })
}

// Functions retain their isolated namespace. This check follows the current
// scalar function admission rules without evaluating or cloning their bodies.
fn validate_functions(
    project: &Project,
    roots: &BTreeSet<FunctionId>,
) -> Result<(), DuplicationError> {
    let mut complete = BTreeMap::<FunctionId, usize>::new();
    let mut active = Vec::new();
    let mut pending = roots
        .iter()
        .rev()
        .map(|&id| (id, false))
        .collect::<Vec<_>>();
    while let Some((id, leaving)) = pending.pop() {
        let error = |message: &str| DuplicationError::FunctionAdmission {
            function: id,
            message: message.into(),
        };
        if !leaving {
            if let Some(&depth) = complete.get(&id) {
                if active.len() + depth > 64 {
                    return Err(error("function call depth exceeds existing limit64"));
                }
                continue;
            }
            if active.contains(&id) {
                return Err(error("function call cycle"));
            }
            if active.len() >= 64 {
                return Err(error("function call depth exceeds existing limit64"));
            }
        }
        let function = project
            .user_functions
            .get(&id)
            .ok_or_else(|| error("missing function"))?;
        let calls = function
            .body
            .nodes
            .values()
            .filter_map(|node| match node {
                Node::UserFunctionCall { function, .. } => Some(*function),
                _ => None,
            })
            .collect::<Vec<_>>();
        if leaving {
            let depth = 1 + calls
                .iter()
                .map(|callee| complete[callee])
                .max()
                .unwrap_or_default();
            complete.insert(id, depth);
            active.pop();
            continue;
        }
        if function.library.trim().is_empty()
            || function.name.trim().is_empty()
            || function.output_name.trim().is_empty()
        {
            return Err(error("empty function metadata"));
        }
        if project.user_functions.iter().any(|(&other, candidate)| {
            other != id
                && candidate.library.trim() == function.library.trim()
                && candidate.name.trim() == function.name.trim()
        }) {
            return Err(error("duplicate library and function name"));
        }
        let mut parameters = BTreeSet::new();
        let mut names = BTreeSet::new();
        for parameter in &function.parameters {
            if !parameters.insert(parameter.id)
                || parameter.name.trim().is_empty()
                || !names.insert(parameter.name.trim())
            {
                return Err(error("invalid ordered parameters"));
            }
        }
        if !function.body.nodes.contains_key(&function.output) {
            return Err(error("missing function output"));
        }
        for node in function.body.nodes.values() {
            match node {
                Node::Const { .. }
                | Node::Unconnected
                | Node::If { .. }
                | Node::Raise { .. }
                | Node::ValueMap { .. }
                | Node::RuntimeValue { .. } => {}
                Node::FunctionParameter { parameter } if parameters.contains(parameter) => {}
                Node::RuntimeParameter { name, .. }
                | Node::RuntimeParameterDefault { name, .. }
                    if !name.is_empty()
                        && !name.contains('\0')
                        && name.len() <= mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES => {}
                Node::Call { function, args }
                    if functions::builtin(function)
                        .is_some_and(|function| function.accepts_arity(args.len())) => {}
                Node::UserFunctionCall { function, args }
                    if project
                        .user_functions
                        .get(function)
                        .is_some_and(|function| function.parameters.len() == args.len()) => {}
                _ => return Err(error("unsupported or malformed scalar function node")),
            }
            if node
                .dependencies()
                .iter()
                .any(|input| !function.body.nodes.contains_key(input))
            {
                return Err(error("missing function body dependency"));
            }
        }
        let mut seen = BTreeSet::new();
        for &root in function.body.nodes.keys() {
            let mut gray = BTreeSet::new();
            let mut nodes = vec![(root, false)];
            while let Some((node, exit)) = nodes.pop() {
                if exit {
                    gray.remove(&node);
                    seen.insert(node);
                    continue;
                }
                if seen.contains(&node) {
                    continue;
                }
                if !gray.insert(node) {
                    return Err(error("function body cycle"));
                }
                nodes.push((node, true));
                nodes.extend(
                    function.body.nodes[&node]
                        .dependencies()
                        .into_iter()
                        .rev()
                        .map(|node| (node, false)),
                );
            }
        }
        active.push(id);
        pending.push((id, true));
        pending.extend(calls.into_iter().rev().map(|id| (id, false)));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SourceContext {
    document: usize,
    introducing_scopes: Vec<Vec<usize>>,
    paths: Vec<Vec<String>>,
}

fn scope_inputs(scope: &Scope) -> Vec<NodeId> {
    let mut inputs = scope
        .bindings
        .iter()
        .map(|binding| binding.node)
        .collect::<Vec<_>>();
    inputs.extend(scope.output_path());
    for binding in &scope.dynamic_bindings {
        inputs.extend([binding.key, binding.value]);
    }
    inputs.extend(scope.dynamic_children.iter().map(|child| child.key));
    match &scope.construction {
        ScopeConstruction::Scalar { value } => inputs.push(*value),
        ScopeConstruction::RecursiveFilter { plan } => inputs.push(plan.predicate()),
        ScopeConstruction::AdjacencyTree { plan } => inputs.extend(plan.root()),
        _ => {}
    }
    inputs
}

// Reachability keeps graph expression roles separate: a nested private body
// introduces its own frame; source bounds/captures stay in the parent frame.
fn reached_contexts(
    graph: &Graph,
    root: NodeId,
    selected: NodeId,
    context: SourceContext,
) -> BTreeSet<SourceContext> {
    let mut result = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut pending = vec![(root, context, Vec::<NodeId>::new())];
    while let Some((node, context, mut path)) = pending.pop() {
        if path.contains(&node) {
            continue;
        }
        path.push(node);
        if node == selected {
            result.insert(context);
            continue;
        }
        if !seen.insert((node, context.clone())) {
            continue;
        }
        let Some(value) = graph.nodes.get(&node) else {
            continue;
        };
        if let Some(sequence) = consumer_sequence(value) {
            pending.extend(
                sequence
                    .inputs()
                    .into_iter()
                    .map(|input| (input, context.clone(), path.clone())),
            );
            let mut private = context.clone();
            private.introducing_scopes.push(vec![
                usize::MAX,
                node as usize,
                sequence.item() as usize,
            ]);
            match value {
                Node::SequenceExists { predicate, .. } => {
                    pending.push((*predicate, private, path.clone()))
                }
                Node::SequenceAggregate {
                    predicate,
                    expression,
                    arg,
                    ..
                } => {
                    pending.extend(
                        predicate
                            .iter()
                            .chain(expression)
                            .map(|&input| (input, private.clone(), path.clone())),
                    );
                    pending.extend(
                        arg.iter()
                            .map(|&input| (input, context.clone(), path.clone())),
                    );
                }
                Node::SequenceItemAt { index, .. } => pending.push((*index, context, path.clone())),
                _ => unreachable!("consumer"),
            }
        } else {
            let mut context = context;
            if matches!(
                value,
                Node::CollectionFind { .. } | Node::Aggregate { .. } | Node::JoinAggregate { .. }
            ) {
                context
                    .introducing_scopes
                    .push(vec![usize::MAX - 1, node as usize]);
            }
            pending.extend(
                value
                    .dependencies()
                    .into_iter()
                    .map(|input| (input, context.clone(), path.clone())),
            );
        }
    }
    result
}

fn validate_parent_contexts(
    project: &Project,
    selected: NodeId,
    sensitive: &BTreeSet<NodeId>,
) -> Result<(), DuplicationError> {
    if sensitive.is_empty() {
        return Ok(());
    }
    let mut contexts = BTreeSet::new();
    let mut scopes = std::iter::once(&project.root)
        .chain(project.extra_targets.iter().map(|target| &target.root))
        .enumerate()
        .map(|(document, scope)| {
            (
                scope,
                Vec::new(),
                SourceContext {
                    document,
                    introducing_scopes: Vec::new(),
                    paths: Vec::new(),
                },
            )
        })
        .collect::<Vec<_>>();
    while let Some((scope, location, mut context)) = scopes.pop() {
        let mut parent_inputs = scope
            .windows
            .iter()
            .flat_map(|window| window.nodes())
            .collect::<Vec<_>>();
        parent_inputs.extend(scope.group_into_blocks);
        if let Some(sequence) = scope.sequence() {
            parent_inputs.extend(sequence.inputs());
        }
        for input in &parent_inputs {
            contexts.extend(reached_contexts(
                &project.graph,
                *input,
                selected,
                context.clone(),
            ));
        }
        if !matches!(
            scope.iteration,
            ScopeIteration::None | ScopeIteration::Concatenate(_)
        ) {
            context.introducing_scopes.push(location.clone());
            context
                .paths
                .push(scope.source().unwrap_or_default().to_vec());
        }
        // Raw candidates, sorted candidates, grouped members and compacted
        // output positions are distinct contexts even within one scope/path.
        let iterating = !matches!(
            scope.iteration,
            ScopeIteration::None | ScopeIteration::Concatenate(_)
        );
        let sorted_phase = usize::from(scope.has_sort());
        let filter_phase = if scope.sort_filter_order == mapping::SortFilterOrder::FilterThenSort {
            0
        } else {
            sorted_phase
        };
        let output_phase = if scope.has_grouping() {
            3
        } else if scope.filter.is_some()
            || scope.has_sort()
            || !scope.windows.is_empty()
            || scope.join().is_some()
        {
            2
        } else {
            0
        };
        let phase_context = |phase| {
            let mut context = context.clone();
            if iterating {
                context.introducing_scopes.push(vec![usize::MAX - 2, phase]);
            }
            context
        };
        for root in scope.sort_keys().map(|key| key.node) {
            contexts.extend(reached_contexts(
                &project.graph,
                root,
                selected,
                phase_context(0),
            ));
        }
        if let Some(root) = scope.filter {
            contexts.extend(reached_contexts(
                &project.graph,
                root,
                selected,
                phase_context(filter_phase),
            ));
        }
        for root in scope
            .grouping_nodes()
            .filter(|root| Some(*root) != scope.group_into_blocks)
            .chain(scope.post_group_filter)
        {
            contexts.extend(reached_contexts(
                &project.graph,
                root,
                selected,
                phase_context(sorted_phase),
            ));
        }
        for input in scope_inputs(scope) {
            contexts.extend(reached_contexts(
                &project.graph,
                input,
                selected,
                phase_context(output_phase),
            ));
        }
        context = phase_context(output_phase);
        for (index, child) in scope.children.iter().enumerate() {
            let mut location = location.clone();
            location.extend([0, index]);
            scopes.push((child, location, context.clone()));
        }
        for (index, child) in scope.dynamic_children.iter().enumerate() {
            let mut location = location.clone();
            location.extend([1, index]);
            scopes.push((&child.scope, location, context.clone()));
        }
        if let Some(segments) = scope.concatenated() {
            for (index, segment) in segments.iter().enumerate() {
                let mut location = location.clone();
                location.extend([2, index]);
                scopes.push((segment, location, context.clone()));
            }
        }
    }
    for (index, rule) in project.failure_rules.iter().enumerate() {
        let parent = SourceContext {
            document: usize::MAX,
            introducing_scopes: vec![vec![index]],
            paths: Vec::new(),
        };
        if let FailureIteration::Sequence { sequence } = &rule.iteration {
            for root in sequence.inputs() {
                contexts.extend(reached_contexts(
                    &project.graph,
                    root,
                    selected,
                    parent.clone(),
                ));
            }
        }
        let mut item = parent;
        item.introducing_scopes.push(vec![index, 0]);
        if let FailureIteration::Source { collection } = &rule.iteration {
            item.paths.push(collection.clone());
        }
        for root in rule.selection.predicate().into_iter().chain(rule.message) {
            contexts.extend(reached_contexts(
                &project.graph,
                root,
                selected,
                item.clone(),
            ));
        }
    }
    for (index, source) in project.extra_sources.iter().enumerate() {
        if let Some(value) = &source.dynamic_path {
            let context = SourceContext {
                document: usize::MAX - 1,
                introducing_scopes: vec![vec![index]],
                paths: vec![value.iteration.clone()],
            };
            contexts.extend(reached_contexts(
                &project.graph,
                value.node,
                selected,
                context,
            ));
        }
    }
    if contexts.len() != 1 {
        let source_frames = contexts
            .iter()
            .flat_map(|context| context.paths.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        return Err(DuplicationError::AmbiguousParentContext {
            node: *sensitive.first().unwrap(),
            consumer: selected,
            source_frames,
        });
    }
    Ok(())
}

fn canvas_inputs(value: &Node) -> Vec<NodeId> {
    match value {
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(predicate.iter().copied())
            .chain(expression.iter().copied())
            .chain(arg.iter().copied())
            .collect(),
        _ => value.dependencies(),
    }
}

struct StagedCanvas {
    snarl: Snarl<CanvasNode>,
    sizes: BTreeMap<CanvasNode, egui::Vec2>,
}

fn stage_canvas(
    name: &str,
    canvas: &CanvasDocumentState,
    project: &Project,
    plan: &DuplicationPlan,
) -> Result<StagedCanvas, DuplicationError> {
    let mut snarl = canvas.snarl.clone();
    let mut sizes = canvas.node_sizes.clone();
    let mut old_ids = BTreeMap::new();
    for (id, node) in snarl.node_ids() {
        if let CanvasNode::Graph(node) | CanvasNode::Placeholder(node) = node
            && old_ids.insert(*node, id).is_some()
            && plan.ids.contains_key(node)
        {
            return Err(DuplicationError::DuplicateCanvasFrame {
                canvas: name.into(),
                node: *node,
            });
        }
    }
    let mut new_ids = BTreeMap::new();
    for (&old, &new) in &plan.ids {
        let old_id =
            old_ids
                .get(&old)
                .copied()
                .ok_or_else(|| DuplicationError::MissingCanvasFrame {
                    canvas: name.into(),
                    node: old,
                })?;
        let info = snarl.get_node_info(old_id).expect("existing frame");
        for (axis, value) in [("x", info.pos.x), ("y", info.pos.y)] {
            if !value.is_finite() {
                return Err(DuplicationError::InvalidFramePosition {
                    node: old,
                    axis,
                    bits: value.to_bits(),
                });
            }
        }
        let position = info.pos + egui::vec2(32.0, 32.0);
        for (axis, value) in [("x", position.x), ("y", position.y)] {
            if !value.is_finite() {
                return Err(DuplicationError::InvalidFramePosition {
                    node: old,
                    axis,
                    bits: value.to_bits(),
                });
            }
        }
        let old_node = info.value;
        let new_node = match old_node {
            CanvasNode::Placeholder(_) => CanvasNode::Placeholder(new),
            _ => CanvasNode::Graph(new),
        };
        let new_id = if info.open {
            snarl.insert_node(position, new_node)
        } else {
            snarl.insert_node_collapsed(position, new_node)
        };
        if let Some(&size) = sizes.get(&old_node) {
            sizes.insert(new_node, size);
        }
        new_ids.insert(new, new_id);
    }
    for (&old, &new) in &plan.ids {
        let old_inputs = canvas_inputs(&project.graph.nodes[&old]);
        let new_inputs = canvas_inputs(&plan.nodes[&new]);
        for (input, (&old_input, &new_input)) in old_inputs.iter().zip(&new_inputs).enumerate() {
            let pin = canvas.snarl.in_pin(InPinId {
                node: old_ids[&old],
                input,
            });
            if pin.remotes.len() != 1 {
                return Err(DuplicationError::InvalidCanvasInput {
                    canvas: name.into(),
                    node: old,
                    input,
                });
            }
            let original = pin.remotes[0];
            let valid_original = if let Some(&expected) = old_ids.get(&old_input) {
                original.node == expected && original.output == 0
            } else if let Some(Node::SourceField { path, frame }) =
                project.graph.nodes.get(&old_input)
            {
                let blocks = source_blocks(&project.source);
                let exact = blocks
                    .iter()
                    .enumerate()
                    .flat_map(|(block, section)| {
                        section
                            .leaves
                            .iter()
                            .enumerate()
                            .filter(move |(_, leaf)| &leaf.frame == frame && &leaf.path == path)
                            .map(move |(pin, _)| (block, pin))
                    })
                    .next();
                let legacy = blocks
                    .iter()
                    .enumerate()
                    .flat_map(|(block, section)| {
                        section
                            .leaves
                            .iter()
                            .enumerate()
                            .filter(move |(_, leaf)| &leaf.path == path)
                            .map(move |(pin, _)| (block, pin))
                    })
                    .collect::<Vec<_>>();
                let expected =
                    exact.or_else(|| (frame.is_none() && legacy.len() == 1).then(|| legacy[0]));
                expected.is_some_and(|(block, output)| {
                    canvas.snarl.get_node(original.node) == Some(&CanvasNode::SourceBlock(block))
                        && original.output == output
                })
            } else {
                false
            };
            if !valid_original {
                return Err(DuplicationError::InvalidCanvasInput {
                    canvas: name.into(),
                    node: old,
                    input,
                });
            }
            let from = if new_input == old_input {
                original
            } else {
                OutPinId {
                    node: *new_ids.get(&new_input).ok_or_else(|| {
                        DuplicationError::InvalidCanvasInput {
                            canvas: name.into(),
                            node: old,
                            input,
                        }
                    })?,
                    output: original.output,
                }
            };
            snarl.connect(
                from,
                InPinId {
                    node: new_ids[&new],
                    input,
                },
            );
        }
    }
    Ok(StagedCanvas { snarl, sizes })
}

impl FerruleApp {
    pub(super) fn duplicate_sequence_consumer(
        &mut self,
        document: MappingDocument,
        node: NodeId,
    ) -> Result<DuplicationPlan, DuplicationError> {
        match document {
            MappingDocument::Function(_) => {
                return Err(DuplicationError::UnavailableContext {
                    context: "Function",
                });
            }
            MappingDocument::Target(index)
                if !self.mapping_workspace.target_canvases.contains_key(&index)
                    || index >= self.project.extra_targets.len() =>
            {
                return Err(DuplicationError::UnavailableContext { context: "Target" });
            }
            _ => {}
        }
        let plan = plan(&self.project, node)?;
        let main = stage_canvas("Main", &self.main_canvas, &self.project, &plan)?;
        let mut targets = BTreeMap::new();
        for (&index, canvas) in &self.mapping_workspace.target_canvases {
            if index >= self.project.extra_targets.len() {
                return Err(DuplicationError::UnavailableContext { context: "Target" });
            }
            targets.insert(
                index,
                stage_canvas(&format!("Target({index})"), canvas, &self.project, &plan)?,
            );
        }
        let mut project = self.project.clone();
        project.graph.nodes.extend(plan.nodes.clone());
        project
            .validate_filter_map_v1()
            .map_err(DuplicationError::FilterMapAdmission)?;
        // No fallible checks remain after this point. Existing canvas camera,
        // search, selection, scroll, tabs, saved marker and history stay intact.
        self.project = project;
        self.main_canvas.snarl = main.snarl;
        self.main_canvas.node_sizes = main.sizes;
        for (index, canvas) in targets {
            let current = self
                .mapping_workspace
                .target_canvases
                .get_mut(&index)
                .expect("staged existing canvas");
            current.snarl = canvas.snarl;
            current.node_sizes = canvas.sizes;
        }
        Ok(plan)
    }

    pub(super) fn apply_sequence_duplication_request(
        &mut self,
        document: MappingDocument,
        node: NodeId,
    ) {
        if !self.ui_project_editing_enabled() {
            return;
        }
        match self.duplicate_sequence_consumer(document, node) {
            Ok(plan) => {
                debug_assert!(plan.nodes.contains_key(&plan.duplicate));
                self.status = "Duplicated sequence consumer".into();
            }
            Err(error) => {
                self.status = "sequence duplication failed".into();
                self.diagnostics
                    .error("Could not duplicate sequence consumer", error.to_string());
            }
        }
    }
}

#[cfg(test)]
#[path = "sequence_consumer_duplication_tests.rs"]
mod tests;
