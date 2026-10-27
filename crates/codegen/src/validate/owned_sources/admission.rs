//! Closed-context admission. Each owned read is admitted only in a Basic
//! context its capability set opens; a failure rule, dynamic loader path,
//! isolated function body, generated item or unadmitted control closes every
//! owned read beneath it with its typed origin.

use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Ancestor<'a> {
    Owned {
        scope: SourceScopeLocation,
        source: &'a mapping::OwnedSourceRef,
    },
    Legacy {
        scope: SourceScopeLocation,
        path: &'a [String],
    },
    Specialized {
        scope: SourceScopeLocation,
        input: &'a IterationSource,
    },
    GroupMembers {
        scope: SourceScopeLocation,
    },
}
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Visit<'a> {
    pub(super) namespace: Namespace,
    pub(super) closed: Option<Closed>,
    pub(super) scope: Option<SourceScopeLocation>,
    pub(super) ancestry: Vec<Ancestor<'a>>,
    /// The origin of the admitted owned join tuple this expression reads: its
    /// join scope, or the joined aggregate that reduces it.
    pub(super) join_tuple: Option<Origin>,
    pub(super) node: NodeId,
}
fn owns(ancestry: &[Ancestor<'_>]) -> bool {
    ancestry.iter().any(|entry| {
        matches!(
            entry,
            Ancestor::Owned { .. } | Ancestor::GroupMembers { .. }
        )
    })
}
pub(super) struct Admission<'a> {
    pub(super) program: &'a Program,
    pub(super) capabilities: OwnedCapabilities,
    /// Expressions admitted inside an owned join tuple.
    pub(super) tuple_nodes: std::collections::BTreeSet<NodeId>,
    visited: Vec<Visit<'a>>,
}
impl<'a> Admission<'a> {
    pub(super) fn expression(&mut self, start: Visit<'a>) -> Result<(), ProgramValidationError> {
        let mut pending = vec![start];
        while let Some(mut visit) = pending.pop() {
            if self.visited.contains(&visit) {
                continue;
            }
            self.visited.push(visit.clone());
            let expression = match visit.namespace {
                Namespace::Main => self
                    .program
                    .expressions
                    .iter()
                    .find(|node| node.id == visit.node)
                    .map(|node| &node.expression),
                Namespace::Function(function) => self
                    .program
                    .user_functions
                    .iter()
                    .find(|definition| definition.id == function)
                    .and_then(|definition| {
                        definition
                            .expressions
                            .iter()
                            .find(|node| node.id == visit.node)
                    })
                    .map(|node| &node.expression),
            };
            let Some(expression) = expression else {
                continue;
            };
            let location = Location::Expression {
                namespace: visit.namespace,
                node: visit.node,
                scope: visit.scope.clone(),
            };
            if let Some(origin) = &visit.join_tuple {
                if !joins::admitted_in_tuple(expression) {
                    return Err(failure(
                        location,
                        request(expression),
                        Closed {
                            reason: Capability::Join,
                            origin: origin.clone(),
                        },
                    ));
                }
                self.tuple_nodes.insert(visit.node);
                for dependency in graph_dependencies::of(expression).into_iter().rev() {
                    let mut child = visit.clone();
                    child.node = dependency;
                    pending.push(child);
                }
                continue;
            }
            // Owned join reads exist only inside their admitted tuple.
            if joins::is_join_read(expression) {
                if visit.namespace == Namespace::Main && self.tuple_nodes.contains(&visit.node) {
                    continue;
                }
                return Err(failure(
                    location,
                    None,
                    visit.closed.clone().unwrap_or(Closed {
                        reason: Capability::Join,
                        origin: Origin::Expression {
                            namespace: visit.namespace,
                            node: visit.node,
                        },
                    }),
                ));
            }
            if let Some(query) = request(expression)
                && let Some(closed) = visit
                    .closed
                    .clone()
                    .or_else(|| {
                        loaded_owner(
                            self.program,
                            &query,
                            self.capabilities.permits_loaded_read(expression),
                        )
                    })
                    .or_else(|| {
                        self.capabilities.closes_xml(expression).then_some(Closed {
                            reason: Capability::XmlOperator,
                            origin: Origin::Expression {
                                namespace: visit.namespace,
                                node: visit.node,
                            },
                        })
                    })
            {
                return Err(failure(location, Some(query), closed));
            }
            if let Expression::OwnedJoinAggregate(aggregate) = expression {
                self.joined_aggregate(&visit, location, aggregate, &mut pending)?;
                continue;
            }
            if let Some(query) =
                recursive_collect::closed_consumer(expression, self.capabilities.sequences)
            {
                return Err(failure(
                    location,
                    Some(query),
                    Closed {
                        reason: Capability::GeneratedItem,
                        origin: Origin::Expression {
                            namespace: visit.namespace,
                            node: visit.node,
                        },
                    },
                ));
            }
            if let Some(query) = dynamic_field::closed_read(expression, self.capabilities) {
                return Err(failure(
                    location,
                    Some(query),
                    Closed {
                        reason: Capability::UnsupportedBasicContext,
                        origin: Origin::Expression {
                            namespace: visit.namespace,
                            node: visit.node,
                        },
                    },
                ));
            }
            if let Expression::UserFunctionCall { function, .. } = expression {
                if owns(&visit.ancestry) {
                    return Err(failure(
                        location,
                        None,
                        Closed {
                            reason: Capability::IsolatedFunction,
                            origin: Origin::FunctionCall {
                                caller_namespace: visit.namespace,
                                call: visit.node,
                                callee: *function,
                            },
                        },
                    ));
                }
                // An isolated body never inherits the caller's source ancestry.
                if let Some(definition) = self
                    .program
                    .user_functions
                    .iter()
                    .find(|definition| definition.id == *function)
                {
                    pending.push(Visit {
                        namespace: Namespace::Function(*function),
                        closed: Some(Closed {
                            reason: Capability::IsolatedFunction,
                            origin: Origin::FunctionBody {
                                function: *function,
                            },
                        }),
                        scope: None,
                        ancestry: Vec::new(),
                        join_tuple: None,
                        node: definition.output,
                    });
                }
            }
            let reason = match expression {
                Expression::SequenceItemAt { .. }
                | Expression::SequenceExists { .. }
                | Expression::SequenceAggregate { .. }
                    if self.capabilities.sequences.permits(expression) =>
                {
                    None
                }
                Expression::SequenceExists { .. }
                | Expression::SequenceItemAt { .. }
                | Expression::SequenceAggregate { .. } => Some(Capability::GeneratedItem),
                Expression::Aggregate { .. }
                | Expression::Lookup { .. }
                | Expression::CollectionFind { .. } => Some(Capability::AggregateLookup),
                Expression::JoinAggregate { .. }
                | Expression::JoinField { .. }
                | Expression::JoinPosition { .. } => Some(Capability::Join),
                Expression::XmlSerialize { .. }
                | Expression::XmlMixedContent { .. }
                | Expression::SourceRootXmlTypeEquals { .. }
                | Expression::SourceDocumentPath => Some(Capability::XmlOperator),
                _ => None, // Raise keeps its legacy dependency/laziness behavior.
            };
            if let Some(reason) = reason {
                let closed = visit.closed.clone().unwrap_or(Closed {
                    reason,
                    origin: Origin::Expression {
                        namespace: visit.namespace,
                        node: visit.node,
                    },
                });
                if owns(&visit.ancestry) {
                    return Err(failure(location, None, closed));
                }
                visit.closed = Some(closed);
            }
            for dependency in graph_dependencies::of(expression).into_iter().rev() {
                let mut child = visit.clone();
                child.node = dependency;
                pending.push(child);
            }
        }
        Ok(())
    }
    fn scope(
        &mut self,
        scope: &'a TargetScope,
        route: SourceScopeLocation,
        mut ancestry: Vec<Ancestor<'a>>,
        inherited: Option<Closed>,
    ) -> Result<(), ProgramValidationError> {
        if let Some(join) = scope
            .iteration
            .as_ref()
            .and_then(|iteration| iteration.owned_inner_join())
        {
            return self.owned_join_scope(scope, route, ancestry, inherited, join);
        }
        // Bounds use the incoming parent, before this scope introduces an item.
        // Keep the complete Visit key when a bound is also a key or target root.
        // An admitted fixed-block size is such a parent expression as well.
        let window_parent = (self.capabilities.permits_windows(scope)
            || self.capabilities.permits_group(scope))
        .then(|| (ancestry.clone(), inherited.clone()));
        let mut closed = inherited;
        if let Some(iteration) = &scope.iteration {
            let reason = if (iteration.grouping().is_some()
                || iteration.post_group_filter().is_some())
                && !self.capabilities.permits_group(scope)
            {
                Some(Capability::Grouping)
            } else if iteration.filter().is_some() && !self.capabilities.permits_filter(scope)
                || iteration.sort().is_some() && !self.capabilities.permits_sort(scope)
                || !iteration.windows().is_empty() && !self.capabilities.permits_windows(scope)
            {
                Some(Capability::ScopeControl)
            } else if iteration.output() != IterationOutput::Repeated
                && !self.capabilities.permits_windows(scope)
            {
                Some(Capability::Document)
            } else {
                match iteration.input() {
                    IterationSource::DynamicDocuments(_) => Some(Capability::Document),
                    IterationSource::Generated(_) => Some(Capability::GeneratedItem),
                    IterationSource::InnerJoin(_) => Some(Capability::Join),
                    IterationSource::Concatenate(_) => Some(Capability::Concatenate),
                    IterationSource::OptionalPresence(_) => Some(Capability::OptionalPresence),
                    _ => None,
                }
            };
            if closed.is_none() {
                closed = reason.map(|reason| Closed {
                    reason,
                    origin: Origin::Scope {
                        scope: route.clone(),
                    },
                });
            }
            let ancestor = match iteration.input() {
                IterationSource::OwnedSource(source) => Ancestor::Owned {
                    scope: route.clone(),
                    source: source.source(),
                },
                IterationSource::Source(source) => Ancestor::Legacy {
                    scope: route.clone(),
                    path: source.path(),
                },
                input => Ancestor::Specialized {
                    scope: route.clone(),
                    input,
                },
            };
            ancestry.push(ancestor);
            // A generated item never becomes an endpoint-bearing frame.
            if let Some(source) = recursive_collect::scope_sequence(iteration) {
                return Err(failure(
                    Location::Iteration {
                        scope: route.clone(),
                    },
                    Some(iteration_request(source)),
                    closed.clone().unwrap_or(Closed {
                        reason: Capability::GeneratedItem,
                        origin: Origin::Scope {
                            scope: route.clone(),
                        },
                    }),
                ));
            }
            if let Some(source) = iteration.owned_source_iteration() {
                let query = iteration_request(source.source());
                let loaded = self.capabilities.dynamic_loading;
                if let Some(closed) = closed
                    .clone()
                    .or_else(|| loaded_owner(self.program, &query, loaded))
                {
                    return Err(failure(
                        Location::Iteration {
                            scope: route.clone(),
                        },
                        Some(query),
                        closed,
                    ));
                }
                if loaded {
                    self.loading_scope(
                        &route,
                        &ancestry,
                        source.source(),
                        iteration.grouping().is_some(),
                    )?;
                }
            }
        }
        let construction_reason = match scope.construction {
            TargetConstruction::Group => None,
            TargetConstruction::OwnedAdjacencyTree { .. }
            | TargetConstruction::RecursiveFilter { .. }
                if self.capabilities.admits_owned_tree_construction(scope) =>
            {
                None
            }
            TargetConstruction::OwnedPathHierarchy { .. }
                if self.capabilities.constructions.path_hierarchy =>
            {
                None
            }
            TargetConstruction::DynamicGroup { .. } => Some(Capability::DynamicTarget),
            TargetConstruction::XmlMixedContent { .. }
                if self.capabilities.admits_xml_construction(scope) =>
            {
                None
            }
            TargetConstruction::XmlMixedContent { .. } => Some(Capability::XmlOperator),
            TargetConstruction::RecursiveFilter { .. }
            | TargetConstruction::PathHierarchy { .. }
            | TargetConstruction::OwnedPathHierarchy { .. }
            | TargetConstruction::AdjacencyTree { .. }
            | TargetConstruction::OwnedAdjacencyTree { .. } => Some(Capability::TreeOperator),
            // A scalar target evaluates one expression in the receiving context
            // and creates no frame, so every Basic profile admits it.
            TargetConstruction::Scalar { .. } => None,
            TargetConstruction::CopyCurrentSource => Some(Capability::SpecialConstruction),
            TargetConstruction::CollectDocuments => Some(Capability::Document),
        };
        if closed.is_none() {
            closed = construction_reason.map(|reason| Closed {
                reason,
                origin: Origin::Scope {
                    scope: route.clone(),
                },
            });
        }
        // A hierarchy's collection or a tree's rows are an owned read in the
        // receiving context; any inherited or construction closure refuses it there.
        if let Some(collection) = owned_path_hierarchy(scope).or_else(|| owned_rows(scope)) {
            let query = iteration_request(collection);
            if let Some(closed) = closed
                .clone()
                .or_else(|| dynamic_owner(self.program, &query))
            {
                return Err(failure(
                    Location::Iteration {
                        scope: route.clone(),
                    },
                    Some(query),
                    closed,
                ));
            }
        }
        if let Some(closed) = closed.clone().filter(|_| owns(&ancestry)) {
            let query = ancestry.iter().rev().find_map(|entry| match entry {
                Ancestor::Owned { source, .. } => Some(iteration_request(source)),
                _ => None,
            });
            return Err(failure(
                Location::Iteration {
                    scope: route.clone(),
                },
                query,
                closed,
            ));
        }
        if self.capabilities.permits_group(scope) {
            let iteration = scope
                .iteration
                .as_ref()
                .expect("admitted grouping iteration");
            // Bounds and a block size run once in the receiving parent; filter,
            // sort, key or boundary, and retention are candidate expressions.
            // Only target expressions and descendants inherit the grouped view.
            if let Some((parent_ancestry, parent_closed)) = &window_parent {
                for node in iteration
                    .windows()
                    .iter()
                    .copied()
                    .flat_map(crate::SequenceWindow::nodes)
                    .chain(
                        iteration
                            .grouping()
                            .and_then(|grouping| grouping.parent_expression()),
                    )
                {
                    self.expression(Visit {
                        namespace: Namespace::Main,
                        closed: parent_closed.clone(),
                        scope: Some(route.clone()),
                        ancestry: parent_ancestry.clone(),
                        join_tuple: None,
                        node,
                    })?;
                }
            }
            for node in iteration
                .filter()
                .into_iter()
                .chain(
                    iteration
                        .sort()
                        .iter()
                        .flat_map(|sort| sort.keys().map(|key| key.expression)),
                )
                .chain(
                    iteration
                        .grouping()
                        .and_then(|grouping| grouping.item_expression()),
                )
                .chain(iteration.post_group_filter())
            {
                self.expression(Visit {
                    namespace: Namespace::Main,
                    closed: closed.clone(),
                    scope: Some(route.clone()),
                    ancestry: ancestry.clone(),
                    join_tuple: None,
                    node,
                })?;
            }
            // A wholly ordinary island must not gain owned ancestry just by
            // using an admitted mode. A demanded paired By/AdjacentBy/StartingWith/
            // EndingWith/IntoBlocks operation establishes a member container; no
            // other mode may mint one.
            if owns(&ancestry) || basic_scope_needs_owned(self.program, scope) {
                ancestry.push(Ancestor::GroupMembers {
                    scope: route.clone(),
                });
            }
        }
        let mut roots = scope
            .bindings
            .iter()
            .map(|binding| binding.expression)
            .collect::<Vec<_>>();
        if let Some(iteration) = &scope.iteration
            && !self.capabilities.permits_group(scope)
        {
            if let Some((parent_ancestry, parent_closed)) = &window_parent {
                for node in iteration
                    .windows()
                    .iter()
                    .copied()
                    .flat_map(crate::SequenceWindow::nodes)
                {
                    self.expression(Visit {
                        namespace: Namespace::Main,
                        closed: parent_closed.clone(),
                        scope: Some(route.clone()),
                        ancestry: parent_ancestry.clone(),
                        join_tuple: None,
                        node,
                    })?;
                }
                // The admitted window scope is a static Source/OwnedSource with
                // no grouping or dynamic output. Its other roots use the item.
                roots.extend(iteration.filter());
                roots.extend(
                    iteration
                        .sort()
                        .iter()
                        .flat_map(|sort| sort.keys().map(|key| key.expression)),
                );
            } else {
                roots.extend(iteration.roots());
            }
        }
        match &scope.construction {
            TargetConstruction::Scalar { expression, .. } => roots.push(*expression),
            TargetConstruction::RecursiveFilter { predicate, .. } => roots.push(*predicate),
            TargetConstruction::AdjacencyTree {
                root: Some(root), ..
            }
            | TargetConstruction::OwnedAdjacencyTree {
                root: Some(root), ..
            } => roots.push(*root),
            TargetConstruction::DynamicGroup {
                bindings, children, ..
            } => {
                for binding in bindings {
                    roots.extend([binding.key, binding.value]);
                }
                for child in children {
                    roots.push(child.key);
                }
            }
            _ => {}
        }
        for node in roots {
            self.expression(Visit {
                namespace: Namespace::Main,
                closed: closed.clone(),
                scope: Some(route.clone()),
                ancestry: ancestry.clone(),
                join_tuple: None,
                node,
            })?;
        }
        if let Some(segments) = scope
            .iteration
            .as_ref()
            .and_then(|iteration| iteration.concatenated())
        {
            for (index, segment) in segments.iter().enumerate() {
                self.scope(
                    segment,
                    route.descendant(SourceScopeStep::Segment(index)),
                    ancestry.clone(),
                    closed.clone(),
                )?;
            }
        }
        for (index, child) in scope.children.iter().enumerate() {
            self.scope(
                child,
                route.descendant(SourceScopeStep::Child(index)),
                ancestry.clone(),
                closed.clone(),
            )?;
        }
        if let TargetConstruction::DynamicGroup { children, .. } = &scope.construction {
            for (index, child) in children.iter().enumerate() {
                self.scope(
                    &child.scope,
                    route.descendant(SourceScopeStep::DynamicChild(index)),
                    ancestry.clone(),
                    closed.clone(),
                )?;
            }
        }
        Ok(())
    }
}
/// Admit every owned read of `program` in its context under `capabilities`.
pub(in crate::validate) fn validate_closed_contexts(
    program: &Program,
    capabilities: OwnedCapabilities,
) -> Result<(), ProgramValidationError> {
    if !has_owned(program) {
        return Ok(());
    }
    let mut admission = Admission {
        program,
        capabilities,
        tuple_nodes: std::collections::BTreeSet::new(),
        visited: Vec::new(),
    };
    admission.scope(
        &program.root,
        SourceScopeLocation::root(SourceScopeTarget::Primary),
        Vec::new(),
        None,
    )?;
    for (index, target) in program.extra_targets.iter().enumerate() {
        admission.scope(
            &target.root,
            SourceScopeLocation::root(SourceScopeTarget::Named {
                index,
                name: target.name.clone(),
            }),
            Vec::new(),
            None,
        )?;
    }
    for (index, rule) in program.failure_rules.iter().enumerate() {
        let mut roots = rule
            .selection
            .predicate()
            .into_iter()
            .chain(rule.message)
            .collect::<Vec<_>>();
        if let FailureIteration::Generated(sequence) = &rule.iteration {
            roots.extend(sequence.roots());
        }
        recursive_collect::admit_failure(index, rule)?;
        failure_sources::admit_iteration(program, index, rule, capabilities)?;
        for node in roots {
            admission.expression(Visit {
                namespace: Namespace::Main,
                closed: (!capabilities.admits_owned_failure_rule(program, rule)).then_some(
                    Closed {
                        reason: Capability::FailureRule,
                        origin: Origin::Failure { index },
                    },
                ),
                scope: None,
                ancestry: Vec::new(),
                join_tuple: None,
                node,
            })?;
        }
    }
    // Lowering represents every stored dynamic path; preserve that exact generated inventory.
    for (index, source) in program.extra_sources.iter().enumerate() {
        if let Some(driver) = source
            .dynamic
            .as_ref()
            .and_then(|dynamic| dynamic.driver.owned())
        {
            // Each loading scope admits its own driver and path; an unused
            // declaration requests nothing.
            if capabilities.dynamic_loading {
                continue;
            }
            return Err(failure(
                driver_location(index, source),
                Some(iteration_request(driver)),
                Closed {
                    reason: Capability::DynamicLoader,
                    origin: Origin::DynamicSource {
                        index,
                        name: source.name.clone(),
                    },
                },
            ));
        }
        if let Some(dynamic) = &source.dynamic {
            admission.expression(Visit {
                namespace: Namespace::Main,
                closed: Some(Closed {
                    reason: Capability::DynamicLoader,
                    origin: Origin::DynamicSource {
                        index,
                        name: source.name.clone(),
                    },
                }),
                scope: None,
                ancestry: Vec::new(),
                join_tuple: None,
                node: dynamic.path,
            })?;
        }
    }
    for function in &program.user_functions {
        for node in &function.expressions {
            admission.expression(Visit {
                namespace: Namespace::Function(function.id),
                closed: Some(Closed {
                    reason: Capability::IsolatedFunction,
                    origin: Origin::FunctionBody {
                        function: function.id,
                    },
                }),
                scope: None,
                ancestry: Vec::new(),
                join_tuple: None,
                node: node.id,
            })?;
        }
    }
    // Retained direct Program main leaves may be unused by a target; named dynamic ownership still closes.
    for node in &program.expressions {
        admission.expression(Visit {
            namespace: Namespace::Main,
            closed: None,
            scope: None,
            ancestry: Vec::new(),
            join_tuple: None,
            node: node.id,
        })?;
    }
    Ok(())
}
