//! Private root-only native execution. Public ownership admission is unchanged.
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

use ir::{Instance, Value};
use mapping::{
    FailureIteration, FunctionId, Graph, IterationOutput, Node, NodeId, OwnedSourceField, Project,
    Scope, ScopeConstruction, SequenceExpr, SourceScopeLocation, SourceScopeStep,
    SourceScopeTarget, SourceSelectorRef,
};

use crate::eval_scope::eval_scope;
use crate::owned_source_context::{OwnedSourceContext, RootReadError, RootScalar};
use crate::trace::TraceScope;
use crate::{
    EngineError, ExecutionContext, ExecutionOutputs, NamedOutput, RequiredTargetSources,
    SelectedTargetOutput, TargetSelection,
};

/// Keeps the original pair allocation intact; both execution paths borrow it.
/// The storage is retained by the caller for complete borrowed diagnostics.
pub(super) const MAX_CAPTURED_ROOT_READS: usize = 128;

pub(super) struct PreparedInputs {
    frame: Instance,
    capture_limit: Option<usize>,
}
impl PreparedInputs {
    pub(super) fn new(named: Vec<(String, Instance)>) -> Self {
        Self {
            frame: Instance::Group(named.into()),
            capture_limit: None,
        }
    }
    /// Explicit opt-in; success diagnostics never grow beyond the fixed cap.
    pub(super) fn with_read_capture(mut self, limit: usize) -> Self {
        self.capture_limit = Some(limit.min(MAX_CAPTURED_ROOT_READS));
        self
    }
    pub(super) fn pairs(&self) -> &[(String, Instance)] {
        let Instance::Group(fields) = &self.frame else {
            unreachable!("constructed group")
        };
        fields.as_slice()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReadPhase {
    Admission,
    Execution,
}
#[derive(Debug, PartialEq)]
pub(super) enum RootExecutionError<'a> {
    Engine(EngineError),
    Read {
        phase: ReadPhase,
        node: NodeId,
        error: RootReadError<'a, 'a>,
    },
    Specialized {
        node: NodeId,
        function: Option<FunctionId>,
        request: &'a OwnedSourceField,
    },
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RootReadWitness<'a> {
    pub(super) node: NodeId,
    pub(super) read: RootScalar<'a, 'a>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CaptureSummary {
    pub(super) enabled: bool,
    pub(super) limit: usize,
    pub(super) attempted: usize,
    pub(super) resolved: usize,
    pub(super) captured: usize,
    pub(super) omitted: usize,
    pub(super) truncated: bool,
    pub(super) count_saturated: bool,
}
impl CaptureSummary {
    fn empty(limit: Option<usize>) -> Self {
        Self {
            enabled: limit.is_some(),
            limit: limit.unwrap_or(0),
            attempted: 0,
            resolved: 0,
            captured: 0,
            omitted: 0,
            truncated: false,
            count_saturated: false,
        }
    }
}
#[derive(Debug)]
pub(super) struct RootExecution<'a, T> {
    pub(super) result: Result<T, RootExecutionError<'a>>,
    pub(super) reads: Vec<RootReadWitness<'a>>,
    pub(super) capture: CaptureSummary,
}

/// Erases the longer caller-input lifetime, without erasing the captured error.
pub(super) trait RootReader {
    fn read(&self, node: NodeId) -> Result<Value, EngineError>;
}
struct Reader<'a> {
    graph: &'a Graph,
    registry: OwnedSourceContext<'a>,
    reads: RefCell<Vec<RootReadWitness<'a>>>,
    failure: RefCell<Option<(NodeId, RootReadError<'a, 'a>)>>,
    capture_limit: Option<usize>,
    attempted: Cell<usize>,
    resolved: Cell<usize>,
    count_saturated: Cell<bool>,
}
impl<'a> RootReader for Reader<'a> {
    fn read(&self, node: NodeId) -> Result<Value, EngineError> {
        self.count(&self.attempted);
        let graph: &'a Graph = self.graph;
        let Some(Node::OwnedSourceField(request)) = graph.nodes.get(&node) else {
            return Err(EngineError::MissingNode(node));
        };
        match self.registry.scalar(request) {
            Ok(read) => {
                self.count(&self.resolved);
                let captured = self.reads.borrow().len();
                if let Some(limit) = self.capture_limit
                    && captured < limit
                {
                    self.reads.borrow_mut().push(RootReadWitness { node, read });
                }
                match read.value {
                    Some(value) => Ok(value.clone()),
                    None if request.frame.is_some() => Ok(Value::Null),
                    None => Err(EngineError::MissingSourceField(
                        request.source.path.join("/"),
                    )),
                }
            }
            Err(error) => {
                let mut failure = self.failure.borrow_mut();
                if failure.is_none() {
                    *failure = Some((node, error));
                }
                // Only this private reader installs this carrier. The outer
                // borrowed result restores the complete typed error.
                Err(EngineError::UnsupportedSourceOwnership {
                    node,
                    function: None,
                    kind: "private_root_read",
                })
            }
        }
    }
}
impl<'a> Reader<'a> {
    fn count(&self, counter: &Cell<usize>) {
        match counter.get().checked_add(1) {
            Some(count) => counter.set(count),
            None => self.count_saturated.set(true),
        }
    }
    fn finish<T>(self, result: Result<T, EngineError>) -> RootExecution<'a, T> {
        let reads = self.reads.into_inner();
        let omitted = self.resolved.get().saturating_sub(reads.len());
        let capture = CaptureSummary {
            enabled: self.capture_limit.is_some(),
            limit: self.capture_limit.unwrap_or(0),
            attempted: self.attempted.get(),
            resolved: self.resolved.get(),
            captured: reads.len(),
            omitted,
            truncated: self.capture_limit.is_some() && omitted > 0,
            count_saturated: self.count_saturated.get(),
        };
        let failure = self.failure.into_inner();
        let result = match (result, failure) {
            (
                Err(EngineError::UnsupportedSourceOwnership {
                    node,
                    function: None,
                    kind: "private_root_read",
                }),
                Some((failed_node, error)),
            ) if node == failed_node => Err(RootExecutionError::Read {
                phase: ReadPhase::Execution,
                node,
                error,
            }),
            (result, _) => result.map_err(RootExecutionError::Engine),
        };
        RootExecution {
            result,
            reads,
            capture,
        }
    }
}

