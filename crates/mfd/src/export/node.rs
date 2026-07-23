use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ir::{Value, XML_TEXT_FIELD};
use mapping::{Graph, Node, NodeId, Project, RuntimeValue, SequenceExpr};

use super::auto_number::{self, AutoNumbers};
use super::function::{
    aggregate_component_name, constant_parts, function_library, scalar_type_name,
    unmap_function_name, value_scalar_type, value_text,
};
use super::join::JoinExports;
use super::position::render_component;
use super::schema::{GeneratedSibling, KeyAlloc, PortMatch, PortPairMatch, PortTree, xml_escape};
use super::sequence::{SequenceExistsPins, collect_scope_sequences};
use super::source::SourceExports;
use super::udf::Exports as UserFunctionExports;

pub(super) struct RenderArgs<'a> {
    pub(super) project: &'a Project,
    pub(super) sources: &'a SourceExports<'a>,
    pub(super) joins: &'a JoinExports,
    pub(super) keys: &'a mut KeyAlloc,
    pub(super) uid: &'a mut u32,
    pub(super) node_out_key: &'a mut BTreeMap<NodeId, u32>,
    pub(super) components: &'a mut String,
    pub(super) edges: &'a mut Vec<(u32, u32)>,
    pub(super) structural_edges: &'a mut BTreeSet<(u32, u32)>,
    pub(super) warnings: &'a mut Vec<String>,
    pub(super) blocked_nodes: &'a BTreeSet<NodeId>,
    pub(super) mfd_path: &'a Path,
    pub(super) user_functions: &'a UserFunctionExports,
}

pub(super) struct RenderedNodes {
    pub(super) position_inputs: BTreeMap<NodeId, u32>,
    pub(super) sequence_exists_pins: Vec<SequenceExistsPins>,
    pub(super) siblings: Vec<GeneratedSibling>,
}

