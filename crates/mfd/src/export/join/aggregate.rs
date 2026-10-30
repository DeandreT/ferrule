//! Joined aggregates: the consumers a canonical kind-32 export can write,
//! and the iteration context each one is evaluated in.

use std::collections::{BTreeMap, BTreeSet};

use mapping::{
    AggregateOp, Graph, JoinId, JoinPlan, Node, NodeId, Project, Scope, ScopeConstruction,
};

use super::super::schema::PortMatch;
use super::super::source::SourceExports;
use super::node_inputs;

pub(super) fn aggregate_contexts(project: &Project) -> BTreeMap<NodeId, bool> {
    let mut contexts = BTreeMap::new();
    collect_scope_aggregate_contexts(&project.root, &project.graph, false, &mut contexts);
    for target in &project.extra_targets {
        collect_scope_aggregate_contexts(&target.root, &project.graph, false, &mut contexts);
    }
    contexts
}

fn collect_scope_aggregate_contexts(
    scope: &Scope,
    graph: &Graph,
    inside_iteration: bool,
    contexts: &mut BTreeMap<NodeId, bool>,
) {
    let nested = inside_iteration || scope.iterates();
    let mut roots = [
        scope.filter,
        scope.post_group_filter,
        scope.group_by,
        scope.group_starting_with,
        scope.group_adjacent_by,
        scope.group_ending_with,
        scope.group_into_blocks,
        scope.sort_by,
        scope.output_path(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    roots.extend(
        scope
            .windows
            .iter()
            .copied()
            .flat_map(|window| window.nodes()),
    );
    roots.extend(scope.sort_then_by.iter().map(|key| key.node));
    roots.extend(scope.bindings.iter().map(|binding| binding.node));
    roots.extend(
        scope
            .dynamic_bindings
            .iter()
            .flat_map(|binding| [binding.key, binding.value]),
    );
    match &scope.construction {
        ScopeConstruction::Scalar { value } => roots.push(*value),
        ScopeConstruction::RecursiveFilter { plan } => roots.push(plan.predicate()),
        ScopeConstruction::AdjacencyTree { plan } => roots.extend(plan.root()),
        ScopeConstruction::OwnedAdjacencyTree { plan } => roots.extend(plan.root()),
        ScopeConstruction::Constructed
        | ScopeConstruction::CollectDocuments
        | ScopeConstruction::CopyCurrentSource
        | ScopeConstruction::CopyCurrentRows
        | ScopeConstruction::XmlMixedContent { .. }
        | ScopeConstruction::PathHierarchy { .. }
        | ScopeConstruction::OwnedPathHierarchy { .. } => {}
    }
    if let Some(sequence) = scope.sequence() {
        roots.extend(sequence.inputs());
    }
    for root in roots {
        mark_aggregate_context(root, nested, graph, contexts, &mut BTreeSet::new());
    }
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            collect_scope_aggregate_contexts(segment, graph, inside_iteration, contexts);
        }
    }
    for child in &scope.children {
        collect_scope_aggregate_contexts(child, graph, nested, contexts);
    }
    for child in &scope.dynamic_children {
        mark_aggregate_context(child.key, nested, graph, contexts, &mut BTreeSet::new());
        collect_scope_aggregate_contexts(&child.scope, graph, nested, contexts);
    }
}

fn mark_aggregate_context(
    node_id: NodeId,
    nested: bool,
    graph: &Graph,
    contexts: &mut BTreeMap<NodeId, bool>,
    visited: &mut BTreeSet<NodeId>,
) {
    if !visited.insert(node_id) {
        return;
    }
    let Some(node) = graph.nodes.get(&node_id) else {
        return;
    };
    if matches!(node, Node::JoinAggregate { .. }) {
        contexts
            .entry(node_id)
            .and_modify(|existing| *existing |= nested)
            .or_insert(nested);
    }
    for dependency in node_inputs(node) {
        mark_aggregate_context(dependency, nested, graph, contexts, visited);
    }
}

