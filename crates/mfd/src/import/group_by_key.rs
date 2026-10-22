//! Present singleton keys for the native core group-by String pins.

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{FormatOptions, Node, NodeId};

use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, schema_node_at};
use super::source::SourcePath;

// Match the established bounded scalar path proof; the design reader separately
// bounds the complete raw document at MAX_MFD_DESIGN_BYTES.
const MAX_DEPTH: usize = 64;

#[cfg(test)]
mod tests;

pub(super) fn ordinary(node: &SchemaNode) -> bool {
    super::presence::plain_non_null(node)
        && !node.json_any
        && node.recursive_ref.is_none()
        && node.alternatives().is_empty()
        && node.dynamic_fields().is_none()
        && node.default.is_none()
        && node.value_generation.is_none()
}

impl GraphBuilder<'_> {
    /// Rejected proofs cannot change graph, allocation, caches or diagnostics.
    pub(super) fn group_by_key_node(
        &mut self,
        index: usize,
        active_anchor: &[String],
    ) -> Option<NodeId> {
        self.present_group_by_key_node(index, active_anchor)
    }

    fn present_group_by_key_node(
        &mut self,
        index: usize,
        active_anchor: &[String],
    ) -> Option<NodeId> {
        let component = self.fn_components.get(index)?;
        let raw = component.group_by_ownership.as_ref()?;
        if component.library != "core"
            || component.kind != 5
            || component.name != "group-by"
            || component.inputs.as_slice() != raw.inputs.map(Some)
            || component.output_pins.as_slice() != raw.outputs.map(Some)
            || component.outputs.as_slice() != raw.outputs
            || self.input_feed(index, 0) != Some(raw.rows)
            || self.input_feed(index, 1) != Some(raw.key)
            || raw.outputs.iter().any(|output| {
                self.fn_by_output.get(output) != Some(&index)
                    || super::constructed_parser_scalar::output_is_nonfunction_shadowed(
                        *output, self,
                    )
                    || self
                        .fn_components
                        .iter()
                        .filter(|function| function.declared_output_keys.contains(output))
                        .count()
                        != 1
            })
        {
            return None;
        }
        for (position, &output) in raw.outputs.iter().enumerate() {
            let consumers = self
                .edge_from
                .iter()
                .filter_map(|(&input, &feed)| (feed == output).then_some(input))
                .collect::<std::collections::BTreeSet<_>>();
            if consumers != raw.consumers[position]
                || consumers.iter().any(|input| {
                    self.fn_components
                        .iter()
                        .any(|function| function.inputs.contains(&Some(*input)))
                        || self
                            .sources
                            .iter()
                            .chain(self.intermediates.iter())
                            .any(|source| source.input_keys.contains(input))
                        || self
                            .udf_calls
                            .iter()
                            .any(|call| call.inputs.values().any(|key| key == input))
                        || self
                            .json_parsers
                            .iter()
                            .any(|parser| parser.input == *input)
                        || self.flextext_parsers.iter().any(|parser| {
                            parser
                                .outputs
                                .keys()
                                .any(|output| self.flextext_parser_input(*output) == Some(*input))
                        })
                })
            {
                return None;
            }
        }
        let primary = *self.sources.first()?;
        if primary.format != ComponentFormat::Json
            || !primary.is_source
            || primary.is_variable
            || primary.is_pass_through
            || primary.schema.repeating
            || primary.compute_when_key.is_some()
            || primary.dynamic_json.is_some()
            || !primary.json_schema_authoritative
            || !primary.db_queries.is_empty()
            || !primary.db_xml_columns.is_empty()
            || primary.options
                != (FormatOptions {
                    json_document: true,
                    ..Default::default()
                })
            || primary.output_keys != raw.primary_outputs
            || primary
                .input_keys
                .iter()
                .any(|input| self.edge_from.contains_key(input))
            || primary.output_keys.iter().any(|output| {
                !primary.unambiguous_declared_output(*output)
                    || self.has_source_node_functions(*output)
                    || self
                        .sources
                        .iter()
                        .filter(|source| source.declares_output(*output))
                        .count()
                        != 1
                    || self
                        .intermediates
                        .iter()
                        .any(|source| source.declares_output(*output))
                    || self
                        .fn_components
                        .iter()
                        .any(|function| function.declared_output_keys.contains(output))
                    || self.udf_by_output.contains_key(output)
                    || self.json_parsers.iter().any(|parser| {
                        parser.outputs.contains_key(output)
                            || parser.object_outputs.contains_key(output)
                    })
                    || self
                        .flextext_parsers
                        .iter()
                        .any(|parser| parser.outputs.contains_key(output))
            })
        {
            return None;
        }
        let rows = primary.ports.get(&raw.rows)?.clone();
        if rows.is_empty()
            || rows.len() > MAX_DEPTH
            || !primary
                .port_declarations
                .as_ref()
                .is_some_and(|facts| facts.json_array_object_frame(raw.rows))
            || !self.framed.contains(&rows)
            || active_anchor.is_empty()
            || active_anchor.len() > MAX_DEPTH
            || !active_anchor.starts_with(&rows)
            || !self.framed.contains(active_anchor)
            || !schema_node_at(&primary.schema, active_anchor).is_some_and(|node| {
                node.repeating && matches!(node.kind, SchemaKind::Group { .. }) && ordinary(node)
            })
            || (rows.len()..=active_anchor.len()).any(|depth| {
                schema_node_at(&primary.schema, &active_anchor[..depth]).is_none_or(|node| {
                    !ordinary(node)
                        || !matches!(node.kind, SchemaKind::Group { .. })
                        || node.repeating && !self.framed.contains(&active_anchor[..depth])
                })
            })
            || !schema_node_at(&primary.schema, &rows).is_some_and(|node| {
                node.repeating && matches!(node.kind, SchemaKind::Group { .. }) && ordinary(node)
            })
            || (0..rows.len()).any(|depth| {
                schema_node_at(&primary.schema, &rows[..depth]).is_none_or(|node| {
                    !ordinary(node)
                        || !matches!(node.kind, SchemaKind::Group { .. })
                        || !super::presence::json_property_required(node, &rows[depth])
                        || node.repeating && !self.framed.contains(&rows[..depth])
                })
            })
        {
            return None;
        }
        let (source, literal, ty) = if let Some(expected) = &raw.literal {
            let literal_index = *self.fn_by_output.get(&raw.key)?;
            let literal = self.fn_components.get(literal_index)?;
            if !literal.raw_single_output_valid
                || !literal.raw_literal_valid
                || !literal.bounded_scalar_pins_valid
                || literal.output_pins.as_slice() != [Some(raw.key)]
                || super::constructed_parser_scalar::output_is_nonfunction_shadowed(raw.key, self)
                || self
                    .fn_components
                    .iter()
                    .filter(|function| function.declared_output_keys.contains(&raw.key))
                    .count()
                    != 1
                || !literal.constant.as_ref().is_some_and(|(value, datatype)| {
                    super::function::parse_constant(value, datatype) == *expected
                })
            {
                return None;
            }
            let ty = match expected {
                Value::String(_) => ScalarType::String,
                Value::Int(_) => ScalarType::Int,
                Value::Bool(_) => ScalarType::Bool,
                _ => return None,
            };
            (None, Some(expected.clone()), ty)
        } else {
            let path = primary.ports.get(&raw.key)?.clone();
            if !path.starts_with(&rows)
                || path.len() <= rows.len()
                || path.len() > MAX_DEPTH
                || path
                    .first()
                    .is_some_and(|first| self.source_names.iter().skip(1).any(|name| name == first))
                || (rows.len() + 1..=path.len()).any(|depth| {
                    let parent = schema_node_at(&primary.schema, &path[..depth - 1]);
                    let node = schema_node_at(&primary.schema, &path[..depth]);
                    !parent.is_some_and(|parent| {
                        super::presence::json_property_required(parent, &path[depth - 1])
                    }) || !node.is_some_and(|node| ordinary(node) && !node.repeating)
                })
            {
                return None;
            }
            let ty = match schema_node_at(&primary.schema, &path)?.kind {
                SchemaKind::Scalar {
                    ty: ScalarType::String,
                } => ScalarType::String,
                SchemaKind::Scalar {
                    ty: ScalarType::Int,
                } => ScalarType::Int,
                SchemaKind::Scalar {
                    ty: ScalarType::Bool,
                } => ScalarType::Bool,
                _ => return None,
            };
            if !primary
                .port_declarations
                .as_ref()
                .is_some_and(|facts| facts.json_scalar(raw.key, ty))
            {
                return None;
            }
            (Some(SourcePath { source: 0, path }), None, ty)
        };
        let mut candidate = self.clone();
        let warnings = candidate.warnings.len();
        // Use the owner's row frame even when the key is projected below a
        // nested target. This does not use group-by's source-position 0.
        let key = if let Some(source) = &source {
            let key = candidate.source_field_at_anchor(source, &rows)?;
            if !matches!(candidate.graph.nodes.get(&key), Some(Node::SourceField { path, frame: Some(frame) }) if frame == &rows && path == &source.path[rows.len()..])
            {
                return None;
            }
            key
        } else {
            let key = candidate.value_node(raw.key)?;
            if !matches!(candidate.graph.nodes.get(&key), Some(Node::Const { value }) if Some(value) == literal.as_ref())
            {
                return None;
            }
            key
        };
        if candidate.warnings.len() != warnings {
            return None;
        }
        let result = if ty == ScalarType::String {
            key
        } else if let Some((&id, _)) = candidate.graph.nodes.iter().find(|(_, node)| {
            matches!(node, Node::Call { function, args } if function == "string" && args.as_slice() == [key])
        }) {
            id
        } else {
            candidate.alloc(Node::Call { function: "string".into(), args: vec![key] })
        };
        *self = candidate;
        Some(result)
    }
}
