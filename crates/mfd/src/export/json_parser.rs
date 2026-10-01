//! Coalescing export of typed JSON string-parser field calls.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{Graph, Node, NodeId};

use crate::MfdError;

use super::json_serializer::entries_xml;
use super::schema::{GeneratedSibling, KeyAlloc, unresolved_json_schema_attribute, xml_escape};

const MAX_COMPONENTS: usize = 1_024;
const MAX_OUTPUTS: usize = 4_096;
const MAX_PATH_DEPTH: usize = 256;
const MAX_PATH_DESCRIPTOR_BYTES: usize = 64 * 1024;
const MAX_SCHEMA_DESCRIPTOR_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub(super) struct Exports {
    handled: BTreeSet<NodeId>,
    pub(super) outputs: BTreeMap<NodeId, u32>,
    pub(super) inputs: Vec<(NodeId, u32)>,
    pub(super) components: String,
    pub(super) siblings: Vec<GeneratedSibling>,
}

impl Exports {
    pub(super) fn handles(&self, node: NodeId) -> bool {
        self.handled.contains(&node)
    }
}

/// Annotated recipes must be well formed before preflight can report a
/// compatibility result. Legacy unannotated parser calls keep best-effort
/// export behavior.
pub(super) fn validate_provenance(graph: &Graph) -> Result<(), MfdError> {
    for (&node, value) in &graph.nodes {
        let Node::Call { function, args } = value else {
            continue;
        };
        if function != "json_parse_field" {
            continue;
        }
        let Some(schema_node) = args.get(1) else {
            continue;
        };
        let Some(Node::Const {
            value: Value::String(schema_text),
        }) = graph.nodes.get(schema_node)
        else {
            continue;
        };
        if crate::json_parser_recipe::contains_metadata(schema_text) {
            read_candidate(args, graph).map_err(|reason| {
                MfdError::Unsupported(format!(
                    "JSON string parser node {node} has invalid unresolved-schema provenance: {reason}"
                ))
            })?;
        }
    }
    Ok(())
}

pub(super) fn ensure_provenance_emitted(
    graph: &Graph,
    emitted: &BTreeSet<NodeId>,
) -> Result<(), MfdError> {
    for (&node, value) in &graph.nodes {
        let Node::Call { function, args } = value else {
            continue;
        };
        if function != "json_parse_field" {
            continue;
        }
        let Some(schema_node) = args.get(1) else {
            continue;
        };
        let Some(Node::Const {
            value: Value::String(schema_text),
        }) = graph.nodes.get(schema_node)
        else {
            continue;
        };
        if crate::json_parser_recipe::contains_metadata(schema_text) && !emitted.contains(&node) {
            return Err(MfdError::Unsupported(format!(
                "JSON string parser node {node} with unresolved-schema provenance was not emitted"
            )));
        }
    }
    Ok(())
}

struct Candidate {
    input: NodeId,
    schema_text: String,
    schema: SchemaNode,
    unresolved_schema_reference: Option<String>,
    path: Vec<String>,
}

struct Group {
    input: NodeId,
    schema: SchemaNode,
    unresolved_schema_reference: Option<String>,
    first_node: NodeId,
    fields: BTreeMap<Vec<String>, Vec<NodeId>>,
}

