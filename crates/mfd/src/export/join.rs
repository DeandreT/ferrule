mod aggregate;
mod computed;
mod nested;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::{SchemaKind, SchemaNode};
use mapping::{
    CollectionJoinSource, Graph, IterationOutput, JoinId, JoinKeyLeft, JoinPlan, JoinSource,
    JoinSourceCardinality, Node, NodeId, Project, Scope, ScopeIteration,
};

use super::schema::{KeyAlloc, PortTree, xml_escape};
use super::source::SourceExports;

use crate::MfdError;

use aggregate::{aggregate_contexts, validate_aggregate_consumer};
use computed::ComputedKey;
pub(super) use computed::validate_computed_placement;
use nested::resolved_nested_source_paths;

pub(super) fn validate(project: &Project) -> Result<(), MfdError> {
    let dynamic_sources = project
        .extra_sources
        .iter()
        .filter(|source| source.dynamic_path.is_some())
        .map(|source| source.name.as_str())
        .collect::<BTreeSet<_>>();
    if dynamic_sources.is_empty() {
        return Ok(());
    }

    validate_scope_dynamic_sources(&project.root, &dynamic_sources)?;
    for target in &project.extra_targets {
        validate_scope_dynamic_sources(&target.root, &dynamic_sources)?;
    }
    for node in project.graph.nodes.values() {
        if let Node::JoinAggregate { join, plan, .. } = node {
            validate_plan_dynamic_sources(*join, plan, &dynamic_sources)?;
        }
    }
    Ok(())
}

fn validate_scope_dynamic_sources(
    scope: &Scope,
    dynamic_sources: &BTreeSet<&str>,
) -> Result<(), MfdError> {
    if let Some((join, plan)) = scope.join() {
        validate_plan_dynamic_sources(join, plan, dynamic_sources)?;
    }
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            validate_scope_dynamic_sources(segment, dynamic_sources)?;
        }
    }
    for child in &scope.children {
        validate_scope_dynamic_sources(child, dynamic_sources)?;
    }
    for child in &scope.dynamic_children {
        validate_scope_dynamic_sources(&child.scope, dynamic_sources)?;
    }
    Ok(())
}

fn validate_plan_dynamic_sources(
    join: JoinId,
    plan: &JoinPlan,
    dynamic_sources: &BTreeSet<&str>,
) -> Result<(), MfdError> {
    let Some(source) = plan.collection_sources().find_map(|source| {
        source
            .collection()
            .first()
            .map(String::as_str)
            .filter(|name| dynamic_sources.contains(name))
    }) else {
        return Ok(());
    };
    Err(MfdError::Unsupported(format!(
        "inner join {} references dynamic additional source `{source}`; kind-32 export requires static source components",
        join.get()
    )))
}

#[derive(Default)]
pub(super) struct JoinExports {
    row_outputs: BTreeMap<JoinId, u32>,
    tuple_outputs: BTreeMap<JoinId, u32>,
    structural_rows: BTreeSet<JoinId>,
    plans: BTreeMap<JoinId, JoinPlan>,
    supported: BTreeSet<JoinId>,
    blocked_nodes: BTreeSet<NodeId>,
    computed_inputs: Vec<(JoinId, ComputedInput)>,
}

/// The join input a computed key feeds, connected after the graph pass.
#[derive(Clone, Copy)]
struct ComputedInput {
    value: NodeId,
    port: u32,
    copy_all: bool,
    /// The input and delimiter pins of a sequence-valued key's `tokenize`
    /// component, each with the node that feeds it.
    tokenize: Option<[(NodeId, u32); 2]>,
}

