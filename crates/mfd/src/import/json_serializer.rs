use ir::{ScalarType, SchemaKind, Value};
use mapping::{Node, NodeId};

use super::graph::GraphBuilder;
use super::schema::{SchemaComponent, parse_u32, schema_node_at};

pub(super) struct Recipe {
    pub(super) output: u32,
    fields: Vec<Field>,
}

#[derive(Clone)]
struct Field {
    input: u32,
    path: Vec<String>,
    scalar_type: ScalarType,
}

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    schema: &SchemaComponent,
) -> Result<Option<Recipe>, String> {
    let string_serializer = component.descendants().any(|node| {
        node.has_tag_name("parameter") && node.attribute("usageKind") == Some("stringserialize")
    });
    if !string_serializer {
        return Ok(None);
    }

    let outputs = component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("outkey")))
        .collect::<Vec<_>>();
    let [output] = outputs.as_slice() else {
        return Err("expected exactly one serialized string output".to_string());
    };

    let mut fields = Vec::new();
    for input in component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("inpkey")))
    {
        let path = schema
            .ports
            .get(&input)
            .cloned()
            .ok_or_else(|| format!("input port `{input}` has no JSON property path"))?;
        let Some(node) = schema_node_at(&schema.schema, &path) else {
            return Err(format!(
                "input port `{input}` path `{}` is absent from its JSON schema",
                path.join("/")
            ));
        };
        let SchemaKind::Scalar { ty } = node.kind else {
            return Err(format!(
                "input port `{input}` path `{}` is not scalar",
                path.join("/")
            ));
        };
        if has_repeating_ancestor(schema, &path) {
            return Err(format!(
                "input port `{input}` path `{}` crosses an array",
                path.join("/")
            ));
        }
        fields.push(Field {
            input,
            path,
            scalar_type: ty,
        });
    }
    if fields.is_empty() {
        return Err("has no scalar property inputs".to_string());
    }
    Ok(Some(Recipe {
        output: *output,
        fields,
    }))
}

fn has_repeating_ancestor(schema: &SchemaComponent, path: &[String]) -> bool {
    (1..=path.len()).any(|length| {
        schema_node_at(&schema.schema, &path[..length]).is_some_and(|node| node.repeating)
    })
}

impl GraphBuilder<'_> {
    pub(super) fn json_serializer_node(&mut self, output: u32) -> Option<NodeId> {
        if let Some(node) = self.json_serializer_nodes.get(&output) {
            return Some(*node);
        }
        let fields = self
            .json_serializers
            .iter()
            .find(|serializer| serializer.output == output)?
            .fields
            .clone();
        let mut args = Vec::with_capacity(fields.len() * 3);
        for field in fields {
            let path = serde_json::to_string(&field.path).ok()?;
            args.push(self.alloc(Node::Const {
                value: Value::String(path),
            }));
            args.push(self.alloc(Node::Const {
                value: Value::String(scalar_type_name(field.scalar_type).to_string()),
            }));
            args.push(
                self.edge_from
                    .get(&field.input)
                    .copied()
                    .and_then(|feed| self.value_node(feed))
                    .unwrap_or_else(|| self.const_null()),
            );
        }
        let node = self.alloc(Node::Call {
            function: "json_serialize_object".to_string(),
            args,
        });
        self.json_serializer_nodes.insert(output, node);
        Some(node)
    }
}

fn scalar_type_name(scalar_type: ScalarType) -> &'static str {
    match scalar_type {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
    }
}
