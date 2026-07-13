use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::{SchemaKind, SchemaNode};
use mapping::{Graph, JoinId, JoinPlan, Node, NodeId, Project, Scope};

use super::schema::{KeyAlloc, PortTree, xml_escape};

#[derive(Default)]
pub(super) struct JoinExports {
    row_outputs: BTreeMap<JoinId, u32>,
    supported: BTreeSet<JoinId>,
    blocked_nodes: BTreeSet<NodeId>,
}

impl JoinExports {
    pub(super) fn row_output(&self, join: JoinId) -> Option<u32> {
        self.row_outputs.get(&join).copied()
    }

    pub(super) fn supports(&self, join: JoinId) -> bool {
        self.supported.contains(&join)
    }

    pub(super) fn node_blocked(&self, node: NodeId) -> bool {
        self.blocked_nodes.contains(&node)
    }
}

#[derive(Clone)]
struct JoinOwner {
    chain: Vec<String>,
    plan: JoinPlan,
    nested: bool,
    duplicate: bool,
}

struct RenderedJoin {
    xml: String,
    tuple_output: u32,
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
    pub(super) source_ports: &'a PortTree,
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
        source_ports,
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
    let extra_source_names = project
        .extra_sources
        .iter()
        .map(|source| source.name.as_str())
        .collect::<BTreeSet<_>>();
    for (join, owner) in owners {
        let result = if owner.duplicate {
            Err("the same join id is owned by multiple scopes".to_string())
        } else if owner.nested {
            Err("nested or correlated join scopes are not exported yet".to_string())
        } else if owner.chain.is_empty() && !target_root_iterable {
            Err("the target document root is not row/array shaped".to_string())
        } else if target_ports.key_for_abs(&owner.chain).is_none() {
            Err(format!(
                "target scope `{}` has no matching target entry",
                owner.chain.join("/")
            ))
        } else {
            render_one(
                join,
                &owner.plan,
                &project.source,
                &project.graph,
                source_ports,
                &extra_source_names,
                keys,
                uid,
            )
        };
        match result {
            Ok(rendered) => {
                components.push_str(&rendered.xml);
                edges.extend(rendered.input_edges);
                node_out_key.extend(rendered.node_outputs);
                exports.row_outputs.insert(join, rendered.tuple_output);
                exports.supported.insert(join);
            }
            Err(reason) => warnings.push(format!(
                "inner join {} is not exported: {reason}; its iteration and node connections are skipped",
                join.get()
            )),
        }
    }
    let aggregate_joins = project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::JoinAggregate { join, .. } => Some(*join),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    for join in aggregate_joins {
        warnings.push(format!(
            "aggregate over inner join {} is not exported; its node connections are skipped",
            join.get()
        ));
    }
    exports.blocked_nodes = blocked_nodes(&project.graph, &exports.supported);
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
    schema: &SchemaNode,
    graph: &Graph,
    source_ports: &PortTree,
    extra_source_names: &BTreeSet<&str>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
) -> Result<RenderedJoin, String> {
    let sources = plan
        .sources()
        .map(|source| source.collection().to_vec())
        .collect::<Vec<_>>();
    if sources.len() < 2 {
        return Err("a join must contain at least two sources".to_string());
    }
    let mut source_indices = BTreeMap::new();
    let mut input_ports = Vec::with_capacity(sources.len());
    let mut input_edges = Vec::with_capacity(sources.len());
    for (index, collection) in sources.iter().enumerate() {
        if collection.is_empty() {
            return Err(format!("input {index} has an empty collection path"));
        }
        if source_indices.insert(collection.clone(), index).is_some() {
            return Err(format!(
                "collection `{}` is used more than once",
                collection.join("/")
            ));
        }
        if collection
            .first()
            .is_some_and(|name| extra_source_names.contains(name.as_str()))
        {
            return Err(format!(
                "input {index} collection `{}` belongs to an extra source, which is not exported",
                collection.join("/")
            ));
        }
        let Some(node) = schema_node_at(schema, collection) else {
            return Err(format!(
                "input {index} collection `{}` is not in the primary source schema",
                collection.join("/")
            ));
        };
        if !node.repeating || !matches!(node.kind, SchemaKind::Group { .. }) {
            return Err(format!(
                "input {index} collection `{}` is not a repeating group",
                collection.join("/")
            ));
        }
        let source_port = source_ports.key_for_abs(collection).ok_or_else(|| {
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
        let Some(field) = schema_node_at(schema, &absolute) else {
            return Err(format!(
                "join field node {node_id} path `{}` is not in the primary source schema",
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
            let left_attribute = validate_key_path(
                schema,
                condition.left_collection(),
                condition.left_path(),
                "left",
            )?;
            let right_attribute =
                validate_key_path(schema, right_collection, condition.right_path(), "right")?;
            let left_id = keypath_id(&mut keypaths, condition.left_path(), left_attribute)?;
            let right_id = keypath_id(&mut keypaths, condition.right_path(), right_attribute)?;
            keypairs.push((left_id, left_index, right_id, right_index));
        }
    }

    let tuple_output = keys.next();
    *uid += 1;
    let component_uid = *uid;
    let mut branches = String::new();
    for (index, collection) in sources.iter().enumerate() {
        let name = collection.last().ok_or("join collection path is empty")?;
        let input_port = input_ports[index];
        let children = output_trees[index].render(9);
        if children.is_empty() {
            let _ = writeln!(
                branches,
                "\t\t\t\t\t\t\t\t<entry name=\"dynamic_tree_node{index}\"><entry name=\"{}\" inpkey=\"{input_port}\"/></entry>",
                xml_escape(name)
            );
        } else {
            let _ = write!(
                branches,
                "\t\t\t\t\t\t\t\t<entry name=\"dynamic_tree_node{index}\">\n\
                 \t\t\t\t\t\t\t\t\t<entry name=\"{}\" inpkey=\"{input_port}\">\n\
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
    for (path, (id, attribute)) in keypaths {
        key_tree.insert(&path, id, attribute)?;
    }
    let key_entries = key_tree.render_keypaths(9);
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
         \t\t\t\t\t\t\t<keypaths><entry><condition/>\n\
         {key_entries}\
         \t\t\t\t\t\t\t</entry></keypaths>\n\
         \t\t\t\t\t\t</join>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n"
    );
    Ok(RenderedJoin {
        xml,
        tuple_output,
        input_edges,
        node_outputs,
    })
}

fn validate_key_path(
    schema: &SchemaNode,
    collection: &[String],
    path: &[String],
    side: &str,
) -> Result<bool, String> {
    if path.is_empty() {
        return Err(format!("join {side} key path is empty"));
    }
    let mut absolute = collection.to_vec();
    absolute.extend(path.iter().cloned());
    let node = schema_node_at(schema, &absolute).ok_or_else(|| {
        format!(
            "join {side} key `{}` is not in the primary source schema",
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

fn blocked_nodes(graph: &Graph, supported: &BTreeSet<JoinId>) -> BTreeSet<NodeId> {
    let mut blocked = graph
        .nodes
        .iter()
        .filter_map(|(&id, node)| match node {
            Node::JoinField { join, .. } | Node::JoinPosition { join }
                if !supported.contains(join) =>
            {
                Some(id)
            }
            Node::JoinAggregate { .. } => Some(id),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
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
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::SourceField { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. } => Vec::new(),
    }
}
