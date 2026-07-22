use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::{SchemaKind, SchemaNode};
use mapping::{
    AggregateOp, Graph, IterationOutput, JoinId, JoinPlan, JoinSource, JoinSourceCardinality, Node,
    NodeId, Project, Scope, ScopeConstruction,
};

use super::schema::{KeyAlloc, PortMatch, PortTree, xml_escape};
use super::source::SourceExports;

#[derive(Default)]
pub(super) struct JoinExports {
    row_outputs: BTreeMap<JoinId, u32>,
    tuple_outputs: BTreeMap<JoinId, u32>,
    structural_rows: BTreeSet<JoinId>,
    plans: BTreeMap<JoinId, JoinPlan>,
    supported: BTreeSet<JoinId>,
    blocked_nodes: BTreeSet<NodeId>,
}

impl JoinExports {
    pub(super) fn row_output(&self, join: JoinId) -> Option<u32> {
        self.row_outputs.get(&join).copied()
    }

    pub(super) fn tuple_output(&self, join: JoinId) -> Option<u32> {
        self.tuple_outputs.get(&join).copied()
    }

    pub(super) fn row_is_structural(&self, join: JoinId) -> bool {
        self.structural_rows.contains(&join)
    }

    pub(super) fn supports(&self, join: JoinId) -> bool {
        self.supported.contains(&join)
    }

    pub(super) fn supports_plan(&self, join: JoinId, plan: &JoinPlan) -> bool {
        self.plans
            .get(&join)
            .is_some_and(|candidate| candidate == plan)
    }

    pub(super) fn node_blocked(&self, node: NodeId) -> bool {
        self.blocked_nodes.contains(&node)
    }
}

#[derive(Clone)]
struct JoinOwner {
    chain: Vec<String>,
    plan: JoinPlan,
    aggregates: Vec<NodeId>,
    mapped_sequence: bool,
    aggregate_only: bool,
    nested: bool,
    duplicate: bool,
}

struct RenderedJoin {
    xml: String,
    tuple_output: u32,
    row_output: u32,
    structural_row: bool,
    input_edges: Vec<(u32, u32)>,
    node_outputs: Vec<(NodeId, u32)>,
}

#[derive(Default)]
struct EntryTree {
    children: BTreeMap<String, EntryTree>,
    output: Option<u32>,
    attribute: bool,
}

impl EntryTree {
    fn insert(&mut self, path: &[String], output: u32, attribute: bool) -> Result<(), String> {
        let Some((name, rest)) = path.split_first() else {
            return Err("join field path is empty".to_string());
        };
        let child = self.children.entry(name.clone()).or_default();
        if rest.is_empty() {
            if let Some(existing) = child.output
                && existing != output
            {
                return Err(format!(
                    "join entry path `{}` has multiple output ports",
                    path.join("/")
                ));
            }
            child.output = Some(output);
            child.attribute = attribute;
            return Ok(());
        }
        child.insert(rest, output, attribute)
    }

    fn render(&self, indent: usize) -> String {
        let mut xml = String::new();
        for (name, child) in &self.children {
            let pad = "\t".repeat(indent);
            let type_attr = if child.attribute {
                " type=\"attribute\""
            } else {
                ""
            };
            let output_attr = child
                .output
                .map(|output| format!(" outkey=\"{output}\""))
                .unwrap_or_default();
            if child.children.is_empty() {
                let _ = writeln!(
                    xml,
                    "{pad}<entry name=\"{}\"{type_attr}{output_attr}/>",
                    xml_escape(name)
                );
            } else {
                let _ = writeln!(
                    xml,
                    "{pad}<entry name=\"{}\"{type_attr}{output_attr}>",
                    xml_escape(name)
                );
                xml.push_str(&child.render(indent + 1));
                let _ = writeln!(xml, "{pad}</entry>");
            }
        }
        xml
    }

    fn render_keypaths(&self, indent: usize) -> String {
        let mut xml = String::new();
        for (name, child) in &self.children {
            let pad = "\t".repeat(indent);
            let type_attr = if child.attribute {
                " type=\"attribute\""
            } else {
                ""
            };
            let output_attr = child
                .output
                .map(|output| format!(" outkey=\"{output}\""))
                .unwrap_or_default();
            let _ = writeln!(
                xml,
                "{pad}<entry name=\"{}\"{type_attr}{output_attr}>",
                xml_escape(name)
            );
            let _ = writeln!(xml, "{pad}\t<condition/>");
            xml.push_str(&child.render_keypaths(indent + 1));
            let _ = writeln!(xml, "{pad}</entry>");
        }
        xml
    }
}

