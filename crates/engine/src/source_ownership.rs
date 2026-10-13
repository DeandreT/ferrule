//! Context-keyed admission for the new basic source domain; acquires no input.
use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError, TargetSelection};
use mapping::{
    FailureIteration, FunctionId, Node, NodeId, Project, Scope, ScopeConstruction, SequenceExpr,
    SourceOwner, SourceScopeLocation, SourceScopeStep, SourceScopeTarget, SourceSelectorRef,
};
use std::collections::BTreeSet;

#[derive(Default, Clone, Copy)]
pub(crate) struct AdmissionSummary {
    pub used_owned: bool,
}

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
        || mapping::first_project_owned_source_scope(project).is_some()
}

pub(crate) fn check_project(
    project: &Project,
    selection: Option<TargetSelection<'_>>,
) -> Result<(), EngineError> {
    admit_project(project, selection).map(|_| ())
}
pub(crate) fn admit_project(
    project: &Project,
    selection: Option<TargetSelection<'_>>,
) -> Result<AdmissionSummary, EngineError> {
    // Legacy projects gain no target lookup, graph traversal or error precedence change.
    if !has_owned_nodes(project) {
        return Ok(AdmissionSummary::default());
    }
    match selection {
        Some(selection) => {
            let target = crate::required_sources::selected_target(project, selection)?;
            let (root, owner) = match target {
                None => (&project.root, SourceScopeTarget::Primary),
                Some(target) => {
                    let index = project
                        .extra_targets
                        .iter()
                        .position(|candidate| std::ptr::eq(candidate, target))
                        .expect("selected target belongs to this project");
                    (
                        &target.root,
                        SourceScopeTarget::Named {
                            index,
                            name: target.name.clone(),
                        },
                    )
                }
            };
            admit_roots(project, [(root, SourceScopeLocation::root(owner))])
        }
        None => {
            admit_roots(
                project,
                std::iter::once((
                    &project.root,
                    SourceScopeLocation::root(SourceScopeTarget::Primary),
                ))
                .chain(project.extra_targets.iter().enumerate().map(
                    |(index, target)| {
                        (
                            &target.root,
                            SourceScopeLocation::root(SourceScopeTarget::Named {
                                index,
                                name: target.name.clone(),
                            }),
                        )
                    },
                )),
            )
        }
    }
}

fn admit_roots<'a>(
    project: &'a Project,
    roots: impl IntoIterator<Item = (&'a Scope, SourceScopeLocation)>,
) -> Result<AdmissionSummary, EngineError> {
    let mut admission = Admission::new(project);
    admission.scopes(roots)?;
    for rule in &project.failure_rules {
        match &rule.iteration {
            FailureIteration::Source { collection } => admission.path(collection),
            FailureIteration::Sequence { sequence } => {
                admission.sequence(sequence, None, OwnedCapabilityContext::FailureRule)
            }
        }
        admission.push(
            None,
            OwnedCapabilityContext::FailureRule,
            false,
            rule.selection.predicate(),
        );
        admission.push(
            None,
            OwnedCapabilityContext::FailureRule,
            false,
            rule.message,
        );
    }
    admission.graph()?;
    Ok(admission.summary)
}

