use std::collections::{BTreeMap, BTreeSet};

use ir::SchemaKind;
use mapping::NodeId;

use crate::{
    Expression, InnerJoin, JoinId, JoinKeySide, JoinSourceCardinality, Program, TargetScope,
};

use super::{ProgramValidationError, graph_dependencies, sources::SourceCatalog};

#[derive(Debug, Clone)]
pub(super) struct ActiveJoin {
    id: JoinId,
    collections: Vec<Vec<String>>,
}

impl ActiveJoin {
    pub(super) fn new(join: &InnerJoin) -> Self {
        Self {
            id: join.id(),
            collections: join
                .plan()
                .sources()
                .map(|source| source.collection().to_vec())
                .collect(),
        }
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
    for child in &scope.children {
        collect_owners(child, owners)?;
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
            JoinSourceCardinality::Singleton => candidates.iter().any(|candidate| {
                !candidate.node().repeating
                    && matches!(candidate.node().kind, SchemaKind::Scalar { .. })
            }),
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
            validate_key(
                sources,
                join.id(),
                key.left_collection(),
                key.left_path(),
                JoinKeySide::Left,
            )?;
            validate_key(
                sources,
                join.id(),
                right.collection(),
                key.right_path(),
                JoinKeySide::Right,
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
) -> Result<(), ProgramValidationError> {
    if scalar_below(sources, collection, path) {
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
                if !owner.collections.contains(collection) {
                    return Err(ProgramValidationError::InvalidJoinFieldCollection {
                        node,
                        join: *join,
                        collection: collection.clone(),
                    });
                }
                if !scalar_below(sources, collection, path) {
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
                    return Err(ProgramValidationError::JoinAggregateRequiresRootContext {
                        node,
                        join: join.id(),
                    });
                }
                validate_plan(sources, join)?;
                if let Some(expression) = expression {
                    validate_expression(
                        *expression,
                        expressions,
                        sources,
                        &[ActiveJoin::new(join)],
                        false,
                    )?;
                }
                if let Some(arg) = arg {
                    validate_expression(*arg, expressions, sources, active_joins, root_context)?;
                }
            }
            Expression::Aggregate { value, arg, .. } => {
                if let Some(value) = value.expression() {
                    validate_expression(value, expressions, sources, active_joins, false)?;
                }
                if let Some(arg) = arg {
                    validate_expression(*arg, expressions, sources, active_joins, root_context)?;
                }
            }
            Expression::CollectionFind {
                predicate, value, ..
            } => {
                validate_expression(*predicate, expressions, sources, active_joins, false)?;
                validate_expression(*value, expressions, sources, active_joins, false)?;
            }
            Expression::SequenceExists {
                sequence,
                predicate,
            } => {
                for input in sequence.inputs() {
                    validate_expression(input, expressions, sources, active_joins, root_context)?;
                }
                validate_expression(*predicate, expressions, sources, active_joins, false)?;
            }
            _ => pending.extend(graph_dependencies::of(expression)),
        }
    }
    Ok(())
}

fn scalar_below(sources: SourceCatalog<'_>, collection: &[String], path: &[String]) -> bool {
    sources.path_targets(collection).iter().any(|candidate| {
        candidate
            .follow(path)
            .is_some_and(|leaf| matches!(leaf.node().kind, SchemaKind::Scalar { .. }))
    })
}