pub(super) struct RenderJoinArgs<'a> {
    pub(super) project: &'a Project,
    pub(super) sources: &'a SourceExports<'a>,
    pub(super) target_ports: &'a PortTree,
    pub(super) target_root_iterable: bool,
    pub(super) keys: &'a mut KeyAlloc,
    pub(super) uid: &'a mut u32,
    pub(super) node_out_key: &'a mut BTreeMap<NodeId, u32>,
    pub(super) components: &'a mut String,
    pub(super) edges: &'a mut Vec<(u32, u32)>,
    pub(super) warnings: &'a mut Vec<String>,
}

pub(super) fn render(args: RenderJoinArgs<'_>) -> JoinExports {
    let RenderJoinArgs {
        project,
        sources,
        target_ports,
        target_root_iterable,
        keys,
        uid,
        node_out_key,
        components,
        edges,
        warnings,
    } = args;
    let mut owners = BTreeMap::new();
    collect_owners(&project.root, &mut Vec::new(), false, &mut owners);
    let aggregate_contexts = aggregate_contexts(project);
    let mut rejected_aggregates = BTreeSet::new();
    for (&node_id, node) in &project.graph.nodes {
        let Node::JoinAggregate { join, plan, .. } = node else {
            continue;
        };
        match owners.get_mut(join) {
            Some(owner) if owner.plan != *plan => {
                rejected_aggregates.insert(node_id);
                warnings.push(format!(
                    "join aggregate node {node_id} is not exported: its plan conflicts with join {}; its connections are skipped",
                    join.get()
                ));
            }
            Some(owner) => owner.aggregates.push(node_id),
            None => {
                owners.insert(
                    *join,
                    JoinOwner {
                        chain: Vec::new(),
                        plan: plan.clone(),
                        aggregates: vec![node_id],
                        mapped_sequence: false,
                        aggregate_only: true,
                        nested: false,
                        duplicate: false,
                    },
                );
            }
        }
    }
    let referenced = project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::JoinField { join, .. } | Node::JoinPosition { join } => Some(*join),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    for join in referenced.iter().filter(|join| !owners.contains_key(join)) {
        warnings.push(format!(
            "inner join {} has graph nodes but no owning scope; it was not exported",
            join.get()
        ));
    }

    let mut exports = JoinExports::default();
    for (join, owner) in owners {
        let mut supported_aggregates = Vec::new();
        for node_id in owner.aggregates.iter().copied() {
            let result = validate_aggregate_consumer(
                node_id,
                join,
                &owner.plan,
                &project.graph,
                sources,
                &aggregate_contexts,
            );
            match result {
                Ok(()) => supported_aggregates.push(node_id),
                Err(reason) => {
                    rejected_aggregates.insert(node_id);
                    warnings.push(format!(
                        "join aggregate node {node_id} is not exported: {reason}; its connections are skipped"
                    ));
                }
            }
        }
        if owner.aggregate_only && supported_aggregates.is_empty() {
            continue;
        }
        let result = if owner.duplicate {
            Err("the same join id is owned by multiple scopes".to_string())
        } else if owner.nested {
            Err("nested or correlated join scopes are not exported yet".to_string())
        } else if !owner.aggregate_only && owner.chain.is_empty() && !target_root_iterable {
            Err("the target document root is not row/array shaped".to_string())
        } else if !owner.aggregate_only && target_ports.key_for_abs(&owner.chain).is_none() {
            Err(format!(
                "target scope `{}` has no matching target entry",
                owner.chain.join("/")
            ))
        } else {
            render_one(
                join,
                &owner.plan,
                &project.graph,
                sources,
                owner
                    .mapped_sequence
                    .then(|| schema_node_at(&project.target, &owner.chain))
                    .flatten(),
                keys,
                uid,
            )
        };
        match result {
            Ok(rendered) => {
                components.push_str(&rendered.xml);
                edges.extend(rendered.input_edges);
                node_out_key.extend(rendered.node_outputs);
                exports.row_outputs.insert(join, rendered.row_output);
                exports.tuple_outputs.insert(join, rendered.tuple_output);
                if rendered.structural_row {
                    exports.structural_rows.insert(join);
                }
                exports.plans.insert(join, owner.plan);
                exports.supported.insert(join);
            }
            Err(reason) => warnings.push(format!(
                "inner join {} is not exported: {reason}; its iteration and node connections are skipped",
                join.get()
            )),
        }
    }
    exports.blocked_nodes = blocked_nodes(&project.graph, &exports.plans, rejected_aggregates);
    exports
}

