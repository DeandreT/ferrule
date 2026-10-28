//! Extension export must preserve the early float-conversion failure order.

use std::collections::{BTreeMap, BTreeSet};

use mapping::{Node, Project};

use super::TargetExport;
use super::schema::SideFormat;
use crate::MfdError;
use crate::float_cast_shape::plain_scope;

pub(super) fn validate(project: &Project, targets: &[TargetExport<'_>]) -> Result<(), MfdError> {
    let affected = float_consumers(project);
    if affected.is_empty() {
        return Ok(());
    }
    for target in targets
        .iter()
        .filter(|target| target.format == SideFormat::Xml && plain_scope(target.root))
    {
        if !target
            .root
            .bindings
            .iter()
            .any(|binding| affected.contains(&binding.node))
        {
            continue;
        }
        let mut counts = BTreeMap::new();
        let mut previous = None;
        for binding in &target.root.bindings {
            let occurrence = counts
                .entry(binding.target_field.as_str())
                .or_insert(0_usize);
            let key = target
                .branches
                .binding_key(
                    &target.ports,
                    None,
                    std::slice::from_ref(&binding.target_field),
                    *occurrence,
                )
                .ok_or_else(|| unsupported("target binding order cannot be proved"))?;
            *occurrence += 1;
            if previous.is_some_and(|previous| previous >= key) {
                return Err(unsupported("target binding order cannot be preserved"));
            }
            previous = Some(key);
        }
    }
    Ok(())
}

fn float_consumers(project: &Project) -> BTreeSet<mapping::NodeId> {
    let mut pending = project
        .graph
        .nodes
        .iter()
        .filter_map(|(&id, node)| {
            matches!(node, Node::Call { function, .. } if function == "coerce_float").then_some(id)
        })
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return BTreeSet::new();
    }
    let mut parents: BTreeMap<mapping::NodeId, Vec<mapping::NodeId>> = BTreeMap::new();
    for (&id, node) in &project.graph.nodes {
        node.visit_inputs(&mut |input| parents.entry(input.node).or_default().push(id));
    }
    let mut affected = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !affected.insert(id) {
            continue;
        }
        if let Some(consumers) = parents.get(&id) {
            pending.extend(consumers);
        }
    }
    affected
}

fn unsupported(message: &str) -> MfdError {
    MfdError::Unsupported(format!("float target conversion export: {message}"))
}
