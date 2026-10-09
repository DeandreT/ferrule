//! Structural admission for the initial ordered scalar filter/map descriptor.
//! This module evaluates no expressions and supplies no execution capability.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ir::ScalarType;
use serde::{Deserialize, Serialize};

use crate::{FunctionId, Graph, Node, NodeId, Project, Scope, SequenceExpr, UserFunction};

pub const MAX_FILTER_MAP_CAPTURES: usize = 16;
pub const MAX_FILTER_MAP_FUNCTION_PARAMETERS: usize = 18;
pub const MAX_FILTER_MAP_FUNCTION_DEPTH: usize = 64;

/// One parent expression evaluated once, before any stage invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterMapCapture {
    pub node: NodeId,
    pub ty: ScalarType,
}

/// Proposed sequence shape. Native and generated execution remain unsupported.
/// Only this new body has strict unknown-field admission; legacy variants do not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterMapV1 {
    pub source: Box<SequenceExpr>,
    pub item: NodeId,
    pub predicate: FunctionId,
    pub mapper: FunctionId,
    pub output_type: ScalarType,
    pub captures: Vec<FilterMapCapture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMapStage {
    Predicate,
    Mapper,
}

/// An exact structural refusal, before any source/capture/stage evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterMapAdmissionError {
    pub item: NodeId,
    pub location: String,
    pub kind: FilterMapAdmissionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterMapAdmissionKind {
    UnsupportedSource {
        found: &'static str,
    },
    UnsupportedConsumer {
        site: &'static str,
    },
    DuplicatePrivateOwner {
        item: NodeId,
        count: usize,
    },
    MissingGraphNode {
        role: &'static str,
        node: NodeId,
    },
    InvalidPrivateOwner {
        item: NodeId,
    },
    CaptureCount {
        found: usize,
        max: usize,
    },
    PrivateCapture {
        capture: usize,
        node: NodeId,
        item: NodeId,
    },
    PrivateSourceArgument {
        node: NodeId,
        item: NodeId,
    },
    GraphCycle {
        role: &'static str,
        path: Vec<NodeId>,
    },
    MissingFunction {
        function: FunctionId,
    },
    StageSignature {
        stage: FilterMapStage,
        function: FunctionId,
        expected_parameters: Vec<ScalarType>,
        found_parameters: Vec<ScalarType>,
        expected_output: ScalarType,
        found_output: ScalarType,
    },
    FunctionParameterCount {
        function: FunctionId,
        found: usize,
        max: usize,
    },
    DuplicateParameter {
        function: FunctionId,
        parameter: u64,
    },
    MissingParameter {
        function: FunctionId,
        node: NodeId,
        parameter: u64,
    },
    MissingBodyNode {
        function: FunctionId,
        node: Option<NodeId>,
        referenced: NodeId,
    },
    UnsupportedStageNode {
        function: FunctionId,
        node: NodeId,
        kind: &'static str,
    },
    UnsupportedBuiltin {
        function: FunctionId,
        node: NodeId,
        builtin: String,
    },
    BuiltinArity {
        function: FunctionId,
        node: NodeId,
        found: usize,
        expected: usize,
    },
    FunctionArity {
        function: FunctionId,
        node: NodeId,
        callee: FunctionId,
        found: usize,
        expected: usize,
    },
    FunctionCycle {
        path: Vec<FunctionId>,
    },
    FunctionDepth {
        path: Vec<FunctionId>,
        limit: usize,
    },
    BodyCycle {
        function: FunctionId,
        path: Vec<NodeId>,
    },
}

impl fmt::Display for FilterMapAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "filter_map_v1 item {} at {}: {:?}",
            self.item, self.location, self.kind
        )
    }
}

impl std::error::Error for FilterMapAdmissionError {}

struct Site<'a> {
    sequence: &'a SequenceExpr,
    location: String,
    refused_consumer: Option<&'static str>,
}

impl Project {
    /// All new descriptors, including unsupported consumer sites, for guards.
    /// This preserves no execution result and follows project declaration order.
    pub fn filter_map_v1_descriptors(&self) -> Vec<&FilterMapV1> {
        sites(self)
            .into_iter()
            .filter_map(|site| match site.sequence {
                SequenceExpr::FilterMapV1(composition) => Some(composition),
                _ => None,
            })
            .collect()
    }

