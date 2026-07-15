use std::collections::BTreeMap;
use std::fmt::Write as _;

use mapping::{Node, NodeId, Project, RuntimeValue, SequenceExpr};

use super::function::{
    aggregate_component_name, constant_parts, function_library, scalar_type_name,
    unmap_function_name, value_scalar_type, value_text,
};
use super::join::JoinExports;
use super::position::render_component;
use super::schema::{KeyAlloc, PortMatch, PortTree, xml_escape};
use super::sequence::{SequenceExistsPins, collect_scope_sequences};

pub(super) struct RenderArgs<'a> {
    pub(super) project: &'a Project,
    pub(super) source_ports: &'a PortTree,
    pub(super) joins: &'a JoinExports,
    pub(super) keys: &'a mut KeyAlloc,
    pub(super) uid: &'a mut u32,
    pub(super) node_out_key: &'a mut BTreeMap<NodeId, u32>,
    pub(super) components: &'a mut String,
    pub(super) edges: &'a mut Vec<(u32, u32)>,
    pub(super) warnings: &'a mut Vec<String>,
}

pub(super) struct RenderedNodes {
    pub(super) position_inputs: BTreeMap<NodeId, u32>,
    pub(super) sequence_exists_pins: Vec<SequenceExistsPins>,
}

pub(super) fn render(args: RenderArgs<'_>) -> RenderedNodes {
    let RenderArgs {
        project,
        source_ports,
        joins,
        keys,
        uid,
        node_out_key,
        components,
        edges,
        warnings,
    } = args;

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
        *uid += 1;
        let _ = write!(
            components,
            "\t\t\t\t<component name=\"{name}\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
             \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{first_key}\"/><datapoint pos=\"1\" key=\"{second_key}\"/></sources>\n\
             \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
             \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
             \t\t\t\t</component>\n"
        );
    }

    let mut fn_inputs: BTreeMap<NodeId, Vec<u32>> = BTreeMap::new();
    let mut position_inputs = BTreeMap::new();
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
                let (input, out) = render_component(keys, uid, components);
                node_out_key.insert(id, out);
                position_inputs.insert(id, input);
            }
            Node::JoinPosition { join } if joins.supports(*join) => {
                let (input, out) = render_component(keys, uid, components);
                node_out_key.insert(id, out);
                position_inputs.insert(id, input);
            }
            Node::JoinField { .. } | Node::JoinPosition { .. } | Node::JoinAggregate { .. } => {}
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
                *uid += 1;
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
                *uid += 1;
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
                *uid += 1;
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
                *uid += 1;
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
                *uid += 1;
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
                *uid += 1;
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
                *uid += 1;
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
                *uid += 1;
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
                *uid += 1;
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
                *uid += 1;
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
                *uid += 1;
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
                    .map(|value| format!(" defaultValue=\"{}\"", xml_escape(&value_text(value))))
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

    connect_inputs(
        project,
        &fn_inputs,
        node_out_key,
        &sequence_inputs,
        edges,
        warnings,
    );

    RenderedNodes {
        position_inputs,
        sequence_exists_pins,
    }
}

fn connect_inputs(
    project: &Project,
    fn_inputs: &BTreeMap<NodeId, Vec<u32>>,
    node_out_key: &BTreeMap<NodeId, u32>,
    sequence_inputs: &[(NodeId, u32)],
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
) {
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
        for (index, arg) in args.iter().enumerate() {
            if let (Some(&from), Some(&to)) = (node_out_key.get(arg), ins.get(index)) {
                edges.push((from, to));
            }
        }
    }
    for &(node, input) in sequence_inputs {
        if let Some(&from) = node_out_key.get(&node) {
            edges.push((from, input));
        } else {
            warnings.push(format!(
                "sequence input references unexported node {node}; connection skipped"
            ));
        }
    }
}
