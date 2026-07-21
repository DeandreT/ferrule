use std::collections::BTreeMap;
use std::num::NonZeroU32;

use ir::{SchemaKind, Value};
use mapping::{FixedWidthRecordField, FlexCommand, FlexLineEnding, FlexTextLayout, Node, NodeId};

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
    let layout = schema
        .options
        .flextext
        .clone()
        .ok_or_else(|| "compiled configuration has no runtime layout".to_string())?;
    build_recipe(component, &schema, layout, schema.ports.clone())
}

pub(super) fn read_fixed_width(
    component: &roxmltree::Node<'_, '_>,
    schema: SchemaComponent,
) -> Result<Recipe, String> {
    let fixed = schema
        .options
        .fixed_width
        .as_ref()
        .ok_or_else(|| "compiled fixed-length parser has no runtime layout".to_string())?;
    let SchemaKind::Group { children, .. } = &schema.schema.kind else {
        return Err("fixed-length parser row schema is not a group".to_string());
    };
    if children.len() != fixed.field_widths().len() {
        return Err("fixed-length parser field declarations do not match its widths".to_string());
    }
    let fields = children
        .iter()
        .zip(fixed.field_widths())
        .map(|(field, width)| {
            let SchemaKind::Scalar { ty } = &field.kind else {
                return Err(format!(
                    "fixed-length parser field `{}` is not scalar",
                    field.name
                ));
            };
            let width = NonZeroU32::new(width.get()).ok_or_else(|| {
                format!("fixed-length parser field `{}` has zero width", field.name)
            })?;
            FixedWidthRecordField::new(&field.name, *ty, width)
                .map_err(|error| format!("invalid fixed-length parser field ({error})"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let block = schema
        .ports
        .iter()
        .find(|(key, path)| schema.output_keys.contains(key) && path.is_empty())
        .and_then(|(key, _)| {
            component.descendants().find(|entry| {
                entry.has_tag_name("entry") && parse_u32(entry.attribute("outkey")) == Some(*key)
            })
        })
        .and_then(|entry| entry.attribute("name"))
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "fixed-length string parser has no connected row output".to_string())?;
    let layout = FlexTextLayout::new(
        &schema.schema.name,
        FlexCommand::FixedWidthRecords {
            name: block.to_string(),
            fields,
            fill_char: fixed.fill_char(),
            record_delimiters: fixed.record_delimiters(),
            treat_empty_as_absent: fixed.treat_empty_as_absent(),
        },
        FlexLineEnding::Lf,
        false,
    )
    .map_err(|error| format!("invalid fixed-length parser layout ({error})"))?;
    let outputs = schema
        .ports
        .clone()
        .into_iter()
        .map(|(key, mut path)| {
            if !path.is_empty() {
                path.insert(0, block.to_string());
            }
            (key, path)
        })
        .collect();
    build_recipe(component, &schema, layout, outputs)
}

fn build_recipe(
    component: &roxmltree::Node<'_, '_>,
    schema: &SchemaComponent,
    layout: FlexTextLayout,
    ports: BTreeMap<u32, Vec<String>>,
) -> Result<Recipe, String> {
    let inputs = component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("inpkey")))
        .collect::<Vec<_>>();
    let [input] = inputs.as_slice() else {
        return Err("string parser requires exactly one run-time string input".to_string());
    };
    let outputs = ports
        .into_iter()
        .filter(|(key, _)| schema.output_keys.contains(key))
        .collect::<BTreeMap<_, _>>();
    if outputs.is_empty() {
        return Err("string parser has no output ports".to_string());
    }
    for (output, path) in &outputs {
        if !path.is_empty()
            && !schema_node_at(&layout.schema(), path)
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