struct Admission<'a> {
    project: &'a Project,
    pending: Vec<(Option<FunctionId>, OwnedCapabilityContext, bool, NodeId)>,
    visited: BTreeSet<(Option<FunctionId>, OwnedCapabilityContext, bool, NodeId)>,
    functions: BTreeSet<FunctionId>,
    dynamic: BTreeSet<usize>,
    summary: AdmissionSummary,
    owned_budget: Option<crate::owned_context::OwnedBudget>,
}
impl<'a> Admission<'a> {
    fn new(project: &'a Project) -> Self {
        Self {
            project,
            pending: Vec::new(),
            visited: BTreeSet::new(),
            functions: BTreeSet::new(),
            dynamic: BTreeSet::new(),
            summary: AdmissionSummary::default(),
            owned_budget: None,
        }
    }
    fn push(
        &mut self,
        function: Option<FunctionId>,
        context: OwnedCapabilityContext,
        ancestry: bool,
        nodes: impl IntoIterator<Item = NodeId>,
    ) {
        self.pending.extend(
            nodes
                .into_iter()
                .map(|node| (function, context, ancestry, node)),
        );
    }
    fn scopes(
        &mut self,
        roots: impl IntoIterator<Item = (&'a Scope, SourceScopeLocation)>,
    ) -> Result<(), EngineError> {
        let mut pending: Vec<_> = roots
            .into_iter()
            .map(|(scope, location)| (scope, location, OwnedCapabilityContext::Basic, false))
            .collect();
        while let Some((scope, location, inherited, inherited_owned)) = pending.pop() {
            let own = matches!(scope.source_selector(), Some(SourceSelectorRef::Owned(_)));
            let ancestry = inherited_owned || own;
            let dynamic_scope = matches!(scope.source_selector(), Some(SourceSelectorRef::Legacy(path))
                if path.first().is_some_and(|name| self.project.extra_sources.iter().any(|source| source.name == *name && source.dynamic_path.is_some())));
            let context = if inherited != OwnedCapabilityContext::Basic {
                inherited
            } else if dynamic_scope {
                OwnedCapabilityContext::DynamicLoader
            } else {
                scope_context(scope)
            };
            if ancestry && context != OwnedCapabilityContext::Basic {
                return Err(EngineError::UnsupportedOwnedContext {
                    node: None,
                    function: None,
                    scope: Some(location),
                    context,
                });
            }
            match scope.source_selector() {
                Some(SourceSelectorRef::Legacy(path)) => self.path(path),
                Some(SourceSelectorRef::Owned(source)) => {
                    self.static_owner(&source.owner, &source.path, None, None, Some(&location))?;
                    self.summary.used_owned = true;
                }
                None => {}
            }
            if let Some(sequence) = scope.sequence() {
                self.sequence(sequence, None, context);
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
                        context,
                        ancestry,
                    )
                }));
            }
            self.push(None, context, ancestry, scope.filter);
            self.push(None, context, ancestry, scope.post_group_filter);
            self.push(None, context, ancestry, scope.grouping_nodes());
            self.push(None, context, ancestry, scope.sort_by);
            self.push(None, context, ancestry, scope.output_path());
            self.push(
                None,
                context,
                ancestry,
                scope
                    .windows
                    .iter()
                    .copied()
                    .flat_map(|window| window.nodes()),
            );
            self.push(
                None,
                context,
                ancestry,
                scope.sort_then_by.iter().map(|key| key.node),
            );
            self.push(
                None,
                context,
                ancestry,
                scope.bindings.iter().map(|binding| binding.node),
            );
            self.push(
                None,
                OwnedCapabilityContext::DynamicTarget,
                ancestry,
                scope
                    .dynamic_bindings
                    .iter()
                    .flat_map(|binding| [binding.key, binding.value]),
            );
            match &scope.construction {
                ScopeConstruction::Scalar { value } => self.push(None, context, ancestry, [*value]),
                ScopeConstruction::RecursiveFilter { plan } => self.push(
                    None,
                    OwnedCapabilityContext::TreeOperator,
                    ancestry,
                    [plan.predicate()],
                ),
                ScopeConstruction::PathHierarchy { plan } => self.path(plan.collection()),
                ScopeConstruction::AdjacencyTree { plan } => {
                    self.path(plan.collection());
                    self.push(
                        None,
                        OwnedCapabilityContext::TreeOperator,
                        ancestry,
                        plan.root(),
                    );
                }
                _ => {}
            }
            pending.extend(scope.children.iter().enumerate().map(|(index, child)| {
                (
                    child,
                    location.descendant(SourceScopeStep::Child(index)),
                    context,
                    ancestry,
                )
            }));
            for (index, child) in scope.dynamic_children.iter().enumerate() {
                self.push(
                    None,
                    OwnedCapabilityContext::DynamicTarget,
                    ancestry,
                    [child.key],
                );
                pending.push((
                    &child.scope,
                    location.descendant(SourceScopeStep::DynamicChild(index)),
                    OwnedCapabilityContext::DynamicTarget,
                    ancestry,
                ));
            }
        }
        Ok(())
    }
    fn static_owner(
        &mut self,
        owner: &SourceOwner,
        path: &[String],
        pin: Option<&[String]>,
        node: Option<NodeId>,
        scope: Option<&SourceScopeLocation>,
    ) -> Result<(), EngineError> {
        if self.owned_budget.is_none() {
            self.owned_budget = Some(
                crate::owned_context::OwnedBudget::new(
                    self.project,
                    crate::owned_context::OwnedLimits::default(),
                )
                .map_err(|source| access_error(node, scope, source))?,
            );
        }
        let name = match owner {
            SourceOwner::Primary => None,
            SourceOwner::Named { name } => Some(name.as_str()),
        };
        self.owned_budget
            .as_ref()
            .expect("lazy budget initialized")
            .work(name, path, pin, &[], &[])
            .map_err(|source| access_error(node, scope, source))?;
        if let SourceOwner::Named { name } = owner {
            if name.is_empty() {
                return Err(access_error(
                    node,
                    scope,
                    OwnedSourceError::InvalidNamedOwner,
                ));
            }
            let mut matches = self
                .project
                .extra_sources
                .iter()
                .filter(|source| source.name == *name);
            let Some(source) = matches.next() else {
                return Err(access_error(
                    node,
                    scope,
                    OwnedSourceError::UnknownOwner { name: name.clone() },
                ));
            };
            if matches.next().is_some() {
                return Err(access_error(
                    node,
                    scope,
                    OwnedSourceError::AmbiguousOwner { name: name.clone() },
                ));
            }
            if source.dynamic_path.is_some() {
                return Err(EngineError::UnsupportedOwnedContext {
                    node,
                    function: None,
                    scope: scope.cloned(),
                    context: OwnedCapabilityContext::DynamicOwner,
                });
            }
        }
        Ok(())
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
                self.push(
                    None,
                    OwnedCapabilityContext::DynamicLoader,
                    false,
                    [dynamic.node],
                );
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
            self.push(
                Some(function),
                OwnedCapabilityContext::IsolatedFunction,
                false,
                [body.output],
            );
        }
    }
    fn sequence(
        &mut self,
        sequence: &SequenceExpr,
        function: Option<FunctionId>,
        inherited: OwnedCapabilityContext,
    ) {
        let context = if matches!(sequence, SequenceExpr::RecursiveCollect { .. })
            && matches!(
                inherited,
                OwnedCapabilityContext::Basic | OwnedCapabilityContext::GeneratedItem
            ) {
            OwnedCapabilityContext::TreeOperator
        } else if inherited == OwnedCapabilityContext::Basic {
            OwnedCapabilityContext::GeneratedItem
        } else {
            inherited
        };
        self.push(function, context, false, sequence.inputs());
        self.push(function, context, false, sequence.owned_items());
        match sequence {
            SequenceExpr::RecursiveCollect { collection, .. } => self.path(collection),
            SequenceExpr::FilterMapV1(composition) => {
                self.function(composition.predicate);
                self.function(composition.mapper);
                self.sequence(&composition.source, function, context);
            }
            _ => {}
        }
    }
    fn graph(&mut self) -> Result<(), EngineError> {
        while let Some((function, context, ancestry, id)) = self.pending.pop() {
            if !self.visited.insert((function, context, ancestry, id)) {
                continue;
            }
            let project = self.project;
            let graph = match function {
                Some(function) => project
                    .user_functions
                    .get(&function)
                    .map(|function| &function.body),
                None => Some(&project.graph),
            };
            let Some(node) = graph.and_then(|graph| graph.nodes.get(&id)) else {
                continue;
            };
            if let Some(kind) = node.owned_source_kind() {
                if function.is_some() {
                    return Err(EngineError::UnsupportedSourceOwnership {
                        node: id,
                        function,
                        kind,
                    });
                }
                if context != OwnedCapabilityContext::Basic {
                    return Err(EngineError::UnsupportedOwnedContext {
                        node: Some(id),
                        function,
                        scope: None,
                        context,
                    });
                }
                match node {
                    Node::OwnedSourceField(field) => {
                        self.static_owner(
                            &field.source.owner,
                            &field.source.path,
                            field.frame.as_deref(),
                            Some(id),
                            None,
                        )?;
                        if let Some(frame) = &field.frame
                            && !field.source.path.starts_with(frame)
                        {
                            return Err(access_error(
                                Some(id),
                                None,
                                OwnedSourceError::UnsupportedFramePathCombination {
                                    path: field.source.path.clone(),
                                    frame: frame.clone(),
                                },
                            ));
                        }
                    }
                    Node::OwnedPosition(position) => self.static_owner(
                        &position.owner,
                        position.collection.as_deref().unwrap_or_default(),
                        None,
                        Some(id),
                        None,
                    )?,
                    _ => unreachable!("owned kind identifies owned payload"),
                }
                self.summary.used_owned = true;
            }
            let child_context = if context == OwnedCapabilityContext::Basic {
                node_context(node)
            } else {
                context
            };
            if ancestry && child_context != OwnedCapabilityContext::Basic {
                return Err(EngineError::UnsupportedOwnedContext {
                    node: Some(id),
                    function,
                    scope: None,
                    context: child_context,
                });
            }
            self.push(function, child_context, ancestry, node.dependencies());
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
                | Node::SequenceAggregate { sequence, .. } => {
                    self.sequence(sequence, function, child_context)
                }
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

fn access_error(
    node: Option<NodeId>,
    scope: Option<&SourceScopeLocation>,
    source: OwnedSourceError,
) -> EngineError {
    EngineError::OwnedSourceAccess {
        node,
        scope: scope.cloned().map(Box::new),
        source,
    }
}

pub(crate) fn scope_context(scope: &Scope) -> OwnedCapabilityContext {
    if scope.iteration_output == mapping::IterationOutput::MappedSequence {
        return OwnedCapabilityContext::XmlOperator;
    }
    if scope.has_grouping() || scope.post_group_filter.is_some() {
        return OwnedCapabilityContext::Grouping;
    }
    if scope.output_path().is_some() {
        return OwnedCapabilityContext::Document;
    }
    if !scope.dynamic_bindings.is_empty()
        || !scope.dynamic_children.is_empty()
        || scope.merge_dynamic_fields
    {
        return OwnedCapabilityContext::DynamicTarget;
    }
    if scope.sequence().is_some() {
        return OwnedCapabilityContext::GeneratedItem;
    }
    if scope.join().is_some() {
        return OwnedCapabilityContext::Join;
    }
    if scope.concatenated().is_some() {
        return OwnedCapabilityContext::Concatenate;
    }
    match scope.construction {
        ScopeConstruction::Constructed => OwnedCapabilityContext::Basic,
        ScopeConstruction::XmlMixedContent { .. } => OwnedCapabilityContext::XmlOperator,
        ScopeConstruction::RecursiveFilter { .. }
        | ScopeConstruction::PathHierarchy { .. }
        | ScopeConstruction::AdjacencyTree { .. } => OwnedCapabilityContext::TreeOperator,
        ScopeConstruction::Scalar { .. } | ScopeConstruction::CopyCurrentSource => {
            OwnedCapabilityContext::SpecialConstruction
        }
    }
}

fn node_context(node: &Node) -> OwnedCapabilityContext {
    match node {
        Node::UserFunctionCall { .. } => OwnedCapabilityContext::IsolatedFunction,
        Node::Aggregate { .. } | Node::Lookup { .. } | Node::CollectionFind { .. } => {
            OwnedCapabilityContext::AggregateLookup
        }
        Node::JoinField { .. } | Node::JoinPosition { .. } | Node::JoinAggregate { .. } => {
            OwnedCapabilityContext::Join
        }
        Node::SequenceExists { .. }
        | Node::SequenceItemAt { .. }
        | Node::SequenceAggregate { .. } => OwnedCapabilityContext::GeneratedItem,
        Node::XmlSerialize { .. }
        | Node::XmlMixedContent { .. }
        | Node::SourceRootXmlTypeEquals { .. } => OwnedCapabilityContext::XmlOperator,
        Node::DynamicSourceField { .. } => OwnedCapabilityContext::DynamicTarget,
        Node::SourceDocumentPath => OwnedCapabilityContext::Document,
        _ => OwnedCapabilityContext::Basic,
    }
}

#[cfg(test)]
pub(crate) fn check_context_reuse_for_test(
    project: &Project,
    node: NodeId,
    closed: OwnedCapabilityContext,
) -> Result<(), EngineError> {
    let mut admission = Admission::new(project);
    admission.push(None, closed, false, [node]);
    admission.push(None, OwnedCapabilityContext::Basic, false, [node]);
    admission.graph()
}