    /// Admit only the new descriptors; ordinary UDF and sequence policies remain
    /// with their existing validators. Static failures stay visible in unselected
    /// branches. No capture value, UDF body or generator is evaluated here.
    pub fn validate_filter_map_v1(&self) -> Result<(), FilterMapAdmissionError> {
        let sites = sites(self);
        if !sites
            .iter()
            .any(|site| matches!(site.sequence, SequenceExpr::FilterMapV1(_)))
        {
            return Ok(());
        }
        let mut owners = BTreeMap::new();
        for site in &sites {
            for item in site.sequence.owned_items() {
                *owners.entry(item).or_insert(0usize) += 1;
            }
        }
        for site in sites {
            let SequenceExpr::FilterMapV1(composition) = site.sequence else {
                continue;
            };
            let error = |kind| FilterMapAdmissionError {
                item: composition.item,
                location: site.location.clone(),
                kind,
            };
            if let Some(consumer) = site.refused_consumer {
                return Err(error(FilterMapAdmissionKind::UnsupportedConsumer {
                    site: consumer,
                }));
            }
            composition
                .admit(&self.graph, &self.user_functions, &owners)
                .map_err(error)?;
        }
        Ok(())
    }
}

fn sites(project: &Project) -> Vec<Site<'_>> {
    let mut result = Vec::new();
    collect_scopes(&project.root, "primary target", &mut result);
    for target in &project.extra_targets {
        collect_scopes(
            &target.root,
            &format!("named target `{}`", target.name),
            &mut result,
        );
    }
    collect_graph(&project.graph, None, &mut result);
    for (index, rule) in project.failure_rules.iter().enumerate() {
        if let crate::FailureIteration::Sequence { sequence } = &rule.iteration {
            result.push(Site {
                sequence,
                location: format!("failure rule {}", index + 1),
                refused_consumer: Some("failure rule"),
            });
        }
    }
    for (&function, definition) in &project.user_functions {
        collect_graph(&definition.body, Some(function), &mut result);
    }
    result
}

fn collect_scopes<'a>(root: &'a Scope, endpoint: &str, result: &mut Vec<Site<'a>>) {
    let mut pending = vec![(root, endpoint.to_owned())];
    while let Some((scope, location)) = pending.pop() {
        if let Some(sequence) = scope.sequence() {
            result.push(Site {
                sequence,
                location: location.clone(),
                refused_consumer: None,
            });
        }
        for (index, child) in scope.dynamic_children.iter().enumerate().rev() {
            pending.push((&child.scope, format!("{location}/dynamic child {index}")));
        }
        for (index, child) in scope.children.iter().enumerate().rev() {
            pending.push((child, format!("{location}/child {index}")));
        }
        if let Some(segments) = scope.concatenated() {
            for (index, segment) in segments
                .iter()
                .enumerate()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                pending.push((segment, format!("{location}/segment {index}")));
            }
        }
    }
}

fn collect_graph<'a>(graph: &'a Graph, function: Option<FunctionId>, result: &mut Vec<Site<'a>>) {
    for (&node, expression) in &graph.nodes {
        if let Node::SequenceExists { sequence, .. }
        | Node::SequenceItemAt { sequence, .. }
        | Node::SequenceAggregate { sequence, .. } = expression
        {
            result.push(Site {
                sequence,
                location: function.map_or_else(
                    || format!("graph node {node}"),
                    |function| format!("function {} body node {node}", function.get()),
                ),
                refused_consumer: function.map(|_| "user-function body"),
            });
        }
    }
}

