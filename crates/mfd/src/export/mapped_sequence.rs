use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::{SchemaKind, SchemaNode, Value, XML_TYPE_FIELD};
use mapping::{Graph, IterationOutput, Node, Scope};

use crate::MfdError;

use super::schema::{KeyAlloc, SideFormat};
use super::source::SourceExports;

#[derive(PartialEq, Eq)]
pub(super) struct ScopePlan {
    source: Option<SourcePlan>,
    explicit_text_port: bool,
}

#[derive(PartialEq, Eq)]
struct SourcePlan {
    collection: Vec<String>,
    group: Vec<String>,
    copy_all: bool,
    alternative: Option<String>,
    absorbed_filter: Option<mapping::NodeId>,
    absorbed_marker: Option<mapping::NodeId>,
}

impl ScopePlan {
    pub(super) fn source(&self) -> Option<(&[String], &[String])> {
        self.source
            .as_ref()
            .map(|source| (source.collection.as_slice(), source.group.as_slice()))
    }

    pub(super) fn copy_all(&self) -> bool {
        self.source.as_ref().is_some_and(|source| source.copy_all)
    }

    pub(super) fn alternative(&self) -> Option<&str> {
        self.source
            .as_ref()
            .and_then(|source| source.alternative.as_deref())
    }

    pub(super) fn absorbed_filter(&self) -> Option<mapping::NodeId> {
        self.source
            .as_ref()
            .and_then(|source| source.absorbed_filter)
    }
}

#[derive(Default)]
pub(super) struct ScopePlans(BTreeMap<(Vec<String>, Option<usize>), ScopePlan>);

impl ScopePlans {
    pub(super) fn get(&self, path: &[String], branch: Option<usize>) -> Option<&ScopePlan> {
        self.0.get(&(path.to_vec(), branch))
    }

    pub(super) fn explicit_text_ports(&self) -> BTreeSet<Vec<String>> {
        self.0
            .iter()
            .filter(|(_, plan)| plan.explicit_text_port)
            .map(|((path, _), _)| {
                let mut text = path.clone();
                text.push(ir::XML_TEXT_FIELD.to_string());
                text
            })
            .collect()
    }

    pub(super) fn absorbed_nodes(&self) -> BTreeSet<mapping::NodeId> {
        self.0
            .values()
            .filter_map(|plan| plan.source.as_ref()?.absorbed_marker)
            .collect()
    }
}

pub(super) fn preflight_mapped_sequences(
    graph: &Graph,
    sources: &SourceExports<'_>,
    target: &SchemaNode,
    root: &Scope,
    target_format: SideFormat,
) -> Result<ScopePlans, MfdError> {
    if scope_has_dynamic_mapping(root) {
        return Err(MfdError::Unsupported(
            "computed JSON property mappings do not yet have a lossless MapForce export"
                .to_string(),
        ));
    }
    if !first_outputs_are_exportable(root, graph, target, target_format, &mut Vec::new()) {
        return Err(MfdError::Unsupported(
            "first-item scope output does not yet have a lossless MapForce export".to_string(),
        ));
    }
    if scope_has_output(root, IterationOutput::MappedSequence)
        && !matches!(target_format, SideFormat::Xml | SideFormat::Xbrl)
    {
        return Err(MfdError::Unsupported(
            "mapped-sequence output is exportable only for XML targets".to_string(),
        ));
    }

    let mut plans = ScopePlans::default();
    if !collect_scope_plans(
        root,
        graph,
        sources,
        target,
        &mut Vec::new(),
        &[],
        None,
        &mut plans,
    ) {
        return Err(MfdError::Unsupported(
            "mapped XML group sequences contain bindings or nested scopes that do not have a lossless MapForce structural-wire export".to_string(),
        ));
    }
    Ok(plans)
}

pub(super) fn render_edge_metadata(
    structural_edges: &BTreeSet<(u32, u32)>,
    keys: &mut KeyAlloc,
) -> (BTreeMap<(u32, u32), u32>, String) {
    let edge_keys = structural_edges
        .iter()
        .map(|edge| (*edge, keys.next()))
        .collect::<BTreeMap<_, _>>();
    let mut xml = String::from("\t\t\t\t<edges>\n");
    for edge_key in edge_keys.values() {
        let _ = writeln!(
            xml,
            "\t\t\t\t\t<edge edgekey=\"{edge_key}\"><data><dataconnection type=\"2\"/></data></edge>"
        );
    }
    xml.push_str("\t\t\t\t</edges>\n");
    (edge_keys, xml)
}