pub(super) fn required_sources<'a>(
    project: &'a Project,
    selection: Option<TargetSelection<'_>>,
) -> Result<RequiredTargetSources<'a>, RootExecutionError<'a>> {
    crate::required_sources::required_for_private_roots(project, selection)
}

pub(super) fn run_outputs<'a>(
    project: &'a Project,
    primary: &'a Instance,
    inputs: &'a PreparedInputs,
    execution: Option<&ExecutionContext<'_>>,
) -> RootExecution<'a, ExecutionOutputs> {
    execute(
        project,
        primary,
        inputs,
        execution,
        None,
        |program, context| {
            let primary = eval_scope(
                program,
                &project.root,
                Some(&project.target),
                context,
                &[],
                &project.extra_sources,
                execution.and_then(|host| host.dynamic_source_loader),
                &TraceScope::primary(),
            )?;
            let mut targets = Vec::with_capacity(project.extra_targets.len());
            for (index, target) in project.extra_targets.iter().enumerate() {
                targets.push(NamedOutput {
                    name: target.name.clone(),
                    instance: eval_scope(
                        program,
                        &target.root,
                        Some(&target.schema),
                        context,
                        &[],
                        &project.extra_sources,
                        execution.and_then(|host| host.dynamic_source_loader),
                        &TraceScope::named(&target.name, index),
                    )?,
                });
            }
            Ok(ExecutionOutputs {
                primary,
                extras: targets,
            })
        },
    )
}

pub(super) fn run_selected<'a>(
    project: &'a Project,
    primary: &'a Instance,
    inputs: &'a PreparedInputs,
    execution: Option<&ExecutionContext<'_>>,
    selection: TargetSelection<'_>,
) -> RootExecution<'a, SelectedTargetOutput> {
    let selected = match crate::required_sources::selected_target(project, selection) {
        Ok(selected) => selected,
        Err(error) => {
            return RootExecution {
                result: Err(RootExecutionError::Engine(error)),
                reads: vec![],
                capture: CaptureSummary::empty(inputs.capture_limit),
            };
        }
    };
    execute(
        project,
        primary,
        inputs,
        execution,
        Some(selection),
        |program, context| {
            let (root, schema, trace) = match selected {
                None => (&project.root, &project.target, TraceScope::primary()),
                Some(target) => {
                    let index = project
                        .extra_targets
                        .iter()
                        .position(|candidate| std::ptr::eq(candidate, target))
                        .expect("selected target belongs to project");
                    (
                        &target.root,
                        &target.schema,
                        TraceScope::named(&target.name, index),
                    )
                }
            };
            let instance = eval_scope(
                program,
                root,
                Some(schema),
                context,
                &[],
                &project.extra_sources,
                execution.and_then(|host| host.dynamic_source_loader),
                &trace,
            )?;
            Ok(match selected {
                None => SelectedTargetOutput::Primary(instance),
                Some(target) => SelectedTargetOutput::Named(NamedOutput {
                    name: target.name.clone(),
                    instance,
                }),
            })
        },
    )
}

