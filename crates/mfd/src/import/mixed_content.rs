use std::collections::{BTreeMap, BTreeSet};

use ir::SchemaKind;
use mapping::NodeId;

use super::graph::GraphBuilder;
use super::iteration::IntermediateFeed;
use super::schema::schema_node_at_resolved;
use super::source::SourcePath;

impl GraphBuilder<'_> {
    pub(super) fn xml_mixed_content_node(
        &mut self,
        intermediate: &IntermediateFeed,
    ) -> Option<NodeId> {
        let source_path = self.sequence_source_path(intermediate.feed)?;
        let source_schema = &self.sources.get(source_path.source)?.schema;
        let SchemaKind::Group { children, .. } =
            &schema_node_at_resolved(source_schema, &source_path.path)?.kind
        else {
            return None;
        };
        if !children.iter().any(|child| child.text)
            || intermediate.ordered_projections.len() < 2
            || intermediate
                .ordered_projections
                .iter()
                .any(|(path, _)| path.as_slice() != [ir::XML_TEXT_FIELD])
        {
            return None;
        }
        let mut text_path = source_path.path.clone();
        text_path.push(ir::XML_TEXT_FIELD.to_string());
        let base_index = intermediate
            .ordered_projections
            .iter()
            .position(|(_, feed)| {
                *feed == intermediate.feed
                    || self.source_abs_path(*feed).is_some_and(|candidate| {
                        candidate.source == source_path.source && candidate.path == text_path
                    })
            })?;
        let mut replacements = Vec::new();
        let mut elements = BTreeSet::new();
        for (index, (_, feed)) in intermediate.ordered_projections.iter().enumerate() {
            if index == base_index {
                continue;
            }
            let child_names = self
                .scalar_dependencies(*feed)?
                .into_iter()
                .filter(|dependency| {
                    dependency.source == source_path.source
                        && dependency.path.starts_with(&source_path.path)
                        && dependency.path.len() > source_path.path.len()
                })
                .map(|dependency| dependency.path[source_path.path.len()].clone())
                .filter(|child| child != ir::XML_TEXT_FIELD)
                .collect::<BTreeSet<_>>();
            let mut child_names = child_names.into_iter();
            let element = child_names.next()?;
            if child_names.next().is_some() || !elements.insert(element.clone()) {
                return None;
            }
            let mut child_path = source_path.path.clone();
            child_path.push(element.clone());
            let child_source = SourcePath {
                source: source_path.source,
                path: child_path,
            };
            let runtime_collection = self.context_path(&child_source);
            let expression = self.value_node(*feed)?;
            let expression = self.reframe_mixed_expression(
                expression,
                &runtime_collection,
                &mut BTreeMap::new(),
            )?;
            replacements.push(mapping::XmlMixedContentReplacement {
                element,
                collection: runtime_collection,
                expression,
            });
        }
        if replacements.is_empty() {
            return None;
        }
        let schema = &self.sources.get(source_path.source)?.schema;
        let path = self.suffix_after_framed(source_path.source, schema, &source_path.path);
        let frame = self.frame_for_field(source_path.source, schema, &source_path.path);
        Some(self.alloc(mapping::Node::XmlMixedContent {
            path,
            frame,
            replacements,
        }))
    }

    fn reframe_mixed_expression(
        &mut self,
        node_id: NodeId,
        child_frame: &[String],
        cloned: &mut BTreeMap<NodeId, NodeId>,
    ) -> Option<NodeId> {
        if let Some(node) = cloned.get(&node_id) {
            return Some(*node);
        }
        let node = self.graph.nodes.get(&node_id)?.clone();
        let reframed = match node {
            mapping::Node::SourceField { path, frame } => {
                let mut absolute = frame.unwrap_or_default();
                absolute.extend(path);
                if absolute == child_frame {
                    self.alloc(mapping::Node::SourceField {
                        path: Vec::new(),
                        frame: Some(child_frame.to_vec()),
                    })
                } else {
                    node_id
                }
            }
            mapping::Node::Call { function, args } => {
                let args = args
                    .into_iter()
                    .map(|arg| self.reframe_mixed_expression(arg, child_frame, cloned))
                    .collect::<Option<Vec<_>>>()?;
                self.alloc(mapping::Node::Call { function, args })
            }
            mapping::Node::If {
                condition,
                then,
                else_,
            } => {
                let condition = self.reframe_mixed_expression(condition, child_frame, cloned)?;
                let then = self.reframe_mixed_expression(then, child_frame, cloned)?;
                let else_ = self.reframe_mixed_expression(else_, child_frame, cloned)?;
                self.alloc(mapping::Node::If {
                    condition,
                    then,
                    else_,
                })
            }
            mapping::Node::ValueMap {
                input,
                input_type,
                table,
                default,
            } => {
                let input = self.reframe_mixed_expression(input, child_frame, cloned)?;
                self.alloc(mapping::Node::ValueMap {
                    input,
                    input_type,
                    table,
                    default,
                })
            }
            mapping::Node::Const { .. } | mapping::Node::RuntimeValue { .. } => node_id,
            _ => return None,
        };
        cloned.insert(node_id, reframed);
        Some(reframed)
    }
}