impl FilterMapV1 {
    fn admit(
        &self,
        graph: &Graph,
        functions: &BTreeMap<FunctionId, UserFunction>,
        owners: &BTreeMap<NodeId, usize>,
    ) -> Result<(), FilterMapAdmissionKind> {
        let SequenceExpr::Generate {
            from,
            to,
            item: source_item,
        } = self.source.as_ref()
        else {
            return Err(FilterMapAdmissionKind::UnsupportedSource {
                found: self.source.kind_name(),
            });
        };
        for item in [*source_item, self.item] {
            let count = owners.get(&item).copied().unwrap_or(0);
            if count != 1 {
                return Err(FilterMapAdmissionKind::DuplicatePrivateOwner { item, count });
            }
            match graph.nodes.get(&item) {
                None => {
                    return Err(FilterMapAdmissionKind::MissingGraphNode {
                        role: "private owner",
                        node: item,
                    });
                }
                Some(Node::SourceField { path, frame: None }) if path.is_empty() => {}
                Some(_) => return Err(FilterMapAdmissionKind::InvalidPrivateOwner { item }),
            }
        }
        for node in from.iter().copied().chain([*to]) {
            validate_parent_graph(graph, node, "source", |item| {
                (*source_item == item || self.item == item)
                    .then_some(FilterMapAdmissionKind::PrivateSourceArgument { node, item })
            })?;
        }
        if self.captures.len() > MAX_FILTER_MAP_CAPTURES {
            return Err(FilterMapAdmissionKind::CaptureCount {
                found: self.captures.len(),
                max: MAX_FILTER_MAP_CAPTURES,
            });
        }
        for (capture, value) in self.captures.iter().enumerate() {
            validate_parent_graph(graph, value.node, "capture", |item| {
                owners
                    .contains_key(&item)
                    .then_some(FilterMapAdmissionKind::PrivateCapture {
                        capture,
                        node: value.node,
                        item,
                    })
            })?;
        }
        let parameters: Vec<_> = [ScalarType::Int, ScalarType::Int]
            .into_iter()
            .chain(self.captures.iter().map(|capture| capture.ty))
            .collect();
        for (stage, function, output) in [
            (FilterMapStage::Predicate, self.predicate, ScalarType::Bool),
            (FilterMapStage::Mapper, self.mapper, self.output_type),
        ] {
            let definition = functions
                .get(&function)
                .ok_or(FilterMapAdmissionKind::MissingFunction { function })?;
            let found: Vec<_> = definition
                .parameters
                .iter()
                .map(|parameter| parameter.ty)
                .collect();
            if found != parameters || definition.output_type != output {
                return Err(FilterMapAdmissionKind::StageSignature {
                    stage,
                    function,
                    expected_parameters: parameters.clone(),
                    found_parameters: found,
                    expected_output: output,
                    found_output: definition.output_type,
                });
            }
        }
        let mut memo = BTreeMap::new();
        validate_function(self.predicate, functions, &mut Vec::new(), &mut memo)?;
        validate_function(self.mapper, functions, &mut Vec::new(), &mut memo)?;
        Ok(())
    }
}

fn validate_parent_graph(
    graph: &Graph,
    root: NodeId,
    role: &'static str,
    private: impl Fn(NodeId) -> Option<FilterMapAdmissionKind>,
) -> Result<(), FilterMapAdmissionKind> {
    let mut complete = BTreeSet::new();
    let mut active = Vec::new();
    let mut pending = vec![(root, false)];
    while let Some((node, leaving)) = pending.pop() {
        if leaving {
            active.pop();
            complete.insert(node);
            continue;
        }
        if let Some(error) = private(node) {
            return Err(error);
        }
        if active.contains(&node) {
            return Err(FilterMapAdmissionKind::GraphCycle {
                role,
                path: active.iter().copied().chain([node]).collect(),
            });
        }
        if complete.contains(&node) {
            continue;
        }
        let expression = graph
            .nodes
            .get(&node)
            .ok_or(FilterMapAdmissionKind::MissingGraphNode { role, node })?;
        active.push(node);
        pending.push((node, true));
        pending.extend(
            expression
                .dependencies()
                .into_iter()
                .rev()
                .map(|input| (input, false)),
        );
    }
    Ok(())
}