fn collect_owners(
    scope: &Scope,
    chain: &mut Vec<String>,
    inside_iteration: bool,
    owners: &mut BTreeMap<JoinId, JoinOwner>,
) {
    if let Some((join, plan)) = scope.join() {
        match owners.get_mut(&join) {
            Some(owner) => owner.duplicate = true,
            None => {
                owners.insert(
                    join,
                    JoinOwner {
                        chain: chain.clone(),
                        plan: plan.clone(),
                        aggregates: Vec::new(),
                        mapped_sequence: scope.iteration_output == IterationOutput::MappedSequence
                            && scope.bindings.is_empty()
                            && scope.children.is_empty()
                            && scope.dynamic_bindings.is_empty()
                            && scope.dynamic_children.is_empty()
                            && !scope.merge_dynamic_fields,
                        aggregate_only: false,
                        nested: inside_iteration,
                        duplicate: false,
                    },
                );
            }
        }
    }
    let child_inside_iteration = inside_iteration || scope.iterates();
    for child in &scope.children {
        chain.push(child.target_field.clone());
        collect_owners(child, chain, child_inside_iteration, owners);
        chain.pop();
    }
}

#[allow(clippy::too_many_arguments)]
fn render_one(
    join: JoinId,
    plan: &JoinPlan,
    graph: &Graph,
    source_exports: &SourceExports<'_>,
    mapped_target: Option<&SchemaNode>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
) -> Result<RenderedJoin, String> {
    let sources = plan.sources().cloned().collect::<Vec<_>>();
    if sources.len() < 2 {
        return Err("a join must contain at least two sources".to_string());
    }
    let mut source_indices = BTreeMap::new();
    let mut input_ports = Vec::with_capacity(sources.len());
    let mut input_edges = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        let collection = source.collection();
        if collection.is_empty() {
            return Err(format!("input {index} has an empty collection path"));
        }
        if source_indices.insert(collection.to_vec(), index).is_some() {
            return Err(format!(
                "collection `{}` is used more than once",
                collection.join("/")
            ));
        }
        let Some(node) = source_exports.schema_node_at(collection) else {
            return Err(format!(
                "input {index} collection `{}` is not in an exported source schema",
                collection.join("/")
            ));
        };
        let valid = match source.cardinality() {
            JoinSourceCardinality::Repeating => {
                node.repeating && matches!(node.kind, SchemaKind::Group { .. })
            }
            JoinSourceCardinality::Singleton => {
                !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
            }
        };
        if !valid {
            return Err(format!(
                "input {index} collection `{}` does not match its declared join cardinality",
                collection.join("/")
            ));
        }
        let source_port = source_exports.key_for_abs(collection).ok_or_else(|| {
            format!(
                "input {index} collection `{}` has no source component port",
                collection.join("/")
            )
        })?;
        let input_port = keys.next();
        input_ports.push(input_port);
        input_edges.push((source_port, input_port));
    }

    let mut output_trees = (0..sources.len())
        .map(|_| EntryTree::default())
        .collect::<Vec<_>>();
    let mut output_ports = BTreeMap::new();
    let mut node_outputs = Vec::new();
    for (&node_id, node) in &graph.nodes {
        let Node::JoinField {
            join: owner,
            collection,
            path,
        } = node
        else {
            continue;
        };
        if *owner != join {
            continue;
        }
        let Some(&source_index) = source_indices.get(collection) else {
            return Err(format!(
                "join field node {node_id} uses unknown collection `{}`",
                collection.join("/")
            ));
        };
        let mut absolute = collection.clone();
        absolute.extend(path.iter().cloned());
        let Some(field) = source_exports.schema_node_at(&absolute) else {
            return Err(format!(
                "join field node {node_id} path `{}` is not in an exported source schema",
                absolute.join("/")
            ));
        };
        if field.repeating || !matches!(field.kind, SchemaKind::Scalar { .. }) {
            return Err(format!(
                "join field node {node_id} path `{}` is not a scalar",
                absolute.join("/")
            ));
        }
        let output = *output_ports
            .entry((collection.clone(), path.clone()))
            .or_insert_with(|| keys.next());
        output_trees[source_index].insert(path, output, field.attribute)?;
        node_outputs.push((node_id, output));
    }

    let mut keypaths = BTreeMap::<Vec<String>, (u32, bool)>::new();
    let mut keypairs = Vec::new();
    for (right, conditions) in plan.stages() {
        let right_collection = right.collection();
        let Some(&right_index) = source_indices.get(right_collection) else {
            return Err(format!(
                "join stage uses unknown collection `{}`",
                right_collection.join("/")
            ));
        };
        for condition in conditions.iter() {
            let Some(&left_index) = source_indices.get(condition.left_collection()) else {
                return Err(format!(
                    "join condition uses unknown left collection `{}`",
                    condition.left_collection().join("/")
                ));
            };
            let left_source = &sources[left_index];
            let left_attribute =
                validate_key_path(source_exports, left_source, condition.left_path(), "left")?;
            let right_attribute =
                validate_key_path(source_exports, right, condition.right_path(), "right")?;
            let left_id = keypath_id(&mut keypaths, condition.left_path(), left_attribute)?;
            let right_id = keypath_id(&mut keypaths, condition.right_path(), right_attribute)?;
            keypairs.push((left_id, left_index, right_id, right_index));
        }
    }

    let structural_index = mapped_target
        .map(|target| matching_structural_source(&sources, source_exports, target))
        .transpose()?
        .flatten();
    let branch_outputs = (0..sources.len())
        .map(|index| (Some(index) == structural_index).then(|| keys.next()))
        .collect::<Vec<_>>();
    let tuple_output = keys.next();
    *uid += 1;
    let component_uid = *uid;
    let mut branches = String::new();
    for (index, source) in sources.iter().enumerate() {
        let collection = source.collection();
        let name = collection.last().ok_or("join collection path is empty")?;
        let input_port = input_ports[index];
        let output_attr = branch_outputs[index]
            .map(|output| format!(" outkey=\"{output}\""))
            .unwrap_or_default();
        let children = output_trees[index].render(9);
        if children.is_empty() {
            let _ = writeln!(
                branches,
                "\t\t\t\t\t\t\t\t<entry name=\"dynamic_tree_node{index}\"><entry name=\"{}\" inpkey=\"{input_port}\"{output_attr}/></entry>",
                xml_escape(name)
            );
        } else {
            let _ = write!(
                branches,
                "\t\t\t\t\t\t\t\t<entry name=\"dynamic_tree_node{index}\">\n\
                 \t\t\t\t\t\t\t\t\t<entry name=\"{}\" inpkey=\"{input_port}\"{output_attr}>\n\
                 {children}\
                 \t\t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t\t</entry>\n",
                xml_escape(name)
            );
        }
    }
    let mut pairs_xml = String::new();
    for (left_id, left_index, right_id, right_index) in keypairs {
        let _ = writeln!(
            pairs_xml,
            "\t\t\t\t\t\t\t\t<keypair><first-key path-id=\"{left_id}\" input-index=\"{left_index}\"/><second-key path-id=\"{right_id}\" input-index=\"{right_index}\"/></keypair>"
        );
    }
    let mut key_tree = EntryTree::default();
    let mut root_key = None;
    for (path, (id, attribute)) in keypaths {
        if path.is_empty() {
            if attribute {
                return Err("a singleton join key cannot be an attribute".to_string());
            }
            root_key = Some(id);
        } else {
            key_tree.insert(&path, id, attribute)?;
        }
    }
    let key_entries = key_tree.render_keypaths(9);
    let root_key_attr = root_key
        .map(|key| format!(" outkey=\"{key}\""))
        .unwrap_or_default();
    let xml = format!(
        "\t\t\t\t<component name=\"join\" library=\"core\" uid=\"{component_uid}\" kind=\"32\">\n\
         \t\t\t\t\t<view ltx=\"360\" lty=\"20\" rbx=\"560\" rby=\"300\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root><entry name=\"document\"><entry name=\"tuple\" outkey=\"{tuple_output}\">\n\
         {branches}\
         \t\t\t\t\t\t</entry></entry></root>\n\
         \t\t\t\t\t\t<join>\n\
         \t\t\t\t\t\t\t<joinkeys>\n\
         {pairs_xml}\
         \t\t\t\t\t\t\t</joinkeys>\n\
         \t\t\t\t\t\t\t<keypaths><entry{root_key_attr}><condition/>\n\
         {key_entries}\
         \t\t\t\t\t\t\t</entry></keypaths>\n\
         \t\t\t\t\t\t</join>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n"
    );
    Ok(RenderedJoin {
        xml,
        tuple_output,
        row_output: structural_index
            .and_then(|index| branch_outputs[index])
            .unwrap_or(tuple_output),
        structural_row: structural_index.is_some(),
        input_edges,
        node_outputs,
    })
}