impl JoinExports {
    /// Connects each computed key's exported node to its join input, with
    /// the connection kind the native application requires.
    pub(super) fn connect_computed_keys(
        &self,
        node_out_key: &BTreeMap<NodeId, u32>,
        edges: &mut Vec<(u32, u32)>,
        structural_edges: &mut BTreeSet<(u32, u32)>,
    ) -> Result<(), MfdError> {
        for (join, input) in &self.computed_inputs {
            let from = node_out_key.get(&input.value).copied().ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "inner join {} computed key node {} has no exported output",
                    join.get(),
                    input.value
                ))
            })?;
            edges.push((from, input.port));
            if input.copy_all {
                structural_edges.insert((from, input.port));
            }
            for (node, pin) in input.tokenize.into_iter().flatten() {
                let from = node_out_key.get(&node).copied().ok_or_else(|| {
                    MfdError::Unsupported(format!(
                        "inner join {} computed key tokenize input node {node} has no exported output",
                        join.get()
                    ))
                })?;
                edges.push((from, pin));
            }
        }
        Ok(())
    }

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
    correlation_anchor: Option<Vec<String>>,
    duplicate: bool,
}

struct RenderedJoin {
    xml: String,
    tuple_output: u32,
    row_output: u32,
    structural_row: bool,
    input_edges: Vec<(u32, u32)>,
    structural_input_edges: Vec<(u32, u32)>,
    computed_input: Option<ComputedInput>,
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
    pub(super) structural_edges: &'a mut BTreeSet<(u32, u32)>,
    pub(super) warnings: &'a mut Vec<String>,
}

pub(super) fn render(args: RenderJoinArgs<'_>) -> Result<JoinExports, MfdError> {
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
        structural_edges,
        warnings,
    } = args;
    let mut owners = BTreeMap::new();
    collect_owners(
        &project.root,
        &mut Vec::new(),
        false,
        Some(Vec::new()),
        sources,
        &mut owners,
    );
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
                        correlation_anchor: None,
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
    // Import adds a computed key's null guard itself, so it is never written.
    let mut guard_nodes = BTreeSet::new();
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
        let mut computed_key = None;
        let result = if owner.duplicate {
            Err("the same join id is owned by multiple scopes".to_string())
        } else {
            resolved_source_paths(&owner, sources).and_then(|source_paths| {
                if owner.plan.computed_source().is_some() {
                    let anchor = owner.correlation_anchor.as_deref().unwrap_or_default();
                    computed_key = Some(computed::admit(project, &owner.plan, anchor, sources)?);
                }
                if !owner.aggregate_only && owner.chain.is_empty() && !target_root_iterable {
                    return Err("the target document root is not row/array shaped".to_string());
                }
                if !owner.aggregate_only && target_ports.key_for_abs(&owner.chain).is_none() {
                    return Err(format!(
                        "target scope `{}` has no matching target entry",
                        owner.chain.join("/")
                    ));
                }
                render_one(
                    join,
                    &owner.plan,
                    &source_paths,
                    computed_key.as_ref(),
                    &project.graph,
                    sources,
                    owner
                        .mapped_sequence
                        .then(|| schema_node_at(&project.target, &owner.chain))
                        .flatten(),
                    keys,
                    uid,
                )
            })
        };
        match result {
            Ok(rendered) => {
                components.push_str(&rendered.xml);
                edges.extend(rendered.input_edges);
                structural_edges.extend(rendered.structural_input_edges);
                node_out_key.extend(rendered.node_outputs);
                exports.row_outputs.insert(join, rendered.row_output);
                exports.tuple_outputs.insert(join, rendered.tuple_output);
                if rendered.structural_row {
                    exports.structural_rows.insert(join);
                }
                if let Some(input) = rendered.computed_input {
                    exports.computed_inputs.push((join, input));
                }
                if let Some(key) = computed_key {
                    guard_nodes.extend(key.guard);
                }
                exports.plans.insert(join, owner.plan);
                exports.supported.insert(join);
            }
            // Import lowers a computed key only for the plan it reads back,
            // so a computed join is exported whole or not at all.
            Err(reason) if owner.plan.computed_source().is_some() => {
                return Err(MfdError::Unsupported(format!(
                    "inner join {} with a computed key cannot be exported to .mfd: {reason}",
                    join.get()
                )));
            }
            Err(reason) => warnings.push(format!(
                "inner join {} is not exported: {reason}; its iteration and node connections are skipped",
                join.get()
            )),
        }
    }
    rejected_aggregates.extend(guard_nodes);
    exports.blocked_nodes = blocked_nodes(&project.graph, &exports.plans, rejected_aggregates);
    Ok(exports)
}

