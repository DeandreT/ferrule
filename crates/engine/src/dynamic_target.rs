use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{Graph, NodeId};

use super::{EngineError, PositionFrame, eval_expr};

pub(super) fn eval_dynamic_key(
    graph: &Graph,
    node: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<String, EngineError> {
    let mut in_progress = HashSet::new();
    match eval_expr(graph, node, context, positions, &mut in_progress)? {
        Value::String(key) => Ok(key),
        other => Err(EngineError::DynamicPropertyName {
            node,
            found: other.type_name(),
        }),
    }
}

pub(super) fn insert_target_field(
    fields: &mut Vec<(String, Instance)>,
    name: String,
    value: Instance,
) -> Result<(), EngineError> {
    if fields.iter().any(|(existing, _)| existing == &name) {
        return Err(EngineError::DuplicateDynamicProperty(name));
    }
    fields.push((name, value));
    Ok(())
}

pub(super) fn insert_dynamic_target_field(
    fields: &mut Vec<(String, Instance)>,
    name: String,
    value: Instance,
    target: Option<&ir::SchemaNode>,
) -> Result<(), EngineError> {
    if target.is_some_and(|schema| schema.child(&name).is_some()) {
        return Err(EngineError::DuplicateDynamicProperty(name));
    }
    insert_target_field(fields, name, value)
}

pub(super) fn merge_dynamic_fragments(fragments: Vec<Instance>) -> Result<Instance, EngineError> {
    let mut merged = Vec::new();
    for fragment in fragments {
        let Instance::Group(fields) = fragment else {
            return Err(EngineError::InvalidDynamicPropertyFragment);
        };
        for (name, value) in fields {
            insert_target_field(&mut merged, name, value)?;
        }
    }
    Ok(Instance::Group(merged))
}
