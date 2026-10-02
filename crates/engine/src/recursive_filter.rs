use std::collections::HashSet;

use ir::{
    Instance, Value, XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD,
};
use mapping::RecursiveFilterPlan;

use crate::EngineError;
use crate::eval_expr::{EvalProgram, eval_expr};
use crate::sequence::MAX_RECURSIVE_SEQUENCE_DEPTH;
use crate::source_iteration::PositionFrame;

pub(super) fn execute(
    program: EvalProgram<'_>,
    plan: &RecursiveFilterPlan,
    current: &Instance,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<Instance, EngineError> {
    let mut context = context.to_vec();
    let mut positions = positions.to_vec();
    filter_group(program, plan, current, &mut context, &mut positions, 0)
}

fn filter_group<'a>(
    program: EvalProgram<'_>,
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
    let mut kept_items = None;
    let mut filtered_children = false;
    for (name, value) in fields {
        let value = if name == plan.items() {
            let (filtered, kept) = filter_items(program, plan, value, context, positions)?;
            kept_items = Some(kept);
            filtered
        } else if name == plan.children() {
            filtered_children = true;
            filter_children(program, plan, value, context, positions, depth)?
        } else {
            value.clone()
        };
        output.push((name.clone(), value));
    }
    if kept_items.is_some() || filtered_children {
        rebuild_ordered_xml(
            fields,
            &mut output,
            plan.items(),
            plan.children(),
            kept_items.as_deref().unwrap_or(&[]),
        );
    }
    Ok(Instance::Group((output).into()))
}

/// The XML choice reader retains an ordered stream of typed child values beside
/// the schema fields. Update that stream with the same per-occurrence decisions
/// so serialization cannot resurrect a filtered child or an unfiltered subtree.
fn rebuild_ordered_xml(
    source: &[(String, Instance)],
    output: &mut [(String, Instance)],
    items: &str,
    children: &str,
    kept_items: &[bool],
) {
    let Some(Instance::Repeated(ordered)) = source
        .iter()
        .find(|(name, _)| name == XML_MIXED_CONTENT_FIELD)
        .map(|(_, value)| value)
    else {
        return;
    };
    let filtered_items = output
        .iter()
        .find(|(name, _)| name == items)
        .and_then(|(_, value)| value.as_repeated());
    let filtered_children = output
        .iter()
        .find(|(name, _)| name == children)
        .and_then(|(_, value)| value.as_repeated());
    let mut item_index = 0;
    let mut kept_index = 0;
    let mut child_index = 0;
    let mut rebuilt = Vec::with_capacity(ordered.len());
    for entry in ordered {
        let Some(name) = entry
            .field(XML_NODE_NAME_FIELD)
            .and_then(Instance::as_scalar)
            .and_then(|value| match value {
                Value::String(name) => Some(name.as_str()),
                _ => None,
            })
        else {
            rebuilt.push(entry.clone());
            continue;
        };
        let replacement = if name == items {
            let keep = kept_items.get(item_index).copied().unwrap_or(false);
            item_index += 1;
            if !keep {
                continue;
            }
            let value = filtered_items.and_then(|items| items.get(kept_index));
            kept_index += 1;
            value
        } else if name == children {
            let value = filtered_children.and_then(|children| children.get(child_index));
            child_index += 1;
            value
        } else {
            None
        };
        if (name == items || name == children) && replacement.is_none() {
            continue;
        }
        let mut entry = entry.clone();
        if let (Some(replacement), Instance::Group(fields)) = (replacement, &mut entry)
            && let Some((_, value)) = fields
                .iter_mut()
                .find(|(name, _)| name == XML_MIXED_CONTENT_VALUE_FIELD)
        {
            *value = replacement.clone();
        }
        rebuilt.push(entry);
    }
    if let Some((_, value)) = output
        .iter_mut()
        .find(|(name, _)| name == XML_MIXED_CONTENT_FIELD)
    {
        *value = Instance::Repeated(rebuilt);
    }
}

fn filter_items<'a>(
    program: EvalProgram<'_>,
    plan: &RecursiveFilterPlan,
    collection: &'a Instance,
    context: &mut Vec<&'a Instance>,
    positions: &mut Vec<PositionFrame>,
) -> Result<(Instance, Vec<bool>), EngineError> {
    let Instance::Repeated(items) = collection else {
        return Err(EngineError::RecursiveFilterRequiresCollection {
            field: plan.items().to_owned(),
            found: instance_kind(collection),
        });
    };
    let mut output = Vec::with_capacity(items.len());
    let mut kept = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        context.push(item);
        positions.push(position(plan.items(), index));
        let mut in_progress = HashSet::new();
        let keep = eval_expr(
            program,
            plan.predicate(),
            context,
            positions,
            &mut in_progress,
        );
        positions.pop();
        context.pop();
        match keep? {
            Value::Bool(true) => {
                output.push(item.clone());
                kept.push(true);
            }
            Value::Bool(false) => kept.push(false),
            value => {
                return Err(EngineError::NotABool {
                    node: plan.predicate(),
                    found: value.type_name(),
                });
            }
        }
    }
    Ok((Instance::Repeated(output), kept))
}

fn filter_children<'a>(
    program: EvalProgram<'_>,
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
        let filtered = filter_group(program, plan, child, context, positions, depth + 1);
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
