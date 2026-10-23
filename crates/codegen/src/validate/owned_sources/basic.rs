//! Complete Basic emission admission over one capability set: the program
//! validator, then the static scope, failure-rule and retained-expression
//! inventories both emitters render.

use super::*;

/// Validate `program` for Basic owned emission under `capabilities`.
///
/// General Program structure and stored ownership contexts are checked first,
/// with their existing diagnostic precedence; the Basic scope, function,
/// failure-rule and retained-expression gates follow.
pub fn validate_basic_owned_emission_with_capabilities(
    program: &Program,
    capabilities: OwnedCapabilities,
) -> Result<(), ProgramValidationError> {
    super::super::validate_program_with_owned_capabilities(program, capabilities)?;
    basic_scope(
        program,
        &program.root,
        SourceScopeLocation::root(SourceScopeTarget::Primary),
        capabilities,
        false,
    )?;
    for (index, target) in program.extra_targets.iter().enumerate() {
        basic_scope(
            program,
            &target.root,
            SourceScopeLocation::root(SourceScopeTarget::Named {
                index,
                name: target.name.clone(),
            }),
            capabilities,
            false,
        )?;
    }
    if let Some(function) = program.user_functions.first() {
        return Err(failure(
            Location::Expression {
                namespace: Namespace::Function(function.id),
                node: function.output,
                scope: None,
            },
            None,
            Closed {
                reason: Capability::IsolatedFunction,
                origin: Origin::FunctionBody {
                    function: function.id,
                },
            },
        ));
    }
    basic_failures(program, capabilities)?;
    let retained = basic_target_expressions(program, capabilities);
    for node in &program.expressions {
        if let Expression::SourceField { path, frame } = &node.expression
            && retained.contains(&node.id)
            && basic_dynamic_path(program, frame.as_ref().unwrap_or(path))
        {
            return Err(owned_expression_emission_error(
                Namespace::Main,
                node.id,
                &node.expression,
            ));
        }
        let reason = match &node.expression {
            Expression::SequenceItemAt { .. }
            | Expression::SequenceExists { .. }
            | Expression::SequenceAggregate { .. }
                if capabilities.sequences.permits(&node.expression) =>
            {
                continue;
            }
            Expression::OwnedLookup(_) | Expression::OwnedCollectionFind(_)
                if capabilities.search.lookup_find =>
            {
                continue;
            }
            Expression::OwnedDynamicSourceField(_)
                if capabilities.permits_dynamic_field(&node.expression) =>
            {
                continue;
            }
            Expression::OwnedXmlSerialize(_) | Expression::OwnedXmlMixedContent(_)
                if !capabilities.closes_xml(&node.expression) =>
            {
                continue;
            }
            Expression::OwnedXmlSerialize(_) | Expression::OwnedXmlMixedContent(_) => {
                Capability::XmlOperator
            }
            Expression::OwnedSourceDocumentPath(_) if capabilities.document_sets => continue,
            Expression::OwnedSourceDocumentPath(_) => Capability::Document,
            Expression::OwnedAggregate(_) if capabilities.search.collection_aggregates => continue,
            Expression::OwnedLookup(_)
            | Expression::OwnedCollectionFind(_)
            | Expression::OwnedAggregate(_) => Capability::AggregateLookup,
            // Validation binds each read to its dominating owned join, whose
            // tuple admission checked the closed scalar subset.
            Expression::OwnedJoinField(_)
            | Expression::OwnedJoinSourcePosition(_)
            | Expression::OwnedJoinAggregate(_)
            | Expression::JoinPosition { .. }
                if capabilities.joins =>
            {
                continue;
            }
            Expression::OwnedJoinField(_)
            | Expression::OwnedJoinSourcePosition(_)
            | Expression::OwnedJoinAggregate(_) => Capability::Join,
            Expression::OwnedSourceField(_)
            | Expression::OwnedPosition(_)
            | Expression::OwnedRowDocumentPath(_)
            | Expression::SourceField { .. }
            | Expression::Position { .. }
            | Expression::Const { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::ValueMap { .. }
            | Expression::RuntimeValue { .. }
            | Expression::RuntimeParameter { .. }
            | Expression::RuntimeParameterDefault { .. }
            | Expression::Raise { .. } => continue,
            Expression::SourceRootXmlTypeEquals { .. }
            | Expression::XmlSerialize { .. }
            | Expression::XmlMixedContent { .. } => Capability::XmlOperator,
            Expression::SourceDocumentPath => Capability::Document,
            Expression::DynamicSourceField { .. } => Capability::DynamicLoader,
            Expression::JoinField { .. } | Expression::JoinPosition { .. } => Capability::Join,
            Expression::Lookup { .. }
            | Expression::CollectionFind { .. }
            | Expression::Aggregate { .. }
            | Expression::JoinAggregate { .. } => Capability::AggregateLookup,
            Expression::SequenceExists { .. }
            | Expression::SequenceItemAt { .. }
            | Expression::SequenceAggregate { .. } => Capability::GeneratedItem,
            Expression::UserFunctionCall { .. } | Expression::FunctionParameter { .. } => {
                Capability::IsolatedFunction
            }
            _ => Capability::UnsupportedBasicContext,
        };
        return Err(failure(
            Location::Expression {
                namespace: Namespace::Main,
                node: node.id,
                scope: None,
            },
            request(&node.expression),
            Closed {
                reason,
                origin: Origin::RetainedExpression {
                    namespace: Namespace::Main,
                    node: node.id,
                },
            },
        ));
    }
    Ok(())
}

fn basic_scope_refusal(
    scope: &TargetScope,
    route: SourceScopeLocation,
    reason: Capability,
) -> ProgramValidationError {
    let query = scope
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.owned_source_iteration())
        .map(|source| iteration_request(source.source()))
        .or_else(|| owned_path_hierarchy(scope).map(iteration_request))
        .or_else(|| owned_rows(scope).map(iteration_request));
    failure(
        Location::Iteration {
            scope: route.clone(),
        },
        query,
        Closed {
            reason,
            origin: Origin::Scope { scope: route },
        },
    )
}

