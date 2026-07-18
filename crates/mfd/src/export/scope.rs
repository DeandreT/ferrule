use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use mapping::{Graph, JoinId, Node, NodeId, Scope, ScopeConstruction, SortFilterOrder};

use super::concatenation::TargetBranches;
use super::join::JoinExports;
use super::mapped_sequence::ScopePlans;
use super::position::connect_scope_position_roots;
use super::schema::{KeyAlloc, PortTree};
use super::source::SourceExports;

pub(super) struct ConnectArgs<'a> {
    pub(super) scope: &'a Scope,
    pub(super) sources: &'a SourceExports<'a>,
    pub(super) target_ports: &'a PortTree,
    pub(super) target_root_iterable: bool,
    pub(super) graph: &'a Graph,
    pub(super) node_out_key: &'a BTreeMap<NodeId, u32>,
    pub(super) position_inputs: &'a BTreeMap<NodeId, u32>,
    pub(super) position_contexts: &'a mut BTreeMap<NodeId, Option<u32>>,
    pub(super) keys: &'a mut KeyAlloc,
    pub(super) uid: &'a mut u32,
    pub(super) components: &'a mut String,
    pub(super) edges: &'a mut Vec<(u32, u32)>,
    pub(super) warnings: &'a mut Vec<String>,
    pub(super) structural_edges: &'a mut BTreeSet<(u32, u32)>,
    pub(super) mapped_scope_plans: &'a ScopePlans,
    pub(super) joins: &'a JoinExports,
    pub(super) target_branches: &'a TargetBranches,
}

