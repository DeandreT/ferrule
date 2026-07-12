//! Schema component and port-tree rendering for MFD export.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::FormatOptions;

use crate::MfdError;

/// Which MapForce component family a mapping side exports as.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum SideFormat {
    Xml,
    Json,
    Csv,
    Db,
}

pub(super) fn side_format(instance_path: &Option<String>) -> SideFormat {
    let ext = instance_path
        .as_deref()
        .and_then(|p| Path::new(p).extension())
        .and_then(|e| e.to_str())
        .map(str::to_lowercase);
    match ext.as_deref() {
        Some("json") => SideFormat::Json,
        Some("csv") | Some("txt") => SideFormat::Csv,
        Some("db") | Some("sqlite") | Some("sqlite3") => SideFormat::Db,
        _ => SideFormat::Xml,
    }
}

/// The datasource name a connection path registers under (its file stem).
pub(super) fn db_datasource_name(instance_path: Option<&str>) -> String {
    instance_path
        .and_then(|p| Path::new(p).file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("data")
        .to_string()
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Side {
    Source,
    Target,
}

impl Side {
    fn port_attr(self) -> &'static str {
        match self {
            Side::Source => "outkey",
            Side::Target => "inpkey",
        }
    }

    fn instance_attr(self) -> &'static str {
        match self {
            Side::Source => "inputinstance",
            Side::Target => "outputinstance",
        }
    }
}

/// Renders one schema component (and writes its schema sibling file, when
/// the family has one) for the source or target side of the design.
pub(super) fn schema_component_xml(
    schema: &SchemaNode,
    format: SideFormat,
    ports: &PortTree,
    side: Side,
    instance_path: Option<&str>,
    options: &FormatOptions,
    mfd_path: &Path,
) -> Result<String, MfdError> {
    let stem = mfd_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mapping");
    let dir = mfd_path.parent().unwrap_or(Path::new("."));
    let (uid, side_name, header, view) = match side {
        Side::Source => (2, "source", "", "<view rbx=\"300\" rby=\"400\"/>"),
        Side::Target => (
            3,
            "target",
            "<properties XSLTDefaultOutput=\"1\"/>\n\t\t\t\t\t",
            "<view ltx=\"700\" rbx=\"1000\" rby=\"400\"/>",
        ),
    };
    let attr = side.port_attr();
    let instance = instance_path
        .map(|p| format!(" {}=\"{}\"", side.instance_attr(), xml_escape(p)))
        .unwrap_or_default();

    let mut out = String::new();
    match format {
        SideFormat::Xml => {
            let schema_file = format!("{stem}-{side_name}.xsd");
            std::fs::write(dir.join(&schema_file), format_xml::xsd::export(schema))?;
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{}\" library=\"xml\" uid=\"{uid}\" kind=\"14\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                 {}\
                 \t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<document schema=\"{}\" instanceroot=\"{{}}{}\"{instance}/>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(&schema.name),
                ports.entries_xml(schema, attr, 9),
                xml_escape(&schema_file),
                xml_escape(&schema.name),
            );
        }
        SideFormat::Json => {
            let schema_file = format!("{stem}-{side_name}.schema.json");
            std::fs::write(
                dir.join(&schema_file),
                format_json::json_schema::export(schema),
            )?;
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{}\" library=\"json\" uid=\"{uid}\" kind=\"31\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t\t<entry name=\"root\" expanded=\"1\">\n\
                 {}\
                 \t\t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<json schema=\"{}\"{instance}/>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(&schema.name),
                ports.json_entries_xml(schema, attr, 10),
                xml_escape(&schema_file),
            );
        }
        SideFormat::Db => {
            // Unlike a csv row schema, a table root is repeating by
            // format-db convention; only the children's shape matters.
            let fields = flat_fields(schema).ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "the {side_name} side maps to a database table but its schema \
                     is not a flat group of scalar fields"
                ))
            })?;
            let datasource = db_datasource_name(instance_path);
            let table_key = ports.required_key_for_abs(&[], "database table")?;
            let mut column_entries = String::new();
            for (column, _) in &fields {
                let key =
                    ports.required_key_for_abs(&[(*column).to_string()], "database column")?;
                let _ = writeln!(
                    column_entries,
                    "\t\t\t\t\t\t\t\t\t\t<entry name=\"{}\" {attr}=\"{key}\"/>",
                    xml_escape(column)
                );
            }
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{0}\" library=\"db\" uid=\"{uid}\" kind=\"15\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t<entry name=\"{0}\" type=\"table\" {attr}=\"{table_key}\" expanded=\"1\">\n\
                 {column_entries}\
                 \t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<database ref=\"{1}\">\n\
                 \t\t\t\t\t\t\t<data><selections><selection><PathElement Name=\"main\" Kind=\"Database\"/><PathElement Name=\"{0}\" Kind=\"Table\"/></selection></selections></data>\n\
                 \t\t\t\t\t\t</database>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(&schema.name),
                xml_escape(&datasource),
            );
        }
        SideFormat::Csv => {
            let fields = csv_fields(schema).ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "the {side_name} side maps to a csv file but its schema is \
                     not a flat group of scalar fields"
                ))
            })?;
            let block_key = ports.required_key_for_abs(&[], "CSV row block")?;
            let mut field_entries = String::new();
            let mut field_decls = String::new();
            for (i, (name, ty)) in fields.iter().enumerate() {
                let key = ports.required_key_for_abs(&[(*name).to_string()], "CSV field")?;
                let _ = writeln!(
                    field_entries,
                    "\t\t\t\t\t\t\t\t\t\t<entry name=\"{}\" {attr}=\"{key}\"/>",
                    xml_escape(name)
                );
                let _ = writeln!(
                    field_decls,
                    "\t\t\t\t\t\t\t\t<field{i} name=\"{}\" type=\"{}\"/>",
                    xml_escape(name),
                    csv_type_name(*ty)
                );
            }
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{}\" library=\"text\" uid=\"{uid}\" kind=\"16\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t\t<entry name=\"Rows\" {attr}=\"{block_key}\" expanded=\"1\">\n\
                 {field_entries}\
                 \t\t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<text type=\"csv\"{instance}>\n\
                 \t\t\t\t\t\t\t<settings separator=\"{}\" quote=\"&quot;\" firstrownames=\"{}\">\n\
                 \t\t\t\t\t\t\t\t<names root=\"{}\" block=\"Rows\">\n\
                 {field_decls}\
                 \t\t\t\t\t\t\t\t</names>\n\
                 \t\t\t\t\t\t\t</settings>\n\
                 \t\t\t\t\t\t</text>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(&schema.name),
                xml_escape(&options.delimiter.unwrap_or(',').to_string()),
                options.has_header_row.unwrap_or(true),
                xml_escape(&schema.name),
            );
        }
    }
    Ok(out)
}