fn basic_scope(
    program: &Program,
    scope: &TargetScope,
    route: SourceScopeLocation,
    capabilities: OwnedCapabilities,
    inherited_owned: bool,
) -> Result<(), ProgramValidationError> {
    if let Some(iteration) = &scope.iteration {
        let reason = match iteration.input() {
            // Closed-context admission validated the whole tuple subtree.
            IterationSource::OwnedInnerJoin(_) if capabilities.joins => {
                return Ok(());
            }
            IterationSource::OwnedSource(_) | IterationSource::Source(_) => None,
            IterationSource::Generated(_) => Some(Capability::GeneratedItem),
            IterationSource::InnerJoin(_) | IterationSource::OwnedInnerJoin(_) => {
                Some(Capability::Join)
            }
            IterationSource::Concatenate(_) => Some(Capability::Concatenate),
            IterationSource::OptionalPresence(_) => Some(Capability::OptionalPresence),
            IterationSource::DynamicDocuments(_) => Some(Capability::Document),
        };
        if let Some(reason) = reason {
            return Err(basic_scope_refusal(scope, route, reason));
        }
        let legacy_grouping_island = capabilities.grouping.any()
            && !inherited_owned
            && !basic_scope_needs_owned(program, scope)
            && iteration.source_iteration().is_some()
            && iteration.grouping().is_some()
            && (iteration.output() == IterationOutput::Repeated
                || iteration.output() == IterationOutput::First
                    && matches!(scope.construction, TargetConstruction::Group));
        if !legacy_grouping_island
            && (iteration.filter().is_some() && !capabilities.permits_filter(scope)
                || iteration.sort().is_some() && !capabilities.permits_sort(scope)
                || (iteration.grouping().is_some() || iteration.post_group_filter().is_some())
                    && !capabilities.permits_group(scope)
                || !iteration.windows().is_empty() && !capabilities.permits_windows(scope)
                || iteration.output() != IterationOutput::Repeated
                    && !capabilities.permits_windows(scope))
        {
            return Err(basic_scope_refusal(scope, route, Capability::ScopeControl));
        }
    }
    let admitted_construction = capabilities.constructions.path_hierarchy
        && owned_path_hierarchy(scope).is_some()
        || capabilities.admits_owned_tree_construction(scope)
        || capabilities.admits_xml_construction(scope);
    if !admitted_construction
        && !matches!(
            scope.construction,
            TargetConstruction::Group | TargetConstruction::Scalar { .. }
        )
    {
        let reason = match scope.construction {
            TargetConstruction::XmlMixedContent { .. } => Capability::XmlOperator,
            TargetConstruction::DynamicGroup { .. } => Capability::DynamicTarget,
            TargetConstruction::RecursiveFilter { .. }
            | TargetConstruction::PathHierarchy { .. }
            | TargetConstruction::OwnedPathHierarchy { .. }
            | TargetConstruction::AdjacencyTree { .. }
            | TargetConstruction::OwnedAdjacencyTree { .. } => Capability::TreeOperator,
            _ => Capability::SpecialConstruction,
        };
        return Err(basic_scope_refusal(scope, route, reason));
    }
    if let Some(iteration) = &scope.iteration
        && let IterationSource::Source(source) = iteration.input()
        && basic_dynamic_path(program, source.path())
    {
        return Err(basic_scope_refusal(scope, route, Capability::DynamicLoader));
    }
    let inherited_owned = inherited_owned
        || scope.iteration.as_ref().is_some_and(|iteration| {
            iteration.owned_source_iteration().is_some()
                || capabilities.permits_group(scope) && basic_scope_needs_owned(program, scope)
        });
    for (index, child) in scope.children.iter().enumerate() {
        basic_scope(
            program,
            child,
            route.descendant(SourceScopeStep::Child(index)),
            capabilities,
            inherited_owned,
        )?;
    }
    Ok(())
}