pub(super) fn validate_aggregate_consumer(
    node_id: NodeId,
    join: JoinId,
    plan: &JoinPlan,
    graph: &Graph,
    sources: &SourceExports<'_>,
    contexts: &BTreeMap<NodeId, bool>,
) -> Result<(), String> {
    match contexts.get(&node_id) {
        Some(false) => {}
        Some(true) => {
            return Err(
                "it is evaluated inside an iterating target context; nested or correlated joined reductions are not representable"
                    .to_string(),
            );
        }
        None => return Err("it is not consumed by an exported target".to_string()),
    }
    let Some(Node::JoinAggregate {
        function,
        join: owner,
        plan: node_plan,
        expression,
        arg,
    }) = graph.nodes.get(&node_id)
    else {
        return Err("the graph node is not a joined aggregate".to_string());
    };
    if *owner != join || node_plan != plan {
        return Err("its join ownership does not match the exported plan".to_string());
    }
    match expression {
        None if *function != AggregateOp::Count => {
            return Err("only count can reduce a raw joined tuple sequence".to_string());
        }
        None => {}
        Some(expression) => {
            let owns_join = validate_join_expression(
                *expression,
                join,
                plan,
                graph,
                &mut BTreeMap::new(),
                &mut BTreeSet::new(),
            )?;
            if !owns_join {
                return Err(
                    "its computed sequence has no field or position owned by the joined tuple"
                        .to_string(),
                );
            }
        }
    }
    if let Some(arg) = arg {
        if !matches!(function, AggregateOp::Join | AggregateOp::ItemAt) {
            return Err(format!(
                "{} does not have a scalar argument in the canonical aggregate shape",
                super::super::function::aggregate_component_name(*function)
            ));
        }
        validate_parent_expression(
            *arg,
            graph,
            sources,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        )?;
    }
    Ok(())
}

fn validate_join_expression(
    node_id: NodeId,
    join: JoinId,
    plan: &JoinPlan,
    graph: &Graph,
    memo: &mut BTreeMap<NodeId, bool>,
    active: &mut BTreeSet<NodeId>,
) -> Result<bool, String> {
    if let Some(owns_join) = memo.get(&node_id) {
        return Ok(*owns_join);
    }
    if !active.insert(node_id) {
        return Err(format!(
            "computed sequence contains a cycle at node {node_id}"
        ));
    }
    let node = graph
        .nodes
        .get(&node_id)
        .ok_or_else(|| format!("computed sequence references missing node {node_id}"))?;
    let owns_join = match node {
        Node::JoinField {
            join: owner,
            collection,
            ..
        } if *owner == join
            && plan
                .collection_sources()
                .any(|source| source.collection() == collection) =>
        {
            true
        }
        Node::JoinPosition { join: owner } if *owner == join => true,
        Node::Unconnected | Node::Const { .. } | Node::RuntimeValue { .. } => false,
        Node::Call { .. } | Node::If { .. } | Node::ValueMap { .. } => {
            let mut owns_join = false;
            for dependency in node_inputs(node) {
                owns_join |= validate_join_expression(dependency, join, plan, graph, memo, active)?;
            }
            owns_join
        }
        Node::JoinField { .. } | Node::JoinPosition { .. } => {
            return Err(format!(
                "computed sequence node {node_id} belongs to a different join or collection"
            ));
        }
        _ => {
            return Err(format!(
                "computed sequence node {node_id} uses a non-scalar or external context"
            ));
        }
    };
    active.remove(&node_id);
    memo.insert(node_id, owns_join);
    Ok(owns_join)
}

fn validate_parent_expression(
    node_id: NodeId,
    graph: &Graph,
    sources: &SourceExports<'_>,
    visited: &mut BTreeSet<NodeId>,
    active: &mut BTreeSet<NodeId>,
) -> Result<(), String> {
    if visited.contains(&node_id) {
        return Ok(());
    }
    if !active.insert(node_id) {
        return Err(format!(
            "scalar argument contains a cycle at node {node_id}"
        ));
    }
    let node = graph
        .nodes
        .get(&node_id)
        .ok_or_else(|| format!("scalar argument references missing node {node_id}"))?;
    match node {
        Node::SourceField { path, frame } => {
            let mut absolute = frame.clone().unwrap_or_default();
            absolute.extend(path.iter().cloned());
            match sources.match_field(&absolute, frame.is_some()) {
                PortMatch::Unique(_) => {}
                PortMatch::Missing => {
                    return Err(format!(
                        "scalar argument source `{}` has no exported port",
                        absolute.join("/")
                    ));
                }
                PortMatch::Ambiguous => {
                    return Err(format!(
                        "scalar argument source `{}` is ambiguous without an explicit frame",
                        absolute.join("/")
                    ));
                }
            }
        }
        Node::Unconnected | Node::Const { .. } | Node::RuntimeValue { .. } => {}
        Node::Call { .. } | Node::If { .. } | Node::ValueMap { .. } => {
            for dependency in node_inputs(node) {
                validate_parent_expression(dependency, graph, sources, visited, active)?;
            }
        }
        Node::JoinField { .. } | Node::JoinPosition { .. } | Node::JoinAggregate { .. } => {
            return Err(format!(
                "scalar argument node {node_id} depends on a joined tuple"
            ));
        }
        _ => {
            return Err(format!(
                "scalar argument node {node_id} is not a canonical parent-context scalar"
            ));
        }
    }
    active.remove(&node_id);
    visited.insert(node_id);
    Ok(())
}