fn validate_key_path(
    sources: &SourceExports<'_>,
    source: &JoinSource,
    path: &[String],
    side: &str,
) -> Result<bool, String> {
    if path.is_empty() {
        return if source.cardinality() == JoinSourceCardinality::Singleton {
            Ok(false)
        } else {
            Err(format!("join {side} key path is empty"))
        };
    }
    let mut absolute = source.collection().to_vec();
    absolute.extend(path.iter().cloned());
    let node = sources.schema_node_at(&absolute).ok_or_else(|| {
        format!(
            "join {side} key `{}` is not in an exported source schema",
            absolute.join("/")
        )
    })?;
    if node.repeating || !matches!(node.kind, SchemaKind::Scalar { .. }) {
        return Err(format!(
            "join {side} key `{}` is not scalar",
            absolute.join("/")
        ));
    }
    Ok(node.attribute)
}

fn matching_structural_source(
    sources: &[JoinSource],
    source_exports: &SourceExports<'_>,
    target: &SchemaNode,
) -> Result<Option<usize>, String> {
    let matches = sources
        .iter()
        .enumerate()
        .filter_map(|(index, source)| {
            (source.cardinality() == JoinSourceCardinality::Repeating)
                .then(|| source_exports.schema_node_at(source.collection()))
                .flatten()
                .filter(|candidate| structurally_compatible(candidate, target))
                .map(|_| index)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err("mapped join output matches no joined source group".to_string()),
        [index] => Ok(Some(*index)),
        _ => {
            Err("mapped join output ambiguously matches multiple joined source groups".to_string())
        }
    }
}

fn structurally_compatible(source: &SchemaNode, target: &SchemaNode) -> bool {
    source.name == target.name
        && source.attribute == target.attribute
        && source.nillable == target.nillable
        && source.text == target.text
        && source.fixed == target.fixed
        && !target.repeating
        && source.kind == target.kind
}

fn keypath_id(
    keypaths: &mut BTreeMap<Vec<String>, (u32, bool)>,
    path: &[String],
    attribute: bool,
) -> Result<u32, String> {
    if let Some((id, existing_attribute)) = keypaths.get(path) {
        if *existing_attribute != attribute {
            return Err(format!(
                "join key path `{}` is an attribute in only some inputs",
                path.join("/")
            ));
        }
        return Ok(*id);
    }
    let id = u32::try_from(keypaths.len() + 1)
        .map_err(|_| "join declares too many key paths".to_string())?;
    keypaths.insert(path.to_vec(), (id, attribute));
    Ok(id)
}

fn schema_node_at<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    let mut node = schema;
    for segment in path {
        node = node.child(segment)?;
    }
    Some(node)
}

