//! Preserve a constructed first-item group's presence when its candidates are empty.
//!
//! The selected payload retains its typed variable context. The enclosing raw
//! group independently creates the target group, including the empty result.

use std::collections::BTreeMap;

use ir::{SchemaKind, SchemaNode};
use mapping::{Graph, IterationOutput, Node, Scope, ScopeConstruction};
use roxmltree::{Document, Node as XmlNode};

use super::source::SourceExports;
use super::{ExportCompatibilityFeature, ExportCompatibilityIssue, TargetExport};
use crate::MfdError;

struct Request {
    target: u32,
    parent_target: u32,
    parent_source: u32,
    collection_source: u32,
    instance_root: String,
    fields: Vec<(u32, Option<String>)>,
    issue: ExportCompatibilityIssue,
}

pub(super) struct Plan {
    requests: Vec<Request>,
    issues: Vec<ExportCompatibilityIssue>,
}

impl Plan {
    pub(super) fn build(
        graph: &Graph,
        sources: &SourceExports<'_>,
        targets: &[TargetExport<'_>],
    ) -> Self {
        let mut plan = Self {
            requests: Vec::new(),
            issues: Vec::new(),
        };
        for target in targets {
            if target.format != super::SideFormat::Xml {
                continue;
            }
            plan.collect(
                graph,
                sources,
                target,
                target.root,
                &mut Vec::new(),
                &[],
                None,
            );
        }
        plan
    }

    #[allow(clippy::too_many_arguments)]
    fn collect(
        &mut self,
        graph: &Graph,
        sources: &SourceExports<'_>,
        target: &TargetExport<'_>,
        scope: &Scope,
        path: &mut Vec<String>,
        anchor: &[String],
        parent: Option<(&Scope, &[String])>,
    ) {
        let collection = scope.source().map_or_else(
            || anchor.to_vec(),
            |source| {
                if sources.is_named_extra_path(source) {
                    source.to_vec()
                } else {
                    sources.resolve_scope_path(anchor, source).0
                }
            },
        );
        if scope.iteration_output == IterationOutput::First {
            let label = if path.is_empty() {
                "<root>".into()
            } else {
                path.join("/")
            };
            let mut issue = ExportCompatibilityIssue {
                feature: ExportCompatibilityFeature::XmlFirstGroupPresence,
                component: target.component_name.to_string(),
                component_uid: Some(target.component_uid),
                message: format!(
                    "first-item scope `{label}` has no qualified construction for a present empty group"
                ),
            };
            let qualified = (|| {
                if path.is_empty()
                    || scope.source().is_none()
                    || scope.construction != ScopeConstruction::Constructed
                    || !scope.children.is_empty()
                {
                    return None;
                }
                let (parent, parent_collection) = parent?;
                if parent.source().is_none()
                    || parent.construction != ScopeConstruction::Constructed
                    || parent.iteration_output != IterationOutput::Repeated
                    || parent.filter.is_some()
                    || parent.post_group_filter.is_some()
                    || parent.has_grouping()
                    || parent.has_sort()
                    || !parent.windows.is_empty()
                    || collection.len() != parent_collection.len() + 1
                    || !collection.starts_with(parent_collection)
                {
                    return None;
                }
                let source_group = sources.schema_node_at(&collection)?;
                let parent_group = sources.schema_node_at(parent_collection)?;
                if !source_group.repeating
                    || !parent_group.repeating
                    || !ordinary_group(source_group)
                    || !ordinary_group(parent_group)
                {
                    return None;
                }
                let group = schema_at(target.schema, path)?;
                let parent_path = &path[..path.len() - 1];
                let parent_target_group = schema_at(target.schema, parent_path)?;
                if group.repeating
                    || !ordinary_group(group)
                    || !parent_target_group.repeating
                    || !ordinary_group(parent_target_group)
                {
                    return None;
                }
                let mapped = target.mapped_scope_plans.get(path, None)?;
                let (mapped_collection, mapped_group) = mapped.source()?;
                if mapped.copy_all()
                    || mapped_collection != collection
                    || mapped_group != collection
                {
                    return None;
                }
                let mut fields = Vec::new();
                let mut position = false;
                for binding in &scope.bindings {
                    let leaf = group.child(&binding.target_field)?;
                    if !leaf.is_scalar() || leaf.repeating || leaf.attribute || leaf.text {
                        return None;
                    }
                    let mut leaf_path = path.clone();
                    leaf_path.push(binding.target_field.clone());
                    let port = target.ports.key_for_abs(&leaf_path)?;
                    match graph.nodes.get(&binding.node)? {
                        Node::Position {
                            collection: position_collection,
                        } if position_collection == &collection
                            || scope.source() == Some(position_collection.as_slice()) =>
                        {
                            position = true;
                            fields.push((port, None));
                        }
                        Node::SourceField { path, frame } => {
                            let local = if path.len() == 1
                                && frame.as_deref() == Some(collection.as_slice())
                            {
                                &path[0]
                            } else if frame.is_none()
                                && scope.source() == Some(collection.as_slice())
                                && path.len() == collection.len() + 1
                                && path.starts_with(&collection)
                            {
                                path.last()?
                            } else {
                                return None;
                            };
                            let source_leaf = source_group.child(local)?;
                            if !source_leaf.is_scalar()
                                || source_leaf.repeating
                                || source_leaf.attribute
                                || source_leaf.text
                            {
                                return None;
                            }
                            fields.push((port, Some(local.clone())));
                        }
                        _ => return None,
                    }
                }
                if !position {
                    return None;
                }
                let target_port = target.ports.key_for_abs(path)?;
                let parent_target = target.ports.key_for_abs(parent_path)?;
                let collection_source = sources.key_for_abs(&collection)?;
                let parent_source = sources.key_for_abs(parent_collection)?;
                let identity = sources
                    .xml_sequence_identity_for_port(collection_source)
                    .ok()??;
                Some((
                    target_port,
                    parent_target,
                    parent_source,
                    collection_source,
                    identity.instance_root,
                    fields,
                ))
            })();
            if let Some((
                target_port,
                parent_target,
                parent_source,
                collection_source,
                instance_root,
                fields,
            )) = qualified
            {
                issue.message.push_str(
                    ": the rendered parent and selected-item contexts must match exactly",
                );
                self.requests.push(Request {
                    target: target_port,
                    parent_target,
                    parent_source,
                    collection_source,
                    instance_root,
                    fields,
                    issue,
                });
            } else {
                issue.message.push_str("; native support requires a nested source-driven constructed group with direct selected scalar and current-item position bindings under one raw repeating parent");
                self.issues.push(issue);
            }
        }
        if let Some(segments) = scope.concatenated() {
            for segment in segments.iter() {
                self.collect(graph, sources, target, segment, path, anchor, None);
            }
        }
        for child in &scope.children {
            path.push(child.target_field.clone());
            self.collect(
                graph,
                sources,
                target,
                child,
                path,
                &collection,
                Some((scope, &collection)),
            );
            path.pop();
        }
    }

    pub(super) fn rewrite(
        mut self,
        xml: &str,
    ) -> Result<(String, Vec<ExportCompatibilityIssue>), MfdError> {
        let mut result = xml.to_string();
        for request in &self.requests {
            if let Some(rewritten) = rewrite_one(&result, request)? {
                result = rewritten;
            } else {
                self.issues.push(request.issue.clone());
            }
        }
        Ok((result, self.issues))
    }
}

fn ordinary_group(node: &SchemaNode) -> bool {
    matches!(&node.kind, SchemaKind::Group { alternatives, dynamic, .. } if alternatives.is_empty() && dynamic.is_none())
        && node.recursive_ref.is_none()
        && node.xml_repeating_choices.is_empty()
        && node.xml_repeating_sequences.is_empty()
}
fn schema_at<'a>(mut node: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    for field in path {
        node = node.child(field)?;
    }
    Some(node)
}
fn key(value: Option<&str>) -> Option<u32> {
    value?.parse().ok()
}
fn child<'a, 'input>(node: XmlNode<'a, 'input>, name: &str) -> Option<XmlNode<'a, 'input>> {
    node.children().find(|node| node.has_tag_name(name))
}
fn ports(node: XmlNode<'_, '_>, side: &str) -> Vec<u32> {
    child(node, side)
        .into_iter()
        .flat_map(|side| side.children())
        .filter_map(|node| key(node.attribute("key")))
        .collect()
}
fn one(edges: &BTreeMap<u32, Vec<(u32, XmlNode<'_, '_>)>>, input: u32) -> Option<(u32, u32)> {
    let [(from, edge)] = edges.get(&input)?.as_slice() else {
        return None;
    };
    Some((*from, key(edge.attribute("edgekey")).unwrap_or(0)))
}
fn rewrite_one(xml: &str, request: &Request) -> Result<Option<String>, MfdError> {
    let document = Document::parse(xml)?;
    let checked = (|| {
        let structure = document
            .descendants()
            .find(|node| node.has_tag_name("structure"))?;
        let components = child(structure, "children")?;
        let graph = child(structure, "graph")?;
        let vertices = child(graph, "vertices")?;
        let mut incoming = BTreeMap::<u32, Vec<(u32, XmlNode<'_, '_>)>>::new();
        for vertex in vertices
            .children()
            .filter(|node| node.has_tag_name("vertex"))
        {
            let from = key(vertex.attribute("vertexkey"))?;
            for edge in child(vertex, "edges")?
                .children()
                .filter(|node| node.has_tag_name("edge"))
            {
                incoming
                    .entry(key(edge.attribute("vertexkey"))?)
                    .or_default()
                    .push((from, edge));
            }
        }
        let outputs = components
            .children()
            .filter(|node| node.has_tag_name("component"))
            .flat_map(|component| {
                ports(component, "targets")
                    .into_iter()
                    .map(move |port| (port, component))
            })
            .collect::<BTreeMap<_, _>>();
        let (selected, metadata) = one(&incoming, request.target)?;
        if metadata != 0 || one(&incoming, request.parent_target)?.0 != request.parent_source {
            return None;
        }
        let variable = components.children().find(|component| {
            component.attribute("library") == Some("xml")
                && component.descendants().any(|node| {
                    node.has_tag_name("parameter")
                        && node.attribute("usageKind") == Some("variable")
                })
                && component
                    .descendants()
                    .any(|node| key(node.attribute("outkey")) == Some(selected))
        })?;
        let payload = variable
            .descendants()
            .find(|node| key(node.attribute("outkey")) == Some(selected))?;
        if variable
            .descendants()
            .find(|node| node.has_tag_name("document"))?
            .attribute("instanceroot")
            != Some(request.instance_root.as_str())
        {
            return None;
        }
        let compute = variable.descendants().find(|node| {
            node.has_tag_name("entry") && node.attribute("name") == Some("compute-when")
        })?;
        if one(&incoming, key(compute.attribute("inpkey"))?)?.0 != request.parent_source {
            return None;
        }
        let (terminal, structural) = one(&incoming, key(payload.attribute("inpkey"))?)?;
        let edge_metadata = child(graph, "edges")?
            .children()
            .find(|node| key(node.attribute("edgekey")) == Some(structural))?;
        if structural == 0
            || !edge_metadata.descendants().any(|node| {
                node.has_tag_name("dataconnection") && node.attribute("type") == Some("2")
            })
        {
            return None;
        }
        let first = *outputs.get(&terminal)?;
        if first.attribute("library") != Some("core")
            || first.attribute("name") != Some("first-items")
            || ports(first, "sources").len() != 1
        {
            return None;
        }
        let mut origin = terminal;
        let mut visited = std::collections::BTreeSet::new();
        while let Some(component) = outputs.get(&origin) {
            if !visited.insert(origin)
                || visited.len() > 256
                || component.attribute("library") != Some("core")
                || !matches!(
                    component.attribute("name"),
                    Some(
                        "filter"
                            | "sort"
                            | "skip-first-items"
                            | "first-items"
                            | "items-from"
                            | "items-from-to"
                            | "last-items"
                    )
                )
            {
                return None;
            }
            origin = one(&incoming, *ports(*component, "sources").first()?)?.0;
        }
        if origin != request.collection_source {
            return None;
        }
        for (target, field) in &request.fields {
            let output = one(&incoming, *target)?.0;
            if let Some(field) = field {
                let leaf = payload.children().find(|node| {
                    node.has_tag_name("entry") && node.attribute("name") == Some(field.as_str())
                })?;
                if key(leaf.attribute("outkey")) != Some(output) {
                    return None;
                }
            } else {
                let position = *outputs.get(&output)?;
                if position.attribute("library") != Some("core")
                    || position.attribute("name") != Some("position")
                    || ports(position, "sources").len() != 1
                    || one(&incoming, ports(position, "sources")[0])?.0 != selected
                {
                    return None;
                }
            }
        }
        let [(_, old)] = incoming.get(&request.target)?.as_slice() else {
            return None;
        };
        let parent = vertices
            .children()
            .find(|node| key(node.attribute("vertexkey")) == Some(request.parent_source))?;
        let parent_edges = child(parent, "edges")?;
        let range = parent_edges.range();
        let end = range.start + xml[range.clone()].rfind("</edges>")?;
        Some((old.range(), end))
    })();
    let Some((remove, insert)) = checked else {
        return Ok(None);
    };
    let mut result = xml.to_string();
    let addition = format!("<edge vertexkey=\"{}\"/>", request.target);
    if insert > remove.start {
        result.insert_str(insert, &addition);
        result.replace_range(remove, "");
    } else {
        result.replace_range(remove, "");
        result.insert_str(insert, &addition);
    }
    Ok(Some(result))
}