fn scope_has_dynamic_mapping(scope: &Scope) -> bool {
    scope.merge_dynamic_fields
        || !scope.dynamic_bindings.is_empty()
        || !scope.dynamic_children.is_empty()
        || scope.children.iter().any(scope_has_dynamic_mapping)
}

fn scope_has_output(scope: &Scope, output: IterationOutput) -> bool {
    scope.iteration_output == output
        || scope.concatenated().is_some_and(|segments| {
            segments
                .iter()
                .any(|segment| scope_has_output(segment, output))
        })
        || scope
            .children
            .iter()
            .any(|child| scope_has_output(child, output))
}

#[allow(clippy::too_many_arguments)]
fn collect_scope_plans(
    scope: &Scope,
    graph: &Graph,
    sources: &SourceExports<'_>,
    target_schema: &SchemaNode,
    chain: &mut Vec<String>,
    anchor: &[String],
    branch: Option<usize>,
    plans: &mut ScopePlans,
) -> bool {
    if let Some(segments) = scope.concatenated() {
        return segments.iter().enumerate().all(|(index, segment)| {
            collect_scope_plans(
                segment,
                graph,
                sources,
                target_schema,
                chain,
                anchor,
                Some(index),
                plans,
            )
        });
    }
    let scope_anchor = scope.source().map_or_else(
        || anchor.to_vec(),
        |source| {
            resolve_source_collection(sources, anchor, source).unwrap_or_else(|| {
                let mut unresolved = anchor.to_vec();
                unresolved.extend(source.iter().cloned());
                unresolved
            })
        },
    );
    let repeated_conditioned_branch = branch.is_some()
        && scope.iteration_output == IterationOutput::Repeated
        && super::concatenation::source_type_condition(scope, graph).is_some();
    if scope.iteration_output == IterationOutput::MappedSequence || repeated_conditioned_branch {
        let Some(plan) =
            mapped_scope_plan(scope, graph, sources, target_schema, chain, &scope_anchor)
        else {
            return false;
        };
        if plans.0.insert((chain.clone(), branch), plan).is_some() {
            return false;
        }
    }
    for child in &scope.children {
        chain.push(child.target_field.clone());
        if !collect_scope_plans(
            child,
            graph,
            sources,
            target_schema,
            chain,
            &scope_anchor,
            branch,
            plans,
        ) {
            return false;
        }
        chain.pop();
    }
    true
}

fn mapped_scope_plan(
    scope: &Scope,
    graph: &Graph,
    sources: &SourceExports<'_>,
    target_schema: &SchemaNode,
    target_path: &[String],
    collection: &[String],
) -> Option<ScopePlan> {
    if target_path.is_empty() {
        return None;
    }
    let target_group = schema_node_at(target_schema, target_path)?;
    if !matches!(target_group.kind, SchemaKind::Group { .. }) {
        return None;
    }
    let explicit_text_port = scope
        .bindings
        .iter()
        .any(|binding| binding.target_field == ir::XML_TEXT_FIELD);
    if explicit_text_port
        && !target_group.child(ir::XML_TEXT_FIELD).is_some_and(|text| {
            text.text && !text.repeating && matches!(text.kind, SchemaKind::Scalar { .. })
        })
    {
        return None;
    }

    if scope.sequence().is_some() {
        return Some(ScopePlan {
            source: None,
            explicit_text_port,
        });
    }
    if let Some((_, join)) = scope.join() {
        let constructs_fields = !scope.bindings.is_empty() || !scope.children.is_empty();
        if !constructs_fields {
            let matching_sources = join
                .sources()
                .filter(|source| {
                    source.cardinality() == mapping::JoinSourceCardinality::Repeating
                        && sources
                            .schema_node_at(source.collection())
                            .is_some_and(|source| exact_join_group(source, target_group))
                })
                .count();
            if matching_sources != 1 {
                return None;
            }
        }
        return Some(ScopePlan {
            source: None,
            explicit_text_port,
        });
    }
    scope.source()?;
    let (alternative, absorbed_marker, alternative_group) = if target_group.repeating {
        let (alternative, marker, group) =
            super::concatenation::source_type_condition(scope, graph)?;
        (Some(alternative), Some(marker), Some(group))
    } else {
        (
            super::concatenation::exact_type_condition(scope, graph, target_group),
            super::concatenation::exact_type_marker(scope, graph, target_group),
            super::concatenation::source_type_condition(scope, graph).map(|(_, _, group)| group),
        )
    };

    let source_collection = sources.schema_node_at(collection)?;
    if !matches!(source_collection.kind, SchemaKind::Group { .. }) {
        return None;
    }

    if let Some(plan) = mapped_copy_plan(
        scope,
        graph,
        sources,
        target_group,
        collection,
        explicit_text_port,
        alternative.as_deref(),
        absorbed_marker,
    ) {
        return Some(plan);
    }

    let group = alternative_group.unwrap_or_else(|| collection.to_vec());
    if alternative.as_ref().is_some_and(|alternative| {
        !sources.schema_node_at(&group).is_some_and(|source| {
            source
                .alternatives()
                .iter()
                .any(|candidate| &candidate.name == alternative)
        })
    }) {
        return None;
    }
    let absorbed_filter = alternative.as_ref().and(scope.filter);
    Some(ScopePlan {
        source: Some(SourcePlan {
            collection: collection.to_vec(),
            group,
            copy_all: false,
            alternative,
            absorbed_filter,
            absorbed_marker,
        }),
        explicit_text_port,
    })
}

