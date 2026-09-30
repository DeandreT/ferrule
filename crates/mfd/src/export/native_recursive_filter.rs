//! Native recursive user-function export for one same-shape XML group filter.
//!
//! A direct item collection is selected by a scalar `contains` predicate at
//! every depth. The remaining root attribute and recursive child collection
//! are preserved. The planner accepts only the graph and schema family that
//! this function can reconstruct from the project, without original MFD XML.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    FormatOptions, IterationOutput, Node, NodeId, Project, ScopeConstruction, ScopeIteration,
};

use crate::MfdError;

use super::TargetExport;
use super::mapped_sequence::render_edge_metadata;
use super::schema::{KeyAlloc, SideFormat, xml_escape};
use super::source::SourceExports;

const FUNCTION_NAME: &str = "FilterRecursiveGroup";

pub(super) struct NativeRecursiveFilter {
    source_root: u32,
    target_root: u32,
    root_name: String,
    root_attribute: String,
    items: String,
    item_value: String,
    children: String,
    parameter_name: String,
    parameter_node: NodeId,
    predicate_node: NodeId,
    value_first: bool,
    source_schema_file: String,
    target_schema_file: String,
}

impl NativeRecursiveFilter {
    pub(super) fn plan(
        project: &Project,
        sources: &SourceExports<'_>,
        targets: &[TargetExport<'_>],
        mfd_path: &Path,
    ) -> Option<Self> {
        if sources.len() != 1 {
            return None;
        }
        let source = sources.iter().next()?;
        let [target] = targets else {
            return None;
        };
        let ScopeConstruction::RecursiveFilter { plan } = &project.root.construction else {
            return None;
        };
        let scope = &project.root;
        let xml_options = FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        };
        if source.format != SideFormat::Xml
            || target.format != SideFormat::Xml
            || source.options != &xml_options
            || target.options != &xml_options
            || source.dynamic_path_node.is_some()
            || source.schema != target.schema
            || !project.user_functions.is_empty()
            || !project.failure_rules.is_empty()
            || !scope.target_field.is_empty()
            || !matches!(scope.iteration, ScopeIteration::None)
            || scope.filter.is_some()
            || scope.post_group_filter.is_some()
            || scope.has_grouping()
            || scope.has_sort()
            || scope.sort_descending
            || scope.sort_filter_order != Default::default()
            || !scope.windows.is_empty()
            || scope.iteration_output != IterationOutput::Repeated
            || !scope.bindings.is_empty()
            || !scope.dynamic_bindings.is_empty()
            || !scope.children.is_empty()
            || !scope.dynamic_children.is_empty()
            || scope.merge_dynamic_fields
        {
            return None;
        }

        let fields = closed_group_children(source.schema)?;
        if source.schema.repeating
            || source.schema.recursive_ref.is_some()
            || source.schema.xml_namespace.is_some()
            || source.schema.xml_type_alternatives
            || fields.len() != 3
        {
            return None;
        }
        let item = source.schema.child(plan.items())?;
        let recursive = source.schema.child(plan.children())?;
        if source
            .schema
            .xml_repeating_choices
            .as_slice()
            .iter()
            .any(|choice| {
                choice.required
                    || !choice.repeating
                    || choice.members.as_slice() != [item.name.as_str(), recursive.name.as_str()]
                        && choice.members.as_slice()
                            != [recursive.name.as_str(), item.name.as_str()]
            })
            || source.schema.xml_repeating_choices.len() > 1
        {
            return None;
        }
        let root_attribute = fields
            .iter()
            .find(|field| field.name != item.name && field.name != recursive.name)?;
        if !plain_attribute(root_attribute)
            || !matches!(
                root_attribute.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            )
            || !item.repeating
            || item.recursive_ref.is_some()
            || item.xml_namespace.is_some()
            || !item.xml_repeating_choices.is_empty()
            || !recursive.repeating
            || recursive.recursive_ref.as_deref() != Some(source.schema.name.as_str())
            || recursive.xml_namespace.is_some()
            || !recursive.xml_repeating_choices.is_empty()
            || !closed_group_children(recursive).is_some_and(|children| children.is_empty())
        {
            return None;
        }
        let item_fields = closed_group_children(item)?;
        if !(1..=2).contains(&item_fields.len()) || !item_fields.iter().all(plain_attribute) {
            return None;
        }

        let Node::Call { function, args } = project.graph.nodes.get(&plan.predicate())? else {
            return None;
        };
        let [first, second] = args.as_slice() else {
            return None;
        };
        if function != "contains" {
            return None;
        }
        let (item_node, parameter_node, value_first) = if matches!(
            project.graph.nodes.get(first),
            Some(Node::SourceField { .. })
        ) {
            (*first, *second, true)
        } else {
            (*second, *first, false)
        };
        let Some(Node::SourceField { path, frame: None }) = project.graph.nodes.get(&item_node)
        else {
            return None;
        };
        let [item_value] = path.as_slice() else {
            return None;
        };
        let value_field = item.child(item_value)?;
        if !plain_attribute(value_field)
            || !matches!(
                value_field.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            )
        {
            return None;
        }

        let mut owned = BTreeSet::from([plan.predicate(), item_node, parameter_node]);
        let parameter_name = match project.graph.nodes.get(&parameter_node)? {
            Node::Const {
                value: Value::String(_),
            } => "SearchFor".to_string(),
            Node::RuntimeParameterDefault {
                name,
                ty: ScalarType::String,
                default,
                ..
            } if !name.is_empty()
                && matches!(
                    project.graph.nodes.get(default),
                    Some(Node::Const {
                        value: Value::String(_)
                    })
                ) =>
            {
                owned.insert(*default);
                name.clone()
            }
            _ => return None,
        };
        if project.graph.nodes.keys().copied().collect::<BTreeSet<_>>() != owned {
            return None;
        }
        let stem = mfd_path.file_stem()?.to_str()?;
        Some(Self {
            source_root: source.ports.key_for_abs(&[])?,
            target_root: target.ports.key_for_abs(&[])?,
            root_name: source.schema.name.clone(),
            root_attribute: root_attribute.name.clone(),
            items: item.name.clone(),
            item_value: item_value.clone(),
            children: recursive.name.clone(),
            parameter_name,
            parameter_node,
            predicate_node: plan.predicate(),
            value_first,
            source_schema_file: format!("{stem}-{}.xsd", source.sibling_suffix),
            target_schema_file: format!("{stem}-{}.xsd", target.sibling_suffix),
        })
    }