fn basic_expression_nodes(
    program: &Program,
    roots: impl IntoIterator<Item = NodeId>,
) -> std::collections::BTreeSet<NodeId> {
    let mut pending = roots.into_iter().collect::<Vec<_>>();
    let mut found = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !found.insert(id) {
            continue;
        }
        let Some(node) = program.expressions.iter().find(|node| node.id == id) else {
            continue;
        };
        // These are precisely the potential dependency edges of the admitted
        // ordinary scalar subset; If stays conservative and execution stays lazy.
        // Owned search and aggregate edges matter only where a companion admits them.
        if matches!(
            node.expression,
            Expression::Call { .. }
                | Expression::If { .. }
                | Expression::RuntimeParameterDefault { .. }
                | Expression::Raise { .. }
                | Expression::OwnedLookup(_)
                | Expression::OwnedCollectionFind(_)
                | Expression::OwnedAggregate(_)
                | Expression::OwnedJoinAggregate(_)
        ) {
            pending.extend(graph_dependencies::of(&node.expression));
        }
    }
    found
}

fn basic_target_expressions(
    program: &Program,
    capabilities: OwnedCapabilities,
) -> std::collections::BTreeSet<NodeId> {
    fn roots(scope: &TargetScope, capabilities: OwnedCapabilities, output: &mut Vec<NodeId>) {
        if let TargetConstruction::Scalar { expression, .. } = &scope.construction {
            output.push(*expression);
        }
        // An owned tree's root is a parent-context target expression; an owned
        // recursive filter's predicate reads each item of that same owner.
        output.extend(owned_root(scope));
        output.extend(recursive_filter::owned_predicate(scope));
        output.extend(scope.bindings.iter().map(|binding| binding.expression));
        if capabilities.permits_filter(scope) {
            // A predicate retains every potential scalar dependency, including
            // unchosen branches/defaults and the existing target ValueMap edge.
            output.extend(
                scope
                    .iteration
                    .as_ref()
                    .and_then(|iteration| iteration.filter()),
            );
        }
        if capabilities.permits_sort(scope)
            && let Some(sort) = scope
                .iteration
                .as_ref()
                .and_then(|iteration| iteration.sort())
        {
            // Every stored key retains its potential scalar dependencies;
            // static admission does not skip secondary or inactive key edges.
            output.extend(sort.keys().map(|key| key.expression));
        }
        if capabilities.permits_windows(scope)
            && let Some(iteration) = &scope.iteration
        {
            // Count bounds retain all potential scalar edges even when the
            // source is empty or a preceding window would discard every row.
            output.extend(
                iteration
                    .windows()
                    .iter()
                    .copied()
                    .flat_map(crate::SequenceWindow::nodes),
            );
        }
        if capabilities.grouping.any()
            && let Some(iteration) = &scope.iteration
        {
            // Grouping profiles also retain controls of ordinary legacy grouping
            // islands. Their capability was checked independently by basic_scope.
            output.extend(iteration.roots());
        }
        for child in &scope.children {
            roots(child, capabilities, output);
        }
    }
    let mut pending = Vec::new();
    roots(&program.root, capabilities, &mut pending);
    for target in &program.extra_targets {
        roots(&target.root, capabilities, &mut pending);
    }
    let mut retained = basic_expression_nodes(program, pending);
    let mut pending = retained.iter().copied().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        let Some(node) = program.expressions.iter().find(|node| node.id == id) else {
            continue;
        };
        let dependencies = match &node.expression {
            Expression::ValueMap { input, .. } => vec![*input],
            expression if capabilities.sequences.permits(expression) => {
                // Producer scalar inputs, an ItemAt index and an aggregate
                // argument are parent demands; an Exists predicate and an
                // aggregate predicate/expression add only their private
                // generated item. Item declarations grant no endpoint permission.
                graph_dependencies::of(expression)
            }
            // The key is a receiving-context scalar demand of the owned read.
            expression if capabilities.permits_dynamic_field(expression) => {
                graph_dependencies::of(expression)
            }
            // Parent and occurrence replacements are scalar demands of the
            // admitted owned group read.
            expression if capabilities.permits_xml_inputs(expression) => {
                graph_dependencies::of(expression)
            }
            _ => continue,
        };
        // Only target admission gains the explicitly permitted consumer edges.
        // Failure traversal keeps its independent, closed scalar subset.
        for dependency in basic_expression_nodes(program, dependencies) {
            if retained.insert(dependency) {
                pending.push(dependency);
            }
        }
    }
    retained
}

