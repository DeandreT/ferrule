use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::{SchemaKind, SchemaNode};
use mapping::{Graph, IterationOutput, Node, Project, Scope};

use crate::MfdError;

use super::schema::{KeyAlloc, SideFormat};

pub(super) struct ScopePlan {
    pub(super) source_collection: Vec<String>,
    pub(super) source_group: Vec<String>,
    pub(super) copy_all: bool,
}

pub(super) type ScopePlans = BTreeMap<Vec<String>, ScopePlan>;

pub(super) fn preflight_mapped_sequences(
    project: &Project,
    target_format: SideFormat,
) -> Result<ScopePlans, MfdError> {
    if scope_has_dynamic_mapping(&project.root) {
        return Err(MfdError::Unsupported(
            "computed JSON property mappings do not yet have a lossless MapForce export"
                .to_string(),
        ));
    }
    if scope_has_output(&project.root, IterationOutput::First) {
        return Err(MfdError::Unsupported(
            "first-item scope output does not yet have a lossless MapForce export".to_string(),
        ));
    }
    if scope_has_output(&project.root, IterationOutput::MappedSequence)
        && target_format != SideFormat::Xml
    {
        return Err(MfdError::Unsupported(
            "mapped-sequence output is exportable only for XML targets".to_string(),
        ));
    }

    let mut plans = BTreeMap::new();
    if !collect_scope_plans(
        &project.root,
        &project.graph,
        &project.source,
        &project.target,
        &mut Vec::new(),
        &[],
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
        || scope
            .children
            .iter()
            .any(|child| scope_has_output(child, output))
}

fn collect_scope_plans(
    scope: &Scope,
    graph: &Graph,
    source_schema: &SchemaNode,
    target_schema: &SchemaNode,
    chain: &mut Vec<String>,
    anchor: &[String],
    plans: &mut ScopePlans,
) -> bool {
    let scope_anchor = scope.source().map_or_else(
        || anchor.to_vec(),
        |source| {
            resolve_source_collection(source_schema, anchor, source).unwrap_or_else(|| {
                let mut unresolved = anchor.to_vec();
                unresolved.extend(source.iter().cloned());
                unresolved
            })
        },
    );
    if scope.iteration_output == IterationOutput::MappedSequence {
        let Some(plan) = mapped_scope_plan(
            scope,
            graph,
            source_schema,
            target_schema,
            chain,
            &scope_anchor,
        ) else {
            return false;
        };
        plans.insert(chain.clone(), plan);
    }
    for child in &scope.children {
        chain.push(child.target_field.clone());
        if !collect_scope_plans(
            child,
            graph,
            source_schema,
            target_schema,
            chain,
            &scope_anchor,
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
    source_schema: &SchemaNode,
    target_schema: &SchemaNode,
    target_path: &[String],
    collection: &[String],
) -> Option<ScopePlan> {
    if scope.source().is_none() || scope.sequence().is_some() || target_path.is_empty() {
        return None;
    }
    let target_group = schema_node_at(target_schema, target_path)?;
    if target_group.repeating || !matches!(target_group.kind, SchemaKind::Group { .. }) {
        return None;
    }

    let source_collection = schema_node_at(source_schema, collection)?;
    if !matches!(source_collection.kind, SchemaKind::Group { .. }) {
        return None;
    }

    if let Some(plan) = mapped_copy_plan(scope, graph, source_schema, target_group, collection) {
        return Some(plan);
    }

    if scope
        .bindings
        .iter()
        .any(|binding| binding.target_field == ir::XML_TEXT_FIELD)
    {
        // XML text and its owning group share one MapForce port. An ordinary
        // occurrence wire plus a direct text wire would connect that input twice.
        return None;
    }

    Some(ScopePlan {
        source_collection: collection.to_vec(),
        source_group: collection.to_vec(),
        copy_all: false,
    })
}

fn mapped_copy_plan(
    scope: &Scope,
    graph: &Graph,
    source_schema: &SchemaNode,
    target_group: &SchemaNode,
    collection: &[String],
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
    let source_group_node = schema_node_at(source_schema, &source_group)?;
    if !matches!(source_group_node.kind, SchemaKind::Group { .. }) {
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
    let copy_all = binding_paths == compatible;
    if !copy_all && binding_paths.contains(&vec![ir::XML_TEXT_FIELD.to_string()]) {
        // XML text and its owning group share one MapForce port. An ordinary
        // occurrence wire plus a direct text wire would connect that input twice.
        return None;
    }
    Some(ScopePlan {
        source_collection: collection.to_vec(),
        source_group,
        copy_all,
    })
}

fn resolve_source_collection(
    schema: &SchemaNode,
    anchor: &[String],
    source: &[String],
) -> Option<Vec<String>> {
    if source.is_empty() {
        return Some(anchor.to_vec());
    }

    let mut bases = vec![anchor.len()];
    bases.extend((1..anchor.len()).rev().filter(|&length| {
        schema_node_at(schema, &anchor[..length]).is_some_and(|node| node.repeating)
    }));
    bases.push(0);
    bases.dedup();

    bases.into_iter().find_map(|length| {
        let mut candidate = anchor[..length].to_vec();
        candidate.extend(source.iter().cloned());
        schema_node_at(schema, &candidate).map(|_| candidate)
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
        || scope.group_by.is_some()
        || scope.group_starting_with.is_some()
        || scope.group_into_blocks.is_some()
        || scope.sort_by.is_some() && !relative.is_empty()
        || scope.take.is_some() && !relative.is_empty()
    {
        return None;
    }
    for binding in &scope.bindings {
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