    pub(super) const fn predicate_node(&self) -> NodeId {
        self.predicate_node
    }

    pub(super) fn render(
        &self,
        keys: &mut KeyAlloc,
        uid: &mut u32,
        node_out_key: &BTreeMap<NodeId, u32>,
        components: &mut String,
        edges: &mut Vec<(u32, u32)>,
        structural_edges: &mut BTreeSet<(u32, u32)>,
    ) -> Result<String, MfdError> {
        let parameter_output =
            node_out_key
                .get(&self.parameter_node)
                .copied()
                .ok_or_else(|| {
                    MfdError::Unsupported(
                        "recursive-filter scalar parameter was not rendered".into(),
                    )
                })?;
        let definition_uid = next_uid(uid);
        let input_uid = next_uid(uid);
        let output_uid = next_uid(uid);
        let parameter_uid = next_uid(uid);
        let call_uid = next_uid(uid);
        let call_source = keys.next();
        let call_parameter = keys.next();
        let call_output = keys.next();
        let root_name = xml_escape(&self.root_name);
        let parameter_name = xml_escape(&self.parameter_name);
        let _ = writeln!(
            components,
            "\t\t\t\t<component name=\"{FUNCTION_NAME}\" library=\"user\" uid=\"{call_uid}\" kind=\"19\"><data>\
             <root><entry name=\"{root_name}\" componentid=\"{input_uid}\" inpkey=\"{call_source}\"/><entry name=\"{parameter_name}\" componentid=\"{parameter_uid}\" inpkey=\"{call_parameter}\"/></root>\
             <root rootindex=\"1\"><entry name=\"{root_name}\" componentid=\"{output_uid}\" outkey=\"{call_output}\"/></root>\
             </data></component>"
        );
        edges.extend([
            (self.source_root, call_source),
            (parameter_output, call_parameter),
            (call_output, self.target_root),
        ]);
        structural_edges.extend([
            (self.source_root, call_source),
            (call_output, self.target_root),
        ]);
        Ok(self.definition(
            keys,
            uid,
            definition_uid,
            input_uid,
            output_uid,
            parameter_uid,
        ))
    }

