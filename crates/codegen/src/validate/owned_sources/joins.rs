//! Owned inner joins. Each source walks exactly its declared static endpoint
//! like owned iteration, and tuple expressions read members only through
//! owned join reads and the flattened join position. Only the explicit join
//! companion admits them; every older profile keeps its Join refusal.

use super::*;

/// Join permission is orthogonal to target controls, consumers, searches and
/// failures. Owned joined aggregates have no admitted form.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::validate) enum JoinPolicy {
    Closed,
    Owned,
}

/// Validate the owned Lookup and CollectionFind profile together with owned
/// inner joins and their tuple reads. Earlier companions keep refusing them.
pub fn validate_basic_owned_joins_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_joins(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndStartingWithAndEndingWithAndIntoBlocksAndWindowsAndSortAndFilter,
        FailurePolicy::OwnedSources,
        SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        PathHierarchyPolicy::Owned,
        AdjacencyTreePolicy::OwnedWithRecursiveFilter,
        SearchPolicy::Basic,
        DynamicFieldPolicy::Closed,
        XmlSerializePolicy::Closed,
        JoinPolicy::Owned,
    )
}

pub(super) fn is_join_read(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::OwnedJoinField(_) | Expression::OwnedJoinSourcePosition(_)
    )
}

/// The closed scalar subset of an owned join tuple.
pub(super) fn admitted_in_tuple(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::OwnedJoinField(_)
            | Expression::OwnedJoinSourcePosition(_)
            | Expression::JoinPosition { .. }
            | Expression::Const { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::ValueMap { .. }
            | Expression::RuntimeValue { .. }
            | Expression::RuntimeParameter { .. }
            | Expression::RuntimeParameterDefault { .. }
            | Expression::Raise { .. }
    )
}

pub(super) fn first_request(join: &crate::OwnedInnerJoin) -> Option<Request> {
    join.plan()
        .sources()
        .next()
        .map(|source| iteration_request(source.collection()))
}

fn member_request(source: &mapping::OwnedJoinSource, relative: &[String]) -> Request {
    let collection = source.collection();
    let mut path = collection.path.clone();
    path.extend(relative.iter().cloned());
    match source.cardinality() {
        mapping::JoinSourceCardinality::Repeating => Request::Field {
            owner: (&collection.owner).into(),
            path,
            frame: Some(collection.path.clone()),
        },
        mapping::JoinSourceCardinality::Singleton => Request::Field {
            owner: (&collection.owner).into(),
            path,
            frame: None,
        },
    }
}

/// Each repeating source names a collection of its own endpoint, each
/// singleton a scalar, and each key a scalar below the member it names.
pub(super) fn plan_structure(
    program: &Program,
    route: &SourceScopeLocation,
    join: &crate::OwnedInnerJoin,
) -> Result<(), ProgramValidationError> {
    let location = || Location::Iteration {
        scope: route.clone(),
    };
    let plan = join.plan();
    for source in plan.sources() {
        let query = match source.cardinality() {
            mapping::JoinSourceCardinality::Repeating => iteration_request(source.collection()),
            mapping::JoinSourceCardinality::Singleton => member_request(source, &[]),
        };
        structure(program, location(), query)?;
    }
    for (right, conditions) in plan.stages() {
        for key in conditions.iter() {
            let left = plan
                .sources()
                .find(|source| source.collection() == key.left_collection())
                .expect("owned plans name joined left sources by construction");
            structure(program, location(), member_request(left, key.left_path()))?;
            structure(program, location(), member_request(right, key.right_path()))?;
        }
    }
    Ok(())
}

