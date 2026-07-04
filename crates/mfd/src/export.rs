//! `mapping::Project` -> `.mfd` conversion for the supported subset, plus
//! generated XSDs next to the design so MapForce can resolve the schemas.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{Node, NodeId, Project, Scope};

use crate::MfdError;

/// Writes `project` as a MapForce design at `path` (plus
/// `<stem>-source.xsd` / `<stem>-target.xsd` siblings). Returns warnings
/// for the parts that have no `.mfd` representation and were skipped.
pub fn export(project: &Project, path: &Path) -> Result<Vec<String>, MfdError> {
    let mut warnings = Vec::new();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mapping")
        .to_string();
    let dir = path.parent().unwrap_or(Path::new("."));
    let source_xsd = format!("{stem}-source.xsd");
    let target_xsd = format!("{stem}-target.xsd");
    std::fs::write(
        dir.join(&source_xsd),
        format_xml::xsd::export(&project.source),
    )?;
    std::fs::write(
        dir.join(&target_xsd),
        format_xml::xsd::export(&project.target),
    )?;

    if !project.extra_sources.is_empty() {
        warnings.push(
            "extra sources are not exported; MapForce multi-input wiring must be redone"
                .to_string(),
        );
    }

    let mut keys = KeyAlloc { next: 1 };
    let source_ports = PortTree::build(&project.source, &mut keys);
    let target_ports = PortTree::build(&project.target, &mut keys);

    // Output key for each mapping node we can represent.
    let mut node_out_key: BTreeMap<NodeId, u32> = BTreeMap::new();
    let mut fn_inputs: BTreeMap<NodeId, Vec<u32>> = BTreeMap::new();
    let mut components = String::new();
    let mut uid = 100u32;
    for (&id, node) in &project.graph.nodes {
        match node {
            Node::SourceField { path } => match source_ports.key_for_suffix(path) {
                Some(key) => {
                    node_out_key.insert(id, key);
                }
                None => warnings.push(format!(
                    "source field `{}` matches no source leaf; its connections \
                         are skipped",
                    path.join("/")
                )),
            },
            Node::Lookup { .. } => warnings
                .push("lookup nodes have no simple MapForce equivalent; skipped".to_string()),
            Node::Const { value } => {
                let out = keys.next();
                node_out_key.insert(id, out);
                let (text, datatype) = constant_parts(value);
                uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"constant\" library=\"core\" uid=\"{uid}\" kind=\"2\">\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t\t<data><constant value=\"{}\" datatype=\"{datatype}\"/></data>\n\
                     \t\t\t\t</component>\n",
                    xml_escape(&text)
                );
            }
            Node::Call { function, args } => {
                let ins: Vec<u32> = args.iter().map(|_| keys.next()).collect();
                let out = keys.next();
                node_out_key.insert(id, out);
                fn_inputs.insert(id, ins.clone());
                uid += 1;
                let name = unmap_function_name(function);
                let mut pins = String::new();
                for (pos, key) in ins.iter().enumerate() {
                    let _ = write!(pins, "<datapoint pos=\"{pos}\" key=\"{key}\"/>");
                }
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"{}\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources>{pins}</sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n",
                    xml_escape(&name)
                );
            }
            Node::If { .. } => {
                let ins: Vec<u32> = (0..3).map(|_| keys.next()).collect();
                let out = keys.next();
                node_out_key.insert(id, out);
                fn_inputs.insert(id, ins.clone());
                uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"if-else\" library=\"core\" uid=\"{uid}\" kind=\"4\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{}\"/><datapoint pos=\"1\" key=\"{}\"/><datapoint pos=\"2\" key=\"{}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n",
                    ins[0], ins[1], ins[2]
                );
            }
            Node::ValueMap { table, default, .. } => {
                let input = keys.next();
                let out = keys.next();
                node_out_key.insert(id, out);
                fn_inputs.insert(id, vec![input]);
                uid += 1;
                let mut rows = String::new();
                for (from, to) in table {
                    let _ = write!(
                        rows,
                        "<entry from=\"{}\" to=\"{}\"/>",
                        xml_escape(&value_text(from)),
                        xml_escape(&value_text(to))
                    );
                }
                let default_attr = default
                    .as_ref()
                    .map(|d| format!(" defaultValue=\"{}\"", xml_escape(&value_text(d))))
                    .unwrap_or_default();
                let mode = if default.is_some() {
                    " defaultValueMode=\"custom\""
                } else {
                    ""
                };
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"value-map\" library=\"core\" uid=\"{uid}\" kind=\"23\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{input}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t\t<data><valuemap{mode}><valuemapTable>{rows}</valuemapTable>\
                     <input name=\"input\" type=\"string\"/><result name=\"result\" type=\"string\"{default_attr}/></valuemap></data>\n\
                     \t\t\t\t</component>\n"
                );
            }
        }
    }

    // Edges: function inputs, then scope iterations + filters, then bindings.
    let mut edges: Vec<(u32, u32)> = Vec::new();
    for (&id, node) in &project.graph.nodes {
        let Some(ins) = fn_inputs.get(&id) else {
            continue;
        };
        let args: Vec<NodeId> = match node {
            Node::Call { args, .. } => args.clone(),
            Node::If {
                condition,
                then,
                else_,
            } => vec![*condition, *then, *else_],
            Node::ValueMap { input, .. } => vec![*input],
            _ => continue,
        };
        for (i, arg) in args.iter().enumerate() {
            if let (Some(&from), Some(&to)) = (node_out_key.get(arg), ins.get(i)) {
                edges.push((from, to));
            }
        }
    }

    let mut filter_components = String::new();
    collect_scope_edges(
        &project.root,
        &mut Vec::new(),
        &mut Vec::new(),
        &source_ports,
        &target_ports,
        &node_out_key,
        &mut keys,
        &mut uid,
        &mut filter_components,
        &mut edges,
        &mut warnings,
    );
    components.push_str(&filter_components);

    let root_name = &project.source.name;
    let target_name = &project.target.name;
    let mut out = String::new();
    let _ = write!(
        out,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <mapping version=\"22\">\n\
         \t<resources/>\n\
         \t<component name=\"defaultmap\" uid=\"1\" editable=\"1\">\n\
         \t\t<properties SelectedLanguage=\"builtin\"/>\n\
         \t\t<structure>\n\
         \t\t\t<children>\n\
         \t\t\t\t<component name=\"{}\" library=\"xml\" uid=\"2\" kind=\"14\">\n\
         \t\t\t\t\t<view rbx=\"300\" rby=\"400\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
         {}\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t\t<document schema=\"{}\" instanceroot=\"{{}}{}\"/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(root_name),
        source_ports.entries_xml(&project.source, "outkey", 9),
        xml_escape(&source_xsd),
        xml_escape(root_name),
    );
    let _ = write!(
        out,
        "\t\t\t\t<component name=\"{}\" library=\"xml\" uid=\"3\" kind=\"14\">\n\
         \t\t\t\t\t<properties XSLTDefaultOutput=\"1\"/>\n\
         \t\t\t\t\t<view ltx=\"700\" rbx=\"1000\" rby=\"400\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
         {}\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t\t<document schema=\"{}\" instanceroot=\"{{}}{}\"/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(target_name),
        target_ports.entries_xml(&project.target, "inpkey", 9),
        xml_escape(&target_xsd),
        xml_escape(target_name),
    );
    out.push_str(&components);
    out.push_str(
        "\t\t\t</children>\n\t\t\t<graph directed=\"1\">\n\t\t\t\t<edges/>\n\t\t\t\t<vertices>\n",
    );
    let mut by_from: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (from, to) in edges {
        by_from.entry(from).or_default().push(to);
    }
    for (from, tos) in by_from {
        let _ = write!(
            out,
            "\t\t\t\t\t<vertex vertexkey=\"{from}\">\n\t\t\t\t\t\t<edges>\n"
        );
        for to in tos {
            let _ = writeln!(out, "\t\t\t\t\t\t\t<edge vertexkey=\"{to}\"/>");
        }
        out.push_str("\t\t\t\t\t\t</edges>\n\t\t\t\t\t</vertex>\n");
    }
    out.push_str(
        "\t\t\t\t</vertices>\n\t\t\t</graph>\n\t\t</structure>\n\t</component>\n</mapping>\n",
    );

    std::fs::write(path, out)?;
    Ok(warnings)
}

