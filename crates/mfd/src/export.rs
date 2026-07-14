//! `mapping::Project` -> `.mfd` conversion for the supported subset, with
//! generated schemas and component families selected from instance paths.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use mapping::{
    Graph, JoinId, Node, NodeId, Project, RuntimeValue, Scope, ScopeConstruction, SequenceExpr,
};

use crate::MfdError;

mod artifact;
mod function;
mod join;
mod mapped_sequence;
mod position;
mod preflight;
mod schema;
mod sequence;
#[cfg(test)]
mod tests;

use function::{
    aggregate_component_name, constant_parts, function_library, scalar_type_name,
    unmap_function_name, value_scalar_type, value_text,
};
use mapped_sequence::{ScopePlans, preflight_mapped_sequences, render_edge_metadata};
use position::{connect_position_roots, connect_scope_position_roots, render_component};
use schema::{
    KeyAlloc, PortMatch, PortTree, Side, SideFormat, db_datasource_name, render_schema_component,
    side_format, xml_escape,
};
use sequence::{SequenceExistsPins, collect_scope_sequences};

/// Writes a MapForce design and generated schema siblings, returning warnings
/// for project features that have no export representation.
pub fn export(project: &Project, path: &Path) -> Result<Vec<String>, MfdError> {
    let mut warnings = Vec::new();

    preflight::validate(project)?;

    if !project.extra_sources.is_empty() {
        warnings.push(
            "extra sources are not exported; MapForce multi-input wiring must be redone"
                .to_string(),
        );
    }

    let source_format = if project.source_options.http_get.is_some() {
        SideFormat::Xml
    } else {
        side_format(&project.source_path, &project.source_options)
    };
    let target_format = side_format(&project.target_path, &project.target_options);
    let copy_document_root = project.root.construction == ScopeConstruction::CopyCurrentSource;
    let target_root_iterable = matches!(
        target_format,
        SideFormat::Csv | SideFormat::FixedWidth | SideFormat::Xlsx | SideFormat::Db
    ) || (target_format == SideFormat::Json && project.target.repeating);
    let mapped_scope_plans = preflight_mapped_sequences(project, target_format)?;

    let mut keys = KeyAlloc { next: 1 };
    let source_ports = PortTree::build(&project.source, &mut keys);
    let target_ports = PortTree::build(&project.target, &mut keys);

    let mut node_out_key: BTreeMap<NodeId, u32> = BTreeMap::new();
    let mut fn_inputs: BTreeMap<NodeId, Vec<u32>> = BTreeMap::new();
    let mut position_inputs: BTreeMap<NodeId, u32> = BTreeMap::new();
    let mut components = String::new();
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut structural_edges = BTreeSet::new();
    let mut uid = 100u32;
    let joins = join::render(join::RenderJoinArgs {
        project,
        source_ports: &source_ports,
        target_ports: &target_ports,
        target_root_iterable,
        keys: &mut keys,
        uid: &mut uid,
        node_out_key: &mut node_out_key,
        components: &mut components,
        edges: &mut edges,
        warnings: &mut warnings,
    });
    let mut sequence_inputs = Vec::new();
    let mut sequences = Vec::new();
    collect_scope_sequences(&project.root, &mut sequences);
    for node in project.graph.nodes.values() {
        if let Node::SequenceExists { sequence, .. } = node {
            sequences.push(sequence);
        }
    }
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
    let mut sequence_exists_pins = Vec::new();
    for (&id, node) in &project.graph.nodes {
        if joins.node_blocked(id) {
            continue;
        }
        match node {
            Node::SourceField { path, frame } => {
                if node_out_key.contains_key(&id) {
                    continue;
                }
                let mut absolute = frame.clone().unwrap_or_default();
                absolute.extend(path.iter().cloned());
                let port_match = if frame.is_some() {
                    source_ports
                        .key_for_abs(&absolute)
                        .map_or(PortMatch::Missing, PortMatch::Unique)
                } else {
                    source_ports.match_suffix(&absolute)
                };
                match port_match {
                    PortMatch::Unique(key) => {
                        node_out_key.insert(id, key);
                    }
                    PortMatch::Missing => warnings.push(format!(
                        "source field `{}` matches no source leaf; its connections \
                         are skipped",
                        absolute.join("/")
                    )),
                    PortMatch::Ambiguous => warnings.push(format!(
                        "source field `{}` matches multiple source leaves; its connections \
                         are skipped until it has an explicit frame",
                        absolute.join("/")
                    )),
                }
            }
            Node::Position { .. } => {
                let (input, out) = render_component(&mut keys, &mut uid, &mut components);
                node_out_key.insert(id, out);
                position_inputs.insert(id, input);
            }
            Node::JoinPosition { join } if joins.supports(*join) => {
                let (input, out) = render_component(&mut keys, &mut uid, &mut components);
                node_out_key.insert(id, out);
                position_inputs.insert(id, input);
            }
            Node::JoinField { .. } | Node::JoinPosition { .. } => {}
            Node::JoinAggregate { .. } => {}
            Node::Lookup {
                collection,
                key,
                matches: _,
                value,
            } => {
                let mut key_path = collection.clone();
                key_path.extend(key.iter().cloned());
                let mut value_path = collection.clone();
                value_path.extend(value.iter().cloned());
                let (Some(key_output), Some(value_output)) = (
                    source_ports.key_for_abs(&key_path),
                    source_ports.key_for_abs(&value_path),
                ) else {
                    warnings.push(format!(
                        "lookup node {id} key/value paths do not match primary source leaves; skipped"
                    ));
                    continue;
                };

                let equal_key = keys.next();
                let equal_match = keys.next();
                let equal_output = keys.next();
                uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"equal\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{equal_key}\"/><datapoint pos=\"1\" key=\"{equal_match}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{equal_output}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );

                let filter_nodes = keys.next();
                let filter_predicate = keys.next();
                let filter_output = keys.next();
                node_out_key.insert(id, filter_output);
                fn_inputs.insert(id, vec![equal_match]);
                uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"filter\" library=\"core\" uid=\"{uid}\" kind=\"3\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{filter_nodes}\"/><datapoint pos=\"1\" key=\"{filter_predicate}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{filter_output}\"/><datapoint/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.extend([
                    (key_output, equal_key),
                    (value_output, filter_nodes),
                    (equal_output, filter_predicate),
                ]);
            }
            Node::SequenceExists {
                sequence,
                predicate,
            } => {
                let Some(&sequence_output) = node_out_key.get(&sequence.item()) else {
                    warnings.push(format!(
                        "sequence-exists node {id} references an unexported sequence item; skipped"
                    ));
                    continue;
                };
                let filter_nodes = keys.next();
                let filter_predicate = keys.next();
                let filter_output = keys.next();
                uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"filter\" library=\"core\" uid=\"{uid}\" kind=\"3\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{filter_nodes}\"/><datapoint pos=\"1\" key=\"{filter_predicate}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{filter_output}\"/><datapoint/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );

                let exists_input = keys.next();
                let exists_output = keys.next();
                node_out_key.insert(id, exists_output);
                uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"exists\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{exists_input}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{exists_output}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((sequence_output, filter_nodes));
                edges.push((filter_output, exists_input));
                sequence_exists_pins.push(SequenceExistsPins {
                    predicate: *predicate,
                    sequence_output,
                    filter_predicate,
                });
            }
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
                    // Path sequences wire to their source; computed ones use their expression.
                    let mut sequence = collection.clone();
                    sequence.extend(value.iter().cloned());
                    let sequence_key = match source_ports.match_suffix(&sequence) {
                        PortMatch::Unique(key) => key,
                        PortMatch::Missing => {
                            warnings.push(format!(
                                "aggregate over `{}` matches no source entry; its \
                             connections are skipped",
                                sequence.join("/")
                            ));
                            continue;
                        }
                        PortMatch::Ambiguous => {
                            warnings.push(format!(
                                "aggregate over `{}` matches multiple source entries; its \
                                 connections are skipped",
                                sequence.join("/")
                            ));
                            continue;
                        }
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
            Node::Const { value } if value.is_xml_nil() => {
                let out = keys.next();
                node_out_key.insert(id, out);
                uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"set-xsi-nil\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
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
            Node::RuntimeValue { value } => {
                let out = keys.next();
                node_out_key.insert(id, out);
                uid += 1;
                let name = match value {
                    RuntimeValue::MappingFilePath => "mfd-filepath",
                    RuntimeValue::MainMappingFilePath => "main-mfd-filepath",
                    RuntimeValue::CurrentDateTime => "now",
                };
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"{name}\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
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
                let growable = if function == "datetime_add" {
                    " growable=\"1\" growablebasename=\"duration\""
                } else {
                    ""
                };
                let mut pins = String::new();
                for (pos, key) in ins.iter().enumerate() {
                    let _ = write!(pins, "<datapoint pos=\"{pos}\" key=\"{key}\"/>");
                }
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"{}\" library=\"{library}\" uid=\"{uid}\" kind=\"5\"{growable}>\n\
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
            Node::ValueMap {
                input_type,
                table,
                default,
                ..
            } => {
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
                let input_type = scalar_type_name(input_type.unwrap_or(ir::ScalarType::String));
                let result_type = table
                    .iter()
                    .find_map(|(_, value)| value_scalar_type(value))
                    .or_else(|| default.as_ref().and_then(value_scalar_type))
                    .map(scalar_type_name)
                    .unwrap_or("string");
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"value-map\" library=\"core\" uid=\"{uid}\" kind=\"23\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{input}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t\t<data><valuemap{mode}><valuemapTable>{rows}</valuemapTable>\
                     <input name=\"input\" type=\"{input_type}\"/><result name=\"result\" type=\"{result_type}\"{default_attr}/></valuemap></data>\n\
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
            Node::Lookup { matches, .. } => vec![*matches],
            Node::Aggregate {
                expression, arg, ..
            } => expression.iter().chain(arg).copied().collect(),
            Node::SequenceExists { .. } => continue,
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

    let mut filter_components = String::new();
    let mut position_contexts: BTreeMap<NodeId, Option<u32>> = BTreeMap::new();
    for pins in sequence_exists_pins {
        match node_out_key.get(&pins.predicate) {
            Some(&predicate_output) => edges.push((predicate_output, pins.filter_predicate)),
            None => warnings.push(format!(
                "sequence-exists predicate references unexported node {}; connection skipped",
                pins.predicate
            )),
        }
        connect_position_roots(
            [pins.predicate],
            None,
            true,
            pins.sequence_output,
            &project.graph,
            &position_inputs,
            &mut position_contexts,
            &mut edges,
            &mut warnings,
        );
    }
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
        false,
        &mut structural_edges,
        &mapped_scope_plans,
        &joins,
    );
    for (id, input) in &position_inputs {
        if !position_contexts.contains_key(id) {
            warnings.push(format!(
                "position node {id} has no matching iteration scope; its context input {input} is unconnected"
            ));
        }
    }
    components.push_str(&filter_components);

    // Database components reference a mapping-level datasource.
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
    let source_component = render_schema_component(
        &project.source,
        source_format,
        &source_ports,
        Side::Source,
        project.source_path.as_deref(),
        &project.source_options,
        path,
        copy_document_root,
    )?;
    let target_component = render_schema_component(
        &project.target,
        target_format,
        &target_ports,
        Side::Target,
        project.target_path.as_deref(),
        &project.target_options,
        path,
        copy_document_root,
    )?;
    out.push_str(&source_component.xml);
    out.push_str(&target_component.xml);
    out.push_str(&components);
    let (structural_edge_keys, edge_metadata) = render_edge_metadata(&structural_edges, &mut keys);
    let _ = write!(
        out,
        "\t\t\t</children>\n\t\t\t<graph directed=\"1\">\n{edge_metadata}\t\t\t\t<vertices>\n"
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
            if let Some(edge_key) = structural_edge_keys.get(&(from, to)) {
                let _ = writeln!(
                    out,
                    "\t\t\t\t\t\t\t<edge vertexkey=\"{to}\" edgekey=\"{edge_key}\"/>"
                );
            } else {
                let _ = writeln!(out, "\t\t\t\t\t\t\t<edge vertexkey=\"{to}\"/>");
            }
        }
        out.push_str("\t\t\t\t\t\t</edges>\n\t\t\t\t\t</vertex>\n");
    }
    out.push_str(
        "\t\t\t\t</vertices>\n\t\t\t</graph>\n\t\t</structure>\n\t</component>\n</mapping>\n",
    );

    let mut artifacts = Vec::new();
    for sibling in [source_component.sibling, target_component.sibling]
        .into_iter()
        .flatten()
    {
        artifacts.push((sibling.path, sibling.contents));
    }
    // Publish the design after its schema siblings reach their final paths.
    artifacts.push((path.to_path_buf(), out));
    write_artifacts(artifacts)?;
    Ok(warnings)
}

#[allow(clippy::too_many_arguments)]
fn append_scope_controls(
    scope: &Scope,
    chain: &[String],
    source_collection: Option<&[String]>,
    join: Option<JoinId>,
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
        connect_scope_position_roots(
            [sort_by],
            source_collection,
            join,
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
        connect_scope_position_roots(
            [filter],
            source_collection,
            join,
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
        connect_scope_position_roots(
            [group_by],
            source_collection,
            join,
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
    if let Some(predicate) = scope.group_starting_with {
        connect_scope_position_roots(
            [predicate],
            source_collection,
            join,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        match node_out_key.get(&predicate) {
            Some(&predicate_src) => {
                let in_nodes = keys.next();
                let in_predicate = keys.next();
                let out_groups = keys.next();
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"group-starting-with\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/><datapoint pos=\"1\" key=\"{in_predicate}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_groups}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_nodes));
                edges.push((predicate_src, in_predicate));
                from = out_groups;
            }
            None => warnings.push(format!(
                "scope `{}` group-starting predicate references an unexported node; grouping dropped",
                chain.join("/")
            )),
        }
    }
    if let Some(block_size) = scope.group_into_blocks {
        connect_scope_position_roots(
            [block_size],
            source_collection,
            join,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
        match node_out_key.get(&block_size) {
            Some(&size_src) => {
                let in_nodes = keys.next();
                let in_size = keys.next();
                let out_groups = keys.next();
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"group-into-blocks\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{in_nodes}\"/><datapoint pos=\"1\" key=\"{in_size}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out_groups}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                edges.push((from, in_nodes));
                edges.push((size_src, in_size));
                from = out_groups;
            }
            None => warnings.push(format!(
                "scope `{}` group block size references an unexported node; grouping dropped",
                chain.join("/")
            )),
        }
    }
    if let Some(take) = scope.take {
        connect_scope_position_roots(
            [take],
            source_collection,
            join,
            true,
            from,
            graph,
            position_inputs,
            position_contexts,
            edges,
            warnings,
        );
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
    join: Option<JoinId>,
    from: u32,
    graph: &Graph,
    position_inputs: &BTreeMap<NodeId, u32>,
    position_contexts: &mut BTreeMap<NodeId, Option<u32>>,
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
) {
    connect_scope_position_roots(
        scope.bindings.iter().map(|binding| binding.node),
        source_collection,
        join,
        true,
        from,
        graph,
        position_inputs,
        position_contexts,
        edges,
        warnings,
    );

    // Named collections can be outer-owned; empty paths stay nested-owned.
    let mut descendant_roots = Vec::new();
    for child in &scope.children {
        descendant_binding_roots(child, &mut descendant_roots);
    }
    connect_scope_position_roots(
        descendant_roots,
        source_collection,
        join,
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
    suppress_mapped_bindings: bool,
    structural_edges: &mut BTreeSet<(u32, u32)>,
    mapped_scope_plans: &ScopePlans,
    joins: &join::JoinExports,
) {
    let mapped_plan = mapped_scope_plans.get(chain);
    let suppress_mapped_bindings =
        suppress_mapped_bindings || mapped_plan.is_some_and(|plan| plan.copy_all);
    let anchor_len = anchor.len();
    if scope.construction == ScopeConstruction::CopyCurrentSource && scope.source().is_none() {
        match (
            source_ports.key_for_abs(anchor),
            target_ports.key_for_abs(chain),
        ) {
            (Some(from), Some(to)) => {
                edges.push((from, to));
                structural_edges.insert((from, to));
            }
            _ => warnings.push(format!(
                "scope `{}` cannot connect its current source group to the target; copy skipped",
                chain.join("/")
            )),
        }
    } else if let Some((join, _)) = scope.join() {
        if let (Some(from), Some(to)) = (joins.row_output(join), target_ports.key_for_abs(chain)) {
            let from = append_scope_controls(
                scope,
                chain,
                None,
                Some(join),
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
                Some(join),
                from,
                graph,
                position_inputs,
                position_contexts,
                edges,
                warnings,
            );
            edges.push((from, to));
        }
    } else if let Some(sequence) = scope.sequence() {
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
    } else if scope.source().is_some() && chain.is_empty() && !target_root_iterable {
        warnings.push(
            "the root scope iterates rows but the target document is not row/array \
             shaped in MapForce terms; the iteration wire is skipped"
                .to_string(),
        );
    } else if let Some(source) = scope.source() {
        let mut abs = anchor.clone();
        abs.extend(source.iter().cloned());
        let structural_source = mapped_plan
            .map(|plan| plan.source_group.as_slice())
            .unwrap_or(&abs);
        match (
            source_ports.key_for_abs(structural_source),
            target_ports.key_for_abs(chain),
        ) {
            (Some(from), Some(to)) => {
                let from = append_scope_controls(
                    scope,
                    chain,
                    Some(&abs),
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
                    Some(&abs),
                    None,
                    from,
                    graph,
                    position_inputs,
                    position_contexts,
                    edges,
                    warnings,
                );
                edges.push((from, to));
                if mapped_plan.is_some_and(|plan| plan.copy_all)
                    || scope.construction == ScopeConstruction::CopyCurrentSource
                {
                    structural_edges.insert((from, to));
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
        if suppress_mapped_bindings {
            continue;
        }
        let mut leaf = chain.clone();
        leaf.push(binding.target_field.clone());
        match (
            node_out_key.get(&binding.node),
            target_ports.key_for_abs(&leaf),
        ) {
            (Some(&from), Some(to)) => edges.push((from, to)),
            (None, _) if joins.node_blocked(binding.node) => {}
            (None, _)
                if matches!(
                    graph.nodes.get(&binding.node),
                    Some(Node::JoinField { .. } | Node::JoinPosition { .. })
                ) => {}
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
            suppress_mapped_bindings,
            structural_edges,
            mapped_scope_plans,
            joins,
        );
        chain.pop();
    }
    anchor.truncate(anchor_len);
}
use artifact::write_artifacts;