/// The flat scalar fields a csv component needs, or `None` when the schema
/// has any other shape.
fn csv_fields(schema: &SchemaNode) -> Option<Vec<(&str, ScalarType)>> {
    if schema.repeating {
        return None;
    }
    flat_fields(schema)
}

/// The scalar children of a flat group, ignoring the root's own
/// repetition (db tables repeat by convention).
fn flat_fields(schema: &SchemaNode) -> Option<Vec<(&str, ScalarType)>> {
    match &schema.kind {
        SchemaKind::Group { children } => children
            .iter()
            .map(|c| match &c.kind {
                SchemaKind::Scalar { ty } if !c.repeating && !c.attribute => {
                    Some((c.name.as_str(), *ty))
                }
                _ => None,
            })
            .collect(),
        SchemaKind::Scalar { .. } => None,
    }
}

fn csv_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
    }
}

pub(super) struct KeyAlloc {
    pub(super) next: u32,
}

impl KeyAlloc {
    pub(super) fn next(&mut self) -> u32 {
        let key = self.next;
        self.next += 1;
        key
    }
}

/// Port keys assigned to every node of a schema, addressable by absolute
/// path.
pub(super) struct PortTree {
    by_abs: BTreeMap<Vec<String>, u32>,
}

impl PortTree {
    pub(super) fn build(schema: &SchemaNode, keys: &mut KeyAlloc) -> Self {
        let mut by_abs = BTreeMap::new();
        // The document root itself: rendered as a port only by row/array
        // shaped components (a csv block, a json root object).
        by_abs.insert(Vec::new(), keys.next());
        fn walk(
            node: &SchemaNode,
            path: &mut Vec<String>,
            keys: &mut KeyAlloc,
            by_abs: &mut BTreeMap<Vec<String>, u32>,
        ) {
            if let SchemaKind::Group { children } = &node.kind {
                for child in children {
                    path.push(child.name.clone());
                    if child.text {
                        let parent = &path[..path.len() - 1];
                        let key = by_abs[parent];
                        by_abs.insert(path.clone(), key);
                        path.pop();
                        continue;
                    }
                    by_abs.insert(path.clone(), keys.next());
                    walk(child, path, keys, by_abs);
                    path.pop();
                }
            }
        }
        walk(schema, &mut Vec::new(), keys, &mut by_abs);
        Self { by_abs }
    }

