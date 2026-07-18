use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{Graph, RecursiveFilterPlan};

use crate::EngineError;
use crate::eval_expr::eval_expr;
use crate::sequence::MAX_RECURSIVE_SEQUENCE_DEPTH;
use crate::source_iteration::PositionFrame;

pub(super) fn execute(
    graph: &Graph,
    plan: &RecursiveFilterPlan,
    current: &Instance,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<Instance, EngineError> {
    let mut context = context.to_vec();
    let mut positions = positions.to_vec();
    filter_group(graph, plan, current, &mut context, &mut positions, 0)
}

fn filter_group<'a>(
    graph: &Graph,
    plan: &RecursiveFilterPlan,
    current: &'a Instance,
    context: &mut Vec<&'a Instance>,
    positions: &mut Vec<PositionFrame>,
    depth: usize,
) -> Result<Instance, EngineError> {
    if depth >= MAX_RECURSIVE_SEQUENCE_DEPTH {
        return Err(EngineError::RecursiveFilterDepth {
            limit: MAX_RECURSIVE_SEQUENCE_DEPTH,
        });
    }
    let Instance::Group(fields) = current else {
        return Err(EngineError::RecursiveFilterRequiresGroup {
            found: instance_kind(current),
        });
    };

    let mut output = Vec::with_capacity(fields.len());
    for (name, value) in fields {
        let value = if name == plan.items() {
            filter_items(graph, plan, value, context, positions)?
        } else if name == plan.children() {
            filter_children(graph, plan, value, context, positions, depth)?
        } else {
            value.clone()
        };
        output.push((name.clone(), value));
    }
    Ok(Instance::Group(output))
}

fn filter_items<'a>(
    graph: &Graph,
    plan: &RecursiveFilterPlan,
    collection: &'a Instance,
    context: &mut Vec<&'a Instance>,
    positions: &mut Vec<PositionFrame>,
) -> Result<Instance, EngineError> {
    let Instance::Repeated(items) = collection else {
        return Err(EngineError::RecursiveFilterRequiresCollection {
            field: plan.items().to_owned(),
            found: instance_kind(collection),
        });
    };
    let mut output = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        context.push(item);
        positions.push(position(plan.items(), index));
        let mut in_progress = HashSet::new();
        let keep = eval_expr(
            graph,
            plan.predicate(),
            context,
            positions,
            &mut in_progress,
        );
        positions.pop();
        context.pop();
        match keep? {
            Value::Bool(true) => output.push(item.clone()),
            Value::Bool(false) => {}
            value => {
                return Err(EngineError::NotABool {
                    node: plan.predicate(),
                    found: value.type_name(),
                });
            }
        }
    }
    Ok(Instance::Repeated(output))
}

fn filter_children<'a>(
    graph: &Graph,
    plan: &RecursiveFilterPlan,
    collection: &'a Instance,
    context: &mut Vec<&'a Instance>,
    positions: &mut Vec<PositionFrame>,
    depth: usize,
) -> Result<Instance, EngineError> {
    let Instance::Repeated(children) = collection else {
        return Err(EngineError::RecursiveFilterRequiresCollection {
            field: plan.children().to_owned(),
            found: instance_kind(collection),
        });
    };
    let mut output = Vec::with_capacity(children.len());
    for (index, child) in children.iter().enumerate() {
        context.push(child);
        positions.push(position(plan.children(), index));
        let filtered = filter_group(graph, plan, child, context, positions, depth + 1);
        positions.pop();
        context.pop();
        output.push(filtered?);
    }
    Ok(Instance::Repeated(output))
}

fn position(collection: &str, index: usize) -> PositionFrame {
    PositionFrame {
        collection: vec![collection.to_owned()],
        index: index + 1,
        grouped: false,
        join: None,
        join_position: None,
        document_path: None,
    }
}

fn instance_kind(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(_) => "scalar",
        Instance::Group(_) => "group",
        Instance::Repeated(_) => "repeated collection",
        Instance::MappedSequence(_) => "mapped sequence",
        Instance::DocumentSet(_) => "document set",
    }
}