struct KeyAlloc {
    next: u32,
}

impl KeyAlloc {
    fn next(&mut self) -> u32 {
        let key = self.next;
        self.next += 1;
        key
    }
}

/// Port keys assigned to every node of a schema, addressable by absolute
/// path.
struct PortTree {
    by_abs: BTreeMap<Vec<String>, u32>,
}

impl PortTree {
    fn build(schema: &SchemaNode, keys: &mut KeyAlloc) -> Self {
        let mut by_abs = BTreeMap::new();
        fn walk(
            node: &SchemaNode,
            path: &mut Vec<String>,
            keys: &mut KeyAlloc,
            by_abs: &mut BTreeMap<Vec<String>, u32>,
        ) {
            if let SchemaKind::Group { children } = &node.kind {
                for child in children {
                    path.push(child.name.clone());
                    by_abs.insert(path.clone(), keys.next());
                    walk(child, path, keys, by_abs);
                    path.pop();
                }
            }
        }
        walk(schema, &mut Vec::new(), keys, &mut by_abs);
        Self { by_abs }
    }

    fn key_for_abs(&self, abs: &[String]) -> Option<u32> {
        self.by_abs.get(abs).copied()
    }

    /// Finds the (first) absolute path ending in `suffix`. SourceField
    /// paths are absolute paths cut at some enclosing iteration frame, so
    /// tail matching recovers a plausible port; with several candidates
    /// this is best-effort (`BTreeMap` order decides).
    fn key_for_suffix(&self, suffix: &[String]) -> Option<u32> {
        self.by_abs
            .iter()
            .find(|(abs, _)| abs.ends_with(suffix))
            .map(|(_, &k)| k)
    }