fn collect_owners(
    scope: &Scope,
    chain: &mut Vec<String>,
    inside_iteration: bool,
    source_anchor: Option<Vec<String>>,
    sources: &SourceExports<'_>,
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
                        correlation_anchor: inside_iteration
                            .then(|| source_anchor.clone())
                            .flatten(),
                        duplicate: false,
                    },
                );
            }
        }
    }
    let child_anchor = match &scope.iteration {
        ScopeIteration::OwnedSource { .. } | ScopeIteration::OwnedInnerJoin { .. } => None,
        // Optional presence adds no frame; export preflight refuses it before this walk.
        ScopeIteration::None | ScopeIteration::OptionalPresence { .. } => source_anchor,
        ScopeIteration::Source(path) => source_anchor
            .as_deref()
            .map(|anchor| sources.resolve_scope_path(anchor, path).0),
        ScopeIteration::Sequence(_)
        | ScopeIteration::InnerJoin { .. }
        | ScopeIteration::DynamicDocuments { .. }
        | ScopeIteration::Concatenate(_) => None,
    };
    let child_inside_iteration = inside_iteration || scope.iterates();
    for child in &scope.children {
        chain.push(child.target_field.clone());
        collect_owners(
            child,
            chain,
            child_inside_iteration,
            child_anchor.clone(),
            sources,
            owners,
        );
        chain.pop();
    }
}

fn resolved_source_paths(
    owner: &JoinOwner,
    sources: &SourceExports<'_>,
) -> Result<Vec<Vec<String>>, String> {
    let join_sources = owner.plan.collection_sources().collect::<Vec<_>>();
    if !owner.nested {
        return Ok(join_sources
            .iter()
            .map(|source| source.collection().to_vec())
            .collect());
    }
    let anchor = owner
        .correlation_anchor
        .as_deref()
        .filter(|anchor| !anchor.is_empty())
        .ok_or("nested join is not enclosed by one ordinary source iteration")?;
    resolved_nested_source_paths(
        &join_sources,
        owner.plan.computed_source().is_some(),
        anchor,
        sources,
    )
}

/// One join input in plan order.
enum RenderInput<'p> {
    Collection {
        source: &'p CollectionJoinSource,
        path: &'p [String],
    },
    Computed(&'p ComputedKey),
}

