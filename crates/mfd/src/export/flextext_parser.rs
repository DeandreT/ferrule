//! Coalescing export of typed FlexText string-parser field calls.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{FlexCommand, FlexLineEnding, FlexTextLayout, Graph, Node, NodeId};

use super::schema::{GeneratedSibling, KeyAlloc, xml_escape};

const MAX_COMPONENTS: usize = 1_024;
const MAX_OUTPUTS: usize = 4_096;
const MAX_PATH_DEPTH: usize = 256;
const MAX_PATH_DESCRIPTOR_BYTES: usize = 64 * 1024;
const MAX_LAYOUT_DESCRIPTOR_BYTES: usize = 1024 * 1024;

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
    layout_text: String,
    layout: FlexTextLayout,
    path: Vec<String>,
}

struct Group {
    input: NodeId,
    layout: FlexTextLayout,
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
        if function != "flextext_parse_field" || excluded.contains(&node) {
            continue;
        }
        exports.handled.insert(node);
        let candidate = match read_candidate(args, graph) {
            Ok(candidate) => candidate,
            Err(reason) => {
                warnings.push(format!(
                    "FlexText string parser node {node} is unsupported: {reason}; skipped"
                ));
                continue;
            }
        };
        let key = (candidate.input, candidate.layout_text);
        let group = groups.entry(key).or_insert_with(|| Group {
            input: candidate.input,
            layout: candidate.layout,
            first_node: node,
            fields: BTreeMap::new(),
        });
        group.fields.entry(candidate.path).or_default().push(node);
    }

    if groups.len() > MAX_COMPONENTS {
        for group in groups.values().skip(MAX_COMPONENTS) {
            warn_group(
                group,
                &format!(
                    "the project declares more than {MAX_COMPONENTS} distinct parser components"
                ),
                warnings,
            );
        }
    }
    for (_, group) in groups.into_iter().take(MAX_COMPONENTS) {
        if group.fields.len() > MAX_OUTPUTS {
            warn_group(
                &group,
                &format!("its component declares more than {MAX_OUTPUTS} scalar outputs"),
                warnings,
            );
            continue;
        }
        if let Err(reason) = render_group(&group, keys, uid, mfd_path, &mut exports) {
            warn_group(&group, &reason, warnings);
        }
    }
    exports
}

fn read_candidate(args: &[NodeId], graph: &Graph) -> Result<Candidate, String> {
    let [input, layout_node, path_node] = args else {
        return Err(format!("expected 3 inputs, found {}", args.len()));
    };
    let layout_text = literal_string(graph, *layout_node, "layout")?;
    if layout_text.len() > MAX_LAYOUT_DESCRIPTOR_BYTES {
        return Err("layout descriptor exceeds 1 MiB".to_string());
    }
    let layout = serde_json::from_str::<FlexTextLayout>(layout_text)
        .map_err(|_| "layout descriptor is invalid".to_string())?;
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
    let schema = layout.schema();
    let field = schema_node_at(&schema, &path)
        .ok_or_else(|| format!("field path `{}` is absent from its layout", path.join("/")))?;
    if !matches!(field.kind, SchemaKind::Scalar { .. }) {
        return Err(format!("field path `{}` is not scalar", path.join("/")));
    }
    Ok(Candidate {
        input: *input,
        layout_text: layout_text.to_string(),
        layout,
        path,
    })
}

fn render_group(
    group: &Group,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    mfd_path: &Path,
    exports: &mut Exports,
) -> Result<(), String> {
    let inline_fixed_width = matches!(
        group.layout.command(),
        FlexCommand::FixedWidthRecords { .. }
    ) && group.layout.output_line_ending() == FlexLineEnding::Lf
        && !group.layout.write_bom();
    let config = if inline_fixed_width {
        None
    } else {
        Some(
            super::flextext::render_config(&group.layout, None, "string parser")
                .map_err(|error| error.to_string())?,
        )
    };

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
    *uid += 1;
    if inline_fixed_width {
        let block_output = keys.next();
        render_fixed_width(
            group,
            input,
            block_output,
            &ports,
            *uid,
            &mut exports.components,
        )?;
    } else {
        let stem = mfd_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("mapping");
        let config_file = format!("{stem}-flextext-parser-{}.mft", group.first_node);
        exports.siblings.push(GeneratedSibling {
            path: mfd_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&config_file),
            contents: config.ok_or_else(|| "missing configuration".to_string())?,
        });
        render_flextext(
            group,
            input,
            &ports,
            *uid,
            &config_file,
            &mut exports.components,
        );
    }
    Ok(())
}

