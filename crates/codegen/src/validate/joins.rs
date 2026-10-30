use std::collections::{BTreeMap, BTreeSet};

use mapping::NodeId;

use crate::{
    Expression, InnerJoin, JoinId, JoinKeySide, JoinSource, JoinSourceCardinality, OwnedInnerJoin,
    Program, TargetScope,
};

use super::{
    ProgramValidationError, graph_dependencies,
    sources::{SchemaCursor, SourceCatalog},
};

#[derive(Debug, Clone)]
enum ActiveSources {
    Legacy(Vec<(Vec<String>, JoinSourceCardinality)>),
    Owned(Vec<(mapping::OwnedSourceRef, JoinSourceCardinality)>),
}

#[derive(Debug, Clone)]
pub(super) struct ActiveJoin {
    id: JoinId,
    sources: ActiveSources,
}

impl ActiveJoin {
    pub(super) fn new(join: &InnerJoin) -> Self {
        Self {
            id: join.id(),
            sources: ActiveSources::Legacy(
                join.plan()
                    .collection_sources()
                    .map(|source| (source.collection().to_vec(), source.cardinality()))
                    .collect(),
            ),
        }
    }

    pub(super) fn owned(join: &OwnedInnerJoin) -> Self {
        Self::owned_plan(join.id(), join.plan())
    }

    fn owned_plan(id: JoinId, plan: &mapping::OwnedJoinPlan) -> Self {
        Self {
            id,
            sources: ActiveSources::Owned(
                plan.sources()
                    .map(|source| (source.collection().clone(), source.cardinality().into()))
                    .collect(),
            ),
        }
    }

    fn cardinality(&self, collection: &[String]) -> Option<JoinSourceCardinality> {
        match &self.sources {
            ActiveSources::Legacy(sources) => {
                sources.iter().find_map(|(candidate, cardinality)| {
                    (candidate == collection).then_some(*cardinality)
                })
            }
            ActiveSources::Owned(_) => None,
        }
    }

    fn owned_cardinality(
        &self,
        collection: &mapping::OwnedSourceRef,
    ) -> Option<JoinSourceCardinality> {
        match &self.sources {
            ActiveSources::Owned(sources) => sources.iter().find_map(|(candidate, cardinality)| {
                (candidate == collection).then_some(*cardinality)
            }),
            ActiveSources::Legacy(_) => None,
        }
    }

    const fn is_owned(&self) -> bool {
        matches!(self.sources, ActiveSources::Owned(_))
    }
}

pub(super) fn validate_owners(program: &Program) -> Result<(), ProgramValidationError> {
    let mut owners = BTreeSet::new();
    collect_owners(&program.root, &mut owners)?;
    for target in &program.extra_targets {
        collect_owners(&target.root, &mut owners)?;
    }
    Ok(())
}

fn collect_owners(
    scope: &TargetScope,
    owners: &mut BTreeSet<JoinId>,
) -> Result<(), ProgramValidationError> {
    if let Some(join) = scope.iteration.as_ref().and_then(|iteration| {
        iteration
            .inner_join()
            .map(InnerJoin::id)
            .or_else(|| iteration.owned_inner_join().map(OwnedInnerJoin::id))
    }) && !owners.insert(join)
    {
        return Err(ProgramValidationError::DuplicateJoinOwner { join });
    }
    if let Some(sequence) = scope
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.concatenated())
    {
        for segment in sequence.iter() {
            collect_owners(segment, owners)?;
        }
    }
    for child in &scope.children {
        collect_owners(child, owners)?;
    }
    if let crate::TargetConstruction::DynamicGroup { children, .. } = &scope.construction {
        for child in children {
            collect_owners(&child.scope, owners)?;
        }
    }
    Ok(())
}