fn execute<'a, T>(
    project: &'a Project,
    primary: &'a Instance,
    inputs: &'a PreparedInputs,
    execution: Option<&ExecutionContext<'_>>,
    selection: Option<TargetSelection<'_>>,
    evaluate: impl FnOnce(crate::eval_expr::EvalProgram<'_>, &[&Instance]) -> Result<T, EngineError>,
) -> RootExecution<'a, T> {
    if let Err(error) = admit(project, selection) {
        return RootExecution {
            result: Err(error),
            reads: vec![],
            capture: CaptureSummary::empty(inputs.capture_limit),
        };
    }
    let reader = Reader {
        graph: &project.graph,
        registry: OwnedSourceContext::new(project, Some(primary), inputs.pairs()),
        reads: RefCell::new(vec![]),
        failure: RefCell::new(None),
        capture_limit: inputs.capture_limit,
        attempted: Cell::new(0),
        resolved: Cell::new(0),
        count_saturated: Cell::new(false),
    };
    let result = crate::evaluate_prepared(
        project,
        primary,
        &inputs.frame,
        execution,
        Some(&reader),
        evaluate,
    );
    reader.finish(result)
}

pub(super) fn admit<'a>(
    project: &'a Project,
    selection: Option<TargetSelection<'_>>,
) -> Result<(), RootExecutionError<'a>> {
    let roots = match selection {
        Some(selection) => {
            let selected = crate::required_sources::selected_target(project, selection)
                .map_err(RootExecutionError::Engine)?;
            match selected {
                None => vec![(
                    &project.root,
                    SourceScopeLocation::root(SourceScopeTarget::Primary),
                )],
                Some(target) => {
                    let index = project
                        .extra_targets
                        .iter()
                        .position(|candidate| std::ptr::eq(candidate, target))
                        .expect("selected target belongs to project");
                    vec![(
                        &target.root,
                        SourceScopeLocation::root(SourceScopeTarget::Named {
                            index,
                            name: target.name.clone(),
                        }),
                    )]
                }
            }
        }
        None => std::iter::once((
            &project.root,
            SourceScopeLocation::root(SourceScopeTarget::Primary),
        ))
        .chain(
            project
                .extra_targets
                .iter()
                .enumerate()
                .map(|(index, target)| {
                    (
                        &target.root,
                        SourceScopeLocation::root(SourceScopeTarget::Named {
                            index,
                            name: target.name.clone(),
                        }),
                    )
                }),
        )
        .collect(),
    };
    let mut admission = Admission {
        project,
        pending: vec![],
        visited: BTreeSet::new(),
        functions: BTreeSet::new(),
        dynamic: BTreeSet::new(),
    };
    admission.scopes(roots)?;
    for rule in &project.failure_rules {
        let specialized = match &rule.iteration {
            FailureIteration::Source { collection } => {
                admission.path(collection);
                false
            }
            FailureIteration::Sequence { sequence } => {
                admission.sequence(sequence, None);
                true
            }
        };
        admission.pending.extend(
            rule.selection
                .predicate()
                .map(|node| (None, node, specialized)),
        );
        admission
            .pending
            .extend(rule.message.map(|node| (None, node, specialized)));
    }
    admission.graph()
}