fn render_fixed_width(
    group: &Group,
    input: u32,
    block_output: u32,
    ports: &BTreeMap<Vec<String>, u32>,
    uid: u32,
    output: &mut String,
) -> Result<(), String> {
    let FlexCommand::FixedWidthRecords {
        name,
        fields,
        fill_char,
        record_delimiters,
        treat_empty_as_absent,
    } = group.layout.command()
    else {
        return Err("layout is not a fixed-width record parser".to_string());
    };
    let mut entries = String::new();
    let mut declarations = String::new();
    for (index, field) in fields.iter().enumerate() {
        let path = vec![name.clone(), field.name().to_string()];
        let port = ports
            .get(&path)
            .map(|port| format!(" outkey=\"{port}\""))
            .unwrap_or_default();
        let _ = writeln!(
            entries,
            "\t\t\t\t\t\t\t\t\t\t<entry name=\"{}\"{port}/>",
            xml_escape(field.name())
        );
        let _ = writeln!(
            declarations,
            "\t\t\t\t\t\t\t\t<field{index} name=\"{}\" type=\"{}\" length=\"{}\"/>",
            xml_escape(field.name()),
            scalar_type(field.ty()),
            field.width().get()
        );
    }
    let _ = write!(
        output,
        "\t\t\t\t<component name=\"{}\" library=\"text\" uid=\"{uid}\" kind=\"16\">\n\
         \t\t\t\t\t<properties/>\n\
         \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"240\" rby=\"180\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root><header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" inpkey=\"{input}\" expanded=\"1\"><entry name=\"document\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<entry name=\"{}\" outkey=\"{block_output}\" expanded=\"1\">\n\
         {entries}\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry></entry></root>\n\
         \t\t\t\t\t\t<text type=\"flf\" encoding=\"1000\" byteorder=\"1\" byteordermark=\"0\">\n\
         \t\t\t\t\t\t\t<settings delimiter=\"{record_delimiters}\" fillchar=\"{}\" removeempty=\"{treat_empty_as_absent}\">\n\
         \t\t\t\t\t\t\t\t<names root=\"{}\" block=\"{}\">\n\
         {declarations}\
         \t\t\t\t\t\t\t\t</names>\n\
         \t\t\t\t\t\t\t</settings>\n\
         \t\t\t\t\t\t</text>\n\
         \t\t\t\t\t\t<parameter usageKind=\"stringparse\"/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(group.layout.root_name()),
        xml_escape(name),
        xml_escape(&fill_char.to_string()),
        xml_escape(group.layout.root_name()),
        xml_escape(name),
    );
    Ok(())
}

fn render_flextext(
    group: &Group,
    input: u32,
    ports: &BTreeMap<Vec<String>, u32>,
    uid: u32,
    config_file: &str,
    output: &mut String,
) {
    let entries = entries_xml(&group.layout.schema(), ports, 8);
    let _ = write!(
        output,
        "\t\t\t\t<component name=\"{}\" library=\"text\" uid=\"{uid}\" kind=\"16\">\n\
         \t\t\t\t\t<properties/>\n\
         \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"240\" rby=\"180\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root><header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" inpkey=\"{input}\" expanded=\"1\"><entry name=\"document\" expanded=\"1\">\n\
         {entries}\
         \t\t\t\t\t\t\t</entry></entry></root>\n\
         \t\t\t\t\t\t<text type=\"txt\" config=\"{}\" encoding=\"52\" byteorder=\"1\" byteordermark=\"{}\"/>\n\
         \t\t\t\t\t\t<parameter usageKind=\"stringparse\"/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(group.layout.root_name()),
        xml_escape(config_file),
        u8::from(group.layout.write_bom()),
    );
}

fn entries_xml(schema: &SchemaNode, ports: &BTreeMap<Vec<String>, u32>, indent: usize) -> String {
    fn render(
        node: &SchemaNode,
        path: &mut Vec<String>,
        ports: &BTreeMap<Vec<String>, u32>,
        indent: usize,
        output: &mut String,
    ) {
        let pad = "\t".repeat(indent);
        let port = ports
            .get(path)
            .map(|port| format!(" outkey=\"{port}\""))
            .unwrap_or_default();
        match &node.kind {
            SchemaKind::Scalar { .. } => {
                let _ = writeln!(
                    output,
                    "{pad}<entry name=\"{}\"{port}/>",
                    xml_escape(&node.name)
                );
            }
            SchemaKind::Group { children, .. } => {
                let _ = writeln!(
                    output,
                    "{pad}<entry name=\"{}\"{port} expanded=\"1\">",
                    xml_escape(&node.name)
                );
                for child in children {
                    path.push(child.name.clone());
                    render(child, path, ports, indent + 1, output);
                    path.pop();
                }
                let _ = writeln!(output, "{pad}</entry>");
            }
        }
    }

    let mut output = String::new();
    render(schema, &mut Vec::new(), ports, indent, &mut output);
    output
}

fn warn_group(group: &Group, reason: &str, warnings: &mut Vec<String>) {
    for nodes in group.fields.values() {
        for node in nodes {
            warnings.push(group_node_error(*node, reason));
        }
    }
}

fn group_node_error(node: NodeId, reason: &str) -> String {
    format!("FlexText string parser node {node} is unsupported: {reason}; skipped")
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

fn scalar_type(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
    }
}
