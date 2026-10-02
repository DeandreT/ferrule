//! Native element-value connection for one exact conditional text occurrence.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use ir::{ScalarType, SchemaKind, SchemaNode, Value, XML_TEXT_FIELD};
use mapping::{
    IterationOutput, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration, SequenceExpr,
};

use super::{
    ExportCompatibilityFeature, ExportCompatibilityIssue, TargetExport, schema::SideFormat,
    source::SourceExports,
};
use crate::MfdError;

struct Plan {
    component: u32,
    group: u32,
    text: u32,
    value: u32,
    item: u32,
    private: BTreeSet<u32>,
}

fn node_at<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    path.iter().try_fold(schema, |node, name| node.child(name))
}

fn string(node: &SchemaNode) -> bool {
    matches!(
        node.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ) && !node.repeating
        && !node.nillable
        && !node.attribute
        && !node.nullable
        && node.fixed.is_none()
        && node.recursive_ref.is_none()
}

fn single_text(node: &SchemaNode) -> bool {
    matches!(&node.kind, SchemaKind::Group { children, alternatives, dynamic, .. }
        if alternatives.is_empty() && dynamic.is_none() && children.len() == 1
            && children[0].name == XML_TEXT_FIELD && children[0].text && string(&children[0]))
        && !node.repeating
        && !node.nillable
        && !node.attribute
        && node.recursive_ref.is_none()
}

struct Planner<'a, 'b> {
    project: &'a Project,
    sources: &'b SourceExports<'a>,
    outputs: &'b BTreeMap<NodeId, u32>,
    item_owners: BTreeMap<NodeId, usize>,
    plans: Vec<Plan>,
    issues: Vec<ExportCompatibilityIssue>,
}

impl Planner<'_, '_> {
    fn constant(&self, id: NodeId, value: i64) -> bool {
        matches!(self.project.graph.nodes.get(&id), Some(Node::Const { value: Value::Int(n) }) if *n == value)
    }

