//! Context-keyed admission for the new basic source domain; acquires no input.
use crate::scope_slots::{self, AdmittedRoot};
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
        || project.failure_rules.iter().any(|rule| {
            rule.iteration.owned_source().is_some()
                || matches!(&rule.iteration, FailureIteration::Sequence { sequence }
                    if sequence.owned_source().is_some())
        })
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
            // Owned producers are qualified only as parent-context ItemAt.
            FailureIteration::Sequence { sequence } if sequence.owned_source().is_some() => {
                return Err(EngineError::UnsupportedOwnedContext {
                    node: None,
                    function: None,
                    scope: None,
                    context: OwnedCapabilityContext::FailureRule,
                });
            }
            FailureIteration::Sequence { sequence } => {
                admission.sequence(sequence, None, OwnedCapabilityContext::FailureRule, false)
            }
            FailureIteration::OwnedSource { source } => {
                admission.owned_failure_rows(&source.owner, &source.path, rule)?;
                continue;
            }
        }
        let root_owned = matches!(&rule.iteration,
            FailureIteration::Source { collection } if collection.is_empty())
            && failure_rule_uses_owned(&project.graph, rule);
        if root_owned {
            admission.push_root_failure(rule.selection.predicate());
            admission.push_root_failure(rule.message);
        } else if matches!(&rule.iteration,
            FailureIteration::Source { collection } if primary_row_failure_is_static(project, collection))
            && failure_rule_uses_owned(&project.graph, rule)
        {
            admission.push_primary_row_failure(rule.selection.predicate());
            admission.push_primary_row_failure(rule.message);
        } else {
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
    }
    admission.graph()?;
    Ok(admission.summary)
}

