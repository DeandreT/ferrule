use std::collections::{BTreeMap, HashSet};

use ir::{Instance, Value};
use mapping::{AdjacencyTreePlan, NodeId, OwnedAdjacencyTreePlan};

use super::EngineError;
use super::eval_expr::{EvalProgram, eval_expr};
use super::resolve::{field_scalar, repeated};
use super::sequence::{MAX_GENERATED_SEQUENCE_ITEMS, MAX_RECURSIVE_SEQUENCE_DEPTH};
use super::source_iteration::PositionFrame;

struct Row {
    key: String,
}

/// Row-relative key/parent paths and generated target names; none of them
/// selects a source.
struct Fields<'a> {
    key: &'a [String],
    parent: &'a [String],
    target_key: &'a str,
    target_children: &'a str,
}

/// The optional root expression, evaluated once in the receiving context.
struct RootExpression<'p, 'c> {
    program: EvalProgram<'p>,
    node: Option<NodeId>,
    context: &'c [&'c Instance],
    positions: &'c [PositionFrame],
}

pub(super) fn construct(
    program: EvalProgram<'_>,
    plan: &AdjacencyTreePlan,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<Instance, EngineError> {
    let collection = repeated(context, plan.collection())
        .ok_or_else(|| EngineError::MissingAdjacencyCollection(plan.collection().join("/")))?;
    build(
        collection,
        Fields {
            key: plan.key(),
            parent: plan.parent(),
            target_key: plan.target_key(),
            target_children: plan.target_children(),
        },
        RootExpression {
            program,
            node: plan.root(),
            context,
            positions,
        },
    )
}

/// Builds from the exact owner's rows in the receiving owned context. An
/// active same-owner frame pins its prefix; otherwise the endpoint root is
/// used. Other owners and legacy frames never supply rows, and the root
/// expression keeps the receiving context.
pub(super) fn construct_owned(
    program: EvalProgram<'_>,
    plan: &OwnedAdjacencyTreePlan,
    state: &crate::owned_context::OwnedContext<'_>,
    context: &[&Instance],
    positions: &[PositionFrame],
    scope: &mapping::SourceScopeLocation,
) -> Result<Instance, EngineError> {
    use crate::source_iteration::{ObservedWalkRequest, select_observed};
    let access = |source| match source {
        crate::OwnedSourceError::UnsupportedSourceDomain { found } => {
            EngineError::UnsupportedOwnedContext {
                node: None,
                function: None,
                scope: Some(scope.clone()),
                context: if found == "a document set" {
                    crate::OwnedCapabilityContext::Document
                } else {
                    crate::OwnedCapabilityContext::GeneratedItem
                },
            }
        }
        source => EngineError::OwnedSourceAccess {
            node: None,
            scope: Some(Box::new(scope.clone())),
            source,
        },
    };
    let (owner, base, prefix, tail, budget) = state.select(plan.collection()).map_err(access)?;
    let selected = select_observed(
        ObservedWalkRequest {
            base,
            path: tail,
            raw_prefix: prefix,
            canonical_prefix: prefix,
            owner: Some(owner),
            context: state,
            position_offset: positions.len(),
            strict_collection: true,
        },
        &budget,
        false,
    )
    .map_err(access)?;
    let rows = selected
        .extensions
        .iter()
        .filter_map(|extension| extension.extension.instances.last().copied())
        .collect::<Vec<_>>();
    build(
        rows,
        Fields {
            key: plan.key(),
            parent: plan.parent(),
            target_key: plan.target_key(),
            target_children: plan.target_children(),
        },
        RootExpression {
            program,
            node: plan.root(),
            context,
            positions,
        },
    )
}

/// Shared by legacy and owned rows: source order, limits and error precedence
/// (rows, then root, then root count and recursion) are identical.
fn build<'a, I>(
    collection: I,
    fields: Fields<'_>,
    root: RootExpression<'_, '_>,
) -> Result<Instance, EngineError>
where
    I: IntoIterator<Item = &'a Instance>,
    I::IntoIter: ExactSizeIterator,
{
    let collection = collection.into_iter();
    if collection.len() as u128 > MAX_GENERATED_SEQUENCE_ITEMS {
        return Err(EngineError::AdjacencyTreeTooLarge {
            max: MAX_GENERATED_SEQUENCE_ITEMS,
        });
    }
    let mut rows = Vec::with_capacity(collection.len());
    let mut by_key = BTreeMap::new();
    let mut by_parent: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();
    for (index, instance) in collection.enumerate() {
        let key = string_field(instance, fields.key, "key")?;
        if by_key.insert(key.clone(), index).is_some() {
            return Err(EngineError::DuplicateAdjacencyKey(key));
        }
        let parent = optional_string_field(instance, fields.parent, "parent")?;
        by_parent.entry(parent.clone()).or_default().push(index);
        rows.push(Row { key });
    }
    let selected = match root.node {
        Some(node) => {
            let mut in_progress = HashSet::<NodeId>::new();
            match eval_expr(
                root.program,
                node,
                root.context,
                root.positions,
                &mut in_progress,
            )? {
                Value::Null | Value::JsonNull(_) => None,
                Value::String(value) => Some(value),
                value => {
                    return Err(EngineError::InvalidAdjacencyRoot {
                        found: value.type_name(),
                    });
                }
            }
        }
        None => None,
    };
    let roots = by_parent
        .get(&selected)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if roots.len() != 1 {
        return Err(EngineError::AdjacencyRootCount { count: roots.len() });
    }
    let mut active = Vec::new();
    build_row(roots[0], &rows, &by_parent, &fields, 0, &mut active)
}