    fn definition(
        &self,
        keys: &mut KeyAlloc,
        uid: &mut u32,
        definition_uid: u32,
        input_uid: u32,
        output_uid: u32,
        parameter_uid: u32,
    ) -> String {
        let input_root = keys.next();
        let input_attribute = keys.next();
        let input_items = keys.next();
        let input_item_value = keys.next();
        let input_children = keys.next();
        let output_root = keys.next();
        let output_attribute = keys.next();
        let output_items = keys.next();
        let output_children = keys.next();
        let parameter_output = keys.next();
        let filter_inputs = [keys.next(), keys.next()];
        let filter_output = keys.next();
        let predicate_inputs = [keys.next(), keys.next()];
        let predicate_output = keys.next();
        let self_source = keys.next();
        let self_parameter = keys.next();
        let self_output = keys.next();
        let root_name = xml_escape(&self.root_name);
        let root_attribute = xml_escape(&self.root_attribute);
        let items = xml_escape(&self.items);
        let item_value = xml_escape(&self.item_value);
        let children_name = xml_escape(&self.children);
        let parameter_name = xml_escape(&self.parameter_name);
        let source_schema = xml_escape(&self.source_schema_file);
        let target_schema = xml_escape(&self.target_schema_file);
        let mut parts = String::new();
        let _ = writeln!(
            parts,
            "\t\t\t\t<component name=\"{root_name}\" library=\"xml\" uid=\"{input_uid}\" kind=\"14\"><data>\
             <root><entry name=\"{root_name}\" outkey=\"{input_root}\"><entry name=\"{root_attribute}\" type=\"attribute\" outkey=\"{input_attribute}\"/><entry name=\"{items}\" outkey=\"{input_items}\"><entry name=\"{item_value}\" type=\"attribute\" outkey=\"{input_item_value}\"/></entry><entry name=\"{children_name}\" outkey=\"{input_children}\"/></entry></root>\
             <document schema=\"{source_schema}\" instanceroot=\"{{}}{root_name}\"/><parameter usageKind=\"input\" name=\"{root_name}\"/>\
             </data></component>"
        );
        let _ = writeln!(
            parts,
            "\t\t\t\t<component name=\"{root_name}\" library=\"xml\" uid=\"{output_uid}\" kind=\"14\"><data>\
             <root><entry name=\"{root_name}\" inpkey=\"{output_root}\"><entry name=\"{root_attribute}\" type=\"attribute\" inpkey=\"{output_attribute}\"/><entry name=\"{items}\" inpkey=\"{output_items}\"/><entry name=\"{children_name}\" inpkey=\"{output_children}\"/></entry></root>\
             <document schema=\"{target_schema}\" instanceroot=\"{{}}{root_name}\"/><parameter usageKind=\"output\" name=\"{root_name}\"/>\
             </data></component>"
        );
        let _ = writeln!(
            parts,
            "\t\t\t\t<component name=\"{parameter_name}\" library=\"core\" uid=\"{parameter_uid}\" kind=\"6\"><targets><datapoint pos=\"0\" key=\"{parameter_output}\"/></targets><data><input datatype=\"string\"/><parameter usageKind=\"input\" name=\"{parameter_name}\"/></data></component>"
        );
        core_function(
            &mut parts,
            uid,
            &self.items,
            3,
            &filter_inputs,
            &[filter_output],
        );
        core_function(
            &mut parts,
            uid,
            "contains",
            5,
            &predicate_inputs,
            &[predicate_output],
        );
        let self_uid = next_uid(uid);
        let _ = writeln!(
            parts,
            "\t\t\t\t<component name=\"{FUNCTION_NAME}\" library=\"user\" uid=\"{self_uid}\" kind=\"19\"><data>\
             <root><entry name=\"{root_name}\" componentid=\"{input_uid}\" inpkey=\"{self_source}\"/><entry name=\"{parameter_name}\" componentid=\"{parameter_uid}\" inpkey=\"{self_parameter}\"/></root>\
             <root rootindex=\"1\"><entry name=\"{root_name}\" componentid=\"{output_uid}\" outkey=\"{self_output}\"/></root>\
             </data></component>"
        );
        let (value_input, parameter_input) = if self.value_first {
            (predicate_inputs[0], predicate_inputs[1])
        } else {
            (predicate_inputs[1], predicate_inputs[0])
        };
        let edges = [
            (input_root, output_root),
            (input_attribute, output_attribute),
            (input_items, filter_inputs[0]),
            (input_item_value, value_input),
            (parameter_output, parameter_input),
            (predicate_output, filter_inputs[1]),
            (filter_output, output_items),
            (input_children, self_source),
            (parameter_output, self_parameter),
            (self_output, output_children),
        ];
        let structural = BTreeSet::from([
            (input_root, output_root),
            (filter_output, output_items),
            (input_children, self_source),
            (self_output, output_children),
        ]);
        let (edge_keys, edge_metadata) = render_edge_metadata(&structural, keys);
        let mut grouped = BTreeMap::<u32, Vec<u32>>::new();
        for (from, to) in edges {
            grouped.entry(from).or_default().push(to);
        }
        let mut vertices = String::new();
        for (from, tos) in grouped {
            let _ = write!(vertices, "\t\t\t\t\t<vertex vertexkey=\"{from}\"><edges>");
            for to in tos {
                if let Some(edge_key) = edge_keys.get(&(from, to)) {
                    let _ = write!(
                        vertices,
                        "<edge vertexkey=\"{to}\" edgekey=\"{edge_key}\"/>"
                    );
                } else {
                    let _ = write!(vertices, "<edge vertexkey=\"{to}\"/>");
                }
            }
            vertices.push_str("</edges></vertex>\n");
        }
        format!(
            "\t<component name=\"{FUNCTION_NAME}\" library=\"user\" uid=\"{definition_uid}\" editable=\"1\">\n\
             \t\t<structure>\n\t\t\t<children>\n{parts}\t\t\t</children>\n\
             \t\t\t<graph directed=\"1\">\n{edge_metadata}\t\t\t\t<vertices>\n{vertices}\t\t\t\t</vertices>\n\t\t\t</graph>\n\
             \t\t</structure>\n\t</component>\n"
        )
    }
}

