use std::collections::BTreeMap;

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{Node, NodeId};

use super::graph::GraphBuilder;
use super::schema::{SchemaComponent, parse_u32, schema_node_at};

pub(super) struct Recipe {
    input: u32,
    schema: SchemaNode,
    pub(super) outputs: BTreeMap<u32, Vec<String>>,
}

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    schema: &SchemaComponent,
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
    for output in component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("outkey")))
    {
        let path = schema
            .ports
            .get(&output)
            .cloned()
            .ok_or_else(|| format!("output port `{output}` has no JSON property path"))?;
        if path.is_empty() {
            return Err(format!("output port `{output}` is structural, not scalar"));
        }
        let Some(node) = schema_node_at(&schema.schema, &path) else {
            return Err(format!(
                "output port `{output}` path `{}` is absent from its JSON schema",
                path.join("/")
            ));
        };
        if !matches!(node.kind, SchemaKind::Scalar { .. }) {
            return Err(format!(
                "output port `{output}` path `{}` is not scalar",
                path.join("/")
            ));
        }
        if has_repeating_ancestor(schema, &path) {
            return Err(format!(
                "output port `{output}` path `{}` crosses an array",
                path.join("/")
            ));
        }
        outputs.insert(output, path);
    }
    if outputs.is_empty() {
        return Err("has no scalar property outputs".to_string());
    }

    Ok(Some(Recipe {
        input: *input,
        schema: schema.schema.clone(),
        outputs,
    }))
}

fn has_repeating_ancestor(schema: &SchemaComponent, path: &[String]) -> bool {
    (1..=path.len()).any(|length| {
        schema_node_at(&schema.schema, &path[..length]).is_some_and(|node| node.repeating)
    })
}

impl GraphBuilder<'_> {
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
        let schema = serde_json::to_string(&parser.schema).ok()?;
        let input_key = parser.input;
        let path = serde_json::to_string(&path).ok()?;
        let input = self
            .edge_from
            .get(&input_key)
            .copied()
            .and_then(|feed| self.value_node(feed))?;
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