pub(super) fn render(args: RenderArgs<'_>) -> RenderedNodes {
    let RenderArgs {
        project,
        sources,
        joins,
        keys,
        uid,
        node_out_key,
        components,
        edges,
        structural_edges,
        warnings,
        blocked_nodes,
        mfd_path,
        user_functions,
    } = args;

    let mut sequence_inputs = Vec::new();
    let mut sequences = Vec::new();
    collect_scope_sequences(&project.root, &mut sequences);
    for node in project.graph.nodes.values() {
        if let Node::SequenceExists { sequence, .. } | Node::SequenceItemAt { sequence, .. } = node
        {
            sequences.push(sequence);
        }
    }
    for sequence in sequences {
        match sequence {
            SequenceExpr::Tokenize {
                input, delimiter, ..
            } => {
                let first_key = keys.next();
                let second_key = keys.next();
                let out = keys.next();
                node_out_key.insert(sequence.item(), out);
                sequence_inputs.push((*input, first_key));
                sequence_inputs.push((*delimiter, second_key));
                render_sequence_component(
                    "tokenize",
                    "core",
                    &[first_key, second_key],
                    out,
                    None,
                    uid,
                    components,
                );
            }
            SequenceExpr::TokenizeByLength { input, length, .. } => {
                let first_key = keys.next();
                let second_key = keys.next();
                let out = keys.next();
                node_out_key.insert(sequence.item(), out);
                sequence_inputs.push((*input, first_key));
                sequence_inputs.push((*length, second_key));
                render_sequence_component(
                    "tokenize-by-length",
                    "core",
                    &[first_key, second_key],
                    out,
                    None,
                    uid,
                    components,
                );
            }
            SequenceExpr::TokenizeRegex {
                input,
                pattern,
                flags,
                ..
            } => {
                let input_key = keys.next();
                let pattern_key = keys.next();
                let flags_key = keys.next();
                let out = keys.next();
                node_out_key.insert(sequence.item(), out);
                sequence_inputs.push((*input, input_key));
                sequence_inputs.push((*pattern, pattern_key));
                if let Some(flags) = flags {
                    sequence_inputs.push((*flags, flags_key));
                }
                render_sequence_component(
                    "tokenize-regexp",
                    "core",
                    &[input_key, pattern_key, flags_key],
                    out,
                    None,
                    uid,
                    components,
                );
            }
            SequenceExpr::Generate { from, to, .. } => {
                let first_key = keys.next();
                let second_key = keys.next();
                let out = keys.next();
                node_out_key.insert(sequence.item(), out);
                if let Some(from) = from {
                    sequence_inputs.push((*from, first_key));
                }
                sequence_inputs.push((*to, second_key));
                render_sequence_component(
                    "generate-sequence",
                    "core",
                    &[first_key, second_key],
                    out,
                    None,
                    uid,
                    components,
                );
            }
            SequenceExpr::RecursiveCollect {
                collection,
                children,
                descent_value,
                values,
                value,
                prefix,
                separator,
                ..
            } => {
                let collection_key = keys.next();
                let prefix_key = keys.next();
                let separator_key = keys.next();
                let out = keys.next();
                node_out_key.insert(sequence.item(), out);
                sequence_inputs.push((*prefix, prefix_key));
                sequence_inputs.push((*separator, separator_key));
                match sources.key_for_abs(collection) {
                    Some(source) => edges.push((source, collection_key)),
                    None => warnings.push(format!(
                        "recursive-collect collection `{}` has no source port; connection skipped",
                        collection.join("/")
                    )),
                }
                let metadata = super::recursive::collect_metadata(
                    collection,
                    children,
                    descent_value,
                    values,
                    value,
                );
                render_sequence_component(
                    "recursive-collect",
                    "ferrule",
                    &[collection_key, prefix_key, separator_key],
                    out,
                    Some(&metadata),
                    uid,
                    components,
                );
            }
        }
    }

    let mut fn_inputs: BTreeMap<NodeId, Vec<u32>> = BTreeMap::new();
    let auto_numbers = AutoNumbers::collect(&project.graph);
    let mut auto_number_inputs = Vec::new();
    let mut json_serializer_inputs = Vec::new();
    let mut position_inputs = BTreeMap::new();
    let mut sequence_exists_pins = Vec::new();
    let mut siblings = Vec::new();
    for (&id, node) in &project.graph.nodes {
        if joins.node_blocked(id) || blocked_nodes.contains(&id) {
            continue;
        }
        if auto_numbers.owns_internal(id) {
            continue;
        }
        if let Some(pattern) = auto_numbers.pattern(id) {
            let (out, inputs) = auto_number::render_component(pattern, keys, uid, components);
            node_out_key.insert(id, out);
            auto_number_inputs.push(inputs);
            continue;
        }
        match node {
            Node::SourceField { path, frame } => {
                if node_out_key.contains_key(&id) {
                    continue;
                }
                let mut absolute = frame.clone().unwrap_or_default();
                absolute.extend(path.iter().cloned());
                match sources.match_field(&absolute, frame.is_some()) {
                    PortMatch::Unique(key) => {
                        node_out_key.insert(id, key);
                    }
                    PortMatch::Missing
                        if xml_type_marker_is_exported(id, &project.graph, sources) => {}
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
            Node::SourceDocumentPath => match sources.match_document_path() {
                PortMatch::Unique(key) => {
                    node_out_key.insert(id, key);
                }
                PortMatch::Missing => warnings.push(
                    "source document path has no local XML file-set boundary; its connections are skipped"
                        .into(),
                ),
                PortMatch::Ambiguous => warnings.push(
                    "source document path matches multiple local XML file-set boundaries; its connections are skipped"
                        .into(),
                ),
            },
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
            Node::JoinField { .. } | Node::JoinPosition { .. } => {}
            Node::JoinAggregate {
                function,
                join,
                plan,
                expression,
                arg,
            } => {
                if !joins.supports_plan(*join, plan) {
                    continue;
                }
                let in_sequence = keys.next();
                let out = keys.next();
                let mut dynamic_inputs = Vec::new();
                if expression.is_some() {
                    dynamic_inputs.push(in_sequence);
                } else if let Some(tuple) = joins.tuple_output(*join) {
                    edges.push((tuple, in_sequence));
                } else {
                    continue;
                }
                let mut pins = format!("<datapoint/><datapoint pos=\"1\" key=\"{in_sequence}\"/>");
                if arg.is_some() {
                    let in_arg = keys.next();
                    dynamic_inputs.push(in_arg);
                    let _ = write!(pins, "<datapoint pos=\"2\" key=\"{in_arg}\"/>");
                }
                if !dynamic_inputs.is_empty() {
                    fn_inputs.insert(id, dynamic_inputs);
                }
                node_out_key.insert(id, out);
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
            Node::Lookup {
                collection,
                key,
                matches: _,
                value,
            } => {
                let (key_output, value_output) = match sources.lookup_ports(collection, key, value)
                {
                    PortPairMatch::Unique(key_output, value_output) => (key_output, value_output),
                    PortPairMatch::Missing => {
                        warnings.push(format!(
                            "lookup node {id} key/value paths match no source collection; skipped"
                        ));
                        continue;
                    }
                    PortPairMatch::Ambiguous => {
                        warnings.push(format!(
                                "lookup node {id} key/value paths match multiple source collections; skipped"
                            ));
                        continue;
                    }
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
            Node::DynamicSourceField { .. } => {
                warnings.push(format!(
                    "dynamic source field node {id} is not exportable; skipped"
                ));
                continue;
            }
            Node::XmlMixedContent {
                path,
                frame,
                replacements,
            } => {
                let mut absolute = frame.clone().unwrap_or_default();
                absolute.extend(path.iter().cloned());
                let source_group = match sources.match_sequence(&absolute) {
                    PortMatch::Unique(key) => key,
                    PortMatch::Missing => {
                        warnings.push(format!(
                            "XML mixed-content source `{}` matches no source group; skipped",
                            absolute.join("/")
                        ));
                        continue;
                    }
                    PortMatch::Ambiguous => {
                        warnings.push(format!(
                            "XML mixed-content source `{}` matches multiple source groups; skipped",
                            absolute.join("/")
                        ));
                        continue;
                    }
                };
                let mut text_path = absolute.clone();
                text_path.push(XML_TEXT_FIELD.to_string());
                let source_text = match sources.match_field(&text_path, frame.is_some()) {
                    PortMatch::Unique(key) => key,
                    PortMatch::Missing | PortMatch::Ambiguous => {
                        warnings.push(format!(
                            "XML mixed-content source `{}` has no unique text port; skipped",
                            absolute.join("/")
                        ));
                        continue;
                    }
                };
                let root_input = keys.next();
                let output = keys.next();
                let text_input = keys.next();
                let replacement_inputs = replacements
                    .iter()
                    .map(|_| keys.next())
                    .collect::<Vec<_>>();
                node_out_key.insert(id, output);
                fn_inputs.insert(id, replacement_inputs.clone());
                edges.extend([(source_group, root_input), (source_text, text_input)]);
                *uid += 1;
                let mut entries = format!(
                    "\t\t\t\t\t\t\t\t\t<entry name=\"#text\" inpkey=\"{text_input}\"/>\n"
                );
                for input in &replacement_inputs {
                    let _ = writeln!(
                        entries,
                        "\t\t\t\t\t\t\t\t\t<entry name=\"#text\" inpkey=\"{input}\" clone=\"1\"/>"
                    );
                }
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"MixedContent\" library=\"xml\" uid=\"{uid}\" kind=\"14\">\n\
                     \t\t\t\t\t<properties/>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"160\" rby=\"120\"/>\n\
                     \t\t\t\t\t<data>\n\
                     \t\t\t\t\t\t<root>\n\
                     \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                     \t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                     \t\t\t\t\t\t\t\t<entry name=\"MixedContent\" inpkey=\"{root_input}\" outkey=\"{output}\" expanded=\"1\">\n\
                     {entries}\
                     \t\t\t\t\t\t\t\t</entry>\n\
                     \t\t\t\t\t\t\t</entry>\n\
                     \t\t\t\t\t\t</root>\n\
                     \t\t\t\t\t\t<document/>\n\
                     \t\t\t\t\t\t<parameter usageKind=\"variable\"/>\n\
                     \t\t\t\t\t</data>\n\
                     \t\t\t\t</component>\n"
                );
            }
            Node::XmlSerialize {
                path,
                frame,
                schema,
                declaration,
                indent,
                namespace,
            } => {
                let mut absolute = frame.clone().unwrap_or_default();
                absolute.extend(path.iter().cloned());
                let source = match sources.match_sequence(&absolute) {
                    PortMatch::Unique(key) => key,
                    PortMatch::Missing => {
                        warnings.push(format!(
                            "XML serializer source `{}` matches no source group; skipped",
                            absolute.join("/")
                        ));
                        continue;
                    }
                    PortMatch::Ambiguous => {
                        warnings.push(format!(
                            "XML serializer source `{}` matches multiple source groups; skipped",
                            absolute.join("/")
                        ));
                        continue;
                    }
                };
                let serializer_ports = PortTree::build(schema, keys);
                let Some(input) = serializer_ports.key_for_abs(&[]) else {
                    warnings.push(format!(
                        "XML serializer node {id} has no document-root input port; skipped"
                    ));
                    continue;
                };
                let output = keys.next();
                let stem = mfd_path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("mapping");
                let schema_file = format!("{stem}-serializer-{id}.xsd");
                let schema_text = match format_xml::xsd::export(schema) {
                    Ok(schema) => schema,
                    Err(error) => {
                        warnings.push(format!(
                            "XML serializer node {id} schema cannot be exported ({error}); skipped"
                        ));
                        continue;
                    }
                };
                siblings.push(GeneratedSibling {
                    path: mfd_path
                        .parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join(&schema_file),
                    contents: schema_text,
                });
                let entries = serializer_ports.entries_xml(
                    schema,
                    "inpkey",
                    9,
                    true,
                    None,
                    None,
                );
                let declaration = u8::from(*declaration);
                let indent = u8::from(*indent);
                let namespace_header = namespace.as_deref().map_or_else(
                    || "<namespace/>".to_string(),
                    |namespace| format!("<namespace uid=\"{}\"/>", xml_escape(namespace)),
                );
                let instance_root = namespace.as_deref().map_or_else(
                    || format!("{{}}{}", schema.name),
                    |namespace| format!("{{{namespace}}}{}", schema.name),
                );
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"{}\" library=\"xml\" uid=\"{uid}\" kind=\"14\">\n\
                     \t\t\t\t\t<properties XSLTTargetEncoding=\"UTF-8\" WriteXMLDeclaration=\"{declaration}\" ferrule-indent=\"{indent}\"/>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"240\" rby=\"180\"/>\n\
                     \t\t\t\t\t<data>\n\
                     \t\t\t\t\t\t<root><header><namespaces>{namespace_header}</namespaces></header>\n\
                     \t\t\t\t\t\t\t<entry name=\"FileInstance\" outkey=\"{output}\" expanded=\"1\"><entry name=\"document\" expanded=\"1\">\n\
                     {entries}\
                     \t\t\t\t\t\t\t</entry></entry></root>\n\
                     \t\t\t\t\t\t<document schema=\"{}\" instanceroot=\"{}\"/>\n\
                     \t\t\t\t\t\t<wsdl/><parameter usageKind=\"stringserialize\"/>\n\
                     \t\t\t\t\t</data>\n\
                     \t\t\t\t</component>\n",
                    xml_escape(&schema.name),
                    xml_escape(&schema_file),
                    xml_escape(&instance_root)
                );
                node_out_key.insert(id, output);
                edges.push((source, input));
                structural_edges.insert((source, input));
            }
            Node::CollectionFind {
                predicate, value, ..
            } => {
                let filter_nodes = keys.next();
                let filter_predicate = keys.next();
                let filter_output = keys.next();
                node_out_key.insert(id, filter_output);
                fn_inputs.insert(id, vec![filter_nodes, filter_predicate]);
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"filter\" library=\"core\" uid=\"{uid}\" kind=\"3\">\n\
                     \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{filter_nodes}\"/><datapoint pos=\"1\" key=\"{filter_predicate}\"/></sources>\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{filter_output}\"/><datapoint/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
                );
                let _ = (predicate, value);
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
            Node::SequenceItemAt { sequence, .. } => {
                let Some(&sequence_output) = node_out_key.get(&sequence.item()) else {
                    warnings.push(format!(
                        "sequence item-at node {id} references an unexported sequence item; skipped"
                    ));
                    continue;
                };
                let sequence_input = keys.next();
                let index_input = keys.next();
                let output = keys.next();
                node_out_key.insert(id, output);
                fn_inputs.insert(id, vec![index_input]);
                edges.push((sequence_output, sequence_input));
                render_sequence_component(
                    "item-at",
                    "core",
                    &[sequence_input, index_input],
                    output,
                    None,
                    uid,
                    components,
                );
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
                let context_input = if expression.is_some() {
                    let input = keys.next();
                    let collection_key = match sources.match_sequence(collection) {
                        PortMatch::Unique(key) => key,
                        PortMatch::Missing => {
                            warnings.push(format!(
                                "computed aggregate over `{}` matches no source collection; its connections are skipped",
                                collection.join("/")
                            ));
                            continue;
                        }
                        PortMatch::Ambiguous => {
                            warnings.push(format!(
                                "computed aggregate over `{}` matches multiple source collections; its connections are skipped",
                                collection.join("/")
                            ));
                            continue;
                        }
                    };
                    edges.push((collection_key, input));
                    dynamic_inputs.push(in_sequence);
                    Some(input)
                } else {
                    let mut sequence = collection.clone();
                    sequence.extend(value.iter().cloned());
                    let sequence_key = match sources.match_sequence(&sequence) {
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
                    None
                };
                node_out_key.insert(id, out);
                let mut pins = context_input.map_or_else(
                    || format!("<datapoint/><datapoint pos=\"1\" key=\"{in_sequence}\"/>"),
                    |context| {
                        format!(
                            "<datapoint pos=\"0\" key=\"{context}\"/><datapoint pos=\"1\" key=\"{in_sequence}\"/>"
                        )
                    },
                );
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
            Node::Unconnected | Node::Const { value: Value::Null } => {
                let out = keys.next();
                node_out_key.insert(id, out);
                *uid += 1;
                let _ = write!(
                    components,
                    "\t\t\t\t<component name=\"set-empty\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                     \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{out}\"/></targets>\n\
                     \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                     \t\t\t\t</component>\n"
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
                if function == "json_serialize_object" {
                    match super::json_serializer::render(
                        id,
                        args,
                        &project.graph,
                        keys,
                        uid,
                        mfd_path,
                    ) {
                        Ok(rendered) => {
                            node_out_key.insert(id, rendered.output);
                            components.push_str(&rendered.xml);
                            json_serializer_inputs.extend(rendered.inputs);
                            siblings.push(rendered.sibling);
                        }
                        Err(reason) => warnings.push(format!(
                            "JSON string serializer node {id} is unsupported: {reason}; skipped"
                        )),
                    }
                    continue;
                }
                if let Some(alternative) =
                    xml_type_alternative_port(node, &project.graph, sources)
                {
                    let input = keys.next();
                    let out = keys.next();
                    node_out_key.insert(id, out);
                    edges.push((alternative, input));
                    render_sequence_component(
                        "exists",
                        "core",
                        &[input],
                        out,
                        None,
                        uid,
                        components,
                    );
                    continue;
                }
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
            Node::UserFunctionCall { function, args } => {
                let Some((ins, out)) = user_functions.render_call(
                    *function,
                    keys,
                    uid,
                    components,
                ) else {
                    warnings.push(format!(
                        "call references missing user-defined function {}; its connections are skipped",
                        function.get()
                    ));
                    continue;
                };
                if ins.len() != args.len() {
                    warnings.push(format!(
                        "call to user-defined function {} has the wrong arity; its connections are skipped",
                        function.get()
                    ));
                    continue;
                }
                node_out_key.insert(id, out);
                fn_inputs.insert(id, ins);
            }
            Node::FunctionParameter { parameter } => warnings.push(format!(
                "main mapping graph contains function parameter {}; its connections are skipped",
                parameter.get()
            )),
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
    connect_deferred_inputs(
        "JSON string serializer",
        &json_serializer_inputs,
        node_out_key,
        edges,
        warnings,
    );
    for inputs in auto_number_inputs {
        for (node, input) in [inputs.start, inputs.increment] {
            match node_out_key.get(&node) {
                Some(&output) => edges.push((output, input)),
                None => warnings.push(format!(
                    "auto-number input references unexported node {node}; connection skipped"
                )),
            }
        }
    }

    RenderedNodes {
        position_inputs,
        sequence_exists_pins,
        siblings,
    }
}

fn connect_deferred_inputs(
    kind: &str,
    inputs: &[(NodeId, u32)],
    node_out_key: &BTreeMap<NodeId, u32>,
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
) {
    for &(node, input) in inputs {
        if let Some(&output) = node_out_key.get(&node) {
            edges.push((output, input));
        } else {
            warnings.push(format!(
                "{kind} input references unexported node {node}; connection skipped"
            ));
        }
    }
}

fn xml_type_alternative_port(
    node: &Node,
    graph: &Graph,
    sources: &SourceExports<'_>,
) -> Option<u32> {
    let Node::Call { function, args } = node else {
        return None;
    };
    let [first, second] = args.as_slice() else {
        return None;
    };
    if function != "equal" {
        return None;
    }
    xml_type_alternative_operand(graph, sources, *first, *second)
        .or_else(|| xml_type_alternative_operand(graph, sources, *second, *first))
}

fn xml_type_marker_is_exported(marker: NodeId, graph: &Graph, sources: &SourceExports<'_>) -> bool {
    graph.nodes.values().any(|node| {
        matches!(node, Node::Call { args, .. } if args.contains(&marker))
            && xml_type_alternative_port(node, graph, sources).is_some()
    })
}

fn xml_type_alternative_operand(
    graph: &Graph,
    sources: &SourceExports<'_>,
    marker: NodeId,
    expected: NodeId,
) -> Option<u32> {
    let Node::SourceField { path, frame } = graph.nodes.get(&marker)? else {
        return None;
    };
    if path.last().is_none_or(|field| field != ir::XML_TYPE_FIELD) {
        return None;
    }
    let Node::Const {
        value: Value::String(expected),
    } = graph.nodes.get(&expected)?
    else {
        return None;
    };
    let mut group = frame.clone().unwrap_or_default();
    group.extend(path[..path.len() - 1].iter().cloned());
    sources.key_for_alternative(&group, expected)
}

#[allow(clippy::too_many_arguments)]
fn render_sequence_component(
    name: &str,
    library: &str,
    inputs: &[u32],
    output: u32,
    metadata: Option<&str>,
    uid: &mut u32,
    components: &mut String,
) {
    let mut pins = String::new();
    for (position, key) in inputs.iter().enumerate() {
        let _ = write!(pins, "<datapoint pos=\"{position}\" key=\"{key}\"/>");
    }
    let data = metadata.map_or_else(String::new, |metadata| format!("<data>{metadata}</data>\n"));
    *uid += 1;
    let _ = write!(
        components,
        "\t\t\t\t<component name=\"{name}\" library=\"{library}\" uid=\"{uid}\" kind=\"5\">\n\
         \t\t\t\t\t<sources>{pins}</sources>\n\
         \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{output}\"/></targets>\n\
         \t\t\t\t\t{data}\
         \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
         \t\t\t\t</component>\n"
    );
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
            Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
            Node::If {
                condition,
                then,
                else_,
            } => vec![*condition, *then, *else_],
            Node::ValueMap { input, .. } => vec![*input],
            Node::Lookup { matches, .. } => vec![*matches],
            Node::DynamicSourceField { key, .. } => vec![*key],
            Node::XmlMixedContent { replacements, .. } => replacements
                .iter()
                .map(|replacement| replacement.expression)
                .collect(),
            Node::CollectionFind {
                predicate, value, ..
            } => vec![*value, *predicate],
            Node::Aggregate {
                expression, arg, ..
            } => expression.iter().chain(arg).copied().collect(),
            Node::JoinAggregate {
                expression, arg, ..
            } => expression.iter().chain(arg).copied().collect(),
            Node::SequenceExists { .. } => continue,
            Node::SequenceItemAt { index, .. } => vec![*index],
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