fn plain_attribute(node: &SchemaNode) -> bool {
    !node.repeating
        && node.attribute
        && node.xml_namespace.is_none()
        && matches!(node.kind, SchemaKind::Scalar { .. })
}

fn closed_group_children(node: &SchemaNode) -> Option<&[SchemaNode]> {
    if node.xml_type_alternatives
        || !node.xml_name_alternatives.is_empty()
        || node.xml_wildcard_namespace.is_some()
        || !node.xml_repeating_sequences.is_empty()
    {
        return None;
    }
    match &node.kind {
        SchemaKind::Group {
            children,
            alternatives,
            required,
            xml_restricted_alternatives,
            dynamic: None,
        } if alternatives.is_empty()
            && required.is_empty()
            && xml_restricted_alternatives.is_empty() =>
        {
            Some(children)
        }
        _ => None,
    }
}

fn next_uid(uid: &mut u32) -> u32 {
    *uid += 1;
    *uid
}

fn core_function(
    output: &mut String,
    uid: &mut u32,
    name: &str,
    kind: u32,
    inputs: &[u32],
    outputs: &[u32],
) {
    let uid = next_uid(uid);
    let name = xml_escape(name);
    let _ = write!(
        output,
        "\t\t\t\t<component name=\"{name}\" library=\"core\" uid=\"{uid}\" kind=\"{kind}\"><sources>"
    );
    for (position, key) in inputs.iter().enumerate() {
        let _ = write!(output, "<datapoint pos=\"{position}\" key=\"{key}\"/>");
    }
    output.push_str("</sources><targets>");
    for (position, key) in outputs.iter().enumerate() {
        let _ = write!(output, "<datapoint pos=\"{position}\" key=\"{key}\"/>");
    }
    output.push_str("</targets></component>\n");
}
