//! Canonical JSON string-serializer components reconstructed from the
//! internal typed object-construction call.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{Graph, Node, NodeId};

use super::schema::{GeneratedSibling, KeyAlloc, xml_escape};

const MAX_FIELDS: usize = 4_096;
const MAX_PATH_DEPTH: usize = 256;
const MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;

pub(super) struct Rendered {
    pub(super) xml: String,
    pub(super) output: u32,
    pub(super) inputs: Vec<(NodeId, u32)>,
    pub(super) sibling: GeneratedSibling,
}

struct Field {
    path: Vec<String>,
    value: NodeId,
}

pub(super) fn render(
    node: NodeId,
    args: &[NodeId],
    graph: &Graph,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    mfd_path: &Path,
) -> Result<Rendered, String> {
    if args.is_empty() || !args.len().is_multiple_of(3) {
        return Err("expected one or more path, scalar type, and value triples".to_string());
    }
    let field_count = args.len() / 3;
    if field_count > MAX_FIELDS {
        return Err(format!("declares more than {MAX_FIELDS} scalar properties"));
    }

    let mut schema = SchemaNode::group(format!("JsonObject{node}"), Vec::new());
    let mut fields = Vec::with_capacity(field_count);
    let (triples, remainder) = args.as_chunks::<3>();
    debug_assert!(remainder.is_empty());
    for triple in triples {
        let path_descriptor = literal_string(graph, triple[0], "property path")?;
        if path_descriptor.len() > MAX_DESCRIPTOR_BYTES {
            return Err("property path descriptor exceeds 64 KiB".to_string());
        }
        let path = serde_json::from_str::<Vec<String>>(path_descriptor)
            .map_err(|_| "property path descriptor is not a JSON string array".to_string())?;
        if path.is_empty() {
            return Err("property paths cannot be empty".to_string());
        }
        if path.len() > MAX_PATH_DEPTH {
            return Err(format!(
                "property path `{}` exceeds {MAX_PATH_DEPTH} segments",
                path.join("/")
            ));
        }
        let scalar_type_descriptor = literal_string(graph, triple[1], "scalar type")?;
        if scalar_type_descriptor.len() > MAX_DESCRIPTOR_BYTES {
            return Err("scalar type descriptor exceeds 64 KiB".to_string());
        }
        let scalar_type = match scalar_type_descriptor {
            "string" => ScalarType::String,
            "integer" => ScalarType::Int,
            "number" => ScalarType::Float,
            "boolean" => ScalarType::Bool,
            other => return Err(format!("scalar type `{other}` is unsupported")),
        };
        insert_field(&mut schema, &path, scalar_type)?;
        fields.push(Field {
            path,
            value: triple[2],
        });
    }

    let output = keys.next();
    let mut inputs_by_path = BTreeMap::new();
    let inputs = fields
        .iter()
        .map(|field| {
            let input = keys.next();
            inputs_by_path.insert(field.path.clone(), input);
            (field.value, input)
        })
        .collect::<Vec<_>>();
    let entries = entries_xml(&schema, &inputs_by_path, "inpkey", 10);

    let stem = mfd_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("mapping");
    let schema_file = format!("{stem}-json-serializer-{node}.schema.json");
    let sibling = GeneratedSibling {
        path: mfd_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&schema_file),
        contents: format_json::json_schema::export(&schema),
    };
    *uid += 1;
    let xml = format!(
        "\t\t\t\t<component name=\"{}\" library=\"json\" uid=\"{uid}\" kind=\"31\">\n\
         \t\t\t\t\t<properties/>\n\
         \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"240\" rby=\"180\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root><header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" outkey=\"{output}\" expanded=\"1\"><entry name=\"document\" expanded=\"1\"><entry name=\"root\" expanded=\"1\">\n\
         {entries}\
         \t\t\t\t\t\t\t</entry></entry></entry></root>\n\
         \t\t\t\t\t\t<parameter usageKind=\"stringserialize\"/>\n\
         \t\t\t\t\t\t<json schema=\"{}\"/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(&schema.name),
        xml_escape(&schema_file),
    );
    Ok(Rendered {
        xml,
        output,
        inputs,
        sibling,
    })
}

