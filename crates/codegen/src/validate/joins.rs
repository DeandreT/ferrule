use std::collections::{BTreeMap, BTreeSet};

use mapping::NodeId;

use crate::{
    Expression, InnerJoin, JoinId, JoinKeySide, JoinSourceCardinality, Program, TargetScope,
};

use super::{
    ProgramValidationError, graph_dependencies,
    sources::{SchemaCursor, SourceCatalog},
};

#[derive(Debug, Clone)]
pub(super) struct ActiveJoin {
    id: JoinId,
    sources: Vec<(Vec<String>, JoinSourceCardinality)>,
}

impl ActiveJoin {
    pub(super) fn new(join: &InnerJoin) -> Self {
        Self {
            id: join.id(),
            sources: join
                .plan()
                .sources()
                .map(|source| (source.collection().to_vec(), source.cardinality()))
                .collect(),
        }
    }

    fn cardinality(&self, collection: &[String]) -> Option<JoinSourceCardinality> {
        self.sources
            .iter()
            .find_map(|(candidate, cardinality)| (candidate == collection).then_some(*cardinality))
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
    if let Some(join) = scope
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.inner_join())
        && !owners.insert(join.id())
    {
        return Err(ProgramValidationError::DuplicateJoinOwner { join: join.id() });
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
    for source in join.plan().sources() {
        let candidates = sources.path_targets(source.collection());
        let valid = match source.cardinality() {
            JoinSourceCardinality::Repeating => {
                source.collection().is_empty()
                    || candidates
                        .iter()
                        .any(|candidate| candidate.node().repeating)
            }
            JoinSourceCardinality::Singleton => candidates
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
    for (right, conditions) in join.plan().stages() {
        for key in conditions.iter() {
            let left_is_singleton = join.plan().sources().any(|source| {
                source.collection() == key.left_collection()
                    && source.cardinality() == JoinSourceCardinality::Singleton
            });
            validate_key(
                sources,
                join.id(),
                key.left_collection(),
                key.left_path(),
                JoinKeySide::Left,
                left_is_singleton,
            )?;
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
                let Some(owner) = active_joins.iter().rev().find(|owner| owner.id == *join) else {
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
            Expression::JoinAggregate {
                join,
                expression,
                arg,
                ..
            } => {
                if !root_context {
                    validate_correlated_aggregate(node, sources, current_source, join)?;
                } else {
                    validate_plan(sources, join)?;
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
            Expression::CollectionFind {
                predicate, value, ..
            } => {
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
    let join_sources = join.plan().sources().collect::<Vec<_>>();
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
    let mut has_current_anchor = false;
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

fn scalar_below(sources: SourceCatalog<'_>, collection: &[String], path: &[String]) -> bool {
    sources.path_targets(collection).iter().any(|candidate| {
        candidate
            .follow(path)
            .is_some_and(|leaf| leaf.node().is_scalar())
    })
}
