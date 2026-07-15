use std::collections::BTreeMap;

use ir::{SchemaKind, Value};
use mapping::{FlexTextLayout, Node, NodeId};

use super::graph::GraphBuilder;
use super::schema::{SchemaComponent, parse_u32, schema_node_at};

pub(super) struct Recipe {
    input: u32,
    layout: FlexTextLayout,
    pub(super) outputs: BTreeMap<u32, Vec<String>>,
}

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    schema: SchemaComponent,
) -> Result<Recipe, String> {
    let inputs = component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("inpkey")))
        .collect::<Vec<_>>();
    let [input] = inputs.as_slice() else {
        return Err("string parser requires exactly one run-time string input".to_string());
    };
    let layout = schema
        .options
        .flextext
        .ok_or_else(|| "compiled configuration has no runtime layout".to_string())?;
    let outputs = schema
        .ports
        .into_iter()
        .filter(|(key, _)| schema.output_keys.contains(key))
        .collect::<BTreeMap<_, _>>();
    if outputs.is_empty() {
        return Err("string parser has no output ports".to_string());
    }
    for (output, path) in &outputs {
        if !path.is_empty()
            && !schema_node_at(&schema.schema, path)
                .is_some_and(|node| matches!(node.kind, SchemaKind::Scalar { .. }))
        {
            return Err(format!(
                "output port `{output}` path `{}` is not scalar",
                path.join("/")
            ));
        }
    }
    Ok(Recipe {
        input: *input,
        layout,
        outputs,
    })
}

impl GraphBuilder<'_> {
    pub(super) fn flextext_parser_input(&self, output: u32) -> Option<u32> {
        self.flextext_parsers
            .iter()
            .find(|parser| parser.outputs.contains_key(&output))
            .map(|parser| parser.input)
    }

    pub(super) fn flextext_parser_node(&mut self, output: u32) -> Option<NodeId> {
        if let Some(node) = self.flextext_parser_nodes.get(&output) {
            return Some(*node);
        }
        let parser = self
            .flextext_parsers
            .iter()
            .find(|parser| parser.outputs.contains_key(&output))?;
        let path = parser.outputs.get(&output)?.clone();
        if path.is_empty() {
            return None;
        }
        let input_key = parser.input;
        let layout = serde_json::to_string(&parser.layout).ok()?;
        let path = serde_json::to_string(&path).ok()?;
        let input = self
            .edge_from
            .get(&input_key)
            .copied()
            .and_then(|feed| self.value_node(feed))?;
        let layout = self.alloc(Node::Const {
            value: Value::String(layout),
        });
        let path = self.alloc(Node::Const {
            value: Value::String(path),
        });
        let node = self.alloc(Node::Call {
            function: "flextext_parse_field".to_string(),
            args: vec![input, layout, path],
        });
        self.flextext_parser_nodes.insert(output, node);
        Some(node)
    }
}
