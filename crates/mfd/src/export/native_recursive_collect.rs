//! Guarded native recursive collection for the flat file-list hierarchy.
//!
//! The source's direct files are emitted before recursively collected child
//! files. The name, separator, and target fields are derived from the exact
//! supported IR shape; the editable user function is regenerated.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    FormatOptions, IterationOutput, Node, NodeId, Project, Scope, ScopeConstruction,
    ScopeIteration, SequenceExpr,
};

use crate::MfdError;

use super::TargetExport;
use super::mapped_sequence::render_edge_metadata;
use super::schema::{KeyAlloc, SideFormat, xml_escape};
use super::source::SourceExports;

const FUNCTION_NAME: &str = "CollectDirectoryFiles";
const PREFIX_NAME: &str = "DirectoryPrefix";

pub(super) struct NativeRecursiveCollect {
    source_root: u32,
    target_root: u32,
    prefix_node: NodeId,
    separator_node: NodeId,
    item_node: NodeId,
    source_schema_file: String,
    target_schema_file: String,
}

impl NativeRecursiveCollect {
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
        let xml_options = FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        };
        if source.format != SideFormat::Xml
            || target.format != SideFormat::Xml
            || source.options != &xml_options
            || target.options != &xml_options
            || source.dynamic_path_node.is_some()
            || !project.user_functions.is_empty()
            || !project.failure_rules.is_empty()
            || !plain_scope(&project.root)
            || !matches!(project.root.iteration, ScopeIteration::None)
            || project.root.construction != ScopeConstruction::Constructed
        {
            return None;
        }
        let [file_scope] = project.root.children.as_slice() else {
            return None;
        };
        if file_scope.target_field != "File"
            || !plain_scope(file_scope)
            || file_scope.iteration_output != IterationOutput::Repeated
        {
            return None;
        }
        let ScopeIteration::Sequence(SequenceExpr::RecursiveCollect {
            collection,
            children,
            descent_value,
            values,
            value,
            prefix,
            separator,
            item,
        }) = &file_scope.iteration
        else {
            return None;
        };
        if !collection.is_empty()
            || children.as_slice() != ["directory"]
            || descent_value.as_slice() != ["name"]
            || values.as_slice() != ["file"]
            || value.as_slice() != ["name"]
            || file_scope.construction != (ScopeConstruction::Scalar { value: *item })
        {
            return None;
        }
        if !matches!(project.graph.nodes.get(prefix), Some(Node::Const { value: Value::String(value) }) if value.is_empty())
            || !matches!(project.graph.nodes.get(separator), Some(Node::Const { value: Value::String(value) }) if value == "\\")
            || !matches!(project.graph.nodes.get(item), Some(Node::SourceField { path, frame: None }) if path.is_empty())
            || project.graph.nodes.keys().copied().collect::<BTreeSet<_>>()
                != BTreeSet::from([*prefix, *separator, *item])
            || !source_shape(source.schema)
            || !target_shape(target.schema)
        {
            return None;
        }
        let stem = mfd_path.file_stem()?.to_str()?;
        Some(Self {
            source_root: source.ports.key_for_abs(&[])?,
            target_root: target.ports.key_for_abs(&[])?,
            prefix_node: *prefix,
            separator_node: *separator,
            item_node: *item,
            source_schema_file: format!("{stem}-{}.xsd", source.sibling_suffix),
            target_schema_file: format!("{stem}-{}.xsd", target.sibling_suffix),
        })
    }

    pub(super) const fn item_node(&self) -> NodeId {
        self.item_node
    }

    pub(super) fn absorbed_nodes(&self) -> [NodeId; 2] {
        [self.separator_node, self.item_node]
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
        let prefix_output = node_out_key
            .get(&self.prefix_node)
            .copied()
            .ok_or_else(|| {
                MfdError::Unsupported("native recursive-collect prefix was not rendered".into())
            })?;
        let definition_uid = next_uid(uid);
        let input_uid = next_uid(uid);
        let output_uid = next_uid(uid);
        let prefix_uid = next_uid(uid);
        let call_uid = next_uid(uid);
        let call_source = keys.next();
        let call_prefix = keys.next();
        let call_output = keys.next();
        let _ = writeln!(
            components,
            "\t\t\t\t<component name=\"{FUNCTION_NAME}\" library=\"user\" uid=\"{call_uid}\" kind=\"19\"><data>\
             <root><entry name=\"directory\" componentid=\"{input_uid}\" inpkey=\"{call_source}\"/><entry name=\"{PREFIX_NAME}\" componentid=\"{prefix_uid}\" inpkey=\"{call_prefix}\"/></root>\
             <root rootindex=\"1\"><entry name=\"FileList\" componentid=\"{output_uid}\" outkey=\"{call_output}\"/></root>\
             </data></component>"
        );
        edges.extend([
            (self.source_root, call_source),
            (prefix_output, call_prefix),
            (call_output, self.target_root),
        ]);
        structural_edges.extend([
            (self.source_root, call_source),
            (call_output, self.target_root),
        ]);
        Ok(self.definition(keys, uid, definition_uid, input_uid, output_uid, prefix_uid))
    }

    fn definition(
        &self,
        keys: &mut KeyAlloc,
        uid: &mut u32,
        definition_uid: u32,
        input_uid: u32,
        output_uid: u32,
        prefix_uid: u32,
    ) -> String {
        let directory_name = keys.next();
        let file_name = keys.next();
        let child_directories = keys.next();
        let direct_file_target = keys.next();
        let recursive_file_target = keys.next();
        let prefix_output = keys.next();
        let separator_output = keys.next();
        let descent_inputs = [keys.next(), keys.next(), keys.next()];
        let descent_output = keys.next();
        let direct_inputs = [keys.next(), keys.next(), keys.next()];
        let direct_output = keys.next();
        let self_directory = keys.next();
        let self_prefix = keys.next();
        let self_files = keys.next();
        let source_schema = xml_escape(&self.source_schema_file);
        let target_schema = xml_escape(&self.target_schema_file);

        let mut parts = String::new();
        let _ = writeln!(
            parts,
            "\t\t\t\t<component name=\"directory\" library=\"xml\" uid=\"{input_uid}\" kind=\"14\"><data>\
             <root><entry name=\"directory\"><entry name=\"name\" type=\"attribute\" outkey=\"{directory_name}\"/><entry name=\"file\"><entry name=\"name\" type=\"attribute\" outkey=\"{file_name}\"/></entry><entry name=\"directory\" outkey=\"{child_directories}\"/></entry></root>\
             <document schema=\"{source_schema}\" instanceroot=\"{{}}directory\"/><parameter usageKind=\"input\" name=\"directory\"/>\
             </data></component>"
        );
        // The direct value is the first File port. The self-call's values are
        // appended through the second, cloned File port.
        let _ = writeln!(
            parts,
            "\t\t\t\t<component name=\"FileList\" library=\"xml\" uid=\"{output_uid}\" kind=\"14\"><data>\
             <root><entry name=\"FileList\"><entry name=\"File\" inpkey=\"{direct_file_target}\"/><entry name=\"File\" inpkey=\"{recursive_file_target}\" clone=\"1\"/></entry></root>\
             <document schema=\"{target_schema}\" instanceroot=\"{{}}FileList\"/><parameter usageKind=\"output\" name=\"FileList\"/>\
             </data></component>"
        );
        let _ = writeln!(
            parts,
            "\t\t\t\t<component name=\"{PREFIX_NAME}\" library=\"core\" uid=\"{prefix_uid}\" kind=\"6\"><targets><datapoint pos=\"0\" key=\"{prefix_output}\"/></targets><data><input datatype=\"string\"/><parameter usageKind=\"input\" name=\"{PREFIX_NAME}\"/></data></component>"
        );
        let _ = writeln!(
            parts,
            "\t\t\t\t<component name=\"constant\" library=\"core\" uid=\"{}\" kind=\"2\"><targets><datapoint pos=\"0\" key=\"{separator_output}\"/></targets><data><constant value=\"{}\" datatype=\"string\"/></data></component>",
            next_uid(uid),
            xml_escape("\\")
        );
        concat(&mut parts, uid, &descent_inputs, descent_output);
        concat(&mut parts, uid, &direct_inputs, direct_output);
        let self_uid = next_uid(uid);
        let _ = writeln!(
            parts,
            "\t\t\t\t<component name=\"{FUNCTION_NAME}\" library=\"user\" uid=\"{self_uid}\" kind=\"19\"><data>\
             <root><entry name=\"directory\" componentid=\"{input_uid}\"><entry name=\"directory\" inpkey=\"{self_directory}\"/></entry><entry name=\"{PREFIX_NAME}\" componentid=\"{prefix_uid}\" inpkey=\"{self_prefix}\"/></root>\
             <root rootindex=\"1\"><entry name=\"FileList\" componentid=\"{output_uid}\"><entry name=\"FileList\"><entry name=\"File\" outkey=\"{self_files}\"/></entry></entry></root>\
             </data></component>"
        );

        let edges = [
            (prefix_output, descent_inputs[0]),
            (separator_output, descent_inputs[1]),
            (directory_name, descent_inputs[2]),
            (descent_output, direct_inputs[0]),
            (separator_output, direct_inputs[1]),
            (file_name, direct_inputs[2]),
            (direct_output, direct_file_target),
            (child_directories, self_directory),
            (descent_output, self_prefix),
            (self_files, recursive_file_target),
        ];
        let structural = BTreeSet::from([(child_directories, self_directory)]);
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

fn plain_scope(scope: &Scope) -> bool {
    scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && !scope.sort_descending
        && scope.sort_filter_order == Default::default()
        && scope.windows.is_empty()
        && scope.iteration_output == IterationOutput::Repeated
        && scope.bindings.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
        && (scope.target_field.is_empty() || scope.children.is_empty())
        && matches!(
            scope.iteration,
            ScopeIteration::None | ScopeIteration::Sequence(_)
        )
}

fn source_shape(root: &SchemaNode) -> bool {
    let SchemaKind::Group {
        children,
        alternatives,
        required,
        xml_restricted_alternatives,
        dynamic: None,
    } = &root.kind
    else {
        return false;
    };
    if root.name != "directory"
        || root.repeating
        || root.xml_namespace.is_some()
        || !root.xml_name_alternatives.is_empty()
        || !root.xml_repeating_sequences.is_empty()
        || root.xml_repeating_choices.len() != 1
        || !alternatives.is_empty()
        || !required.is_empty()
        || !xml_restricted_alternatives.is_empty()
        || children.len() != 3
    {
        return false;
    }
    let choice = &root.xml_repeating_choices[0];
    if choice.required || !choice.repeating || choice.members != ["file", "directory"] {
        return false;
    }
    let [file, directory, name] = children.as_slice() else {
        return false;
    };
    if file.name != "file"
        || !file.repeating
        || file.recursive_ref.is_some()
        || file.xml_namespace.is_some()
        || directory.name != "directory"
        || !directory.repeating
        || directory.recursive_ref.as_deref() != Some("directory")
        || directory.xml_namespace.is_some()
        || !matches!(&directory.kind, SchemaKind::Group { children, alternatives, required, xml_restricted_alternatives, dynamic: None } if children.is_empty() && alternatives.is_empty() && required.is_empty() && xml_restricted_alternatives.is_empty())
        || !plain_attribute(name, "name", ScalarType::String)
    {
        return false;
    }
    let SchemaKind::Group {
        children: file_fields,
        alternatives,
        required,
        xml_restricted_alternatives,
        dynamic: None,
    } = &file.kind
    else {
        return false;
    };
    let [file_name, size] = file_fields.as_slice() else {
        return false;
    };
    alternatives.is_empty()
        && required.is_empty()
        && xml_restricted_alternatives.is_empty()
        && plain_attribute(file_name, "name", ScalarType::String)
        && plain_attribute(size, "size", ScalarType::Int)
}

fn target_shape(root: &SchemaNode) -> bool {
    let SchemaKind::Group {
        children,
        alternatives,
        required,
        xml_restricted_alternatives,
        dynamic: None,
    } = &root.kind
    else {
        return false;
    };
    let [file] = children.as_slice() else {
        return false;
    };
    root.name == "FileList"
        && !root.repeating
        && root.xml_namespace.is_none()
        && root.xml_repeating_choices.is_empty()
        && root.xml_repeating_sequences.is_empty()
        && alternatives.is_empty()
        && required.is_empty()
        && xml_restricted_alternatives.is_empty()
        && file.name == "File"
        && file.repeating
        && !file.attribute
        && file.xml_namespace.is_none()
        && matches!(
            file.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        )
}

fn plain_attribute(node: &SchemaNode, name: &str, ty: ScalarType) -> bool {
    node.name == name
        && !node.repeating
        && node.attribute
        && node.xml_namespace.is_none()
        && matches!(node.kind, SchemaKind::Scalar { ty: actual } if actual == ty)
}

fn next_uid(uid: &mut u32) -> u32 {
    *uid += 1;
    *uid
}

fn concat(output: &mut String, uid: &mut u32, inputs: &[u32; 3], out: u32) {
    let uid = next_uid(uid);
    let _ = write!(
        output,
        "\t\t\t\t<component name=\"concat\" library=\"core\" uid=\"{uid}\" kind=\"5\" growable=\"1\" growablebasename=\"value\"><sources>"
    );
    for (position, key) in inputs.iter().enumerate() {
        let _ = write!(output, "<datapoint pos=\"{position}\" key=\"{key}\"/>");
    }
    let _ = writeln!(
        output,
        "</sources><targets><datapoint pos=\"0\" key=\"{out}\"/></targets></component>"
    );
}