fn aggregate_contexts(project: &Project) -> BTreeMap<NodeId, bool> {
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
        ScopeConstruction::Constructed
        | ScopeConstruction::CopyCurrentSource
        | ScopeConstruction::XmlMixedContent { .. }
        | ScopeConstruction::PathHierarchy { .. } => {}
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

fn validate_aggregate_consumer(
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
                super::function::aggregate_component_name(*function)
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
                .sources()
                .any(|source| source.collection() == collection) =>
        {
            true
        }
        Node::JoinPosition { join: owner } if *owner == join => true,
        Node::Const { .. } | Node::RuntimeValue { .. } => false,
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
        Node::Const { .. } | Node::RuntimeValue { .. } => {}
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

fn blocked_nodes(
    graph: &Graph,
    plans: &BTreeMap<JoinId, JoinPlan>,
    mut blocked: BTreeSet<NodeId>,
) -> BTreeSet<NodeId> {
    blocked.extend(graph.nodes.iter().filter_map(|(&id, node)| match node {
        Node::JoinField { join, .. } | Node::JoinPosition { join } if !plans.contains_key(join) => {
            Some(id)
        }
        Node::JoinAggregate { join, plan, .. }
            if plans.get(join).is_none_or(|exported| exported != plan) =>
        {
            Some(id)
        }
        _ => None,
    }));
    loop {
        let added = graph.nodes.iter().any(|(&id, node)| {
            if blocked.contains(&id)
                || !node_inputs(node)
                    .into_iter()
                    .any(|input| blocked.contains(&input))
            {
                return false;
            }
            blocked.insert(id)
        });
        if !added {
            return blocked;
        }
    }
}

fn node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. }
        | Node::XmlSerialize { .. } => Vec::new(),
    }
}
