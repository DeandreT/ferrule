//! Lazy native substitution for immutable ordinary-JSON String fields.

use ir::{JsonNullability, ScalarType, SchemaKind, SchemaNode};
use mapping::{Node, NodeId};

use super::constructed_parser_scalar;
use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, schema_node_at};
use super::source::SourcePath;

#[cfg(test)]
#[path = "lazy_string_substitution/tests.rs"]
mod tests;

enum Fallback {
    Physical(SourcePath),
    Constant(String),
    Parser(ParserWitness),
}

struct ParserWitness {
    output: u32,
    feed: u32,
    parameter: usize,
    name: String,
    optional: bool,
    preview: Option<String>,
    schema: String,
    path: String,
}

impl GraphBuilder<'_> {
    /// Speculative lowering owns a complete builder clone. A failed proof or
    /// failed feed lowering cannot change allocation, caches, or diagnostics.
    pub(super) fn lazy_string_substitution_node(&mut self, index: usize) -> Option<NodeId> {
        let component = self.fn_components.get(index)?;
        if component.kind != 5
            || !matches!(
                (component.library.as_str(), component.name.as_str()),
                ("db", "substitute-null") | ("core", "substitute-missing")
            )
            || !component.raw_single_output_valid
        {
            return None;
        }
        let [Some(left_pin), Some(fallback_pin)] = component.inputs.as_slice() else {
            return None;
        };
        let [output] = component.outputs.as_slice() else {
            return None;
        };
        if self.fn_by_output.get(output) != Some(&index)
            || constructed_parser_scalar::output_is_nonfunction_shadowed(*output, self)
            || self
                .fn_components
                .iter()
                .filter(|function| function.declared_output_keys.contains(output))
                .count()
                != 1
        {
            return None;
        }
        let left_feed = *self.edge_from.get(left_pin)?;
        let fallback_feed = *self.edge_from.get(fallback_pin)?;
        let raw = component.lazy_string_substitution.as_ref()?;
        if (
            raw.left_pin,
            raw.fallback_pin,
            raw.output,
            raw.left,
            raw.fallback,
        ) != (*left_pin, *fallback_pin, *output, left_feed, fallback_feed)
        {
            return None;
        }
        let left = self.physical_json_string(left_feed, true)?;
        let (frame, path) = self.source_location_at(&left)?;
        let fallback = if let Some(source) = self.physical_json_string(fallback_feed, false) {
            // Both physical reads must have the same enclosing iteration.
            if self.source_location_at(&source)?.0 != frame {
                return None;
            }
            Fallback::Physical(source)
        } else if self.exact_string_constant(fallback_feed) {
            Fallback::Constant(raw.constant.clone()?)
        } else if constructed_parser_scalar::schema(fallback_feed, self).is_some_and(|node| {
            matches!(
                node.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            )
        }) {
            let recipe = self
                .json_parsers
                .iter()
                .find(|recipe| recipe.outputs.contains_key(&fallback_feed))?;
            let feed = *self.edge_from.get(&recipe.input)?;
            if raw.parser_input != Some(feed) {
                return None;
            }
            let parameter = self
                .fn_components
                .iter()
                .position(|component| component.outputs.as_slice() == [feed])?;
            let component = &self.fn_components[parameter];
            let input = component.input_parameter_name.as_ref()?;
            Fallback::Parser(ParserWitness {
                output: fallback_feed,
                feed,
                parameter,
                name: input.name.clone(),
                optional: input.optional,
                preview: component.input_preview.clone(),
                schema: crate::json_parser_recipe::encode_schema(&recipe.schema, None).ok()?,
                path: serde_json::to_string(recipe.outputs.get(&fallback_feed)?).ok()?,
            })
        } else {
            return None;
        };

        let mut candidate = self.clone();
        let id = candidate.next_id;
        candidate.next_id += 1;
        candidate.fn_nodes.insert(index, id);
        let warnings = candidate.warnings.len();
        let left_id = candidate.value_node(left_feed)?;
        if !matches!(candidate.graph.nodes.get(&left_id),
            Some(Node::SourceField { path: actual_path, frame: actual_frame })
                if actual_path == &path && actual_frame == &frame)
        {
            return None;
        }
        let fallback_id = candidate.value_node(fallback_feed)?;
        let fallback_matches = match fallback {
            Fallback::Physical(source) => {
                let (frame, path) = candidate.source_location_at(&source)?;
                matches!(candidate.graph.nodes.get(&fallback_id),
                    Some(Node::SourceField { path: actual_path, frame: actual_frame })
                        if actual_path == &path && actual_frame == &frame)
            }
            Fallback::Constant(expected) => matches!(
                candidate.graph.nodes.get(&fallback_id),
                Some(Node::Const { value: ir::Value::String(actual) }) if actual == &expected
            ),
            Fallback::Parser(witness) => candidate.exact_parser_fallback(fallback_id, &witness),
        };
        if !fallback_matches || candidate.warnings.len() != warnings {
            return None;
        }
        let condition = candidate.alloc(Node::Call {
            function: "exists".into(),
            args: vec![left_id],
        });
        candidate.graph.nodes.insert(
            id,
            Node::If {
                condition,
                then: left_id,
                else_: fallback_id,
            },
        );
        *self = candidate;
        Some(id)
    }

    fn exact_parser_fallback(&self, node: NodeId, witness: &ParserWitness) -> bool {
        let Some(Node::Call { function, args }) = self.graph.nodes.get(&node) else {
            return false;
        };
        let [input, schema, path] = args.as_slice() else {
            return false;
        };
        if function != "json_parse_field"
            || self.json_parser_nodes.get(&witness.output) != Some(&node)
            || !matches!(self.graph.nodes.get(schema), Some(Node::Const { value: ir::Value::String(value) }) if value == &witness.schema)
            || !matches!(self.graph.nodes.get(path), Some(Node::Const { value: ir::Value::String(value) }) if value == &witness.path)
        {
            return false;
        }
        if witness.optional {
            let Some(Node::RuntimeParameterDefault {
                name,
                ty,
                default,
                preview,
            }) = self.graph.nodes.get(input)
            else {
                return false;
            };
            name == &witness.name
                && *ty == ScalarType::String
                && preview == &witness.preview
                && matches!(
                    self.graph.nodes.get(default),
                    Some(Node::Const {
                        value: ir::Value::Null
                    })
                )
                && self.parser_input_nodes.get(&witness.feed) == Some(input)
                && self.optional_parser_input_names.get(input) == Some(name)
                && !self.fn_nodes.contains_key(&witness.parameter)
        } else {
            matches!(self.graph.nodes.get(input), Some(Node::RuntimeParameter { name, ty, preview })
                if name == &witness.name && *ty == ScalarType::String && preview == &witness.preview)
                && self.fn_nodes.get(&witness.parameter) == Some(input)
        }
    }

    fn physical_json_string(&self, output: u32, nullable: bool) -> Option<SourcePath> {
        let source = *self.sources.first()?;
        if source.format != ComponentFormat::Json
            || !source.is_source
            || source.is_variable
            || source.is_pass_through
            || source.schema.repeating
            || !source.options.json_document
            || source.options.local_xml_file_set
            || source.options.json_schema_unresolved_reference.is_some()
            || source.dynamic_json.is_some()
            || !source.unambiguous_declared_output(output)
            || self
                .sources
                .iter()
                .filter(|source| source.declares_output(output))
                .count()
                != 1
            || self
                .intermediates
                .iter()
                .any(|source| source.declares_output(output))
            || self
                .fn_components
                .iter()
                .any(|function| function.declared_output_keys.contains(&output))
            || self.udf_by_output.contains_key(&output)
            || self.json_parsers.iter().any(|parser| {
                parser.outputs.contains_key(&output) || parser.object_outputs.contains_key(&output)
            })
            || self.source_node_functions.contains_key(&output)
            || source
                .output_keys
                .iter()
                .any(|key| self.has_source_node_functions(*key))
        {
            return None;
        }
        let path = source.ports.get(&output)?.clone();
        let leaf = schema_node_at(&source.schema, &path)?;
        if !substitution_leaf(leaf, nullable) || path.is_empty() {
            return None;
        }
        for depth in 0..path.len() {
            let node = schema_node_at(&source.schema, &path[..depth])?;
            if !substitution_ancestor(node)
                || (node.repeating && !self.framed.contains(&path[..depth]))
            {
                return None;
            }
        }
        Some(SourcePath { source: 0, path })
    }

    fn exact_string_constant(&self, output: u32) -> bool {
        if constructed_parser_scalar::output_is_nonfunction_shadowed(output, self)
            || self.json_parsers.iter().any(|parser| {
                parser.outputs.contains_key(&output) || parser.object_outputs.contains_key(&output)
            })
        {
            return false;
        }
        let mut producers = self
            .fn_components
            .iter()
            .filter(|function| function.declared_output_keys.contains(&output));
        let Some(function) = producers.next() else {
            return false;
        };
        producers.next().is_none()
            && function.library == "core"
            && function.kind == 2
            && function.name == "constant"
            && function.bounded_scalar_pins_valid
            && function.outputs.as_slice() == [output]
            && function
                .constant
                .as_ref()
                .is_some_and(|(_, ty)| ty == "string")
    }
}

/// A singular plain JSON string leaf. A nullable substitution requires the
/// leaf's own value to admit explicit null; a non-null one accepts either.
pub(super) fn substitution_leaf(leaf: &SchemaNode, nullable: bool) -> bool {
    !leaf.repeating
        && matches!(
            leaf.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        )
        && matches!(
            leaf.effective_json_nullability(),
            Ok(JsonNullability::Occurrence { null }) if null || !nullable
        )
        && !leaf.json_any
        && leaf.default.is_none()
        && leaf.value_generation.is_none()
        && leaf.recursive_ref.is_none()
}

/// An ordinary JSON object, or array of objects, with no null at any level.
pub(super) fn substitution_ancestor(node: &SchemaNode) -> bool {
    matches!(node.kind, SchemaKind::Group { .. })
        && super::presence::json_plain_non_null(node)
        && !node.json_any
        && node.recursive_ref.is_none()
        && node.alternatives().is_empty()
        && node.dynamic_fields().is_none()
}