    pub(super) fn key_for_abs(&self, abs: &[String]) -> Option<u32> {
        self.by_abs.get(abs).copied()
    }

    fn required_key_for_abs(&self, abs: &[String], kind: &str) -> Result<u32, MfdError> {
        self.key_for_abs(abs).ok_or_else(|| {
            let path = if abs.is_empty() {
                "<root>".to_string()
            } else {
                abs.join("/")
            };
            MfdError::Unsupported(format!("internal {kind} port `{path}` was not allocated"))
        })
    }

    /// Finds the (first) absolute path ending in `suffix`. SourceField
    /// paths are absolute paths cut at some enclosing iteration frame, so
    /// tail matching recovers a plausible port; with several candidates
    /// this is best-effort (`BTreeMap` order decides).
    pub(super) fn key_for_suffix(&self, suffix: &[String]) -> Option<u32> {
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
                for child in children.iter().filter(|child| !child.text) {
                    path.push(child.name.clone());
                    let pad = "\t".repeat(indent);
                    let key = by_abs[&*path];
                    let type_attr = if child.attribute {
                        " type=\"attribute\""
                    } else {
                        ""
                    };
                    let _ = write!(
                        out,
                        "{pad}<entry name=\"{}\"{type_attr} {attr}=\"{key}\" expanded=\"1\"",
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
        let root_port = schema.text_child().map_or_else(String::new, |_| {
            let key = self.by_abs[&Vec::<String>::new()];
            format!(" {attr}=\"{key}\"")
        });
        let _ = writeln!(
            out,
            "{pad}<entry name=\"{}\"{root_port} expanded=\"1\">",
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

    /// Entry-tree XML for a json component, mirroring MapForce's
    /// normalized shape (and the importer's inverse): property entries
    /// carry `type="json-property"`, structural `object`/`array`/`item`
    /// entries carry the keys -- object/iteration keys on `object`, scalar
    /// keys on the type leaf.
    fn json_entries_xml(&self, schema: &SchemaNode, attr: &str, indent: usize) -> String {
        let mut out = String::new();
        if schema.repeating {
            let pad = "\t".repeat(indent);
            let _ = writeln!(out, "{pad}<entry name=\"array\" expanded=\"1\">");
            let _ = writeln!(
                out,
                "{pad}\t<entry name=\"item\" type=\"json-item\" expanded=\"1\">"
            );
            self.json_value_xml(schema, &mut Vec::new(), attr, indent + 2, &mut out);
            let _ = writeln!(out, "{pad}\t</entry>");
            let _ = writeln!(out, "{pad}</entry>");
        } else {
            self.json_value_xml(schema, &mut Vec::new(), attr, indent, &mut out);
        }
        out
    }

    /// Renders the value shape of `node` (its own repetition is the
    /// caller's concern).
    fn json_value_xml(
        &self,
        node: &SchemaNode,
        path: &mut Vec<String>,
        attr: &str,
        indent: usize,
        out: &mut String,
    ) {
        let pad = "\t".repeat(indent);
        let key = self.by_abs[&*path];
        match &node.kind {
            SchemaKind::Scalar { ty } => {
                let _ = writeln!(
                    out,
                    "{pad}<entry name=\"{}\" {attr}=\"{key}\"/>",
                    json_type_name(*ty)
                );
            }
            SchemaKind::Group { children } => {
                let _ = writeln!(
                    out,
                    "{pad}<entry name=\"object\" {attr}=\"{key}\" expanded=\"1\">"
                );
                for child in children {
                    let _ = writeln!(
                        out,
                        "{pad}\t<entry name=\"{}\" type=\"json-property\" expanded=\"1\">",
                        xml_escape(&child.name)
                    );
                    path.push(child.name.clone());
                    if child.repeating {
                        let _ = writeln!(out, "{pad}\t\t<entry name=\"array\" expanded=\"1\">");
                        let _ = writeln!(
                            out,
                            "{pad}\t\t\t<entry name=\"item\" type=\"json-item\" expanded=\"1\">"
                        );
                        self.json_value_xml(child, path, attr, indent + 4, out);
                        let _ = writeln!(out, "{pad}\t\t\t</entry>");
                        let _ = writeln!(out, "{pad}\t\t</entry>");
                    } else {
                        self.json_value_xml(child, path, attr, indent + 2, out);
                    }
                    path.pop();
                    let _ = writeln!(out, "{pad}\t</entry>");
                }
                let _ = writeln!(out, "{pad}</entry>");
            }
        }
    }
}

fn json_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
    }
}

pub(super) fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