fn validate_function(
    id: FunctionId,
    functions: &BTreeMap<FunctionId, UserFunction>,
    stack: &mut Vec<FunctionId>,
    memo: &mut BTreeMap<FunctionId, Vec<FunctionId>>,
) -> Result<Vec<FunctionId>, FilterMapAdmissionKind> {
    if stack.contains(&id) {
        return Err(FilterMapAdmissionKind::FunctionCycle {
            path: stack.iter().copied().chain([id]).collect(),
        });
    }
    if let Some(longest) = memo.get(&id) {
        if stack.len() + longest.len() > MAX_FILTER_MAP_FUNCTION_DEPTH {
            return Err(FilterMapAdmissionKind::FunctionDepth {
                path: stack
                    .iter()
                    .copied()
                    .chain(longest.iter().copied())
                    .collect(),
                limit: MAX_FILTER_MAP_FUNCTION_DEPTH,
            });
        }
        return Ok(longest.clone());
    }
    if stack.len() >= MAX_FILTER_MAP_FUNCTION_DEPTH {
        return Err(FilterMapAdmissionKind::FunctionDepth {
            path: stack.iter().copied().chain([id]).collect(),
            limit: MAX_FILTER_MAP_FUNCTION_DEPTH,
        });
    }
    let definition = functions
        .get(&id)
        .ok_or(FilterMapAdmissionKind::MissingFunction { function: id })?;
    if definition.parameters.len() > MAX_FILTER_MAP_FUNCTION_PARAMETERS {
        return Err(FilterMapAdmissionKind::FunctionParameterCount {
            function: id,
            found: definition.parameters.len(),
            max: MAX_FILTER_MAP_FUNCTION_PARAMETERS,
        });
    }
    let mut parameters = BTreeSet::new();
    for parameter in &definition.parameters {
        if !parameters.insert(parameter.id) {
            return Err(FilterMapAdmissionKind::DuplicateParameter {
                function: id,
                parameter: parameter.id.get(),
            });
        }
    }
    if !definition.body.nodes.contains_key(&definition.output) {
        return Err(FilterMapAdmissionKind::MissingBodyNode {
            function: id,
            node: None,
            referenced: definition.output,
        });
    }
    stack.push(id);
    let mut longest = vec![id];
    for (&node, expression) in &definition.body.nodes {
        match expression {
            Node::Const { .. } | Node::Unconnected | Node::If { .. } | Node::Raise { .. } => {}
            Node::FunctionParameter { parameter } => {
                if !parameters.contains(parameter) {
                    return Err(FilterMapAdmissionKind::MissingParameter {
                        function: id,
                        node,
                        parameter: parameter.get(),
                    });
                }
            }
            Node::Call { function, args } => {
                if !matches!(
                    function.as_str(),
                    "add"
                        | "multiply"
                        | "divide"
                        | "equal"
                        | "not_equal"
                        | "less_than"
                        | "greater_than"
                        | "concat"
                ) {
                    return Err(FilterMapAdmissionKind::UnsupportedBuiltin {
                        function: id,
                        node,
                        builtin: function.clone(),
                    });
                }
                if args.len() != 2 {
                    return Err(FilterMapAdmissionKind::BuiltinArity {
                        function: id,
                        node,
                        found: args.len(),
                        expected: 2,
                    });
                }
            }
            Node::UserFunctionCall { .. } => {}
            other => {
                return Err(FilterMapAdmissionKind::UnsupportedStageNode {
                    function: id,
                    node,
                    kind: unsupported_kind(other),
                });
            }
        }
        for referenced in expression.dependencies() {
            if !definition.body.nodes.contains_key(&referenced) {
                return Err(FilterMapAdmissionKind::MissingBodyNode {
                    function: id,
                    node: Some(node),
                    referenced,
                });
            }
        }
        if let Node::UserFunctionCall {
            function: callee,
            args,
        } = expression
        {
            let called = functions
                .get(callee)
                .ok_or(FilterMapAdmissionKind::MissingFunction { function: *callee })?;
            if args.len() != called.parameters.len() {
                return Err(FilterMapAdmissionKind::FunctionArity {
                    function: id,
                    node,
                    callee: *callee,
                    found: args.len(),
                    expected: called.parameters.len(),
                });
            }
            let child = validate_function(*callee, functions, stack, memo)?;
            if child.len() + 1 > longest.len() {
                longest = [vec![id], child].concat();
            }
        }
    }
    validate_body_cycles(id, &definition.body)?;
    stack.pop();
    memo.insert(id, longest.clone());
    Ok(longest)
}

fn validate_body_cycles(function: FunctionId, graph: &Graph) -> Result<(), FilterMapAdmissionKind> {
    let mut complete = BTreeSet::new();
    for &root in graph.nodes.keys() {
        if complete.contains(&root) {
            continue;
        }
        let mut active = Vec::new();
        let mut pending = vec![(root, false)];
        while let Some((node, leaving)) = pending.pop() {
            if leaving {
                active.pop();
                complete.insert(node);
                continue;
            }
            if active.contains(&node) {
                return Err(FilterMapAdmissionKind::BodyCycle {
                    function,
                    path: active.iter().copied().chain([node]).collect(),
                });
            }
            if complete.contains(&node) {
                continue;
            }
            let Some(expression) = graph.nodes.get(&node) else {
                continue;
            };
            active.push(node);
            pending.push((node, true));
            pending.extend(
                expression
                    .dependencies()
                    .into_iter()
                    .rev()
                    .map(|input| (input, false)),
            );
        }
    }
    Ok(())
}

fn unsupported_kind(node: &Node) -> &'static str {
    match node {
        Node::Position { .. } => "position",
        Node::SourceField { .. } => "source_field",
        Node::RuntimeValue { .. } => "runtime_value",
        Node::RuntimeParameter { .. } => "runtime_parameter",
        Node::RuntimeParameterDefault { .. } => "runtime_parameter_default",
        Node::ValueMap { .. } => "value_map",
        Node::SequenceExists { .. } => "sequence_exists",
        Node::SequenceItemAt { .. } => "sequence_item_at",
        Node::SequenceAggregate { .. } => "sequence_aggregate",
        _ => "context-dependent node",
    }
}