    /// Entry-tree XML for a schema with `attr` (outkey/inpkey) on every
    /// entry.
    fn entries_xml(&self, schema: &SchemaNode, attr: &str, indent: usize) -> String {
        let mut out = String::new();
        fn walk(
            node: &SchemaNode,
            path: &mut Vec<String>,
            attr: &str,
            indent: usize,
            by_abs: &BTreeMap<Vec<String>, u32>,
            out: &mut String,
        ) {
            if let SchemaKind::Group { children } = &node.kind {
                for child in children {
                    path.push(child.name.clone());
                    let pad = "\t".repeat(indent);
                    let key = by_abs[&*path];
                    let _ = write!(
                        out,
                        "{pad}<entry name=\"{}\" {attr}=\"{key}\" expanded=\"1\"",
                        xml_escape(&child.name)
                    );
                    if matches!(child.kind, SchemaKind::Scalar { .. }) {
                        out.push_str("/>\n");
                    } else {
                        out.push_str(">\n");
                        walk(child, path, attr, indent + 1, by_abs, out);
                        let _ = writeln!(out, "{pad}</entry>");
                    }
                    path.pop();
                }
            }
        }
        // The document root itself is one entry level wrapping the children.
        let pad = "\t".repeat(indent);
        let _ = writeln!(
            out,
            "{pad}<entry name=\"{}\" expanded=\"1\">",
            xml_escape(&schema.name)
        );
        walk(
            schema,
            &mut Vec::new(),
            attr,
            indent + 1,
            &self.by_abs,
            &mut out,
        );
        let _ = writeln!(out, "{pad}</entry>");
        out
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_scope_edges(
    scope: &Scope,
    chain: &mut Vec<String>,
    anchor: &mut Vec<String>,
    source_ports: &PortTree,
    target_ports: &PortTree,
    node_out_key: &BTreeMap<NodeId, u32>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    filter_components: &mut String,
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
) {
    let anchor_len = anchor.len();
    if let Some(source) = &scope.source
        && !chain.is_empty()
    {
        let mut abs = anchor.clone();
        abs.extend(source.iter().cloned());
        match (
            source_ports.key_for_abs(&abs),
            target_ports.key_for_abs(chain),
        ) {
            (Some(from), Some(to)) => {
                if let Some(filter) = scope.filter {
                    match node_out_key.get(&filter) {
                        Some(&bool_key_src) => {
                            // filter component: node+bool in, on-true out.
                            let in_node = keys.next();
                            let in_bool = keys.next();
                            let out_true = keys.next();
                            *uid += 1;
                            let _ = write!(
                                filter_components,
                                "\t\t\t\t<component name=\"filter\" library=\"core\" uid=\"{uid}\" kind=\"3\">\n\
                                 \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_node}\"/><datapoint pos=\"1\" key=\"{in_bool}\"/></sources>\n\
                                 \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_true}\"/><datapoint/></targets>\n\
                                 \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                                 \t\t\t\t</component>\n"
                            );
                            edges.push((from, in_node));
                            edges.push((bool_key_src, in_bool));
                            edges.push((out_true, to));
                        }
                        None => {
                            warnings.push(format!(
                                "scope `{}` filter references an unexported node; \
                                 filter dropped",
                                chain.join("/")
                            ));
                            edges.push((from, to));
                        }
                    }
                } else {
                    edges.push((from, to));
                }
                *anchor = abs;
            }
            _ => warnings.push(format!(
                "scope `{}` iterates `{}` which maps to no schema entry; skipped",
                chain.join("/"),
                source.join("/")
            )),
        }
    }
    for binding in &scope.bindings {
        let mut leaf = chain.clone();
        leaf.push(binding.target_field.clone());
        match (
            node_out_key.get(&binding.node),
            target_ports.key_for_abs(&leaf),
        ) {
            (Some(&from), Some(to)) => edges.push((from, to)),
            (None, _) => warnings.push(format!(
                "binding `{}` references an unexported node; skipped",
                leaf.join("/")
            )),
            (_, None) => warnings.push(format!(
                "binding `{}` matches no target entry; skipped",
                leaf.join("/")
            )),
        }
    }
    for child in &scope.children {
        chain.push(child.target_field.clone());
        collect_scope_edges(
            child,
            chain,
            anchor,
            source_ports,
            target_ports,
            node_out_key,
            keys,
            uid,
            filter_components,
            edges,
            warnings,
        );
        chain.pop();
    }
    anchor.truncate(anchor_len);
}

fn constant_parts(value: &Value) -> (String, &'static str) {
    match value {
        Value::Null => (String::new(), "string"),
        Value::Bool(b) => (b.to_string(), "boolean"),
        Value::Int(i) => (i.to_string(), "integer"),
        Value::Float(f) => (f.to_string(), "decimal"),
        Value::String(s) => (s.clone(), "string"),
    }
}

fn value_text(value: &Value) -> String {
    constant_parts(value).0
}

fn unmap_function_name(name: &str) -> String {
    match name {
        "not_equal" => "not-equal",
        "greater_than" => "greater",
        "less_than" => "less",
        "greater_or_equal" => "greater-equal",
        "less_or_equal" => "less-equal",
        "and" => "logical-and",
        "or" => "logical-or",
        "not" => "logical-not",
        "length" => "string-length",
        "starts_with" => "starts-with",
        "upper" => "upper-case",
        "lower" => "lower-case",
        other => other,
    }
    .to_string()
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