pub(super) fn validate_plan(
    sources: SourceCatalog<'_>,
    join: &InnerJoin,
) -> Result<(), ProgramValidationError> {
    for source in join.plan().collection_sources() {
        let valid = match source.cardinality() {
            JoinSourceCardinality::Repeating => {
                source.collection().is_empty()
                    || sources
                        .path_targets(source.collection())
                        .iter()
                        .any(|candidate| candidate.node().repeating)
            }
            // A singleton crossing repetition would select one value per
            // repeated item, so only direct paths are singleton sources.
            JoinSourceCardinality::Singleton => sources
                .direct_path_targets(source.collection())
                .iter()
                .any(|candidate| !candidate.node().repeating && candidate.node().is_scalar()),
        };
        if !valid {
            return Err(ProgramValidationError::InvalidJoinSource {
                join: join.id(),
                collection: source.collection().to_vec(),
                cardinality: source.cardinality(),
            });
        }
    }
    // A computed member is its own key, with no key path to resolve; plan
    // construction keeps its right paths empty.
    for (right, conditions) in join.plan().stages() {
        for key in conditions.iter() {
            if let Some(left_collection) = key.left_collection() {
                let left_is_singleton = join.plan().collection_sources().any(|source| {
                    source.collection() == left_collection
                        && source.cardinality() == JoinSourceCardinality::Singleton
                });
                validate_key(
                    sources,
                    join.id(),
                    left_collection,
                    key.left_path(),
                    JoinKeySide::Left,
                    left_is_singleton,
                )?;
            }
            if let JoinSource::Collection(right) = right {
                validate_key(
                    sources,
                    join.id(),
                    right.collection(),
                    key.right_path(),
                    JoinKeySide::Right,
                    right.cardinality() == JoinSourceCardinality::Singleton,
                )?;
            }
        }
    }
    Ok(())
}

/// A computed key is one scalar per join execution, so it admits only the
/// expression kinds the interpreter admits there: unframed source fields,
/// constants, calls, conditionals, value maps, runtime values and runtime
/// parameters. Each source read must not cross or end at repetition from
/// any anchor, and a root-context read must resolve from the root.
pub(super) fn validate_computed_key(
    join: &InnerJoin,
    expressions: &BTreeMap<NodeId, &Expression>,
    sources: SourceCatalog<'_>,
    root_context: bool,
) -> Result<(), ProgramValidationError> {
    let Some(computed) = join.plan().computed_source() else {
        return Ok(());
    };
    if !expressions.contains_key(&computed.value()) {
        return Err(ProgramValidationError::MissingComputedJoinKey {
            join: join.id(),
            computed: computed.id(),
            expression: computed.value(),
        });
    }
    let mut pending = vec![computed.value()];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            continue;
        }
        // A missing dependency is reported by expression validation.
        let Some(expression) = expressions.get(&node) else {
            continue;
        };
        match expression {
            Expression::SourceField { path, frame: None } => {
                let all = sources.path_targets(path);
                let direct = sources.direct_path_targets(path);
                if all.len() != direct.len()
                    || direct.iter().any(|candidate| candidate.node().repeating)
                {
                    return Err(ProgramValidationError::ComputedJoinKeyCrossesRepetition {
                        join: join.id(),
                        computed: computed.id(),
                        node,
                        path: path.clone(),
                    });
                }
                if root_context && !direct.is_empty() && sources.root_direct_target(path).is_none()
                {
                    return Err(ProgramValidationError::UnresolvedRootComputedJoinKey {
                        join: join.id(),
                        computed: computed.id(),
                        node,
                        path: path.clone(),
                    });
                }
            }
            Expression::Const { .. }
            | Expression::RuntimeValue { .. }
            | Expression::RuntimeParameter { .. }
            | Expression::RuntimeParameterDefault { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::ValueMap { .. } => pending.extend(graph_dependencies::of(expression)),
            _ => {
                return Err(ProgramValidationError::InvalidComputedJoinKeyNode {
                    join: join.id(),
                    computed: computed.id(),
                    node,
                });
            }
        }
    }
    Ok(())
}

/// A root-context join has no active item frame, so each singleton must
/// resolve from the primary root or a named source rather than from a
/// descendant that exists only beneath some other collection.
pub(super) fn validate_root_plan(
    sources: SourceCatalog<'_>,
    join: &InnerJoin,
) -> Result<(), ProgramValidationError> {
    validate_plan(sources, join)?;
    for source in join.plan().collection_sources() {
        if source.cardinality() == JoinSourceCardinality::Singleton
            && !sources
                .root_direct_target(source.collection())
                .is_some_and(|candidate| {
                    !candidate.node().repeating && candidate.node().is_scalar()
                })
        {
            return Err(ProgramValidationError::UnresolvedRootJoinSingleton {
                join: join.id(),
                collection: source.collection().to_vec(),
            });
        }
    }
    Ok(())
}

