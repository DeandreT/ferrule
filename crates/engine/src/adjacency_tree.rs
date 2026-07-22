use std::collections::{BTreeMap, HashSet};

use ir::{Instance, Value};
use mapping::{AdjacencyTreePlan, NodeId};

use super::EngineError;
use super::eval_expr::{EvalProgram, eval_expr};
use super::resolve::{field_scalar, repeated};
use super::sequence::{MAX_GENERATED_SEQUENCE_ITEMS, MAX_RECURSIVE_SEQUENCE_DEPTH};
use super::source_iteration::PositionFrame;

struct Row {
    key: String,
}

pub(super) fn construct(
    program: EvalProgram<'_>,
    plan: &AdjacencyTreePlan,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<Instance, EngineError> {
    let collection = repeated(context, plan.collection())
        .ok_or_else(|| EngineError::MissingAdjacencyCollection(plan.collection().join("/")))?;
    if collection.len() as u128 > MAX_GENERATED_SEQUENCE_ITEMS {
        return Err(EngineError::AdjacencyTreeTooLarge {
            max: MAX_GENERATED_SEQUENCE_ITEMS,
        });
    }
    let mut rows = Vec::with_capacity(collection.len());
    let mut by_key = BTreeMap::new();
    let mut by_parent: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();
    for (index, instance) in collection.iter().enumerate() {
        let key = string_field(instance, plan.key(), "key")?;
        if by_key.insert(key.clone(), index).is_some() {
            return Err(EngineError::DuplicateAdjacencyKey(key));
        }
        let parent = optional_string_field(instance, plan.parent(), "parent")?;
        by_parent.entry(parent.clone()).or_default().push(index);
        rows.push(Row { key });
    }
    let root = match plan.root() {
        Some(node) => {
            let mut in_progress = HashSet::<NodeId>::new();
            match eval_expr(program, node, context, positions, &mut in_progress)? {
                Value::Null => None,
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
    let roots = by_parent.get(&root).map(Vec::as_slice).unwrap_or_default();
    if roots.len() != 1 {
        return Err(EngineError::AdjacencyRootCount { count: roots.len() });
    }
    let mut active = Vec::new();
    build_row(roots[0], &rows, &by_parent, plan, 0, &mut active)
}

fn build_row(
    index: usize,
    rows: &[Row],
    by_parent: &BTreeMap<Option<String>, Vec<usize>>,
    plan: &AdjacencyTreePlan,
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
        .map(|child| build_row(*child, rows, by_parent, plan, depth + 1, active))
        .collect::<Result<Vec<_>, _>>()?;
    active.pop();
    Ok(Instance::Group(vec![
        (
            plan.target_key().to_string(),
            Instance::Scalar(Value::String(row.key.clone())),
        ),
        (
            plan.target_children().to_string(),
            Instance::Repeated(children),
        ),
    ]))
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
        Some(Value::Null) | None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(value) => Err(EngineError::InvalidAdjacencyField {
            role,
            path: path.join("/"),
            found: value.type_name(),
        }),
    }
}