    fn qualify(
        &self,
        target: &TargetExport<'_>,
        scope: &Scope,
        path: &[String],
        anchor: &[String],
        context: bool,
        (group, text): (u32, u32),
    ) -> Result<Plan, &'static str> {
        let target_node =
            node_at(target.schema, path).ok_or("target schema ownership is unresolved")?;
        if !single_text(target_node) {
            return Err(
                "requires one non-nillable string text payload and no attributes or other children",
            );
        }
        if !context
            || anchor.is_empty()
            || self.sources.is_named_extra_path(anchor)
            || !self
                .sources
                .schema_node_at(anchor)
                .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
            || !self
                .sources
                .xml_sequence_identity(anchor)
                .map_err(|_| "source identity is unresolved")?
                .is_some()
        {
            return Err("requires one static primary XML item context");
        }
        if (0..=path.len()).any(|len| {
            target.branches.count(&path[..len]).is_some()
                || node_at(target.schema, &path[..len])
                    .is_some_and(|node| !node.alternatives().is_empty())
        }) {
            return Err("conditional or cloned target branches are not qualified");
        }
        if scope.construction != ScopeConstruction::Constructed
            || scope.iteration_output != IterationOutput::MappedSequence
            || scope.bindings.len() != 1
            || scope.bindings[0].target_field != XML_TEXT_FIELD
            || !scope.children.is_empty()
            || !scope.dynamic_bindings.is_empty()
            || !scope.dynamic_children.is_empty()
            || scope.merge_dynamic_fields
            || scope.filter.is_some()
            || scope.post_group_filter.is_some()
            || scope.has_grouping()
            || scope.has_sort()
            || !scope.windows.is_empty()
        {
            return Err(
                "requires one unchanged conditional text occurrence without additional construction controls",
            );
        }
        let Some(SequenceExpr::Generate { from, to, item }) = scope.sequence() else {
            return Err("requires a conditional zero-or-one generated occurrence");
        };
        if from.is_some_and(|id| !self.constant(id, 1))
            || self.item_owners.get(item) != Some(&1)
            || self
                .project
                .graph
                .nodes
                .values()
                .any(|node| node.dependencies().contains(item))
        {
            return Err("generated item ownership or first bound is not qualified");
        }
        let value_id = scope.bindings[0].node;
        let Some(Node::If {
            condition,
            then,
            else_,
        }) = self.project.graph.nodes.get(to)
        else {
            return Err("occurrence count must be exists(value) ? 1 : 0");
        };
        if !self.constant(*then, 1)
            || !self.constant(*else_, 0)
            || !matches!(self.project.graph.nodes.get(condition), Some(Node::Call { function, args }) if function == "exists" && args.as_slice() == [value_id])
        {
            return Err("occurrence count must test exactly the same text value");
        }
        let Some(Node::SourceField {
            path: value_path,
            frame: Some(frame),
        }) = self.project.graph.nodes.get(&value_id)
        else {
            return Err("text must be a direct field pinned to its current XML item");
        };
        if frame != anchor {
            return Err("text field uses a different item frame");
        }
        let mut absolute = anchor.to_vec();
        absolute.extend(value_path.iter().cloned());
        if value_path.is_empty()
            || self.sources.schema_node_at(&absolute).is_none()
            || (anchor.len() + 1..=absolute.len()).any(|len| {
                self.sources
                    .schema_node_at(&absolute[..len])
                    .is_some_and(|node| {
                        node.repeating
                            || node.nillable
                            || node.attribute
                            || node.recursive_ref.is_some()
                            || !node.alternatives().is_empty()
                    })
            })
        {
            return Err(
                "text field crosses an unqualified repetition, nil, attribute, or alternative",
            );
        }
        let scalar = self
            .sources
            .schema_node_at(&absolute)
            .ok_or("text source is unresolved")?;
        if !string(scalar) {
            return Err("text source must be a non-nillable string scalar");
        }
        if scalar.text {
            let parent = self
                .sources
                .schema_node_at(&absolute[..absolute.len() - 1])
                .ok_or("text parent is unresolved")?;
            if !single_text(parent) {
                return Err("source simple content must contain only its string text payload");
            }
        }
        let value = *self
            .outputs
            .get(&value_id)
            .ok_or("text value has no exported port")?;
        let item = *self
            .outputs
            .get(item)
            .ok_or("generated occurrence has no exported port")?;
        let private = [
            Some(*condition),
            Some(*then),
            Some(*else_),
            Some(*to),
            *from,
        ]
        .into_iter()
        .flatten()
        .filter_map(|id| self.outputs.get(&id).copied())
        .chain([item])
        .collect();
        Ok(Plan {
            component: target.component_uid,
            group,
            text,
            value,
            item,
            private,
        })
    }

    fn walk(
        &mut self,
        target: &TargetExport<'_>,
        scope: &Scope,
        path: &mut Vec<String>,
        anchor: &[String],
        context: bool,
    ) {
        if (scope.sequence().is_some() || scope.iteration_output == IterationOutput::MappedSequence)
            && scope
                .bindings
                .iter()
                .any(|binding| binding.target_field == XML_TEXT_FIELD)
            && let (Some(group), Some(text)) = (target.ports.key_for_abs(path), {
                let mut text_path = path.clone();
                text_path.push(XML_TEXT_FIELD.into());
                target.ports.key_for_abs(&text_path)
            })
            && group != text
        {
            match self.qualify(target, scope, path, anchor, context, (group, text)) {
                Ok(plan) => self.plans.push(plan),
                Err(reason) => self.issues.push(ExportCompatibilityIssue {
                    feature: ExportCompatibilityFeature::XmlTextOccurrence,
                    component: target.component_name.to_string(),
                    component_uid: Some(target.component_uid),
                    message: format!(
                        "XML simple-content scope `{}` is not natively qualified: {reason}",
                        path.join("/")
                    ),
                }),
            }
        }
        let (next_anchor, next_context) = match &scope.iteration {
            ScopeIteration::None => (anchor.to_vec(), context),
            ScopeIteration::Source(source) if context => {
                (self.sources.resolve_scope_path(anchor, source).0, true)
            }
            _ => (anchor.to_vec(), false),
        };
        for child in &scope.children {
            path.push(child.target_field.clone());
            self.walk(target, child, path, &next_anchor, next_context);
            path.pop();
        }
        if let Some(segments) = scope.concatenated() {
            for segment in segments.iter() {
                self.walk(target, segment, path, anchor, false);
            }
        }
    }
}

struct Edge<'a, 'input> {
    from: u32,
    to: u32,
    node: roxmltree::Node<'a, 'input>,
}
fn key(node: roxmltree::Node<'_, '_>, name: &str) -> Option<u32> {
    node.attribute(name)?.parse().ok()
}