fn validate_key(
    sources: SourceCatalog<'_>,
    join: JoinId,
    collection: &[String],
    path: &[String],
    side: JoinKeySide,
    singleton: bool,
) -> Result<(), ProgramValidationError> {
    if (!singleton || path.is_empty()) && scalar_below(sources, collection, path) {
        Ok(())
    } else {
        Err(ProgramValidationError::InvalidJoinKey {
            join,
            side,
            collection: collection.to_vec(),
            path: path.to_vec(),
        })
    }
}

pub(super) fn validate_expression(
    root: NodeId,
    expressions: &BTreeMap<NodeId, &Expression>,
    sources: SourceCatalog<'_>,
    current_source: Option<SchemaCursor<'_>>,
    active_joins: &[ActiveJoin],
    root_context: bool,
) -> Result<(), ProgramValidationError> {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            continue;
        }
        let Some(expression) = expressions.get(&node) else {
            continue;
        };
        match expression {
            Expression::JoinField {
                join,
                collection,
                path,
            } => {
                let Some(owner) = active_joins
                    .iter()
                    .rev()
                    .find(|owner| owner.id == *join && !owner.is_owned())
                else {
                    return Err(ProgramValidationError::InactiveJoinExpression {
                        node,
                        join: *join,
                    });
                };
                let Some(cardinality) = owner.cardinality(collection) else {
                    return Err(ProgramValidationError::InvalidJoinFieldCollection {
                        node,
                        join: *join,
                        collection: collection.clone(),
                    });
                };
                if (cardinality == JoinSourceCardinality::Singleton && !path.is_empty())
                    || !scalar_below(sources, collection, path)
                {
                    return Err(ProgramValidationError::InvalidJoinFieldPath {
                        node,
                        join: *join,
                        collection: collection.clone(),
                        path: path.clone(),
                    });
                }
            }
            Expression::JoinPosition { join }
                if !active_joins.iter().any(|owner| owner.id == *join) =>
            {
                return Err(ProgramValidationError::InactiveJoinExpression { node, join: *join });
            }
            Expression::OwnedJoinField(mapping::OwnedJoinField {
                join, collection, ..
            })
            | Expression::OwnedJoinSourcePosition(mapping::OwnedJoinSourcePosition {
                join,
                collection,
            }) => {
                let join = JoinId::from(*join);
                let Some(owner) = active_joins
                    .iter()
                    .rev()
                    .find(|owner| owner.id == join && owner.is_owned())
                else {
                    return Err(ProgramValidationError::InactiveJoinExpression { node, join });
                };
                let Some(cardinality) = owner.owned_cardinality(collection) else {
                    return Err(ProgramValidationError::InvalidOwnedJoinMember {
                        node,
                        join,
                        collection: collection.clone(),
                    });
                };
                let field_path = match expression {
                    Expression::OwnedJoinField(field) => Some(&field.path),
                    _ => None,
                };
                if let Some(path) = field_path
                    && !owned_member_scalar(sources, collection, cardinality, path)
                {
                    return Err(ProgramValidationError::InvalidOwnedJoinFieldPath {
                        node,
                        join,
                        collection: collection.clone(),
                        path: path.clone(),
                    });
                }
            }
            Expression::JoinAggregate {
                join,
                expression,
                arg,
                ..
            } => {
                if !root_context {
                    validate_correlated_aggregate(node, sources, current_source, join)?;
                } else {
                    validate_root_plan(sources, join)?;
                }
                // The computed key keeps the receiving context, like the
                // argument.
                validate_computed_key(join, expressions, sources, root_context)?;
                if let Some(computed) = join.plan().computed_source() {
                    validate_expression(
                        computed.value(),
                        expressions,
                        sources,
                        current_source,
                        active_joins,
                        root_context,
                    )?;
                }
                if let Some(expression) = expression {
                    validate_expression(
                        *expression,
                        expressions,
                        sources,
                        None,
                        &[ActiveJoin::new(join)],
                        false,
                    )?;
                }
                if let Some(arg) = arg {
                    validate_expression(
                        *arg,
                        expressions,
                        sources,
                        current_source,
                        active_joins,
                        root_context,
                    )?;
                }
            }
            // Only this join is active in its tuple expression; the argument
            // keeps the receiving context and its dominating joins.
            Expression::OwnedJoinAggregate(aggregate) => {
                if let Some(expression) = aggregate.expression {
                    validate_expression(
                        expression,
                        expressions,
                        sources,
                        None,
                        &[ActiveJoin::owned_plan(
                            JoinId::from(aggregate.join),
                            &aggregate.plan,
                        )],
                        false,
                    )?;
                }
                if let Some(arg) = aggregate.arg {
                    validate_expression(
                        arg,
                        expressions,
                        sources,
                        current_source,
                        active_joins,
                        root_context,
                    )?;
                }
            }
            Expression::Aggregate { value, arg, .. } => {
                if let Some(value) = value.expression() {
                    validate_expression(value, expressions, sources, None, active_joins, false)?;
                }
                if let Some(arg) = arg {
                    validate_expression(
                        *arg,
                        expressions,
                        sources,
                        current_source,
                        active_joins,
                        root_context,
                    )?;
                }
            }
            Expression::OwnedAggregate(aggregate) => {
                if let Some(value) = aggregate.expression() {
                    validate_expression(value, expressions, sources, None, active_joins, false)?;
                }
                if let Some(arg) = aggregate.arg {
                    validate_expression(
                        arg,
                        expressions,
                        sources,
                        current_source,
                        active_joins,
                        root_context,
                    )?;
                }
            }
            Expression::CollectionFind {
                predicate, value, ..
            }
            | Expression::OwnedCollectionFind(mapping::OwnedCollectionFind {
                predicate,
                value,
                ..
            }) => {
                validate_expression(*predicate, expressions, sources, None, active_joins, false)?;
                validate_expression(*value, expressions, sources, None, active_joins, false)?;
            }
            Expression::SequenceExists {
                sequence,
                predicate,
            } => {
                for input in sequence.inputs() {
                    validate_expression(
                        input,
                        expressions,
                        sources,
                        current_source,
                        active_joins,
                        root_context,
                    )?;
                }
                validate_expression(*predicate, expressions, sources, None, active_joins, false)?;
            }
            Expression::SequenceAggregate {
                sequence,
                predicate,
                expression,
                arg,
                ..
            } => {
                for input in sequence.inputs().chain(arg.iter().copied()) {
                    validate_expression(
                        input,
                        expressions,
                        sources,
                        current_source,
                        active_joins,
                        root_context,
                    )?;
                }
                if let Some(predicate) = predicate {
                    validate_expression(
                        *predicate,
                        expressions,
                        sources,
                        None,
                        active_joins,
                        false,
                    )?;
                }
                if let Some(expression) = expression {
                    validate_expression(
                        *expression,
                        expressions,
                        sources,
                        None,
                        active_joins,
                        false,
                    )?;
                }
            }
            _ => pending.extend(graph_dependencies::of(expression)),
        }
    }
    Ok(())
}