fn exact_join_group(source: &SchemaNode, target: &SchemaNode) -> bool {
    source.name == target.name
        && source.repeating
        && !target.repeating
        && source.attribute == target.attribute
        && source.nillable == target.nillable
        && source.text == target.text
        && source.fixed == target.fixed
        && source.kind == target.kind
}

#[allow(clippy::too_many_arguments)]
fn mapped_copy_plan(
    scope: &Scope,
    graph: &Graph,
    sources: &SourceExports<'_>,
    target_group: &SchemaNode,
    collection: &[String],
    explicit_text_port: bool,
    alternative: Option<&str>,
    absorbed_marker: Option<mapping::NodeId>,
) -> Option<ScopePlan> {
    let mut bindings = Vec::new();
    collect_mapped_bindings(scope, graph, &mut Vec::new(), &mut bindings)?;
    let first = bindings.first()?;
    let source_group = first.source_group.clone();
    if !bindings
        .iter()
        .all(|binding| binding.source_group == source_group)
        || !source_group.starts_with(collection)
    {
        return None;
    }
    let source_group_node = sources.schema_node_at(&source_group)?;
    if !matches!(source_group_node.kind, SchemaKind::Group { .. }) {
        return None;
    }
    if alternative.is_some_and(|name| {
        !source_group_node
            .alternatives()
            .iter()
            .any(|candidate| candidate.name == name)
    }) {
        return None;
    }

    let binding_paths = bindings
        .into_iter()
        .map(|binding| binding.target_relative)
        .collect::<BTreeSet<_>>();
    let mut compatible = BTreeSet::new();
    collect_matching_scalars(
        source_group_node,
        target_group,
        &mut Vec::new(),
        &mut compatible,
    );
    if !binding_paths.is_subset(&compatible) {
        return None;
    }
    let copy_all = exact_copy_group(source_group_node, target_group) && binding_paths == compatible;
    Some(ScopePlan {
        source: Some(SourcePlan {
            collection: collection.to_vec(),
            group: source_group,
            copy_all,
            alternative: alternative.map(str::to_string),
            absorbed_filter: alternative.and(scope.filter),
            absorbed_marker,
        }),
        explicit_text_port: explicit_text_port && !copy_all,
    })
}

fn exact_copy_group(source: &SchemaNode, target: &SchemaNode) -> bool {
    source.name == target.name
        && source.attribute == target.attribute
        && source.nillable == target.nillable
        && source.text == target.text
        && source.fixed == target.fixed
        && source.value_generation == target.value_generation
        && source.kind == target.kind
}

fn first_outputs_are_exportable(
    scope: &Scope,
    graph: &Graph,
    target: &SchemaNode,
    target_format: SideFormat,
    path: &mut Vec<String>,
) -> bool {
    if scope.iteration_output == IterationOutput::First {
        let mixed_content = matches!(
            scope.construction,
            mapping::ScopeConstruction::XmlMixedContent { .. }
        ) && !path.is_empty()
            && matches!(target_format, SideFormat::Xml | SideFormat::Xbrl)
            && scope.source().is_some()
            && schema_node_at(target, path).is_some_and(|node| {
                !node.repeating && matches!(node.kind, SchemaKind::Group { .. })
            })
            && scope.filter.is_none()
            && scope.post_group_filter.is_none()
            && scope.group_by.is_none()
            && scope.group_starting_with.is_none()
            && scope.group_adjacent_by.is_none()
            && scope.group_ending_with.is_none()
            && scope.group_into_blocks.is_none()
            && scope.sort_by.is_none()
            && scope.windows.is_empty();
        let ordinary_root = path.is_empty()
            && matches!(target_format, SideFormat::Xml | SideFormat::Xbrl)
            && scope.source() == Some(&[])
            && !target.repeating
            && matches!(target.kind, SchemaKind::Group { .. })
            && scope.group_by.is_none()
            && scope.group_starting_with.is_none()
            && scope.group_adjacent_by.is_none()
            && scope.group_ending_with.is_none()
            && scope.group_into_blocks.is_none()
            && matches!(scope.windows.as_slice(), [mapping::SequenceWindow::First { count }] if matches!(
                graph.nodes.get(count),
                Some(Node::Const { value: Value::Int(1) })
            ));
        if !mixed_content && !ordinary_root {
            return false;
        }
    }
    for child in &scope.children {
        path.push(child.target_field.clone());
        if !first_outputs_are_exportable(child, graph, target, target_format, path) {
            return false;
        }
        path.pop();
    }
    true
}