impl<'a> Admission<'a> {
    /// Admit one owned join scope under the explicit join policy. Window bounds
    /// keep the receiving parent; every other root reads the tuple.
    pub(super) fn owned_join_scope(
        &mut self,
        scope: &'a TargetScope,
        route: SourceScopeLocation,
        ancestry: Vec<Ancestor<'a>>,
        inherited: Option<Closed>,
        join: &'a crate::OwnedInnerJoin,
    ) -> Result<(), ProgramValidationError> {
        let query = first_request(join);
        let refusal = |reason| {
            failure(
                Location::Iteration {
                    scope: route.clone(),
                },
                query.clone(),
                Closed {
                    reason,
                    origin: Origin::Scope {
                        scope: route.clone(),
                    },
                },
            )
        };
        if let Some(closed) = inherited {
            return Err(failure(
                Location::Iteration {
                    scope: route.clone(),
                },
                query,
                closed,
            ));
        }
        if self.join == JoinPolicy::Closed {
            return Err(refusal(Capability::Join));
        }
        for source in join.plan().sources() {
            let source_query = iteration_request(source.collection());
            if let Some(closed) = dynamic_owner(self.program, &source_query) {
                return Err(failure(
                    Location::Iteration {
                        scope: route.clone(),
                    },
                    Some(source_query),
                    closed,
                ));
            }
        }
        let iteration = scope
            .iteration
            .as_ref()
            .expect("an owned join scope iterates");
        if iteration.grouping().is_some() || iteration.post_group_filter().is_some() {
            return Err(refusal(Capability::Grouping));
        }
        match (&scope.construction, iteration.output()) {
            (
                TargetConstruction::Group | TargetConstruction::Scalar { .. },
                IterationOutput::Repeated,
            )
            | (TargetConstruction::Group, IterationOutput::First) => {}
            (TargetConstruction::Group | TargetConstruction::Scalar { .. }, _) => {
                return Err(refusal(Capability::Document));
            }
            (TargetConstruction::DynamicGroup { .. }, _) => {
                return Err(refusal(Capability::DynamicTarget));
            }
            (TargetConstruction::XmlMixedContent { .. }, _) => {
                return Err(refusal(Capability::XmlOperator));
            }
            (TargetConstruction::CopyCurrentSource, _) => {
                return Err(refusal(Capability::SpecialConstruction));
            }
            (_, _) => return Err(refusal(Capability::TreeOperator)),
        }
        for node in iteration
            .windows()
            .iter()
            .copied()
            .flat_map(crate::SequenceWindow::nodes)
        {
            self.expression(Visit {
                namespace: Namespace::Main,
                closed: None,
                scope: Some(route.clone()),
                ancestry: ancestry.clone(),
                join_tuple: None,
                node,
            })?;
        }
        self.join_tuple(scope, route.clone(), &route)
    }

    fn join_tuple(
        &mut self,
        scope: &'a TargetScope,
        route: SourceScopeLocation,
        join_route: &SourceScopeLocation,
    ) -> Result<(), ProgramValidationError> {
        let mut roots = scope
            .bindings
            .iter()
            .map(|binding| binding.expression)
            .collect::<Vec<_>>();
        if let Some(iteration) = &scope.iteration {
            roots.extend(iteration.filter());
            roots.extend(
                iteration
                    .sort()
                    .iter()
                    .flat_map(|sort| sort.keys().map(|key| key.expression)),
            );
        }
        if let TargetConstruction::Scalar { expression, .. } = &scope.construction {
            roots.push(*expression);
        }
        for node in roots {
            self.expression(Visit {
                namespace: Namespace::Main,
                closed: None,
                scope: Some(route.clone()),
                ancestry: Vec::new(),
                join_tuple: Some(join_route.clone()),
                node,
            })?;
        }
        for (index, child) in scope.children.iter().enumerate() {
            let child_route = route.descendant(SourceScopeStep::Child(index));
            // Only static descendant projections share the tuple.
            if child.iteration.is_some() || !matches!(child.construction, TargetConstruction::Group)
            {
                return Err(failure(
                    Location::Iteration { scope: child_route },
                    None,
                    Closed {
                        reason: Capability::Join,
                        origin: Origin::Scope {
                            scope: join_route.clone(),
                        },
                    },
                ));
            }
            self.join_tuple(child, child_route, join_route)?;
        }
        Ok(())
    }
}