fn validate_correlated_aggregate(
    node: NodeId,
    sources: SourceCatalog<'_>,
    current_source: Option<SchemaCursor<'_>>,
    join: &InnerJoin,
) -> Result<(), ProgramValidationError> {
    let Some(current_source) = current_source else {
        return Err(ProgramValidationError::JoinAggregateRequiresRootContext {
            node,
            join: join.id(),
        });
    };
    if !is_bounded_correlated_plan(sources, current_source, join) {
        return Err(ProgramValidationError::JoinAggregateRequiresRootContext {
            node,
            join: join.id(),
        });
    }
    validate_plan(sources, join)
}

pub(super) fn validate_correlated_scope(
    target_path: &[String],
    sources: SourceCatalog<'_>,
    current_source: Option<SchemaCursor<'_>>,
    join: &InnerJoin,
) -> Result<(), ProgramValidationError> {
    let Some(current_source) = current_source else {
        return Err(ProgramValidationError::JoinRequiresRootContext {
            target_path: target_path.to_vec(),
            join: join.id(),
        });
    };
    if !is_bounded_correlated_plan(sources, current_source, join) {
        return Err(ProgramValidationError::JoinRequiresRootContext {
            target_path: target_path.to_vec(),
            join: join.id(),
        });
    }
    validate_plan(sources, join)
}