pub(super) fn rewrite(
    xml: &str,
    project: &Project,
    sources: &SourceExports<'_>,
    targets: &[TargetExport<'_>],
    outputs: &BTreeMap<NodeId, u32>,
) -> Result<(String, Vec<ExportCompatibilityIssue>), MfdError> {
    let mut sequences = Vec::new();
    for target in targets {
        super::sequence::collect_scope_sequences(target.root, &mut sequences);
    }
    let mut item_owners = BTreeMap::new();
    for sequence in sequences {
        *item_owners.entry(sequence.item()).or_default() += 1;
    }
    for node in project.graph.nodes.values() {
        if let Node::SequenceExists { sequence, .. }
        | Node::SequenceItemAt { sequence, .. }
        | Node::SequenceAggregate { sequence, .. } = node
        {
            *item_owners.entry(sequence.item()).or_default() += 1;
        }
    }
    let mut planner = Planner {
        project,
        sources,
        outputs,
        item_owners,
        plans: Vec::new(),
        issues: Vec::new(),
    };
    for target in targets
        .iter()
        .filter(|target| target.format == SideFormat::Xml)
    {
        planner.walk(target, target.root, &mut Vec::new(), &[], true);
    }
    if planner.plans.is_empty() {
        return Ok((xml.to_string(), planner.issues));
    }
    let document = roxmltree::Document::parse(xml)?;
    let structure = document
        .descendants()
        .find(|node| node.has_tag_name("structure"))
        .ok_or(MfdError::NotMfd("missing rendered structure"))?;
    let children = structure
        .children()
        .find(|node| node.has_tag_name("children"))
        .ok_or(MfdError::NotMfd("missing rendered children"))?;
    let graph = structure
        .children()
        .find(|node| node.has_tag_name("graph"))
        .ok_or(MfdError::NotMfd("missing rendered graph"))?;
    let mut edges = Vec::new();
    let mut vertices = BTreeMap::new();
    for vertex in graph
        .descendants()
        .filter(|node| node.has_tag_name("vertex"))
    {
        if let Some(from) = key(vertex, "vertexkey") {
            vertices.insert(from, vertex);
            for edge in vertex
                .descendants()
                .filter(|node| node.has_tag_name("edge"))
            {
                if let Some(to) = key(edge, "vertexkey") {
                    edges.push(Edge {
                        from,
                        to,
                        node: edge,
                    });
                }
            }
        }
    }
    let mut removed_edges = BTreeSet::new();
    let mut redirects = BTreeMap::new();
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut private = BTreeSet::new();
    for plan in planner.plans {
        let text_entries = children
            .children()
            .filter(|node| {
                node.has_tag_name("component") && key(*node, "uid") == Some(plan.component)
            })
            .flat_map(|component| component.descendants())
            .filter(|entry| entry.has_tag_name("entry") && key(*entry, "inpkey") == Some(plan.text))
            .collect::<Vec<_>>();
        let driver = edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| edge.to == plan.group)
            .collect::<Vec<_>>();
        let value = edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| edge.to == plan.text)
            .collect::<Vec<_>>();
        if text_entries.len() != 1
            || driver.len() != 1
            || driver[0].1.from != plan.item
            || value.len() != 1
            || value[0].1.from != plan.value
            || edges
                .iter()
                .any(|edge| edge.from == plan.item && edge.to != plan.group)
        {
            planner.issues.push(ExportCompatibilityIssue {
                feature: ExportCompatibilityFeature::XmlTextOccurrence,
                component: targets
                    .iter()
                    .find(|target| target.component_uid == plan.component)
                    .map_or_else(|| "XML target".into(), |target| target.component_name.to_string()),
                component_uid: Some(plan.component),
                message: "XML simple-content occurrence has ambiguous rendered endpoint ownership; native lowering skipped".into(),
            });
            continue;
        }
        edits.push((text_entries[0].range(), String::new()));
        removed_edges.insert(driver[0].0);
        redirects.insert(value[0].0, plan.group);
        private.extend(plan.private);
    }
    let components = children
        .children()
        .filter(|node| node.has_tag_name("component") && node.attribute("library") == Some("core"))
        .map(|component| {
            let outputs = component
                .children()
                .filter(|node| node.has_tag_name("targets"))
                .flat_map(|targets| targets.descendants())
                .filter_map(|node| key(node, "key"))
                .collect::<BTreeSet<_>>();
            let inputs = component
                .children()
                .filter(|node| node.has_tag_name("sources"))
                .flat_map(|sources| sources.descendants())
                .filter_map(|node| key(node, "key"))
                .collect::<BTreeSet<_>>();
            (component, inputs, outputs)
        })
        .collect::<Vec<_>>();
    let mut removed_components = BTreeSet::new();
    let mut removed_outputs = BTreeSet::new();
    loop {
        let mut changed = false;
        for (index, (component, inputs, outputs)) in components.iter().enumerate() {
            if removed_components.contains(&index)
                || outputs.is_empty()
                || !outputs.is_subset(&private)
                || edges.iter().enumerate().any(|(index, edge)| {
                    outputs.contains(&edge.from) && !removed_edges.contains(&index)
                })
            {
                continue;
            }
            removed_components.insert(index);
            removed_outputs.extend(outputs);
            edits.push((component.range(), String::new()));
            for (index, edge) in edges.iter().enumerate() {
                if inputs.contains(&edge.to) {
                    removed_edges.insert(index);
                }
            }
            changed = true;
        }
        if !changed {
            break;
        }
    }
    for output in &removed_outputs {
        if let Some(vertex) = vertices.get(output) {
            edits.push((vertex.range(), String::new()));
        }
    }
    for (index, edge) in edges.iter().enumerate() {
        if removed_outputs.contains(&edge.from) {
            continue;
        }
        if removed_edges.contains(&index) {
            edits.push((edge.node.range(), String::new()));
        } else if let Some(to) = redirects.get(&index) {
            let attribute = edge
                .node
                .attributes()
                .find(|attribute| attribute.name() == "vertexkey")
                .ok_or(MfdError::NotMfd("missing rendered connection key"))?;
            edits.push((attribute.range_value(), to.to_string()));
        }
    }
    edits.sort_by_key(|(range, _)| range.start);
    if edits.windows(2).any(|pair| pair[0].0.end > pair[1].0.start) {
        return Err(MfdError::Unsupported(
            "overlapping XML simple-content lowering edits".into(),
        ));
    }
    let mut rewritten = xml.to_string();
    for (range, text) in edits.into_iter().rev() {
        rewritten.replace_range(range, &text);
    }
    Ok((rewritten, planner.issues))
}
