//! Coalescing export of typed JSON string-parser field calls.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{Graph, Node, NodeId};

use super::json_serializer::entries_xml;
use super::schema::{GeneratedSibling, KeyAlloc, xml_escape};

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

struct Candidate {
    input: NodeId,
    schema_text: String,
    schema: SchemaNode,
    path: Vec<String>,
}

struct Group {
    input: NodeId,
    schema: SchemaNode,
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
) -> Exports {
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
        render_group(group, keys, uid, mfd_path, &mut exports);
    }
    exports
}

fn read_candidate(args: &[NodeId], graph: &Graph) -> Result<Candidate, String> {
    let [input, schema_node, path_node] = args else {
        return Err(format!("expected 3 inputs, found {}", args.len()));
    };
    let schema_text = literal_string(graph, *schema_node, "schema")?;
    if schema_text.len() > MAX_SCHEMA_DESCRIPTOR_BYTES {
        return Err("schema descriptor exceeds 1 MiB".to_string());
    }
    let schema = serde_json::from_str::<SchemaNode>(schema_text)
        .map_err(|_| "schema descriptor is invalid".to_string())?;
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
        path,
    })
}

fn render_group(
    group: Group,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    mfd_path: &Path,
    exports: &mut Exports,
) {
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
    exports.siblings.push(GeneratedSibling {
        path: mfd_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&schema_file),
        contents: format_json::json_schema::export(&group.schema),
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
         \t\t\t\t\t\t<json schema=\"{}\"/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(&group.schema.name),
        xml_escape(&schema_file),
    );
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