impl RenderInput<'_> {
    fn collection(&self) -> Option<(&CollectionJoinSource, &[String])> {
        match self {
            Self::Collection { source, path } => Some((source, path)),
            Self::Computed(_) => None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_one(
    join: JoinId,
    plan: &JoinPlan,
    source_paths: &[Vec<String>],
    computed: Option<&ComputedKey>,
    graph: &Graph,
    source_exports: &SourceExports<'_>,
    mapped_target: Option<&SchemaNode>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
) -> Result<RenderedJoin, String> {
    let mismatch = "join source resolution does not match its input count";
    let mut collection_paths = source_paths.iter();
    let mut inputs = Vec::new();
    for source in plan.sources() {
        inputs.push(match source {
            JoinSource::Collection(source) => RenderInput::Collection {
                source,
                path: collection_paths.next().ok_or(mismatch)?,
            },
            JoinSource::Computed(_) => {
                RenderInput::Computed(computed.ok_or("the computed key was not admitted")?)
            }
        });
    }
    if collection_paths.next().is_some() {
        return Err(mismatch.to_string());
    }
    if inputs.len() < 2 {
        return Err("a join must contain at least two sources".to_string());
    }
    let mut source_indices = BTreeMap::new();
    let mut computed_index = None;
    let mut computed_input = None;
    let mut input_ports = Vec::with_capacity(inputs.len());
    let mut input_edges = Vec::with_capacity(inputs.len());
    let mut structural_input_edges = Vec::with_capacity(inputs.len());
    let mut sequence_components = String::new();
    let mut node_outputs = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let (source, resolved_path) = match input {
            RenderInput::Collection { source, path } => (source, path),
            RenderInput::Computed(key) => {
                // The key's own node is written by the graph pass; its edge
                // is connected once that node has an output port. A
                // sequence-valued key's `tokenize` component is written here,
                // and its item takes that component's output.
                let input_port = keys.next();
                input_ports.push(input_port);
                computed_index = Some(index);
                let tokenize = key.tokenize.map(|tokenize| {
                    let input_pin = keys.next();
                    let delimiter_pin = keys.next();
                    let output = keys.next();
                    super::node::render_sequence_component(
                        "tokenize",
                        "core",
                        &[input_pin, delimiter_pin],
                        output,
                        None,
                        uid,
                        &mut sequence_components,
                    );
                    node_outputs.push((key.value, output));
                    [
                        (tokenize.input, input_pin),
                        (tokenize.delimiter, delimiter_pin),
                    ]
                });
                computed_input = Some(ComputedInput {
                    value: key.value,
                    port: input_port,
                    copy_all: key.copy_all,
                    tokenize,
                });
                continue;
            }
        };
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
        let Some(resolved) = source_exports.join_collection(resolved_path) else {
            return Err(format!(
                "input {index} collection `{}` is not in an exported source schema or has no source component port",
                resolved_path.join("/")
            ));
        };
        let node = resolved.schema;
        let valid = match source.cardinality() {
            JoinSourceCardinality::Repeating => {
                node.repeating && matches!(node.kind, SchemaKind::Group { .. })
            }
            JoinSourceCardinality::Singleton => !node.repeating && node.is_scalar(),
        };
        if !valid {
            return Err(format!(
                "input {index} collection `{}` does not match its declared join cardinality",
                collection.join("/")
            ));
        }
        let source_port = resolved.port;
        let input_port = keys.next();
        input_ports.push(input_port);
        input_edges.push((source_port, input_port));
        // The reference application accepts a join input fed directly by a source
        // port only through a copy-all connection, for a singleton scalar as well
        // as a repeating group.
        structural_input_edges.push((source_port, input_port));
    }

    let mut output_trees = (0..inputs.len())
        .map(|_| EntryTree::default())
        .collect::<Vec<_>>();
    let mut output_ports = BTreeMap::new();
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
        let Some((&source_index, (_, source_path))) = source_indices
            .get(collection)
            .and_then(|index| Some((index, inputs[*index].collection()?)))
        else {
            return Err(format!(
                "join field node {node_id} uses unknown collection `{}`",
                collection.join("/")
            ));
        };
        let mut absolute = source_path.to_vec();
        absolute.extend(path.iter().cloned());
        let Some(field) = source_exports.schema_node_at(&absolute) else {
            return Err(format!(
                "join field node {node_id} path `{}` is not in an exported source schema",
                absolute.join("/")
            ));
        };
        if field.repeating || !field.is_scalar() {
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

    let input_index = |source: &JoinSource| match source {
        JoinSource::Collection(source) => source_indices.get(source.collection()).copied(),
        JoinSource::Computed(_) => computed_index,
    };
    let mut keypaths = BTreeMap::<Vec<String>, (u32, bool)>::new();
    let mut keypairs = Vec::new();
    for (right, conditions) in plan.stages() {
        let Some(right_index) = input_index(right) else {
            return Err("join stage uses an unknown source".to_string());
        };
        for condition in conditions.iter() {
            let left_index = match condition.left() {
                JoinKeyLeft::Collection { collection, .. } => source_indices.get(collection),
                JoinKeyLeft::Computed(_) => computed_index.as_ref(),
            };
            let Some(&left_index) = left_index else {
                return Err("join condition uses an unknown left source".to_string());
            };
            let left_attribute = key_attribute(
                source_exports,
                &inputs[left_index],
                condition.left_path(),
                "left",
            )?;
            let right_attribute = key_attribute(
                source_exports,
                &inputs[right_index],
                condition.right_path(),
                "right",
            )?;
            let left_id = keypath_id(&mut keypaths, condition.left_path(), left_attribute)?;
            let right_id = keypath_id(&mut keypaths, condition.right_path(), right_attribute)?;
            keypairs.push((left_id, left_index, right_id, right_index));
        }
    }

    let structural_index = mapped_target
        .map(|target| matching_structural_source(&inputs, source_exports, target))
        .transpose()?
        .flatten();
    let branch_outputs = (0..inputs.len())
        .map(|index| (Some(index) == structural_index).then(|| keys.next()))
        .collect::<Vec<_>>();
    let tuple_output = keys.next();
    *uid += 1;
    let component_uid = *uid;
    let mut branches = String::new();
    for (index, input) in inputs.iter().enumerate() {
        let name = match input {
            RenderInput::Collection { source, .. } => source
                .collection()
                .last()
                .ok_or("join collection path is empty")?,
            RenderInput::Computed(key) => &key.input_name,
        };
        let input_port = input_ports[index];
        let output_attr = branch_outputs[index]
            .map(|output| format!(" outkey=\"{output}\""))
            .unwrap_or_default();
        let children = output_trees[index].render(9);
        if children.is_empty() {
            let _ = writeln!(
                branches,
                "\t\t\t\t\t\t\t\t<entry name=\"dynamic_tree_node{index}\" ns=\"1\"><entry name=\"{}\" inpkey=\"{input_port}\"{output_attr}/></entry>",
                xml_escape(name)
            );
        } else {
            let _ = write!(
                branches,
                "\t\t\t\t\t\t\t\t<entry name=\"dynamic_tree_node{index}\" ns=\"1\">\n\
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
        "{sequence_components}\
         \t\t\t\t<component name=\"join\" library=\"core\" uid=\"{component_uid}\" kind=\"32\">\n\
         \t\t\t\t\t<view ltx=\"360\" lty=\"20\" rbx=\"560\" rby=\"300\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root><header><namespaces><namespace/><namespace uid=\"http://www.altova.com/mapforce\"/></namespaces></header><entry name=\"document\" ns=\"1\"><entry name=\"tuple\" ns=\"1\" outkey=\"{tuple_output}\">\n\
         {branches}\
         \t\t\t\t\t\t</entry></entry></root>\n\
         \t\t\t\t\t\t<join>\n\
         \t\t\t\t\t\t\t<joinkeys>\n\
         {pairs_xml}\
         \t\t\t\t\t\t\t</joinkeys>\n\
         \t\t\t\t\t\t\t<keypaths><header><namespaces><namespace/></namespaces></header><entry{root_key_attr}><condition/>\n\
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
        structural_input_edges,
        computed_input,
        node_outputs,
    })
}

/// Whether the key path `path` of `input` names an attribute. A computed
/// input is its own key, keyed by the root entry like a singleton.
fn key_attribute(
    sources: &SourceExports<'_>,
    input: &RenderInput<'_>,
    path: &[String],
    side: &str,
) -> Result<bool, String> {
    match input {
        RenderInput::Collection {
            source,
            path: source_path,
        } => validate_key_path(sources, source, source_path, path, side),
        RenderInput::Computed(_) if path.is_empty() => Ok(false),
        RenderInput::Computed(_) => Err(format!("join {side} computed key path is not empty")),
    }
}

fn validate_key_path(
    sources: &SourceExports<'_>,
    source: &CollectionJoinSource,
    source_path: &[String],
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
    let mut absolute = source_path.to_vec();
    absolute.extend(path.iter().cloned());
    let node = sources.schema_node_at(&absolute).ok_or_else(|| {
        format!(
            "join {side} key `{}` is not in an exported source schema",
            absolute.join("/")
        )
    })?;
    if node.repeating || !node.is_scalar() {
        return Err(format!(
            "join {side} key `{}` is not scalar",
            absolute.join("/")
        ));
    }
    Ok(node.attribute)
}

fn matching_structural_source(
    inputs: &[RenderInput<'_>],
    source_exports: &SourceExports<'_>,
    target: &SchemaNode,
) -> Result<Option<usize>, String> {
    let matches = inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let (source, path) = input.collection()?;
            (source.cardinality() == JoinSourceCardinality::Repeating)
                .then(|| source_exports.schema_node_at(path))
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
        && source.default == target.default
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
    node.evaluated_inputs()
}
