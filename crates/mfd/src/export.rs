//! `mapping::Project` -> `.mfd` conversion for the supported subset, plus
//! generated schema files (XSD / JSON Schema) next to the design so
//! MapForce can resolve them. The component family per side follows the
//! project's instance-path extension: `.json` becomes a json component,
//! `.csv`/`.txt` a csv text component, everything else (including no path
//! at all) an XML component.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{FormatOptions, Graph, Node, NodeId, Project, Scope, SequenceExpr};

use crate::MfdError;

/// Which MapForce component family a mapping side exports as.
#[derive(Clone, Copy, PartialEq)]
enum SideFormat {
    Xml,
    Json,
    Csv,
    Db,
}

fn side_format(instance_path: &Option<String>) -> SideFormat {
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
fn db_datasource_name(instance_path: Option<&str>) -> String {
    instance_path
        .and_then(|p| Path::new(p).file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("data")
        .to_string()
}

/// Writes `project` as a MapForce design at `path`, plus generated schema
/// siblings (`<stem>-source.xsd` / `.schema.json`, dito target) where the
/// component family needs one. Returns warnings for the parts that have no
/// `.mfd` representation and were skipped.
pub fn export(project: &Project, path: &Path) -> Result<Vec<String>, MfdError> {
    let mut warnings = Vec::new();

    if !project.extra_sources.is_empty() {
        warnings.push(
            "extra sources are not exported; MapForce multi-input wiring must be redone"
                .to_string(),
        );
    }

    let source_format = side_format(&project.source_path);
    let target_format = side_format(&project.target_path);

    let mut keys = KeyAlloc { next: 1 };
    let source_ports = PortTree::build(&project.source, &mut keys);
    let target_ports = PortTree::build(&project.target, &mut keys);

    // Output key for each mapping node we can represent.
    let mut node_out_key: BTreeMap<NodeId, u32> = BTreeMap::new();
    let mut fn_inputs: BTreeMap<NodeId, Vec<u32>> = BTreeMap::new();
    let mut position_inputs: BTreeMap<NodeId, u32> = BTreeMap::new();
    let mut components = String::new();
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut uid = 100u32;
    let mut sequence_inputs = Vec::new();
    let mut sequences = Vec::new();
    collect_sequences(&project.root, &mut sequences);
    for sequence in sequences {
        let first_key = keys.next();
        let second_key = keys.next();
        let out = keys.next();
        node_out_key.insert(sequence.item(), out);
        let name = match sequence {
            SequenceExpr::Tokenize {
                input, delimiter, ..
            } => {
                sequence_inputs.push((*input, first_key));
                sequence_inputs.push((*delimiter, second_key));
                "tokenize"
            }
            SequenceExpr::TokenizeByLength { input, length, .. } => {
                sequence_inputs.push((*input, first_key));
                sequence_inputs.push((*length, second_key));
                "tokenize-by-length"
            }
            SequenceExpr::Generate { from, to, .. } => {
                if let Some(from) = from {
                    sequence_inputs.push((*from, first_key));
                }
                sequence_inputs.push((*to, second_key));
                "generate-sequence"
            }
        };
        uid += 1;
        let _ = write!(
            components,
            "\t\t\t\t<component name=\"{name}\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
             \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{first_key}\"/><datapoint pos=\"1\" key=\"{second_key}\"/></sources>\n\
             \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
             \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
             \t\t\t\t</component>\n"
        );
    }
    for (&id, node) in &project.graph.nodes {
        match node {
            Node::SourceField { path, frame } => {
                if node_out_key.contains_key(&id) {
                    continue;
                }
                let mut absolute = frame.clone().unwrap_or_default();
                absolute.extend(path.iter().cloned());
                match source_ports.key_for_suffix(&absolute) {
                    Some(key) => {
                        node_out_key.insert(id, key);
                    }
                    None => warnings.push(format!(
                        "source field `{}` matches no source leaf; its connections \
                         are skipped",
                        absolute.join("/")
                    )),
                }
            }
            Node::Position { .. } => {
                let input = keys.next();
                let out = keys.next();
                node_out_key.insert(id, out);
                position_inputs.insert(id, input);
                uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"position\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{input}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
            }
            Node::Lookup { .. } => warnings
                .push("lookup nodes have no simple MapForce equivalent; skipped".to_string()),
            Node::Aggregate {
                function,
                collection,
                value,
                expression,
                arg,
            } => {
                let in_sequence = keys.next();
                let out = keys.next();
                let mut dynamic_inputs = Vec::new();
                if expression.is_some() {
                    dynamic_inputs.push(in_sequence);
                } else {
                    // A path-selected sequence wires straight to its source
                    // entry; computed sequences wire their graph expression.
                    let mut sequence = collection.clone();
                    sequence.extend(value.iter().cloned());
                    let Some(sequence_key) = source_ports.key_for_suffix(&sequence) else {
                        warnings.push(format!(
                            "aggregate over `{}` matches no source entry; its \
                             connections are skipped",
                            sequence.join("/")
                        ));
                        continue;
                    };
                    edges.push((sequence_key, in_sequence));
                }
                node_out_key.insert(id, out);
                let mut pins = format!("<datapoint/><datapoint pos=\"1\" key=\"{in_sequence}\"/>");
                if arg.is_some() {
                    let in_arg = keys.next();
                    dynamic_inputs.push(in_arg);
                    let _ = write!(pins, "<datapoint pos=\"2\" key=\"{in_arg}\"/>");
                }
                if !dynamic_inputs.is_empty() {
                    fn_inputs.insert(id, dynamic_inputs);
                }
                uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"{}\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources>{pins}</sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n",
                    aggregate_component_name(*function)
                );
            }
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
                let library = function_library(function);
                let mut pins = String::new();
                for (pos, key) in ins.iter().enumerate() {
                    let _ = write!(pins, "<datapoint pos=\"{pos}\" key=\"{key}\"/>");
                }
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"{}\" library=\"{library}\" uid=\"{uid}\" kind=\"5\">\n\
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
            Node::Aggregate {
                expression, arg, ..
            } => expression.iter().chain(arg).copied().collect(),
            _ => continue,
        };
        for (i, arg) in args.iter().enumerate() {
            if let (Some(&from), Some(&to)) = (node_out_key.get(arg), ins.get(i)) {
                edges.push((from, to));
            }
        }
    }
    for (node, input) in sequence_inputs {
        if let Some(&from) = node_out_key.get(&node) {
            edges.push((from, input));
        } else {
            warnings.push(format!(
                "sequence input references unexported node {node}; connection skipped"
            ));
        }
    }

    // A root-scope iteration is only representable when the target side
    // has a row/array-shaped document root to connect to.
    let target_root_iterable = matches!(target_format, SideFormat::Csv | SideFormat::Db)
        || (target_format == SideFormat::Json && project.target.repeating);
    let mut filter_components = String::new();
    let mut position_contexts: BTreeMap<NodeId, Option<u32>> = BTreeMap::new();
    collect_scope_edges(
        &project.root,
        &mut Vec::new(),
        &mut Vec::new(),
        &source_ports,
        &target_ports,
        target_root_iterable,
        &project.graph,
        &node_out_key,
        &position_inputs,
        &mut position_contexts,
        &mut keys,
        &mut uid,
        &mut filter_components,
        &mut edges,
        &mut warnings,
    );
    for (id, input) in &position_inputs {
        if !position_contexts.contains_key(id) {
            warnings.push(format!(
                "position node {id} has no matching iteration scope; its context input {input} is unconnected"
            ));
        }
    }
    components.push_str(&filter_components);

    // Database sides register their connection as a mapping-level
    // datasource, which the components reference by name.
    let mut datasources: Vec<(String, String)> = Vec::new();
    for (format, instance) in [
        (source_format, project.source_path.as_deref()),
        (target_format, project.target_path.as_deref()),
    ] {
        if format == SideFormat::Db
            && let Some(conn) = instance
        {
            let name = db_datasource_name(Some(conn));
            if !datasources.iter().any(|(n, _)| *n == name) {
                datasources.push((name, conn.to_string()));
            }
        }
    }
    let resources = if datasources.is_empty() {
        "\t<resources/>\n".to_string()
    } else {
        let mut r = String::from("\t<resources>\n\t\t<datasources>\n");
        for (name, conn) in &datasources {
            let _ = write!(
                r,
                "\t\t\t<datasource name=\"{0}\">\n\
                 \t\t\t\t<properties JDBCDriver=\"org.sqlite.JDBC\" JDBCDatabaseURL=\"jdbc:sqlite:{1}\" DBDataSource=\"{1}\" DBCatalog=\"main\"/>\n\
                 \t\t\t\t<database_connection database_kind=\"SQLite\" import_kind=\"SQLite\" ConnectionString=\"{1}\" name=\"{0}\" path=\"{0}\"/>\n\
                 \t\t\t</datasource>\n",
                xml_escape(name),
                xml_escape(conn),
            );
        }
        r.push_str("\t\t</datasources>\n\t</resources>\n");
        r
    };

    let mut out = String::new();
    let _ = write!(
        out,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <mapping version=\"22\">\n\
         {resources}\
         \t<component name=\"defaultmap\" uid=\"1\" editable=\"1\">\n\
         \t\t<properties SelectedLanguage=\"builtin\"/>\n\
         \t\t<structure>\n\
         \t\t\t<children>\n"
    );
    out.push_str(&schema_component_xml(
        &project.source,
        source_format,
        &source_ports,
        Side::Source,
        project.source_path.as_deref(),
        &project.source_options,
        path,
    )?);
    out.push_str(&schema_component_xml(
        &project.target,
        target_format,
        &target_ports,
        Side::Target,
        project.target_path.as_deref(),
        &project.target_options,
        path,
    )?);
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

fn collect_sequences<'a>(scope: &'a Scope, sequences: &mut Vec<&'a SequenceExpr>) {
    if let Some(sequence) = &scope.sequence {
        sequences.push(sequence);
    }
    for child in &scope.children {
        collect_sequences(child, sequences);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
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
fn schema_component_xml(
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
            let table_key = ports.key_for_abs(&[]).expect("root port always keyed");
            let mut column_entries = String::new();
            for (column, _) in &fields {
                let key = ports
                    .key_for_abs(&[(*column).to_string()])
                    .expect("column keyed");
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
            let block_key = ports.key_for_abs(&[]).expect("root port always keyed");
            let mut field_entries = String::new();
            let mut field_decls = String::new();
            for (i, (name, ty)) in fields.iter().enumerate() {
                let key = ports
                    .key_for_abs(&[(*name).to_string()])
                    .expect("field keyed");
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

#[allow(clippy::too_many_arguments)]
fn append_scope_controls(
    scope: &Scope,
    chain: &[String],
    source_collection: Option<&[String]>,
    graph: &Graph,
    node_out_key: &BTreeMap<NodeId, u32>,
    position_inputs: &BTreeMap<NodeId, u32>,
    position_contexts: &mut BTreeMap<NodeId, Option<u32>>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    components: &mut String,
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
    mut from: u32,
) -> u32 {
    if let Some(sort_by) = scope.sort_by {
        connect_position_roots(
            [sort_by],
            source_collection,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        match node_out_key.get(&sort_by) {
            Some(&key_src) => {
                let in_nodes = keys.next();
                let in_key = keys.next();
                let out_nodes = keys.next();
                let direction = if scope.sort_descending {
                    "descending"
                } else {
                    "ascending"
                };
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"sort\" library=\"core\" uid=\"{uid}\" kind=\"30\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/><datapoint pos=\"1\" key=\"{in_key}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_nodes}\"/></targets>\n\
                     \t\t\t\t\t<data><sort><collation/><key direction=\"{direction}\"/></sort></data>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_nodes));
                edges.push((key_src, in_key));
                from = out_nodes;
            }
            None => warnings.push(format!(
                "scope `{}` sort key references an unexported node; sorting dropped",
                chain.join("/")
            )),
        }
    }
    if let Some(filter) = scope.filter {
        connect_position_roots(
            [filter],
            source_collection,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        match node_out_key.get(&filter) {
            Some(&bool_key_src) => {
                let in_node = keys.next();
                let in_bool = keys.next();
                let out_true = keys.next();
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"filter\" library=\"core\" uid=\"{uid}\" kind=\"3\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_node}\"/><datapoint pos=\"1\" key=\"{in_bool}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_true}\"/><datapoint/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_node));
                edges.push((bool_key_src, in_bool));
                from = out_true;
            }
            None => warnings.push(format!(
                "scope `{}` filter references an unexported node; filter dropped",
                chain.join("/")
            )),
        }
    }
    if let Some(group_by) = scope.group_by {
        connect_position_roots(
            [group_by],
            source_collection,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        match node_out_key.get(&group_by) {
            Some(&key_src) => {
                let in_nodes = keys.next();
                let in_key = keys.next();
                let out_groups = keys.next();
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"group-by\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/><datapoint pos=\"1\" key=\"{in_key}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_groups}\"/><datapoint/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_nodes));
                edges.push((key_src, in_key));
                from = out_groups;
            }
            None => warnings.push(format!(
                "scope `{}` group-by key references an unexported node; grouping dropped",
                chain.join("/")
            )),
        }
    }
    if let Some(take) = scope.take {
        match node_out_key.get(&take) {
            Some(&count_src) => {
                let in_nodes = keys.next();
                let in_count = keys.next();
                let out_nodes = keys.next();
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"first-items\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/><datapoint pos=\"1\" key=\"{in_count}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_nodes}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_nodes));
                edges.push((count_src, in_count));
                from = out_nodes;
            }
            None => warnings.push(format!(
                "scope `{}` take count references an unexported node; item limit dropped",
                chain.join("/")
            )),
        }
    }
    from
}

fn graph_node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::Aggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::SourceField { .. } | Node::Position { .. } | Node::Const { .. } => Vec::new(),
    }
}

fn position_nodes_for_roots(
    roots: impl IntoIterator<Item = NodeId>,
    graph: &Graph,
) -> std::collections::BTreeSet<NodeId> {
    let mut pending: Vec<NodeId> = roots.into_iter().collect();
    let mut visited = std::collections::BTreeSet::new();
    let mut positions = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        match graph.nodes.get(&id) {
            Some(Node::Position { .. }) => {
                positions.insert(id);
            }
            Some(node) => pending.extend(graph_node_inputs(node)),
            None => {}
        }
    }
    positions
}

#[allow(clippy::too_many_arguments)]
fn connect_position_roots(
    roots: impl IntoIterator<Item = NodeId>,
    source_collection: Option<&[String]>,
    allow_empty: bool,
    from: u32,
    graph: &Graph,
    position_inputs: &BTreeMap<NodeId, u32>,
    position_contexts: &mut BTreeMap<NodeId, Option<u32>>,
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
) {
    let referenced = position_nodes_for_roots(roots, graph);
    for id in referenced {
        let Some(&input) = position_inputs.get(&id) else {
            continue;
        };
        let Some(Node::Position { collection }) = graph.nodes.get(&id) else {
            continue;
        };
        let matches_scope = if collection.is_empty() {
            allow_empty
        } else {
            source_collection.is_some_and(|source| source.ends_with(collection))
        };
        if !matches_scope {
            continue;
        }
        match position_contexts.get(&id).copied() {
            None => {
                position_contexts.insert(id, Some(from));
                edges.push((from, input));
            }
            Some(Some(existing)) if existing != from => {
                warnings.push(format!(
                    "position node {id} is used in multiple iteration stages or scopes; \
                     its first context connection was kept"
                ));
                position_contexts.insert(id, None);
            }
            Some(_) => {}
        }
    }
}

fn descendant_binding_roots(scope: &Scope, roots: &mut Vec<NodeId>) {
    roots.extend(scope.bindings.iter().map(|binding| binding.node));
    for child in &scope.children {
        descendant_binding_roots(child, roots);
    }
}

#[allow(clippy::too_many_arguments)]
fn connect_binding_positions(
    scope: &Scope,
    source_collection: Option<&[String]>,
    from: u32,
    graph: &Graph,
    position_inputs: &BTreeMap<NodeId, u32>,
    position_contexts: &mut BTreeMap<NodeId, Option<u32>>,
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
) {
    connect_position_roots(
        scope.bindings.iter().map(|binding| binding.node),
        source_collection,
        true,
        from,
        graph,
        position_inputs,
        position_contexts,
        edges,
        warnings,
    );

    // A nested binding can explicitly request an outer named collection.
    // Empty-path positions remain owned by the nested scope itself.
    let mut descendant_roots = Vec::new();
    for child in &scope.children {
        descendant_binding_roots(child, &mut descendant_roots);
    }
    connect_position_roots(
        descendant_roots,
        source_collection,
        false,
        from,
        graph,
        position_inputs,
        position_contexts,
        edges,
        warnings,
    );
}

#[allow(clippy::too_many_arguments)]
fn collect_scope_edges(
    scope: &Scope,
    chain: &mut Vec<String>,
    anchor: &mut Vec<String>,
    source_ports: &PortTree,
    target_ports: &PortTree,
    target_root_iterable: bool,
    graph: &Graph,
    node_out_key: &BTreeMap<NodeId, u32>,
    position_inputs: &BTreeMap<NodeId, u32>,
    position_contexts: &mut BTreeMap<NodeId, Option<u32>>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    filter_components: &mut String,
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
) {
    let anchor_len = anchor.len();
    if let Some(sequence) = &scope.sequence {
        if chain.is_empty() && !target_root_iterable {
            warnings.push(
                "the root scope generates rows but the target document is not row/array \
                 shaped in MapForce terms; the iteration wire is skipped"
                    .to_string(),
            );
        } else {
            match (
                node_out_key.get(&sequence.item()),
                target_ports.key_for_abs(chain),
            ) {
                (Some(&from), Some(to)) => {
                    let from = append_scope_controls(
                        scope,
                        chain,
                        None,
                        graph,
                        node_out_key,
                        position_inputs,
                        position_contexts,
                        keys,
                        uid,
                        filter_components,
                        edges,
                        warnings,
                        from,
                    );
                    connect_binding_positions(
                        scope,
                        None,
                        from,
                        graph,
                        position_inputs,
                        position_contexts,
                        edges,
                        warnings,
                    );
                    edges.push((from, to));
                }
                (None, _) => warnings.push(format!(
                    "scope `{}` sequence item references an unexported node; skipped",
                    chain.join("/")
                )),
                (_, None) => warnings.push(format!(
                    "scope `{}` has no matching target entry; sequence skipped",
                    chain.join("/")
                )),
            }
        }
    } else if scope.source.is_some() && chain.is_empty() && !target_root_iterable {
        warnings.push(
            "the root scope iterates rows but the target document is not row/array \
             shaped in MapForce terms; the iteration wire is skipped"
                .to_string(),
        );
    } else if let Some(source) = &scope.source {
        let mut abs = anchor.clone();
        abs.extend(source.iter().cloned());
        match (
            source_ports.key_for_abs(&abs),
            target_ports.key_for_abs(chain),
        ) {
            (Some(from), Some(to)) => {
                let from = append_scope_controls(
                    scope,
                    chain,
                    Some(&abs),
                    graph,
                    node_out_key,
                    position_inputs,
                    position_contexts,
                    keys,
                    uid,
                    filter_components,
                    edges,
                    warnings,
                    from,
                );
                connect_binding_positions(
                    scope,
                    Some(&abs),
                    from,
                    graph,
                    position_inputs,
                    position_contexts,
                    edges,
                    warnings,
                );
                edges.push((from, to));
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
            target_root_iterable,
            graph,
            node_out_key,
            position_inputs,
            position_contexts,
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

fn aggregate_component_name(op: mapping::AggregateOp) -> &'static str {
    use mapping::AggregateOp;
    match op {
        AggregateOp::Count => "count",
        AggregateOp::Sum => "sum",
        AggregateOp::Avg => "avg",
        AggregateOp::Min => "min",
        AggregateOp::Max => "max",
        AggregateOp::Join => "string-join",
        AggregateOp::ItemAt => "item-at",
    }
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
        "left_trim" => "left-trim",
        "right_trim" => "right-trim",
        "pad_string_left" => "pad-string-left",
        "pad_string_right" => "pad-string-right",
        "substring_before" => "substring-before",
        "substring_after" => "substring-after",
        "date_from_datetime" => "date-from-datetime",
        "format_number" => "format-number",
        other => other,
    }
    .to_string()
}

fn function_library(name: &str) -> &'static str {
    match name {
        "left_trim" | "right_trim" | "pad_string_left" | "pad_string_right" => "lang",
        _ => "core",
    }
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::{function_library, unmap_function_name};

    #[test]
    fn canonical_scalar_names_export_as_mapforce_core_functions() {
        assert_eq!(unmap_function_name("string"), "string");
        assert_eq!(unmap_function_name("format_number"), "format-number");
        assert_eq!(function_library("string"), "core");
        assert_eq!(function_library("format_number"), "core");
    }
}