fn basic_failure_refusal(index: usize, reason: Capability) -> ProgramValidationError {
    failure(
        Location::Iteration {
            scope: SourceScopeLocation::root(SourceScopeTarget::Primary),
        },
        None,
        Closed {
            reason,
            origin: Origin::Failure { index },
        },
    )
}

fn basic_failures(
    program: &Program,
    capabilities: OwnedCapabilities,
) -> Result<(), ProgramValidationError> {
    for (index, rule) in program.failure_rules.iter().enumerate() {
        if capabilities.admits_owned_failure_rule(program, rule) {
            continue;
        }
        let FailureIteration::Source(source) = &rule.iteration else {
            return Err(basic_failure_refusal(index, Capability::FailureRule));
        };
        if basic_dynamic_path(program, source.path()) {
            return Err(basic_failure_refusal(index, Capability::DynamicLoader));
        }
        for root in rule.selection.predicate().into_iter().chain(rule.message) {
            let retained = basic_expression_nodes(program, [root]);
            for node in &program.expressions {
                if !retained.contains(&node.id) {
                    continue;
                }
                let dynamic = match &node.expression {
                    Expression::SourceField { path, frame } => {
                        basic_dynamic_path(program, frame.as_ref().unwrap_or(path))
                    }
                    Expression::DynamicSourceField { .. } => true,
                    _ => false,
                };
                if dynamic {
                    return Err(failure(
                        Location::Expression {
                            namespace: Namespace::Main,
                            node: node.id,
                            scope: None,
                        },
                        None,
                        Closed {
                            reason: Capability::DynamicLoader,
                            origin: Origin::Failure { index },
                        },
                    ));
                }
                if !matches!(
                    &node.expression,
                    Expression::SourceField { .. }
                        | Expression::Position { .. }
                        | Expression::Const { .. }
                        | Expression::Call { .. }
                        | Expression::If { .. }
                        | Expression::RuntimeValue { .. }
                        | Expression::RuntimeParameter { .. }
                        | Expression::RuntimeParameterDefault { .. }
                        | Expression::Raise { .. }
                ) {
                    return Err(basic_failure_refusal(index, Capability::FailureRule));
                }
            }
        }
    }
    Ok(())
}