fn literal_string<'a>(graph: &'a Graph, node: NodeId, kind: &str) -> Result<&'a str, String> {
    let Some(Node::Const {
        value: Value::String(value),
    }) = graph.nodes.get(&node)
    else {
        return Err(format!(
            "{kind} descriptor node {node} is not a string literal"
        ));
    };
    Ok(value)
}

fn insert_field(
    schema: &mut SchemaNode,
    path: &[String],
    scalar_type: ScalarType,
) -> Result<(), String> {
    let Some((name, rest)) = path.split_first() else {
        return Err("property paths cannot be empty".to_string());
    };
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return Err(format!(
            "property path `{}` conflicts with a scalar property",
            path.join("/")
        ));
    };
    if rest.is_empty() {
        if children.iter().any(|child| child.name == *name) {
            return Err(format!("property path `{}` is duplicated", path.join("/")));
        }
        children.push(SchemaNode::scalar(name, scalar_type));
        return Ok(());
    }
    let child = match children.iter_mut().find(|child| child.name == *name) {
        Some(child) if matches!(child.kind, SchemaKind::Group { .. }) => child,
        Some(_) => {
            return Err(format!(
                "property path `{}` conflicts with a scalar property",
                path.join("/")
            ));
        }
        None => {
            children.push(SchemaNode::group(name, Vec::new()));
            children.last_mut().ok_or_else(|| {
                "JSON serializer property tree could not be constructed".to_string()
            })?
        }
    };
    insert_field(child, rest, scalar_type)
}

pub(super) fn entries_xml(
    schema: &SchemaNode,
    ports: &BTreeMap<Vec<String>, u32>,
    attr: &str,
    indent: usize,
) -> String {
    let mut out = String::new();
    render_value(schema, &mut Vec::new(), ports, attr, indent, &mut out);
    out
}

fn render_value(
    schema: &SchemaNode,
    path: &mut Vec<String>,
    ports: &BTreeMap<Vec<String>, u32>,
    attr: &str,
    indent: usize,
    out: &mut String,
) {
    let pad = "\t".repeat(indent);
    if schema.repeating {
        let _ = writeln!(out, "{pad}<entry name=\"array\" expanded=\"1\">");
        let _ = writeln!(
            out,
            "{pad}\t<entry name=\"item\" type=\"json-item\" expanded=\"1\">"
        );
        render_shape(schema, path, ports, attr, indent + 2, out);
        let _ = writeln!(out, "{pad}\t</entry>");
        let _ = writeln!(out, "{pad}</entry>");
    } else {
        render_shape(schema, path, ports, attr, indent, out);
    }
}

fn render_shape(
    schema: &SchemaNode,
    path: &mut Vec<String>,
    ports: &BTreeMap<Vec<String>, u32>,
    attr: &str,
    indent: usize,
    out: &mut String,
) {
    let pad = "\t".repeat(indent);
    match &schema.kind {
        SchemaKind::Scalar { ty } => {
            let port = ports
                .get(path)
                .map(|key| format!(" {attr}=\"{key}\""))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "{pad}<entry name=\"{}\"{port}/>",
                scalar_type_name(*ty)
            );
        }
        SchemaKind::Group { children, .. } => {
            let _ = writeln!(out, "{pad}<entry name=\"object\" expanded=\"1\">");
            for child in children {
                let _ = writeln!(
                    out,
                    "{pad}\t<entry name=\"{}\" type=\"json-property\" expanded=\"1\">",
                    xml_escape(&child.name)
                );
                path.push(child.name.clone());
                render_value(child, path, ports, attr, indent + 2, out);
                path.pop();
                let _ = writeln!(out, "{pad}\t</entry>");
            }
            let _ = writeln!(out, "{pad}</entry>");
        }
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