pub(super) fn render(
    graph: &Graph,
    excluded: &BTreeSet<NodeId>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<Exports, MfdError> {
    let mut exports = Exports::default();
    let mut groups = BTreeMap::<(NodeId, String), Group>::new();
    for (&node, value) in &graph.nodes {
        let Node::Call { function, args } = value else {
            continue;
        };
        if function != "json_parse_field" || excluded.contains(&node) {
            continue;
        }
        exports.handled.insert(node);
        let candidate = match read_candidate(args, graph) {
            Ok(candidate) => candidate,
            Err(reason) => {
                warnings.push(format!(
                    "JSON string parser node {node} is unsupported: {reason}; skipped"
                ));
                continue;
            }
        };
        let key = (candidate.input, candidate.schema_text);
        let group = groups.entry(key).or_insert_with(|| Group {
            input: candidate.input,
            schema: candidate.schema,
            unresolved_schema_reference: candidate.unresolved_schema_reference,
            first_node: node,
            fields: BTreeMap::new(),
        });
        group.fields.entry(candidate.path).or_default().push(node);
    }

    if groups.len() > MAX_COMPONENTS {
        for group in groups.values().skip(MAX_COMPONENTS) {
            for nodes in group.fields.values() {
                for node in nodes {
                    warnings.push(format!(
                        "JSON string parser node {node} is unsupported: the project declares more than {MAX_COMPONENTS} distinct parser components; skipped"
                    ));
                }
            }
        }
    }
    for (_, group) in groups.into_iter().take(MAX_COMPONENTS) {
        if group.fields.len() > MAX_OUTPUTS {
            for nodes in group.fields.values() {
                for node in nodes {
                    warnings.push(format!(
                        "JSON string parser node {node} is unsupported: its component declares more than {MAX_OUTPUTS} scalar outputs; skipped"
                    ));
                }
            }
            continue;
        }
        let nodes = group.fields.values().flatten().copied().collect::<Vec<_>>();
        let provenance = group.unresolved_schema_reference.clone();
        let first_node = group.first_node;
        match render_group(group, keys, uid, mfd_path, &mut exports) {
            Ok(()) => {
                if let Some(reference) = provenance {
                    warnings.push(format!(
                        "JSON string parser node {first_node} uses a generated entry-tree schema because original schema `{reference}` was unavailable at import"
                    ));
                }
            }
            Err(error @ MfdError::SchemaFidelity(_)) => return Err(error),
            Err(reason) => {
                for node in nodes {
                    warnings.push(format!(
                        "JSON string parser node {node} is unsupported: {reason}; skipped"
                    ));
                }
            }
        }
    }
    Ok(exports)
}

fn read_candidate(args: &[NodeId], graph: &Graph) -> Result<Candidate, String> {
    let [input, schema_node, path_node] = args else {
        return Err(format!("expected 3 inputs, found {}", args.len()));
    };
    let schema_text = literal_string(graph, *schema_node, "schema")?;
    if schema_text.len() > MAX_SCHEMA_DESCRIPTOR_BYTES {
        return Err("schema descriptor exceeds 1 MiB".to_string());
    }
    let (schema, unresolved_schema_reference) =
        crate::json_parser_recipe::decode_schema(schema_text)?;
    if schema.repeating {
        return Err("root arrays are not representable as scalar parser outputs".to_string());
    }
    if !matches!(schema.kind, SchemaKind::Group { .. }) {
        return Err("schema root is not an object".to_string());
    }
    let path_text = literal_string(graph, *path_node, "field path")?;
    if path_text.len() > MAX_PATH_DESCRIPTOR_BYTES {
        return Err("field path descriptor exceeds 64 KiB".to_string());
    }
    let path = serde_json::from_str::<Vec<String>>(path_text)
        .map_err(|_| "field path descriptor is not a JSON string array".to_string())?;
    if path.is_empty() {
        return Err("field path cannot be empty".to_string());
    }
    if path.len() > MAX_PATH_DEPTH {
        return Err(format!(
            "field path `{}` exceeds {MAX_PATH_DEPTH} segments",
            path.join("/")
        ));
    }
    let field = schema_node_at(&schema, &path)
        .ok_or_else(|| format!("field path `{}` is absent from its schema", path.join("/")))?;
    if field.repeating || !matches!(field.kind, SchemaKind::Scalar { .. }) {
        return Err(format!("field path `{}` is not scalar", path.join("/")));
    }
    if (1..path.len())
        .any(|length| schema_node_at(&schema, &path[..length]).is_some_and(|node| node.repeating))
    {
        return Err(format!("field path `{}` crosses an array", path.join("/")));
    }
    Ok(Candidate {
        input: *input,
        schema_text: schema_text.to_string(),
        schema,
        unresolved_schema_reference,
        path,
    })
}

fn render_group(
    group: Group,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    mfd_path: &Path,
    exports: &mut Exports,
) -> Result<(), MfdError> {
    let schema_contents = super::json_schema_fidelity::render(
        &group.schema,
        &format!("JSON string parser node {}", group.first_node),
    )?;
    let input = keys.next();
    exports.inputs.push((group.input, input));
    let mut ports = BTreeMap::new();
    for (path, nodes) in &group.fields {
        let output = keys.next();
        ports.insert(path.clone(), output);
        for node in nodes {
            exports.outputs.insert(*node, output);
        }
    }
    let entries = entries_xml(&group.schema, &ports, "outkey", 10);
    let stem = mfd_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("mapping");
    let schema_file = format!("{stem}-json-parser-{}.schema.json", group.first_node);
    let unresolved_json_schema =
        unresolved_json_schema_attribute(group.unresolved_schema_reference.as_deref());
    exports.siblings.push(GeneratedSibling {
        path: mfd_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&schema_file),
        contents: schema_contents,
    });
    *uid += 1;
    let _ = write!(
        exports.components,
        "\t\t\t\t<component name=\"{}\" library=\"json\" uid=\"{uid}\" kind=\"31\">\n\
         \t\t\t\t\t<properties/>\n\
         \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"240\" rby=\"180\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root><header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" inpkey=\"{input}\" expanded=\"1\"><entry name=\"document\" expanded=\"1\"><entry name=\"root\" expanded=\"1\">\n\
         {entries}\
         \t\t\t\t\t\t\t</entry></entry></entry></root>\n\
         \t\t\t\t\t\t<parameter usageKind=\"stringparse\"/>\n\
         \t\t\t\t\t\t<json schema=\"{}\"{unresolved_json_schema}/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(&group.schema.name),
        xml_escape(&schema_file),
    );
    Ok(())
}

fn literal_string<'a>(graph: &'a Graph, node: NodeId, label: &str) -> Result<&'a str, String> {
    let Some(Node::Const {
        value: Value::String(value),
    }) = graph.nodes.get(&node)
    else {
        return Err(format!(
            "{label} descriptor node {node} is not a string literal"
        ));
    };
    Ok(value)
}

fn schema_node_at<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    path.iter()
        .try_fold(schema, |node, segment| node.child(segment))
}