struct Admission<'a> {
    project: &'a Project,
    pending: Vec<(Option<FunctionId>, NodeId, bool)>,
    visited: BTreeSet<(Option<FunctionId>, NodeId, bool)>,
    functions: BTreeSet<FunctionId>,
    dynamic: BTreeSet<usize>,
}
impl<'a> Admission<'a> {
    fn scopes(
        &mut self,
        roots: Vec<(&'a Scope, SourceScopeLocation)>,
    ) -> Result<(), RootExecutionError<'a>> {
        let mut pending: Vec<_> = roots
            .into_iter()
            .map(|(scope, location)| (scope, location, false))
            .collect();
        while let Some((scope, location, inherited)) = pending.pop() {
            match scope.source_selector() {
                Some(SourceSelectorRef::Owned(_)) => {
                    return Err(RootExecutionError::Engine(
                        EngineError::UnsupportedSourceIteration { scope: location },
                    ));
                }
                Some(SourceSelectorRef::Legacy(path)) => self.path(path),
                None => {}
            }
            let dynamic = scope.source_selector().is_some_and(|source| match source {
                SourceSelectorRef::Legacy(path) => path.first().is_some_and(|name| {
                    self.project
                        .extra_sources
                        .iter()
                        .any(|source| source.name == *name && source.dynamic_path.is_some())
                }),
                SourceSelectorRef::Owned(_) => false,
            });
            let specialized = inherited
                || dynamic
                || scope.sequence().is_some()
                || scope.join().is_some()
                || scope.concatenated().is_some()
                || scope.has_grouping()
                || scope.output_path().is_some()
                || scope.iteration_output == IterationOutput::MappedSequence
                || !matches!(
                    scope.construction,
                    ScopeConstruction::Constructed | ScopeConstruction::Scalar { .. }
                );
            if let Some(sequence) = scope.sequence() {
                self.sequence(sequence, None);
            }
            if let Some((_, join)) = scope.join() {
                for source in join.sources() {
                    self.path(source.collection());
                }
            }
            if let Some(segments) = scope.concatenated() {
                pending.extend(segments.iter().enumerate().map(|(index, segment)| {
                    (
                        segment,
                        location.descendant(SourceScopeStep::Segment(index)),
                        specialized,
                    )
                }));
            }
            self.pending
                .extend(scope.filter.map(|node| (None, node, specialized)));
            self.pending.extend(
                scope
                    .post_group_filter
                    .map(|node| (None, node, specialized)),
            );
            self.pending
                .extend(scope.grouping_nodes().map(|node| (None, node, specialized)));
            self.pending
                .extend(scope.sort_by.map(|node| (None, node, specialized)));
            self.pending
                .extend(scope.output_path().map(|node| (None, node, specialized)));
            self.pending.extend(
                scope
                    .windows
                    .iter()
                    .copied()
                    .flat_map(|window| window.nodes())
                    .map(|node| (None, node, specialized)),
            );
            self.pending.extend(
                scope
                    .sort_then_by
                    .iter()
                    .map(|key| (None, key.node, specialized)),
            );
            self.pending.extend(
                scope
                    .bindings
                    .iter()
                    .map(|binding| (None, binding.node, specialized)),
            );
            self.pending
                .extend(scope.dynamic_bindings.iter().flat_map(|binding| {
                    [
                        (None, binding.key, specialized),
                        (None, binding.value, specialized),
                    ]
                }));
            match &scope.construction {
                ScopeConstruction::Scalar { value } => {
                    self.pending.push((None, *value, specialized))
                }
                ScopeConstruction::RecursiveFilter { plan } => {
                    self.pending.push((None, plan.predicate(), true))
                }
                ScopeConstruction::PathHierarchy { plan } => self.path(plan.collection()),
                ScopeConstruction::AdjacencyTree { plan } => {
                    self.path(plan.collection());
                    self.pending
                        .extend(plan.root().map(|node| (None, node, true)));
                }
                _ => {}
            }
            pending.extend(scope.children.iter().enumerate().map(|(index, child)| {
                (
                    child,
                    location.descendant(SourceScopeStep::Child(index)),
                    specialized,
                )
            }));
            for (index, child) in scope.dynamic_children.iter().enumerate() {
                self.pending.push((None, child.key, specialized));
                pending.push((
                    &child.scope,
                    location.descendant(SourceScopeStep::DynamicChild(index)),
                    specialized,
                ));
            }
        }
        Ok(())
    }
    fn path(&mut self, path: &[String]) {
        let Some(name) = path.first() else { return };
        let project = self.project;
        for (index, source) in project
            .extra_sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.name == *name)
        {
            if let Some(dynamic) = &source.dynamic_path
                && self.dynamic.insert(index)
            {
                self.pending.push((None, dynamic.node, true));
                if let Some(driver) = dynamic.iteration.first() {
                    self.path(std::slice::from_ref(driver));
                }
            }
        }
    }
    fn function(&mut self, function: FunctionId) {
        if self.functions.insert(function)
            && let Some(body) = self.project.user_functions.get(&function)
        {
            self.pending.push((Some(function), body.output, true));
        }
    }
    fn sequence(&mut self, sequence: &SequenceExpr, function: Option<FunctionId>) {
        self.pending.extend(
            sequence
                .inputs()
                .into_iter()
                .map(|node| (function, node, true)),
        );
        self.pending.extend(
            sequence
                .owned_items()
                .into_iter()
                .map(|node| (function, node, true)),
        );
        match sequence {
            SequenceExpr::RecursiveCollect { collection, .. } => self.path(collection),
            SequenceExpr::FilterMapV1(composition) => {
                self.function(composition.predicate);
                self.function(composition.mapper);
                self.sequence(&composition.source, function);
            }
            _ => {}
        }
    }
    fn graph(&mut self) -> Result<(), RootExecutionError<'a>> {
        // No inputs are supplied to the metadata-only probe. MissingEndpoint
        // proves the entire declared scalar path was accepted before any data read.
        let catalogue = OwnedSourceContext::new(self.project, None, &[]);
        while let Some((function, id, specialized)) = self.pending.pop() {
            if !self.visited.insert((function, id, specialized)) {
                continue;
            }
            let project: &'a Project = self.project;
            let graph = function
                .and_then(|function| {
                    project
                        .user_functions
                        .get(&function)
                        .map(|function| &function.body)
                })
                .or_else(|| function.is_none().then_some(&project.graph));
            let Some(node) = graph.and_then(|graph| graph.nodes.get(&id)) else {
                continue;
            };
            match node {
                Node::OwnedSourceField(_) if function.is_some() => {
                    return Err(RootExecutionError::Engine(
                        EngineError::UnsupportedSourceOwnership {
                            node: id,
                            function,
                            kind: "owned_source_field",
                        },
                    ));
                }
                Node::OwnedSourceField(request) if specialized => {
                    return Err(RootExecutionError::Specialized {
                        node: id,
                        function,
                        request,
                    });
                }
                Node::OwnedSourceField(request) => match catalogue.scalar(request) {
                    Err(RootReadError::MissingEndpoint { .. }) => {}
                    Err(error) => {
                        return Err(RootExecutionError::Read {
                            phase: ReadPhase::Admission,
                            node: id,
                            error,
                        });
                    }
                    Ok(_) => unreachable!("metadata-only registry has no supplied roots"),
                },
                Node::OwnedPosition(_) => {
                    return Err(RootExecutionError::Engine(
                        EngineError::UnsupportedSourceOwnership {
                            node: id,
                            function,
                            kind: "owned_position",
                        },
                    ));
                }
                _ => {}
            }
            let nested = specialized
                || matches!(
                    node,
                    Node::Aggregate { .. }
                        | Node::CollectionFind { .. }
                        | Node::JoinAggregate { .. }
                        | Node::SequenceExists { .. }
                        | Node::SequenceItemAt { .. }
                        | Node::SequenceAggregate { .. }
                );
            self.pending.extend(
                node.dependencies()
                    .into_iter()
                    .map(|node| (function, node, nested)),
            );
            match node {
                Node::SourceField { path, frame }
                | Node::DynamicSourceField {
                    object: path,
                    frame,
                    ..
                }
                | Node::XmlSerialize { path, frame, .. } => {
                    self.path(frame.as_deref().unwrap_or(path))
                }
                Node::Position { collection }
                | Node::JoinField { collection, .. }
                | Node::Lookup { collection, .. }
                | Node::CollectionFind { collection, .. }
                | Node::Aggregate { collection, .. } => self.path(collection),
                Node::XmlMixedContent {
                    path,
                    frame,
                    replacements,
                } => {
                    self.path(frame.as_deref().unwrap_or(path));
                    for replacement in replacements {
                        self.path(&replacement.collection);
                    }
                }
                Node::SequenceExists { sequence, .. }
                | Node::SequenceItemAt { sequence, .. }
                | Node::SequenceAggregate { sequence, .. } => self.sequence(sequence, function),
                Node::JoinAggregate { plan, .. } => {
                    for source in plan.sources() {
                        self.path(source.collection());
                    }
                }
                Node::UserFunctionCall { function, .. } => self.function(*function),
                _ => {}
            }
        }
        Ok(())
    }
}
