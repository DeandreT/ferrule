use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{Node, NodeId};

use super::graph::GraphBuilder;
use super::schema::{SchemaComponent, parse_u32, schema_node_at};

pub(super) struct Recipe {
    pub(super) input: u32,
    pub(super) schema: SchemaNode,
    pub(super) unresolved_schema_reference: Option<String>,
    pub(super) outputs: BTreeMap<u32, Vec<String>>,
    pub(super) object_outputs: BTreeMap<u32, Vec<String>>,
    pub(super) runtime_object_outputs: BTreeSet<u32>,
    pub(super) runtime_scalar_outputs: BTreeSet<u32>,
    pub(super) required_string_inputs: Vec<super::constructed_parser_scalar::RequiredStringInput>,
    pub(super) optional_string_inputs: Vec<super::optional_parser_input::OptionalStringInput>,
}

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    schema: &SchemaComponent,
    connected_outputs: &BTreeSet<u32>,
) -> Result<Option<Recipe>, String> {
    let string_parser = component.descendants().any(|node| {
        node.has_tag_name("parameter") && node.attribute("usageKind") == Some("stringparse")
    });
    if !string_parser {
        return Ok(None);
    }

    let inputs = component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("inpkey")))
        .collect::<Vec<_>>();
    let [input] = inputs.as_slice() else {
        return Err("requires exactly one run-time string input".to_string());
    };
    if schema.schema.repeating {
        return Err("root arrays are not representable as scalar parser outputs".to_string());
    }

    let mut outputs = BTreeMap::new();
    let mut object_outputs = BTreeMap::new();
    let required_string_inputs =
        super::constructed_parser_scalar::required_string_inputs(*component);
    let optional_string_inputs = super::optional_parser_input::read(*component);
    for output in component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("outkey")))
        .filter(|output| connected_outputs.contains(output))
    {
        let path = schema
            .ports
            .get(&output)
            .cloned()
            .ok_or_else(|| format!("output port `{output}` has no JSON property path"))?;
        let Some(node) = schema_node_at(&schema.schema, &path) else {
            return Err(format!(
                "output port `{output}` path `{}` is absent from its JSON schema",
                path.join("/")
            ));
        };
        if has_repeating_ancestor(schema, &path) {
            return Err(format!(
                "output port `{output}` path `{}` crosses an array",
                path.join("/")
            ));
        }
        if ordinary_object(node) {
            if required_string_inputs.is_empty() && optional_string_inputs.is_empty() {
                return Err(format!(
                    "output port `{output}` is structural; object presence requires a declared global String input without a connected default",
                ));
            }
            object_outputs.insert(output, path);
            continue;
        }
        if !node.is_scalar() || path.is_empty() {
            return Err(format!(
                "output port `{output}` path `{}` is not scalar",
                path.join("/")
            ));
        }
        outputs.insert(output, path);
    }
    if outputs.is_empty() && object_outputs.is_empty() {
        return Err("has no supported property outputs".to_string());
    }

    Ok(Some(Recipe {
        input: *input,
        schema: schema.schema.clone(),
        unresolved_schema_reference: schema.options.json_schema_unresolved_reference.clone(),
        runtime_scalar_outputs: outputs
            .keys()
            .copied()
            .filter(|output| {
                super::constructed_parser_scalar::raw_parser_output_is_unique(*component, *output)
            })
            .collect(),
        runtime_object_outputs: object_outputs
            .keys()
            .copied()
            .filter(|output| {
                super::constructed_parser_scalar::raw_parser_output_is_unique(*component, *output)
            })
            .collect(),
        object_outputs,
        required_string_inputs,
        optional_string_inputs,
        outputs,
    }))
}

pub(super) fn ordinary_object(node: &SchemaNode) -> bool {
    matches!(node.kind, SchemaKind::Group { .. })
        && !node.repeating
        && !node.json_array_wrapper
        && !node.json_any
        && node.dynamic_fields().is_none()
        && node.alternatives().is_empty()
}

fn has_repeating_ancestor(schema: &SchemaComponent, path: &[String]) -> bool {
    (1..=path.len()).any(|length| {
        schema_node_at(&schema.schema, &path[..length]).is_some_and(|node| node.repeating)
    })
}

impl GraphBuilder<'_> {
    pub(super) fn json_parser_declares_object(&self, output: u32) -> bool {
        self.json_parsers
            .iter()
            .any(|recipe| recipe.object_outputs.contains_key(&output))
    }

    pub(super) fn json_parser_object_exists_node(&mut self, output: u32) -> Option<NodeId> {
        let mut recipes = self
            .json_parsers
            .iter()
            .filter(|recipe| recipe.object_outputs.contains_key(&output));
        let recipe = recipes.next()?;
        if recipes.next().is_some()
            || !recipe.runtime_object_outputs.contains(&output)
            || recipe.unresolved_schema_reference.is_some()
            || super::constructed_parser_scalar::output_is_shadowed(output, self)
            || !(super::constructed_parser_scalar::input_is_required_string(recipe, self)
                || super::optional_parser_input::input_is_optional_string(recipe, self))
        {
            return None;
        }
        let path = recipe.object_outputs.get(&output)?.clone();
        if !(0..=path.len()).all(|length| {
            schema_node_at(&recipe.schema, &path[..length]).is_some_and(ordinary_object)
        }) {
            return None;
        }
        if let Some(node) = self.json_parser_nodes.get(&output) {
            return Some(*node);
        }
        let schema = crate::json_parser_recipe::encode_schema(&recipe.schema, None).ok()?;
        let input_key = recipe.input;
        let path = serde_json::to_string(&path).ok()?;
        let input = self.parser_input_node(input_key)?;
        let schema = self.alloc(Node::Const {
            value: Value::String(schema),
        });
        let path = self.alloc(Node::Const {
            value: Value::String(path),
        });
        let node = self.alloc(Node::Call {
            function: "json_parse_object_exists".into(),
            args: vec![input, schema, path],
        });
        self.json_parser_nodes.insert(output, node);
        Some(node)
    }

    pub(super) fn json_parser_input(&self, output: u32) -> Option<u32> {
        self.json_parsers
            .iter()
            .find(|parser| parser.outputs.contains_key(&output))
            .map(|parser| parser.input)
    }

    pub(super) fn json_parser_node(&mut self, output: u32) -> Option<NodeId> {
        if let Some(node) = self.json_parser_nodes.get(&output) {
            return Some(*node);
        }
        let parser = self
            .json_parsers
            .iter()
            .find(|parser| parser.outputs.contains_key(&output))?;
        let path = parser.outputs.get(&output)?.clone();
        let schema = crate::json_parser_recipe::encode_schema(
            &parser.schema,
            parser.unresolved_schema_reference.as_deref(),
        )
        .ok()?;
        let input_key = parser.input;
        let path = serde_json::to_string(&path).ok()?;
        let input = self.parser_input_node(input_key)?;
        let schema = self.alloc(Node::Const {
            value: Value::String(schema),
        });
        let path = self.alloc(Node::Const {
            value: Value::String(path),
        });
        let node = self.alloc(Node::Call {
            function: "json_parse_field".to_string(),
            args: vec![input, schema, path],
        });
        self.json_parser_nodes.insert(output, node);
        Some(node)
    }
}