fn is_bounded_correlated_plan(
    sources: SourceCatalog<'_>,
    current_source: SchemaCursor<'_>,
    join: &InnerJoin,
) -> bool {
    let join_sources = join.plan().collection_sources().collect::<Vec<_>>();
    let singleton_sources = join_sources
        .iter()
        .copied()
        .filter(|source| source.cardinality() == JoinSourceCardinality::Singleton)
        .collect::<Vec<_>>();
    let repeating_sources = join_sources
        .iter()
        .copied()
        .filter(|source| source.cardinality() == JoinSourceCardinality::Repeating)
        .collect::<Vec<_>>();
    if singleton_sources.len() + repeating_sources.len() != join_sources.len() {
        return false;
    }
    let runtime_ancestors = sources.runtime_ancestors(current_source);
    // A computed key is evaluated once per receiving item, so it anchors the
    // plan there like a current-item singleton; its own reads are validated
    // in that context.
    let mut has_current_anchor = join.plan().computed_source().is_some();
    let singletons_are_bounded_scalars = singleton_sources.into_iter().all(|singleton| {
        let candidate =
            match runtime_candidate(current_source, &runtime_ancestors, singleton.collection()) {
                Some((owned_by_current, candidate)) => {
                    has_current_anchor |= owned_by_current;
                    candidate.and_then(SchemaCursor::resolved)
                }
                None => sources
                    .root_schema_at(singleton.collection())
                    .and_then(SchemaCursor::resolved),
            };
        candidate
            .is_some_and(|candidate| !candidate.node().repeating && candidate.node().is_scalar())
    });
    let repeating_sources_are_bounded = repeating_sources.into_iter().all(|repeating| {
        match runtime_candidate(current_source, &runtime_ancestors, repeating.collection()) {
            Some((true, Some(candidate))) => {
                let bounded = !repeating.collection().is_empty() && candidate.node().repeating;
                has_current_anchor |= bounded;
                bounded
            }
            Some((true, None)) | Some((false, None)) => false,
            Some((false, Some(candidate))) => candidate.node().repeating,
            None => sources
                .root_schema_at(repeating.collection())
                .is_some_and(|candidate| candidate.node().repeating),
        }
    });
    has_current_anchor && singletons_are_bounded_scalars && repeating_sources_are_bounded
}

/// Finds the innermost active runtime frame owning a path's first segment.
/// A frame that owns the prefix but not the complete path shadows every outer
/// frame, matching generated runtime source selection.
fn runtime_candidate<'a>(
    current: SchemaCursor<'a>,
    ancestors: &[SchemaCursor<'a>],
    path: &[String],
) -> Option<(bool, Option<SchemaCursor<'a>>)> {
    std::iter::once((true, current))
        .chain(ancestors.iter().copied().map(|ancestor| (false, ancestor)))
        .find_map(|(is_current, candidate)| {
            candidate
                .owns_first(path)
                .then(|| (is_current, candidate.follow(path)))
        })
}

/// A singleton member is read with an empty path; a repeating member's field
/// is a scalar reached without crossing another repetition.
fn owned_member_scalar(
    sources: SourceCatalog<'_>,
    collection: &mapping::OwnedSourceRef,
    cardinality: JoinSourceCardinality,
    path: &[String],
) -> bool {
    if cardinality == JoinSourceCardinality::Singleton {
        return path.is_empty();
    }
    let Some(mut current) = sources
        .owned_schema_at(collection)
        .and_then(SchemaCursor::resolved)
    else {
        return false;
    };
    for segment in path {
        let Some(next) = current
            .follow(std::slice::from_ref(segment))
            .and_then(SchemaCursor::resolved)
        else {
            return false;
        };
        if next.node().repeating {
            return false;
        }
        current = next;
    }
    current.node().is_scalar()
}

fn scalar_below(sources: SourceCatalog<'_>, collection: &[String], path: &[String]) -> bool {
    sources.path_targets(collection).iter().any(|candidate| {
        candidate
            .follow(path)
            .is_some_and(|leaf| leaf.node().is_scalar())
    })
}
