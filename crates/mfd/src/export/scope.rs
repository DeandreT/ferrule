use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use mapping::{
    Graph, JoinId, Node, NodeId, Scope, ScopeConstruction, SequenceWindow, SortFilterOrder,
};

use super::concatenation::TargetBranches;
use super::join::JoinExports;
use super::mapped_sequence::{ScopePlan, ScopePlans};
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
fn append_sort_control(
    scope: &Scope,
    chain: &[String],
    source_stages: Option<&[(Vec<String>, u32)]>,
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
    from: u32,
) -> u32 {
    let sort_keys = scope.sort_keys().collect::<Vec<_>>();
    connect_scope_position_roots(
        sort_keys.iter().map(|key| key.node),
        source_stages,
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
    let Some(key_sources) = sort_keys
        .iter()
        .map(|key| node_out_key.get(&key.node).copied())
        .collect::<Option<Vec<_>>>()
    else {
        warnings.push(format!(
            "scope `{}` sort key references an unexported node; sorting dropped",
            chain.join("/")
        ));
        return from;
    };

    let in_nodes = keys.next();
    let in_keys = (0..sort_keys.len())
        .map(|_| keys.next())
        .collect::<Vec<_>>();
    let out_nodes = keys.next();
    *uid += 1;
    let _ = write!(
        components,
        "\t\t\t\t<component name=\"sort\" library=\"core\" uid=\"{uid}\" kind=\"30\">\n\
         \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/>"
    );
    for (index, input) in in_keys.iter().enumerate() {
        let position = index + 1;
        let _ = write!(
            components,
            "<datapoint pos=\"{position}\" key=\"{input}\"/>"
        );
    }
    let _ = write!(
        components,
        "</sources>\n\
         \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_nodes}\"/></targets>\n\
         \t\t\t\t\t<data><sort><collation/>"
    );
    for (index, key) in sort_keys.iter().enumerate() {
        let direction = if key.descending {
            "descending"
        } else {
            "ascending"
        };
        if index == 0 {
            let _ = write!(components, "<key direction=\"{direction}\"/>");
        } else {
            let _ = write!(
                components,
                "<key index=\"{index}\" direction=\"{direction}\"/>"
            );
        }
    }
    let _ = write!(
        components,
        "</sort></data>\n\
         \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
         \t\t\t\t</component>\n"
    );
    edges.push((from, in_nodes));
    edges.extend(key_sources.into_iter().zip(in_keys));
    out_nodes
}

#[allow(clippy::too_many_arguments)]
fn append_scope_controls(
    scope: &Scope,
    chain: &[String],
    source_stages: Option<&[(Vec<String>, u32)]>,
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
    absorbed_filter: Option<NodeId>,
) -> u32 {
    if scope.sort_filter_order == SortFilterOrder::SortThenFilter && scope.has_sort() {
        from = append_sort_control(
            scope,
            chain,
            source_stages,
            source_collection,
            join,
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
    }
    if let Some(filter) = scope.filter
        && Some(filter) != absorbed_filter
    {
        connect_scope_position_roots(
            [filter],
            source_stages,
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
    if scope.sort_filter_order == SortFilterOrder::FilterThenSort && scope.has_sort() {
        from = append_sort_control(
            scope,
            chain,
            source_stages,
            source_collection,
            join,
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
    }
    if let Some(group_by) = scope.group_by {
        connect_scope_position_roots(
            [group_by],
            source_stages,
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
            source_stages,
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
            source_stages,
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
    for window in scope.windows.iter().copied() {
        let bounds = window.nodes().collect::<Vec<_>>();
        connect_scope_position_roots(
            bounds.iter().copied(),
            source_stages,
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
        let Some(bound_sources) = bounds
            .iter()
            .map(|bound| node_out_key.get(bound).copied())
            .collect::<Option<Vec<_>>>()
        else {
            warnings.push(format!(
                "scope `{}` sequence window references an unexported bound; window dropped",
                chain.join("/")
            ));
            continue;
        };
        let name = match window {
            SequenceWindow::SkipFirst { .. } => "skip-first-items",
            SequenceWindow::First { .. } => "first-items",
            SequenceWindow::From { .. } => "items-from",
            SequenceWindow::FromTo { .. } => "items-from-to",
            SequenceWindow::Last { .. } => "last-items",
        };
        let in_nodes = keys.next();
        let in_bounds = (0..bound_sources.len())
            .map(|_| keys.next())
            .collect::<Vec<_>>();
        let out_nodes = keys.next();
        *uid += 1;
        let _ = write!(
            components,
            "\t\t\t\t<component name=\"{name}\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
             \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/>"
        );
        for (index, input) in in_bounds.iter().enumerate() {
            let position = index + 1;
            let _ = write!(
                components,
                "<datapoint pos=\"{position}\" key=\"{input}\"/>"
            );
        }
        let _ = write!(
            components,
            "</sources>\n\
             \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_nodes}\"/></targets>\n\
             \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
             \t\t\t\t</component>\n"
        );
        edges.push((from, in_nodes));
        edges.extend(bound_sources.into_iter().zip(in_bounds));
        from = out_nodes;
    }
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
    source_stages: Option<&[(Vec<String>, u32)]>,
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
        source_stages,
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
        source_stages,
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

fn source_position_stages(
    sources: &SourceExports<'_>,
    collection: &[String],
    first_stage_len: usize,
    terminal_from: u32,
) -> Vec<(Vec<String>, u32)> {
    if collection.is_empty() {
        return vec![(Vec::new(), terminal_from)];
    }
    let mut stages = (first_stage_len..=collection.len())
        .filter_map(|len| {
            let path = collection[..len].to_vec();
            sources
                .schema_node_at(&path)
                .is_some_and(|node| node.repeating)
                .then(|| sources.key_for_abs(&path).map(|from| (path, from)))
                .flatten()
        })
        .collect::<Vec<_>>();
    if stages.last().is_none_or(|(path, _)| path != collection) {
        stages.push((collection.to_vec(), terminal_from));
    }
    stages
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
    let mapped_plan = mapped_scope_plans.get(chain, target_branch.map(|(_, index)| index));
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
                None,
            );
            connect_binding_positions(
                scope,
                None,
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
            if joins.row_is_structural(join) {
                structural_edges.insert((from, to));
            }
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
                        None,
                    );
                    connect_binding_positions(
                        scope,
                        None,
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
        if scope.output_path().is_none() {
            warnings.push(
                "the root scope iterates rows but the target document is not row/array \
                 shaped in MapForce terms; the iteration wire is skipped"
                    .to_string(),
            );
        } else if let Some(source) = scope.source() {
            if sources.is_named_extra_path(source) {
                anchor.clone_from(&source.to_vec());
            } else {
                anchor.extend(source.iter().cloned());
            }
        }
    } else if let Some(source) = scope.source() {
        let mapped_source =
            mapped_plan.and_then(|plan| plan.source().map(|(collection, _)| collection.to_vec()));
        let named_extra_path = sources.is_named_extra_path(source);
        let abs = mapped_source.clone().unwrap_or_else(|| {
            if named_extra_path {
                source.to_vec()
            } else {
                let mut abs = anchor.clone();
                abs.extend(source.iter().cloned());
                abs
            }
        });
        let structural_source = mapped_plan
            .and_then(|plan| plan.source().map(|(_, group)| group))
            .unwrap_or(&abs);
        let structural_key = mapped_plan
            .and_then(ScopePlan::alternative)
            .and_then(|alternative| sources.key_for_alternative(structural_source, alternative))
            .or_else(|| sources.key_for_abs(structural_source));
        match (
            structural_key,
            target_key(target_ports, target_branches, target_branch, chain),
        ) {
            (Some(from), Some(to)) => {
                let from = if target_branch.is_some() {
                    append_concatenation_identity(keys, uid, components, edges, from)
                } else {
                    from
                };
                let first_stage_len = if mapped_source.is_some() || named_extra_path {
                    1
                } else if source.is_empty() {
                    abs.len()
                } else {
                    anchor.len() + 1
                };
                let position_stages = source_position_stages(sources, &abs, first_stage_len, from);
                let from = append_scope_controls(
                    scope,
                    chain,
                    Some(&position_stages),
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
                    mapped_plan.and_then(ScopePlan::absorbed_filter),
                );
                connect_binding_positions(
                    scope,
                    Some(&position_stages),
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
    let mut binding_occurrences = BTreeMap::<&str, usize>::new();
    for binding in &scope.bindings {
        if suppress_mapped_bindings {
            continue;
        }
        let mut leaf = chain.clone();
        leaf.push(binding.target_field.clone());
        let occurrence = binding_occurrences
            .entry(&binding.target_field)
            .or_default();
        match (
            node_out_key.get(&binding.node),
            target_branches.binding_key(target_ports, target_branch, &leaf, *occurrence),
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
        *occurrence += 1;
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