struct Admission<'a> {
    project: &'a Project,
    pending: Vec<(
        Option<FunctionId>,
        OwnedCapabilityContext,
        bool,
        bool,
        bool,
        NodeId,
    )>,
    visited: BTreeSet<(
        Option<FunctionId>,
        OwnedCapabilityContext,
        bool,
        bool,
        bool,
        NodeId,
    )>,
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
                .map(|node| (function, context, ancestry, false, false, node)),
        );
    }
    fn push_root_failure(&mut self, nodes: impl IntoIterator<Item = NodeId>) {
        self.pending.extend(nodes.into_iter().map(|node| {
            (
                None,
                OwnedCapabilityContext::FailureRule,
                false,
                true,
                false,
                node,
            )
        }));
    }
    fn push_primary_row_failure(&mut self, nodes: impl IntoIterator<Item = NodeId>) {
        self.pending.extend(nodes.into_iter().map(|node| {
            (
                None,
                OwnedCapabilityContext::FailureRule,
                false,
                false,
                true,
                node,
            )
        }));
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
            if let Some((_, plan)) = scope.iteration.owned_join() {
                self.owned_join(scope, &location, inherited, inherited_owned, plan)?;
                continue;
            }
            let own = matches!(scope.source_selector(), Some(SourceSelectorRef::Owned(_)))
                || matches!(
                    scope.construction,
                    ScopeConstruction::OwnedPathHierarchy { .. }
                        | ScopeConstruction::OwnedAdjacencyTree { .. }
                );
            let mut ancestry = inherited_owned || own;
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
            // Only admitted By/AdjacentBy/StartingWith/EndingWith/IntoBlocks scopes
            // introduce grouped ancestry. Closed legacy scopes still report their
            // original owned-node cause, rather than being promoted into an
            // earlier scope-level refusal.
            ancestry |= context == OwnedCapabilityContext::Basic
                && (scope.group_by.is_some()
                    || scope.group_adjacent_by.is_some()
                    || scope.group_starting_with.is_some()
                    || scope.group_ending_with.is_some()
                    || scope.group_into_blocks.is_some())
                && scope_demands_owned(self.project, scope);
            match scope.source_selector() {
                Some(SourceSelectorRef::Legacy(path)) => self.path(path),
                Some(SourceSelectorRef::Owned(source)) => {
                    self.loaded_owner(&source.owner, &source.path, None, None, Some(&location))?;
                    self.loading(&source.owner, &location, context)?;
                    self.summary.used_owned = true;
                }
                None => {}
            }
            if let Some(sequence) = scope.sequence() {
                // A generated item never becomes an endpoint-bearing frame.
                if sequence.owned_source().is_some() {
                    return Err(EngineError::UnsupportedOwnedContext {
                        node: None,
                        function: None,
                        scope: Some(location),
                        context: OwnedCapabilityContext::GeneratedItem,
                    });
                }
                self.sequence(sequence, None, context, false);
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
            self.scope_roots(scope, &location, context, ancestry)?;
            pending.extend(scope.children.iter().enumerate().map(|(index, child)| {
                (
                    child,
                    location.descendant(SourceScopeStep::Child(index)),
                    context,
                    ancestry,
                )
            }));
            for (index, child) in scope.dynamic_children.iter().enumerate() {
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
    /// Admits a scope's own roots other than its sequence's, in visit order
    /// with its dynamic child keys last, and its construction's collection.
    fn scope_roots(
        &mut self,
        scope: &'a Scope,
        location: &SourceScopeLocation,
        context: OwnedCapabilityContext,
        ancestry: bool,
    ) -> Result<(), EngineError> {
        let mut child_keys = Vec::new();
        scope.visit_roots(&mut |root| {
            let context = match scope_slots::admitted(root.slot) {
                Some(AdmittedRoot::Scope) => context,
                Some(AdmittedRoot::DynamicBinding) => OwnedCapabilityContext::DynamicTarget,
                Some(AdmittedRoot::Construction) => match &scope.construction {
                    // A legacy tree's rows are walked before its root is read.
                    ScopeConstruction::AdjacencyTree { plan } => {
                        self.path(plan.collection());
                        OwnedCapabilityContext::TreeOperator
                    }
                    // An owned selector's filter evaluates its predicate in
                    // Basic item frames of that same owner; a legacy filter
                    // stays closed.
                    ScopeConstruction::RecursiveFilter { .. }
                        if scope.owned_recursive_filter().is_none() =>
                    {
                        OwnedCapabilityContext::TreeOperator
                    }
                    // A scalar value and an owned tree's root, whose rows are
                    // an exact owned read in the receiving context, keep that
                    // context.
                    _ => context,
                },
                Some(AdmittedRoot::DynamicChildKey) => {
                    child_keys.push(root.node);
                    return;
                }
                None => return,
            };
            self.push(None, context, ancestry, [root.node]);
        });
        let owned_rows = match &scope.construction {
            ScopeConstruction::PathHierarchy { plan } => {
                self.path(plan.collection());
                None
            }
            // A tree with a root walked its rows before admitting the root.
            ScopeConstruction::AdjacencyTree { plan } if plan.root().is_none() => {
                self.path(plan.collection());
                None
            }
            ScopeConstruction::OwnedPathHierarchy { plan } => Some(plan.collection()),
            ScopeConstruction::OwnedAdjacencyTree { plan } => Some(plan.collection()),
            _ => None,
        };
        if let Some(source) = owned_rows {
            self.static_owner(&source.owner, &source.path, None, None, Some(location))?;
            self.summary.used_owned = true;
        }
        self.push(
            None,
            OwnedCapabilityContext::DynamicTarget,
            ancestry,
            child_keys,
        );
        Ok(())
    }
    /// Admit one owned join scope. Its sources name static endpoints and its
    /// window bounds stay receiving-context Basic expressions. Every tuple
    /// expression, including those of its static descendants, reads members
    /// only through owned join reads and the flattened join position.
    fn owned_join(
        &mut self,
        scope: &'a Scope,
        location: &SourceScopeLocation,
        inherited: OwnedCapabilityContext,
        inherited_owned: bool,
        plan: &mapping::OwnedJoinPlan,
    ) -> Result<(), EngineError> {
        let closed = if inherited != OwnedCapabilityContext::Basic {
            inherited
        } else if !matches!(
            scope.construction,
            ScopeConstruction::Constructed | ScopeConstruction::Scalar { .. }
        ) {
            OwnedCapabilityContext::SpecialConstruction
        } else {
            scope_context(scope)
        };
        if closed != OwnedCapabilityContext::Join {
            return Err(EngineError::UnsupportedOwnedContext {
                node: None,
                function: None,
                scope: Some(location.clone()),
                context: closed,
            });
        }
        for source in plan.sources() {
            let collection = source.collection();
            self.static_owner(
                &collection.owner,
                &collection.path,
                None,
                None,
                Some(location),
            )?;
        }
        self.summary.used_owned = true;
        self.push(
            None,
            OwnedCapabilityContext::Basic,
            inherited_owned,
            scope_slots::roots(scope, scope_slots::owned_join_receiving),
        );
        self.join_tuple_scope(scope, location)
    }
    fn join_tuple_scope(
        &mut self,
        scope: &'a Scope,
        location: &SourceScopeLocation,
    ) -> Result<(), EngineError> {
        self.join_tuple_nodes(scope_slots::roots(scope, scope_slots::join_tuple))?;
        for (index, child) in scope.children.iter().enumerate() {
            let location = location.descendant(SourceScopeStep::Child(index));
            // Only static descendant projections share a tuple; a nested
            // iteration or specialized construction stays closed.
            let context = match scope_context(child) {
                OwnedCapabilityContext::Basic
                    if !matches!(child.iteration, mapping::ScopeIteration::None)
                        || !matches!(child.construction, ScopeConstruction::Constructed) =>
                {
                    OwnedCapabilityContext::Join
                }
                context => context,
            };
            if context != OwnedCapabilityContext::Basic {
                return Err(EngineError::UnsupportedOwnedContext {
                    node: None,
                    function: None,
                    scope: Some(location),
                    context,
                });
            }
            self.join_tuple_scope(child, &location)?;
        }
        Ok(())
    }
    /// The closed scalar subset of an owned join tuple. Ordinary and owned
    /// source reads, searches, aggregates, sequences and functions stay closed.
    fn join_tuple_nodes(&mut self, roots: Vec<NodeId>) -> Result<(), EngineError> {
        let mut pending = roots;
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(node) = self.project.graph.nodes.get(&id) else {
                continue;
            };
            match node {
                Node::OwnedJoinField(_) | Node::OwnedJoinSourcePosition(_) => {
                    self.summary.used_owned = true;
                }
                Node::JoinPosition { .. }
                | Node::Unconnected
                | Node::Const { .. }
                | Node::Call { .. }
                | Node::If { .. }
                | Node::ValueMap { .. }
                | Node::RuntimeValue { .. }
                | Node::RuntimeParameter { .. }
                | Node::RuntimeParameterDefault { .. }
                | Node::Raise { .. } => {}
                _ => {
                    return Err(EngineError::UnsupportedOwnedContext {
                        node: Some(id),
                        function: None,
                        scope: None,
                        context: OwnedCapabilityContext::Join,
                    });
                }
            }
            pending.extend(node.dependencies());
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
        self.endpoint_owner(owner, path, pin, node, scope, false)
    }
    /// Like [`Self::static_owner`], also admitting an owned dynamic endpoint:
    /// only exact field and position reads and source iterations read loaded
    /// documents, which exist only inside their loading scope.
    fn loaded_owner(
        &mut self,
        owner: &SourceOwner,
        path: &[String],
        pin: Option<&[String]>,
        node: Option<NodeId>,
        scope: Option<&SourceScopeLocation>,
    ) -> Result<(), EngineError> {
        self.endpoint_owner(owner, path, pin, node, scope, true)
    }
    fn endpoint_owner(
        &mut self,
        owner: &SourceOwner,
        path: &[String],
        pin: Option<&[String]>,
        node: Option<NodeId>,
        scope: Option<&SourceScopeLocation>,
        loaded: bool,
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
            let owned_driver = source
                .dynamic_path
                .as_ref()
                .is_some_and(|dynamic| dynamic.owned_driver().is_some());
            if source.dynamic_path.is_some() && !(loaded && owned_driver) {
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
    /// Admit the exact driver and path expression of an owned dynamic owner
    /// iterated by `location`. The path is a Basic expression of each driver
    /// item; its driver may itself be an owned dynamic endpoint.
    fn loading(
        &mut self,
        owner: &SourceOwner,
        location: &SourceScopeLocation,
        context: OwnedCapabilityContext,
    ) -> Result<(), EngineError> {
        let SourceOwner::Named { name } = owner else {
            return Ok(());
        };
        let project = self.project;
        for (index, source) in project.extra_sources.iter().enumerate() {
            let Some(dynamic) = source.dynamic_path.as_ref() else {
                continue;
            };
            let Some(driver) = dynamic.owned_driver() else {
                continue;
            };
            if source.name == *name && self.dynamic.insert(index) {
                self.loaded_owner(&driver.owner, &driver.path, None, None, Some(location))?;
                self.push(None, context, true, [dynamic.node]);
                self.loading(&driver.owner, location, context)?;
            }
        }
        Ok(())
    }
    /// Admit an explicitly owned failure walk and its closed row scalar subset.
    /// Owned reads may name any declared static endpoint; nothing here grants
    /// generated items, functions, lookups or another specialized context.
    fn owned_failure_rows(
        &mut self,
        owner: &SourceOwner,
        path: &[String],
        rule: &mapping::FailureRule,
    ) -> Result<(), EngineError> {
        self.static_owner(owner, path, None, None, None)?;
        self.summary.used_owned = true;
        let project = self.project;
        let mut pending: Vec<_> = rule
            .selection
            .predicate()
            .into_iter()
            .chain(rule.message)
            .collect();
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(node) = project.graph.nodes.get(&id) else {
                continue;
            };
            if !owned_row_failure_node_is_static(project, node) {
                return Err(EngineError::UnsupportedOwnedContext {
                    node: Some(id),
                    function: None,
                    scope: None,
                    context: OwnedCapabilityContext::FailureRule,
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
                Node::OwnedRowDocumentPath(path) => self.static_owner(
                    &path.owner,
                    path.collection.as_deref().unwrap_or_default(),
                    None,
                    Some(id),
                    None,
                )?,
                _ => {}
            }
            pending.extend(node.dependencies());
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
            // An owned driver is admitted only through its owned loading scope.
            let Some((dynamic, driver)) = source
                .dynamic_path
                .as_ref()
                .and_then(|dynamic| Some((dynamic, dynamic.legacy_driver()?)))
            else {
                continue;
            };
            if self.dynamic.insert(index) {
                self.push(
                    None,
                    OwnedCapabilityContext::DynamicLoader,
                    false,
                    [dynamic.node],
                );
                if let Some(driver) = driver.first() {
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
    fn graph(&mut self) -> Result<(), EngineError> {
        while let Some((function, context, ancestry, root_failure, primary_row_failure, id)) =
            self.pending.pop()
        {
            if !self.visited.insert((
                function,
                context,
                ancestry,
                root_failure,
                primary_row_failure,
                id,
            )) {
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
            if root_failure && !root_failure_node_is_static(project, node) {
                return Err(EngineError::UnsupportedOwnedContext {
                    node: Some(id),
                    function,
                    scope: None,
                    context: OwnedCapabilityContext::FailureRule,
                });
            }
            if primary_row_failure && !primary_row_failure_node_is_static(project, node) {
                return Err(EngineError::UnsupportedOwnedContext {
                    node: Some(id),
                    function,
                    scope: None,
                    context: OwnedCapabilityContext::FailureRule,
                });
            }
            if let Some(kind) = node.owned_source_kind() {
                // Owned join reads are admitted only inside their tuple.
                if matches!(
                    node,
                    Node::OwnedJoinField(_) | Node::OwnedJoinSourcePosition(_)
                ) {
                    return Err(EngineError::UnsupportedOwnedContext {
                        node: Some(id),
                        function,
                        scope: None,
                        context: OwnedCapabilityContext::Join,
                    });
                }
                if function.is_some() {
                    return Err(EngineError::UnsupportedSourceOwnership {
                        node: id,
                        function,
                        kind,
                    });
                }
                let root_field = root_failure
                    && context == OwnedCapabilityContext::FailureRule
                    && matches!(node, Node::OwnedSourceField(_));
                let row_value = primary_row_failure
                    && context == OwnedCapabilityContext::FailureRule
                    && matches!(
                        node,
                        Node::OwnedSourceField(_)
                            | Node::OwnedPosition(_)
                            | Node::OwnedRowDocumentPath(_)
                    );
                if context != OwnedCapabilityContext::Basic && !root_field && !row_value {
                    return Err(EngineError::UnsupportedOwnedContext {
                        node: Some(id),
                        function,
                        scope: None,
                        context,
                    });
                }
                match node {
                    // Failure scalars never see a loaded document.
                    Node::OwnedSourceField(field) => {
                        self.endpoint_owner(
                            &field.source.owner,
                            &field.source.path,
                            field.frame.as_deref(),
                            Some(id),
                            None,
                            context == OwnedCapabilityContext::Basic,
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
                    Node::OwnedPosition(position) => self.endpoint_owner(
                        &position.owner,
                        position.collection.as_deref().unwrap_or_default(),
                        None,
                        Some(id),
                        None,
                        context == OwnedCapabilityContext::Basic,
                    )?,
                    Node::OwnedRowDocumentPath(path) => self.static_owner(
                        &path.owner,
                        path.collection.as_deref().unwrap_or_default(),
                        None,
                        Some(id),
                        None,
                    )?,
                    // A static document-set member or an owned dynamic
                    // endpoint's loaded document carries the location.
                    Node::OwnedSourceDocumentPath(path) => {
                        self.loaded_owner(&path.owner, &[], None, Some(id), None)?
                    }
                    // The object is an exact owned read; its key stays a
                    // Basic parent-context scalar.
                    Node::OwnedDynamicSourceField(field) => {
                        self.static_owner(
                            &field.object.owner,
                            &field.object.path,
                            field.frame.as_deref(),
                            Some(id),
                            None,
                        )?;
                        if let Some(frame) = &field.frame
                            && !field.object.path.starts_with(frame)
                        {
                            return Err(access_error(
                                Some(id),
                                None,
                                OwnedSourceError::UnsupportedFramePathCombination {
                                    path: field.object.path.clone(),
                                    frame: frame.clone(),
                                },
                            ));
                        }
                    }
                    Node::SequenceExists { sequence, .. }
                    | Node::SequenceItemAt { sequence, .. }
                    | Node::SequenceAggregate { sequence, .. } => {
                        // Only a Basic parent-context ItemAt consumes an owned
                        // producer; reducers with private item phases stay closed.
                        let qualified = matches!(node, Node::SequenceItemAt { .. })
                            && basic_consumer_parents(
                                project,
                                graph.expect("node belongs to a graph"),
                                node,
                            );
                        let Some(source) = sequence.owned_source().filter(|_| qualified) else {
                            return Err(EngineError::UnsupportedOwnedContext {
                                node: Some(id),
                                function,
                                scope: None,
                                context: OwnedCapabilityContext::GeneratedItem,
                            });
                        };
                        self.static_owner(&source.owner, &source.path, None, Some(id), None)?;
                    }
                    // The needle, predicate and value remain Basic scalar demands;
                    // the walk itself names exactly one static endpoint.
                    Node::OwnedLookup(mapping::OwnedLookup { collection, .. })
                    | Node::OwnedCollectionFind(mapping::OwnedCollectionFind {
                        collection, ..
                    })
                    | Node::OwnedAggregate(mapping::OwnedAggregate { collection, .. }) => self
                        .static_owner(&collection.owner, &collection.path, None, Some(id), None)?,
                    // Each source names a static endpoint walked in the
                    // receiving context. The tuple expression has the closed
                    // scalar subset of a join tuple, while the argument stays
                    // a Basic receiving-context scalar.
                    Node::OwnedJoinAggregate(aggregate) => {
                        for source in aggregate.plan.sources() {
                            let collection = source.collection();
                            self.static_owner(
                                &collection.owner,
                                &collection.path,
                                None,
                                Some(id),
                                None,
                            )?;
                        }
                        self.join_tuple_nodes(aggregate.expression.into_iter().collect())?;
                        self.pending.extend(aggregate.arg.map(|arg| {
                            (
                                function,
                                context,
                                ancestry,
                                root_failure,
                                primary_row_failure,
                                arg,
                            )
                        }));
                        self.summary.used_owned = true;
                        continue;
                    }
                    // The structured read has the field's frame rules and
                    // evaluates no expression of its own.
                    Node::OwnedXmlSerialize(serializer) => {
                        self.static_owner(
                            &serializer.source.owner,
                            &serializer.source.path,
                            serializer.frame.as_deref(),
                            Some(id),
                            None,
                        )?;
                        if let Some(frame) = &serializer.frame
                            && !serializer.source.path.starts_with(frame)
                        {
                            return Err(access_error(
                                Some(id),
                                None,
                                OwnedSourceError::UnsupportedFramePathCombination {
                                    path: serializer.source.path.clone(),
                                    frame: frame.clone(),
                                },
                            ));
                        }
                    }
                    // The group read has the field's frame rules; replacement
                    // expressions stay Basic demands of this same context.
                    Node::OwnedXmlMixedContent(mixed) => {
                        self.static_owner(
                            &mixed.source.owner,
                            &mixed.source.path,
                            mixed.frame.as_deref(),
                            Some(id),
                            None,
                        )?;
                        if let Some(frame) = &mixed.frame
                            && !mixed.source.path.starts_with(frame)
                        {
                            return Err(access_error(
                                Some(id),
                                None,
                                OwnedSourceError::UnsupportedFramePathCombination {
                                    path: mixed.source.path.clone(),
                                    frame: frame.clone(),
                                },
                            ));
                        }
                    }
                    _ => unreachable!("owned kind identifies owned payload"),
                }
                self.summary.used_owned = true;
            }
            let child_context = if context == OwnedCapabilityContext::Basic {
                if matches!(
                    node,
                    Node::SequenceItemAt { .. }
                        | Node::SequenceExists { .. }
                        | Node::SequenceAggregate { .. }
                ) && !basic_consumer_parents(
                    project,
                    graph.expect("node belongs to a graph"),
                    node,
                ) {
                    OwnedCapabilityContext::GeneratedItem
                } else {
                    node_context(node)
                }
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
            self.pending
                .extend(node.dependencies().into_iter().map(|node| {
                    (
                        function,
                        child_context,
                        ancestry,
                        root_failure,
                        primary_row_failure,
                        node,
                    )
                }));
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
                Node::SequenceItemAt { sequence, .. }
                | Node::SequenceExists { sequence, .. }
                | Node::SequenceAggregate { sequence, .. } => {
                    self.sequence(sequence, function, child_context, true)
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

/// Potential main-graph demand, independent of runtime If/message selection.
/// Function bodies remain separately isolated by admission.
pub(crate) fn failure_rule_uses_owned(graph: &mapping::Graph, rule: &mapping::FailureRule) -> bool {
    let mut pending: Vec<_> = rule
        .selection
        .predicate()
        .into_iter()
        .chain(rule.message)
        .collect();
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(node) = graph.nodes.get(&id) else {
            continue;
        };
        if node.owned_source_kind().is_some() {
            return true;
        }
        pending.extend(node.dependencies());
    }
    false
}

fn root_failure_node_is_static(project: &Project, node: &Node) -> bool {
    if matches!(
        node,
        Node::SequenceItemAt { .. } | Node::SequenceExists { .. } | Node::SequenceAggregate { .. }
    ) || node_context(node) != OwnedCapabilityContext::Basic
    {
        return false;
    }
    match node {
        Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedSourceDocumentPath(_)
        | Node::Position { .. }
        | Node::OwnedDynamicSourceField(_)
        | Node::OwnedLookup(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedAggregate(_) => false,
        Node::OwnedSourceField(field) => field.frame.as_ref().is_none_or(Vec::is_empty),
        Node::SourceField { path, frame } => {
            frame.as_ref().is_none_or(Vec::is_empty)
                && !path.first().is_some_and(|name| {
                    project
                        .extra_sources
                        .iter()
                        .any(|source| source.name == *name && source.dynamic_path.is_some())
                })
        }
        _ => true,
    }
}

/// The iteration has no owner field, so dynamic aliases stay closed before input acquisition.
fn primary_row_failure_is_static(project: &Project, collection: &[String]) -> bool {
    collection.len() == 1
        && !collection[0].is_empty()
        && !project
            .extra_sources
            .iter()
            .any(|source| source.name == collection[0] && source.dynamic_path.is_some())
}

fn primary_row_failure_node_is_static(project: &Project, node: &Node) -> bool {
    if matches!(
        node,
        Node::SequenceItemAt { .. } | Node::SequenceExists { .. } | Node::SequenceAggregate { .. }
    ) || node_context(node) != OwnedCapabilityContext::Basic
    {
        return false;
    }
    match node {
        // Owned dynamic reads are qualified only in Basic target contexts.
        Node::OwnedDynamicSourceField(_)
        | Node::OwnedLookup(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedSourceDocumentPath(_) => false,
        Node::OwnedPosition(position) => matches!(position.owner, SourceOwner::Primary),
        Node::OwnedSourceField(field) => match &field.source.owner {
            SourceOwner::Primary => true,
            SourceOwner::Named { .. } => field.frame.as_ref().is_none_or(Vec::is_empty),
        },
        Node::SourceField { path, .. } => !path.first().is_some_and(|name| {
            project
                .extra_sources
                .iter()
                .any(|source| source.name == *name && source.dynamic_path.is_some())
        }),
        _ => true,
    }
}

/// Owned failure rows add exact row positions and frame pins for every owner to
/// the static scalar subset. Specialized contexts, including the generated-item
/// consumers ItemAt, Exists and SequenceAggregate, stay closed as generation
/// refuses them.
fn owned_row_failure_node_is_static(project: &Project, node: &Node) -> bool {
    if matches!(
        node,
        Node::SequenceItemAt { .. } | Node::SequenceExists { .. } | Node::SequenceAggregate { .. }
    ) || node_context(node) != OwnedCapabilityContext::Basic
    {
        return false;
    }
    match node {
        Node::OwnedPosition(_) | Node::OwnedSourceField(_) => true,
        Node::OwnedLookup(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedSourceDocumentPath(_) => false,
        Node::UserFunctionCall { .. } | Node::OwnedDynamicSourceField(_) => false,
        // Owned XML operators are target-context operators only.
        Node::OwnedXmlSerialize(_) | Node::OwnedXmlMixedContent(_) => false,
        Node::SourceField { path, frame } => ![Some(path.as_slice()), frame.as_deref()]
            .into_iter()
            .flatten()
            .any(|path| {
                path.first().is_some_and(|name| {
                    project
                        .extra_sources
                        .iter()
                        .any(|source| source.name == *name && source.dynamic_path.is_some())
                })
            }),
        Node::Position { collection } => !collection.first().is_some_and(|name| {
            project
                .extra_sources
                .iter()
                .any(|source| source.name == *name && source.dynamic_path.is_some())
        }),
        _ => true,
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

fn scope_demands_owned(project: &Project, scope: &Scope) -> bool {
    let mut pending = scope_slots::roots(scope, scope_slots::owned_probe);
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if visited.insert(id)
            && let Some(node) = project.graph.nodes.get(&id)
        {
            if node.owned_source_kind().is_some() {
                return true;
            }
            pending.extend(node.dependencies());
        }
    }
    scope.children.iter().any(|child| {
        matches!(child.source_selector(), Some(SourceSelectorRef::Owned(_)))
            || matches!(
                child.construction,
                ScopeConstruction::OwnedPathHierarchy { .. }
                    | ScopeConstruction::OwnedAdjacencyTree { .. }
            )
            || scope_demands_owned(project, child)
    })
}

pub(crate) fn scope_context(scope: &Scope) -> OwnedCapabilityContext {
    if scope.iteration_output == mapping::IterationOutput::MappedSequence {
        return OwnedCapabilityContext::XmlOperator;
    }
    // Only By, AdjacentBy, StartingWith, EndingWith and IntoBlocks authenticate
    // owned member containers. A retention predicate alone, source-free or
    // conflicting modes do not.
    if (scope.has_grouping() || scope.post_group_filter.is_some())
        && !((scope.group_by.is_some()
            || scope.group_adjacent_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_ending_with.is_some()
            || scope.group_into_blocks.is_some())
            && !scope.has_conflicting_grouping()
            && scope.source_selector().is_some())
    {
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
    if scope.join().is_some() || scope.iteration.owned_join().is_some() {
        return OwnedCapabilityContext::Join;
    }
    if scope.concatenated().is_some() {
        return OwnedCapabilityContext::Concatenate;
    }
    if scope.presence().is_some() {
        return OwnedCapabilityContext::OptionalPresence;
    }
    match scope.construction {
        // Owned hierarchies and trees read their collection through the
        // receiving exact-owner context; a tree evaluates only its root there.
        // Neither creates an item frame.
        ScopeConstruction::Constructed
        | ScopeConstruction::Scalar { .. }
        | ScopeConstruction::OwnedPathHierarchy { .. }
        | ScopeConstruction::OwnedAdjacencyTree { .. } => OwnedCapabilityContext::Basic,
        // Only the scope's own owned selector qualifies a mixed-content
        // construction; grouped and legacy-selected forms stay closed.
        ScopeConstruction::XmlMixedContent { .. } if owned_mixed_construction(scope) => {
            OwnedCapabilityContext::Basic
        }
        ScopeConstruction::XmlMixedContent { .. } => OwnedCapabilityContext::XmlOperator,
        // Only the scope's own owned selector qualifies a recursive filter.
        ScopeConstruction::RecursiveFilter { .. } if scope.owned_recursive_filter().is_some() => {
            OwnedCapabilityContext::Basic
        }
        ScopeConstruction::RecursiveFilter { .. }
        | ScopeConstruction::PathHierarchy { .. }
        | ScopeConstruction::AdjacencyTree { .. } => OwnedCapabilityContext::TreeOperator,
        ScopeConstruction::CopyCurrentSource => OwnedCapabilityContext::SpecialConstruction,
    }
}

/// An ungrouped scope whose own owned selector supplies the constructed
/// mixed-content item.
pub(crate) fn owned_mixed_construction(scope: &Scope) -> bool {
    matches!(
        scope.construction,
        ScopeConstruction::XmlMixedContent { .. }
    ) && matches!(scope.source_selector(), Some(SourceSelectorRef::Owned(_)))
        && !scope.has_grouping()
        && scope.post_group_filter.is_none()
}

/// Parent-context ItemAt producers. The owned recursive walk is qualified
/// here; the legacy recursive walk keeps its specialized tree classification.
/// Exists predicates and SequenceAggregate item phases are qualified only over
/// the four pure scalar producers.
fn basic_exists_producer(sequence: &SequenceExpr) -> bool {
    basic_item_at_producer(sequence) && !matches!(sequence, SequenceExpr::OwnedRecursiveCollect(_))
}

fn basic_item_at_producer(sequence: &SequenceExpr) -> bool {
    matches!(
        sequence,
        SequenceExpr::Generate { .. }
            | SequenceExpr::Tokenize { .. }
            | SequenceExpr::TokenizeByLength { .. }
            | SequenceExpr::TokenizeRegex { .. }
            | SequenceExpr::OwnedRecursiveCollect(_)
    )
}

/// ItemAt producer arguments/index, Exists producer arguments/predicate and
/// SequenceAggregate producer arguments/predicate/expression/argument remain
/// Basic scalars, including nested eligible consumers. Item phases add only
/// their private generated item, which is not an endpoint; an aggregate
/// argument keeps the receiving context.
/// A closed operator in one argument cannot gain permission merely because an
/// owned read is in another. Legacy islands keep their original classification.
fn basic_consumer_parents(project: &Project, graph: &mapping::Graph, node: &Node) -> bool {
    let (sequence, consumer) = match node {
        Node::SequenceItemAt { sequence, index } if basic_item_at_producer(sequence) => {
            (sequence, vec![*index])
        }
        Node::SequenceExists {
            sequence,
            predicate,
        } if basic_exists_producer(sequence) => (sequence, vec![*predicate]),
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } if basic_exists_producer(sequence) => (
            sequence,
            [*predicate, *expression, *arg]
                .into_iter()
                .flatten()
                .collect(),
        ),
        _ => return false,
    };
    let mut pending: Vec<_> = sequence.inputs().into_iter().chain(consumer).collect();
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(node) = graph.nodes.get(&id) else {
            continue;
        };
        match node {
            Node::UserFunctionCall { .. } => return false,
            Node::SourceField { path, frame }
                if frame
                    .as_deref()
                    .unwrap_or(path)
                    .first()
                    .is_some_and(|name| {
                        project
                            .extra_sources
                            .iter()
                            .any(|source| source.name == *name && source.dynamic_path.is_some())
                    }) =>
            {
                return false;
            }
            _ if node_context(node) != OwnedCapabilityContext::Basic => return false,
            _ => pending.extend(node.dependencies()),
        }
    }
    true
}

fn node_context(node: &Node) -> OwnedCapabilityContext {
    match node {
        // Arguments execute in the caller; function() visits the body separately.
        Node::UserFunctionCall { .. } => OwnedCapabilityContext::Basic,
        Node::Aggregate { .. } | Node::Lookup { .. } | Node::CollectionFind { .. } => {
            OwnedCapabilityContext::AggregateLookup
        }
        Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::JoinAggregate { .. }
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedJoinAggregate(_) => OwnedCapabilityContext::Join,
        Node::SequenceItemAt { sequence, .. } if basic_item_at_producer(sequence) => {
            OwnedCapabilityContext::Basic
        }
        Node::SequenceExists { sequence, .. } | Node::SequenceAggregate { sequence, .. }
            if basic_exists_producer(sequence) =>
        {
            OwnedCapabilityContext::Basic
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

mod sequence;

#[cfg(test)]
#[path = "tests/root_admission.rs"]
mod root_admission_tests;

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
