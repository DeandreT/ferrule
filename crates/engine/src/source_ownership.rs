//! Admission for new owned expressions; this module acquires no input or frame.
use std::collections::BTreeSet;

use mapping::{
    FailureIteration, FunctionId, Node, NodeId, Project, Scope, ScopeConstruction, SequenceExpr,
};

use crate::{EngineError, TargetSelection};

fn has_owned_nodes(project: &Project) -> bool {
    project
        .graph
        .nodes
        .values()
        .chain(
            project
                .user_functions
                .values()
                .flat_map(|function| function.body.nodes.values()),
        )
        .any(|node| node.owned_source_kind().is_some())
}

pub(crate) fn check_project(
    project: &Project,
    selection: Option<TargetSelection<'_>>,
) -> Result<(), EngineError> {
    // Legacy projects gain no target lookup, graph traversal or error precedence change.
    if !has_owned_nodes(project) {
        return Ok(());
    }
    match selection {
        Some(selection) => {
            let target = crate::required_sources::selected_target(project, selection)?;
            check_roots(
                project,
                [target.map_or(&project.root, |target| &target.root)],
            )
        }
        None => check_roots(
            project,
            std::iter::once(&project.root)
                .chain(project.extra_targets.iter().map(|target| &target.root)),
        ),
    }
}

pub(crate) fn check_roots<'a>(
    project: &'a Project,
    roots: impl IntoIterator<Item = &'a Scope>,
) -> Result<(), EngineError> {
    if !has_owned_nodes(project) {
        return Ok(());
    }
    let mut admission = Admission {
        project,
        pending: Vec::new(),
        visited: BTreeSet::new(),
        functions: BTreeSet::new(),
        dynamic: BTreeSet::new(),
    };
    admission.scopes(roots);
    for rule in &project.failure_rules {
        match &rule.iteration {
            FailureIteration::Source { collection } => admission.path(collection),
            FailureIteration::Sequence { sequence } => admission.sequence(sequence, None),
        }
        admission
            .pending
            .extend(rule.selection.predicate().map(|node| (None, node)));
        admission
            .pending
            .extend(rule.message.map(|node| (None, node)));
    }
    admission.graph()
}

struct Admission<'a> {
    project: &'a Project,
    pending: Vec<(Option<FunctionId>, NodeId)>,
    visited: BTreeSet<(Option<FunctionId>, NodeId)>,
    functions: BTreeSet<FunctionId>,
    dynamic: BTreeSet<usize>,
}

impl<'a> Admission<'a> {
    fn scopes(&mut self, roots: impl IntoIterator<Item = &'a Scope>) {
        let mut pending: Vec<_> = roots.into_iter().collect();
        while let Some(scope) = pending.pop() {
            if let Some(path) = scope.source() {
                self.path(path);
            }
            if let Some(sequence) = scope.sequence() {
                self.sequence(sequence, None);
            }
            if let Some((_, join)) = scope.join() {
                for source in join.sources() {
                    self.path(source.collection());
                }
            }
            if let Some(segments) = scope.concatenated() {
                pending.extend(segments.iter());
            }
            self.pending.extend(scope.filter.map(|node| (None, node)));
            self.pending
                .extend(scope.post_group_filter.map(|node| (None, node)));
            self.pending
                .extend(scope.grouping_nodes().map(|node| (None, node)));
            self.pending.extend(scope.sort_by.map(|node| (None, node)));
            self.pending
                .extend(scope.output_path().map(|node| (None, node)));
            self.pending.extend(
                scope
                    .windows
                    .iter()
                    .copied()
                    .flat_map(|window| window.nodes())
                    .map(|node| (None, node)),
            );
            self.pending
                .extend(scope.sort_then_by.iter().map(|key| (None, key.node)));
            self.pending
                .extend(scope.bindings.iter().map(|binding| (None, binding.node)));
            self.pending.extend(
                scope
                    .dynamic_bindings
                    .iter()
                    .flat_map(|binding| [(None, binding.key), (None, binding.value)]),
            );
            match &scope.construction {
                ScopeConstruction::Scalar { value } => self.pending.push((None, *value)),
                ScopeConstruction::RecursiveFilter { plan } => {
                    self.pending.push((None, plan.predicate()))
                }
                ScopeConstruction::PathHierarchy { plan } => self.path(plan.collection()),
                ScopeConstruction::AdjacencyTree { plan } => {
                    self.path(plan.collection());
                    self.pending.extend(plan.root().map(|node| (None, node)));
                }
                ScopeConstruction::Constructed
                | ScopeConstruction::CopyCurrentSource
                | ScopeConstruction::XmlMixedContent { .. } => {}
            }
            pending.extend(scope.children.iter());
            for child in &scope.dynamic_children {
                self.pending.push((None, child.key));
                pending.push(&child.scope);
            }
        }
    }

    fn path(&mut self, path: &[String]) {
        let Some(name) = path.first() else {
            return;
        };
        let project = self.project;
        for (index, source) in project
            .extra_sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.name == *name)
        {
            let Some(dynamic) = &source.dynamic_path else {
                continue;
            };
            if self.dynamic.insert(index) {
                self.pending.push((None, dynamic.node));
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
            self.pending.push((Some(function), body.output));
        }
    }

    fn sequence(&mut self, sequence: &SequenceExpr, function: Option<FunctionId>) {
        self.pending
            .extend(sequence.inputs().into_iter().map(|node| (function, node)));
        self.pending.extend(
            sequence
                .owned_items()
                .into_iter()
                .map(|node| (function, node)),
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

    fn graph(&mut self) -> Result<(), EngineError> {
        while let Some((function, id)) = self.pending.pop() {
            if !self.visited.insert((function, id)) {
                continue;
            }
            let project = self.project;
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
            if let Some(kind) = node.owned_source_kind() {
                return Err(EngineError::UnsupportedSourceOwnership {
                    node: id,
                    function,
                    kind,
                });
            }
            self.pending
                .extend(node.dependencies().into_iter().map(|node| (function, node)));
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