fn resolve_source_collection(
    sources: &SourceExports<'_>,
    anchor: &[String],
    source: &[String],
) -> Option<Vec<String>> {
    if source.is_empty() {
        return Some(anchor.to_vec());
    }

    let mut bases = vec![anchor.len()];
    bases.extend((1..anchor.len()).rev().filter(|&length| {
        sources
            .schema_node_at(&anchor[..length])
            .is_some_and(|node| node.repeating)
    }));
    bases.push(0);
    bases.dedup();

    bases.into_iter().find_map(|length| {
        let mut candidate = anchor[..length].to_vec();
        candidate.extend(source.iter().cloned());
        sources.schema_node_at(&candidate).map(|_| candidate)
    })
}

struct MappedBinding {
    target_relative: Vec<String>,
    source_group: Vec<String>,
}

fn collect_mapped_bindings(
    scope: &Scope,
    graph: &Graph,
    relative: &mut Vec<String>,
    bindings: &mut Vec<MappedBinding>,
) -> Option<()> {
    if scope.source().is_some() && !relative.is_empty()
        || scope.sequence().is_some()
        || scope.filter.is_some() && !relative.is_empty()
        || scope.post_group_filter.is_some()
        || scope.group_by.is_some()
        || scope.group_starting_with.is_some()
        || scope.group_adjacent_by.is_some()
        || scope.group_ending_with.is_some()
        || scope.group_into_blocks.is_some()
        || scope.sort_by.is_some() && !relative.is_empty()
        || !scope.windows.is_empty() && !relative.is_empty()
    {
        return None;
    }
    for binding in &scope.bindings {
        if binding.target_field == XML_TYPE_FIELD {
            continue;
        }
        relative.push(binding.target_field.clone());
        let Node::SourceField { path, frame } = graph.nodes.get(&binding.node)? else {
            relative.pop();
            return None;
        };
        let mut absolute = frame.clone().unwrap_or_default();
        absolute.extend(path.iter().cloned());
        if !absolute.ends_with(relative.as_slice()) {
            relative.pop();
            return None;
        }
        let source_group = absolute[..absolute.len() - relative.len()].to_vec();
        bindings.push(MappedBinding {
            target_relative: relative.clone(),
            source_group,
        });
        relative.pop();
    }
    for child in &scope.children {
        relative.push(child.target_field.clone());
        collect_mapped_bindings(child, graph, relative, bindings)?;
        relative.pop();
    }
    Some(())
}

fn schema_node_at<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    let mut node = schema;
    for segment in path {
        node = node.child(segment)?;
    }
    Some(node)
}

fn collect_matching_scalars(
    source: &SchemaNode,
    target: &SchemaNode,
    path: &mut Vec<String>,
    paths: &mut BTreeSet<Vec<String>>,
) {
    match (&source.kind, &target.kind) {
        (SchemaKind::Scalar { .. }, SchemaKind::Scalar { .. })
            if !source.repeating && !target.repeating =>
        {
            paths.insert(path.clone());
        }
        (
            SchemaKind::Group {
                children: source_children,
                dynamic: source_dynamic,
                ..
            },
            SchemaKind::Group {
                children: target_children,
                dynamic: target_dynamic,
                ..
            },
        ) if source_dynamic.is_none() && target_dynamic.is_none() => {
            for target_child in target_children {
                let Some(source_child) = source_children
                    .iter()
                    .find(|source_child| source_child.name == target_child.name)
                else {
                    continue;
                };
                if source_child.repeating || target_child.repeating {
                    continue;
                }
                path.push(target_child.name.clone());
                collect_matching_scalars(source_child, target_child, path, paths);
                path.pop();
            }
        }
        _ => {}
    }
}