fn build_row(
    index: usize,
    rows: &[Row],
    by_parent: &BTreeMap<Option<String>, Vec<usize>>,
    fields: &Fields<'_>,
    depth: usize,
    active: &mut Vec<usize>,
) -> Result<Instance, EngineError> {
    if depth >= MAX_RECURSIVE_SEQUENCE_DEPTH {
        return Err(EngineError::AdjacencyTreeDepth {
            limit: MAX_RECURSIVE_SEQUENCE_DEPTH,
        });
    }
    if active.contains(&index) {
        return Err(EngineError::AdjacencyCycle(rows[index].key.clone()));
    }
    active.push(index);
    let row = &rows[index];
    let child_indices = by_parent
        .get(&Some(row.key.clone()))
        .map(Vec::as_slice)
        .unwrap_or_default();
    let children = child_indices
        .iter()
        .map(|child| build_row(*child, rows, by_parent, fields, depth + 1, active))
        .collect::<Result<Vec<_>, _>>()?;
    active.pop();
    Ok(Instance::Group(
        (vec![
            (
                fields.target_key.to_string(),
                Instance::Scalar(Value::String(row.key.clone())),
            ),
            (
                fields.target_children.to_string(),
                Instance::Repeated(children),
            ),
        ])
        .into(),
    ))
}

fn string_field(
    instance: &Instance,
    path: &[String],
    role: &'static str,
) -> Result<String, EngineError> {
    match field_scalar(instance, path) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(value) => Err(EngineError::InvalidAdjacencyField {
            role,
            path: path.join("/"),
            found: value.type_name(),
        }),
        None => Err(EngineError::InvalidAdjacencyField {
            role,
            path: path.join("/"),
            found: "missing value",
        }),
    }
}

fn optional_string_field(
    instance: &Instance,
    path: &[String],
    role: &'static str,
) -> Result<Option<String>, EngineError> {
    match field_scalar(instance, path) {
        Some(Value::Null | Value::JsonNull(_)) | None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(value) => Err(EngineError::InvalidAdjacencyField {
            role,
            path: path.join("/"),
            found: value.type_name(),
        }),
    }
}