pub(super) fn connect(args: ConnectArgs<'_>) {
    let ConnectArgs {
        scope,
        sources,
        target_ports,
        target_root_iterable,
        graph,
        node_out_key,
        position_inputs,
        position_contexts,
        keys,
        uid,
        components,
        edges,
        warnings,
        structural_edges,
        mapped_scope_plans,
        joins,
        target_branches,
    } = args;
    collect_scope_edges(
        scope,
        &mut Vec::new(),
        &mut Vec::new(),
        sources,
        target_ports,
        target_root_iterable,
        graph,
        node_out_key,
        position_inputs,
        position_contexts,
        keys,
        uid,
        components,
        edges,
        warnings,
        false,
        structural_edges,
        mapped_scope_plans,
        joins,
        target_branches,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn append_scope_controls(
    scope: &Scope,
    chain: &[String],
    source_collection: Option<&[String]>,
    join: Option<JoinId>,
    graph: &Graph,
    node_out_key: &BTreeMap<NodeId, u32>,
    position_inputs: &BTreeMap<NodeId, u32>,
    position_contexts: &mut BTreeMap<NodeId, Option<u32>>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    components: &mut String,
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
    mut from: u32,
) -> u32 {
    if scope.sort_filter_order == SortFilterOrder::SortThenFilter
        && let Some(sort_by) = scope.sort_by
    {
        connect_scope_position_roots(
            [sort_by],
            source_collection,
            join,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        match node_out_key.get(&sort_by) {
            Some(&key_src) => {
                let in_nodes = keys.next();
                let in_key = keys.next();
                let out_nodes = keys.next();
                let direction = if scope.sort_descending {
                    "descending"
                } else {
                    "ascending"
                };
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"sort\" library=\"core\" uid=\"{uid}\" kind=\"30\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/><datapoint pos=\"1\" key=\"{in_key}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_nodes}\"/></targets>\n\
                     \t\t\t\t\t<data><sort><collation/><key direction=\"{direction}\"/></sort></data>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_nodes));
                edges.push((key_src, in_key));
                from = out_nodes;
            }
            None => warnings.push(format!(
                "scope `{}` sort key references an unexported node; sorting dropped",
                chain.join("/")
            )),
        }
    }
    if let Some(filter) = scope.filter {
        connect_scope_position_roots(
            [filter],
            source_collection,
            join,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        match node_out_key.get(&filter) {
            Some(&bool_key_src) => {
                let in_node = keys.next();
                let in_bool = keys.next();
                let out_true = keys.next();
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"filter\" library=\"core\" uid=\"{uid}\" kind=\"3\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_node}\"/><datapoint pos=\"1\" key=\"{in_bool}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_true}\"/><datapoint/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_node));
                edges.push((bool_key_src, in_bool));
                from = out_true;
            }
            None => warnings.push(format!(
                "scope `{}` filter references an unexported node; filter dropped",
                chain.join("/")
            )),
        }
    }
    if scope.sort_filter_order == SortFilterOrder::FilterThenSort
        && let Some(sort_by) = scope.sort_by
    {
        connect_scope_position_roots(
            [sort_by],
            source_collection,
            join,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        match node_out_key.get(&sort_by) {
            Some(&key_src) => {
                let in_nodes = keys.next();
                let in_key = keys.next();
                let out_nodes = keys.next();
                let direction = if scope.sort_descending {
                    "descending"
                } else {
                    "ascending"
                };
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"sort\" library=\"core\" uid=\"{uid}\" kind=\"30\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/><datapoint pos=\"1\" key=\"{in_key}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_nodes}\"/></targets>\n\
                     \t\t\t\t\t<data><sort><collation/><key direction=\"{direction}\"/></sort></data>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_nodes));
                edges.push((key_src, in_key));
                from = out_nodes;
            }
            None => warnings.push(format!(
                "scope `{}` sort key references an unexported node; sorting dropped",
                chain.join("/")
            )),
        }
    }
    if let Some(group_by) = scope.group_by {
        connect_scope_position_roots(
            [group_by],
            source_collection,
            join,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        match node_out_key.get(&group_by) {
            Some(&key_src) => {
                let in_nodes = keys.next();
                let in_key = keys.next();
                let out_groups = keys.next();
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"group-by\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/><datapoint pos=\"1\" key=\"{in_key}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_groups}\"/><datapoint/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_nodes));
                edges.push((key_src, in_key));
                from = out_groups;
            }
            None => warnings.push(format!(
                "scope `{}` group-by key references an unexported node; grouping dropped",
                chain.join("/")
            )),
        }
    }
    if let Some(predicate) = scope.group_starting_with {
        connect_scope_position_roots(
            [predicate],
            source_collection,
            join,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        match node_out_key.get(&predicate) {
            Some(&predicate_src) => {
                let in_nodes = keys.next();
                let in_predicate = keys.next();
                let out_groups = keys.next();
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"group-starting-with\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/><datapoint pos=\"1\" key=\"{in_predicate}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_groups}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_nodes));
                edges.push((predicate_src, in_predicate));
                from = out_groups;
            }
            None => warnings.push(format!(
                "scope `{}` group-starting predicate references an unexported node; grouping dropped",
                chain.join("/")
            )),
        }
    }
    if let Some(block_size) = scope.group_into_blocks {
        connect_scope_position_roots(
            [block_size],
            source_collection,
            join,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        match node_out_key.get(&block_size) {
            Some(&size_src) => {
                let in_nodes = keys.next();
                let in_size = keys.next();
                let out_groups = keys.next();
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"group-into-blocks\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/><datapoint pos=\"1\" key=\"{in_size}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_groups}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_nodes));
                edges.push((size_src, in_size));
                from = out_groups;
            }
            None => warnings.push(format!(
                "scope `{}` group block size references an unexported node; grouping dropped",
                chain.join("/")
            )),
        }
    }
    if let Some(take) = scope.take {
        connect_scope_position_roots(
            [take],
            source_collection,
            join,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        if scope.iteration_output == mapping::IterationOutput::First && chain.is_empty() {
            let in_nodes = keys.next();
            let out_nodes = keys.next();
            *uid += 1;
            let _ = write!(
                components,
                "\t\t\t\t<component name=\"first-items\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                 \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/></sources>\n\
                 \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_nodes}\"/></targets>\n\
                 \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                 \t\t\t\t</component>\n"
            );
            edges.push((from, in_nodes));
            from = out_nodes;
        } else {
            match node_out_key.get(&take) {
                Some(&count_src) => {
                    let in_nodes = keys.next();
                    let in_count = keys.next();
                    let out_nodes = keys.next();
                    *uid += 1;
                    let _ = write!(
                        components,
                        "\t\t\t\t<component name=\"first-items\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/><datapoint pos=\"1\" key=\"{in_count}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_nodes}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                    );
                    edges.push((from, in_nodes));
                    edges.push((count_src, in_count));
                    from = out_nodes;
                }
                None => warnings.push(format!(
                    "scope `{}` take count references an unexported node; item limit dropped",
                    chain.join("/")
                )),
            }
        }
    }
    from
}

fn append_concatenation_identity(
    keys: &mut KeyAlloc,
    uid: &mut u32,
    components: &mut String,
    edges: &mut Vec<(u32, u32)>,
    from: u32,
) -> u32 {
    let constant_output = keys.next();
    *uid += 1;
    let _ = write!(
        components,
        "\t\t\t\t<component name=\"constant\" library=\"core\" uid=\"{uid}\" kind=\"2\">\n\
         \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{constant_output}\"/></targets>\n\
         \t\t\t\t\t<data><constant value=\"true\" datatype=\"boolean\"/></data>\n\
         \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
         \t\t\t\t</component>\n"
    );

    let nodes_input = keys.next();
    let predicate_input = keys.next();
    let true_output = keys.next();
    *uid += 1;
    let _ = write!(
        components,
        "\t\t\t\t<component name=\"filter\" library=\"core\" uid=\"{uid}\" kind=\"3\">\n\
         \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{nodes_input}\"/><datapoint pos=\"1\" key=\"{predicate_input}\"/></sources>\n\
         \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{true_output}\"/><datapoint/></targets>\n\
         \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
         \t\t\t\t</component>\n"
    );
    edges.push((from, nodes_input));
    edges.push((constant_output, predicate_input));
    true_output
}

fn descendant_binding_roots(scope: &Scope, roots: &mut Vec<NodeId>) {
    roots.extend(scope.bindings.iter().map(|binding| binding.node));
    for child in &scope.children {
        descendant_binding_roots(child, roots);
    }
}

#[allow(clippy::too_many_arguments)]
fn connect_binding_positions(
    scope: &Scope,
    source_collection: Option<&[String]>,
    join: Option<JoinId>,
    from: u32,
    graph: &Graph,
    position_inputs: &BTreeMap<NodeId, u32>,
    position_contexts: &mut BTreeMap<NodeId, Option<u32>>,
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
) {
    connect_scope_position_roots(
        scope.bindings.iter().map(|binding| binding.node),
        source_collection,
        join,
        true,
        from,
        graph,
        position_inputs,
        position_contexts,
        edges,
        warnings,
    );

    // Named collections can be outer-owned; empty paths stay nested-owned.
    let mut descendant_roots = Vec::new();
    for child in &scope.children {
        descendant_binding_roots(child, &mut descendant_roots);
    }
    connect_scope_position_roots(
        descendant_roots,
        source_collection,
        join,
        false,
        from,
        graph,
        position_inputs,
        position_contexts,
        edges,
        warnings,
    );
}

#[allow(clippy::too_many_arguments)]
fn collect_scope_edges(
    scope: &Scope,
    chain: &mut Vec<String>,
    anchor: &mut Vec<String>,
    sources: &SourceExports<'_>,
    target_ports: &PortTree,
    target_root_iterable: bool,
    graph: &Graph,
    node_out_key: &BTreeMap<NodeId, u32>,
    position_inputs: &BTreeMap<NodeId, u32>,
    position_contexts: &mut BTreeMap<NodeId, Option<u32>>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    components: &mut String,
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
    suppress_mapped_bindings: bool,
    structural_edges: &mut BTreeSet<(u32, u32)>,
    mapped_scope_plans: &ScopePlans,
    joins: &JoinExports,
    target_branches: &TargetBranches,
    target_branch: Option<(&[String], usize)>,
) {
    if let Some(segments) = scope.concatenated() {
        let branch_root = chain.clone();
        let parent_anchor = anchor.clone();
        for (index, segment) in segments.iter().enumerate() {
            anchor.clone_from(&parent_anchor);
            collect_scope_edges(
                segment,
                chain,
                anchor,
                sources,
                target_ports,
                target_root_iterable,
                graph,
                node_out_key,
                position_inputs,
                position_contexts,
                keys,
                uid,
                components,
                edges,
                warnings,
                suppress_mapped_bindings,
                structural_edges,
                mapped_scope_plans,
                joins,
                target_branches,
                Some((&branch_root, index)),
            );
        }
        anchor.clone_from(&parent_anchor);
        return;
    }
    let mapped_plan = mapped_scope_plans.get(chain);
    let suppress_mapped_bindings =
        suppress_mapped_bindings || mapped_plan.is_some_and(|plan| plan.copy_all());
    let anchor_len = anchor.len();
    if scope.construction == ScopeConstruction::CopyCurrentSource && scope.source().is_none() {
        match (
            sources.key_for_abs(anchor),
            target_key(target_ports, target_branches, target_branch, chain),
        ) {
            (Some(from), Some(to)) => {
                edges.push((from, to));
                structural_edges.insert((from, to));
            }
            _ => warnings.push(format!(
                "scope `{}` cannot connect its current source group to the target; copy skipped",
                chain.join("/")
            )),
        }
    } else if let Some((join, _)) = scope.join() {
        if let (Some(from), Some(to)) = (
            joins.row_output(join),
            target_key(target_ports, target_branches, target_branch, chain),
        ) {
            let from = append_scope_controls(
                scope,
                chain,
                None,
                Some(join),
                graph,
                node_out_key,
                position_inputs,
                position_contexts,
                keys,
                uid,
                components,
                edges,
                warnings,
                from,
            );
            connect_binding_positions(
                scope,
                None,
                Some(join),
                from,
                graph,
                position_inputs,
                position_contexts,
                edges,
                warnings,
            );
            edges.push((from, to));
        }
    } else if let Some(sequence) = scope.sequence() {
        if chain.is_empty() && !target_root_iterable {
            warnings.push(
                "the root scope generates rows but the target document is not row/array \
                 shaped in MapForce terms; the iteration wire is skipped"
                    .to_string(),
            );
        } else {
            match (
                node_out_key.get(&sequence.item()),
                target_key(target_ports, target_branches, target_branch, chain),
            ) {
                (Some(&from), Some(to)) => {
                    let from = if target_branch.is_some() {
                        append_concatenation_identity(keys, uid, components, edges, from)
                    } else {
                        from
                    };
                    let from = append_scope_controls(
                        scope,
                        chain,
                        None,
                        None,
                        graph,
                        node_out_key,
                        position_inputs,
                        position_contexts,
                        keys,
                        uid,
                        components,
                        edges,
                        warnings,
                        from,
                    );
                    connect_binding_positions(
                        scope,
                        None,
                        None,
                        from,
                        graph,
                        position_inputs,
                        position_contexts,
                        edges,
                        warnings,
                    );
                    edges.push((from, to));
                }
                (None, _) => warnings.push(format!(
                    "scope `{}` sequence item references an unexported node; skipped",
                    chain.join("/")
                )),
                (_, None) => warnings.push(format!(
                    "scope `{}` has no matching target entry; sequence skipped",
                    chain.join("/")
                )),
            }
        }
    } else if scope.source().is_some()
        && chain.is_empty()
        && !target_root_iterable
        && scope.iteration_output != mapping::IterationOutput::First
    {
        warnings.push(
            "the root scope iterates rows but the target document is not row/array \
             shaped in MapForce terms; the iteration wire is skipped"
                .to_string(),
        );
    } else if let Some(source) = scope.source() {
        let abs = mapped_plan
            .and_then(|plan| plan.source().map(|(collection, _)| collection.to_vec()))
            .unwrap_or_else(|| {
                let mut abs = anchor.clone();
                abs.extend(source.iter().cloned());
                abs
            });
        let structural_source = mapped_plan
            .and_then(|plan| plan.source().map(|(_, group)| group))
            .unwrap_or(&abs);
        match (
            sources.key_for_abs(structural_source),
            target_key(target_ports, target_branches, target_branch, chain),
        ) {
            (Some(from), Some(to)) => {
                let from = if target_branch.is_some() {
                    append_concatenation_identity(keys, uid, components, edges, from)
                } else {
                    from
                };
                let from = append_scope_controls(
                    scope,
                    chain,
                    Some(&abs),
                    None,
                    graph,
                    node_out_key,
                    position_inputs,
                    position_contexts,
                    keys,
                    uid,
                    components,
                    edges,
                    warnings,
                    from,
                );
                connect_binding_positions(
                    scope,
                    Some(&abs),
                    None,
                    from,
                    graph,
                    position_inputs,
                    position_contexts,
                    edges,
                    warnings,
                );
                edges.push((from, to));
                if mapped_plan.is_some_and(|plan| plan.copy_all())
                    || scope.construction == ScopeConstruction::CopyCurrentSource
                {
                    structural_edges.insert((from, to));
                }
                *anchor = abs;
            }
            _ => warnings.push(format!(
                "scope `{}` iterates `{}` which maps to no schema entry; skipped",
                chain.join("/"),
                source.join("/")
            )),
        }
    }
    for binding in &scope.bindings {
        if suppress_mapped_bindings {
            continue;
        }
        let mut leaf = chain.clone();
        leaf.push(binding.target_field.clone());
        match (
            node_out_key.get(&binding.node),
            target_key(target_ports, target_branches, target_branch, &leaf),
        ) {
            (Some(&from), Some(to)) => edges.push((from, to)),
            (None, _) if joins.node_blocked(binding.node) => {}
            (None, _)
                if matches!(
                    graph.nodes.get(&binding.node),
                    Some(Node::JoinField { .. } | Node::JoinPosition { .. })
                ) => {}
            (None, _) => warnings.push(format!(
                "binding `{}` references an unexported node; skipped",
                leaf.join("/")
            )),
            (_, None) => warnings.push(format!(
                "binding `{}` matches no target entry; skipped",
                leaf.join("/")
            )),
        }
    }
    for child in &scope.children {
        chain.push(child.target_field.clone());
        collect_scope_edges(
            child,
            chain,
            anchor,
            sources,
            target_ports,
            target_root_iterable,
            graph,
            node_out_key,
            position_inputs,
            position_contexts,
            keys,
            uid,
            components,
            edges,
            warnings,
            suppress_mapped_bindings,
            structural_edges,
            mapped_scope_plans,
            joins,
            target_branches,
            target_branch,
        );
        chain.pop();
    }
    anchor.truncate(anchor_len);
}

fn target_key(
    ports: &PortTree,
    branches: &TargetBranches,
    branch: Option<(&[String], usize)>,
    path: &[String],
) -> Option<u32> {
    branch
        .and_then(|(root, index)| branches.key_for(ports, root, index, path))
        .or_else(|| ports.key_for_abs(path))
}
