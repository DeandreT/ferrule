//! Native recursive user-function export for a flat list of delimited paths.
//!
//! The guarded shape is the inverse of the native path-hierarchy importer:
//! one repeated string feeds a recursive directory/file tree. All internal
//! components and connections are generated from the validated IR plan.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{FormatOptions, IterationOutput, Project, ScopeConstruction, ScopeIteration};

use crate::MfdError;

use super::TargetExport;
use super::mapped_sequence::render_edge_metadata;
use super::schema::{KeyAlloc, SideFormat, xml_escape};
use super::source::SourceExports;

const FUNCTION_NAME: &str = "BuildPathHierarchy";

pub(super) struct NativePathHierarchy {
    source_root: u32,
    target_root: u32,
    source_name: String,
    target_name: String,
    collection: String,
    directories: String,
    files: String,
    name: String,
    separator: String,
    source_schema_file: String,
    target_schema_file: String,
}

impl NativePathHierarchy {
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
        let ScopeConstruction::PathHierarchy { plan } = &project.root.construction else {
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
            || !project.graph.nodes.is_empty()
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
            || plan.collection().len() != 1
        {
            return None;
        }
        let SchemaKind::Group {
            children: source_fields,
            ..
        } = &source.schema.kind
        else {
            return None;
        };
        let [source_value] = source_fields.as_slice() else {
            return None;
        };
        if source_value.name != plan.collection()[0]
            || !source_value.repeating
            || !matches!(
                source_value.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            )
            || source.schema.xml_namespace.is_some()
            || source_value.xml_namespace.is_some()
        {
            return None;
        }
        let SchemaKind::Group {
            children: target_fields,
            ..
        } = &target.schema.kind
        else {
            return None;
        };
        if target_fields.len() != 3 || target.schema.xml_namespace.is_some() {
            return None;
        }
        let directories = target.schema.child(plan.directories())?;
        let files = target.schema.child(plan.files())?;
        let name = target.schema.child(plan.name())?;
        if !directories.repeating
            || directories.recursive_ref.as_deref() != Some(target.schema.name.as_str())
            || !matches!(directories.kind, SchemaKind::Group { .. })
            || directories.xml_namespace.is_some()
            || !files.repeating
            || files.recursive_ref.is_some()
            || !matches!(files.kind, SchemaKind::Group { .. })
            || files.xml_namespace.is_some()
            || !plain_string_attribute(name)
            || !files.child(plan.name()).is_some_and(plain_string_attribute)
        {
            return None;
        }
        let source_root = source.ports.key_for_abs(&[])?;
        let target_root = target.ports.key_for_abs(&[])?;
        let stem = mfd_path.file_stem()?.to_str()?;
        Some(Self {
            source_root,
            target_root,
            source_name: source.schema.name.clone(),
            target_name: target.schema.name.clone(),
            collection: source_value.name.clone(),
            directories: plan.directories().to_string(),
            files: plan.files().to_string(),
            name: plan.name().to_string(),
            separator: plan.separator().to_string(),
            source_schema_file: format!("{stem}-{}.xsd", source.sibling_suffix),
            target_schema_file: format!("{stem}-{}.xsd", target.sibling_suffix),
        })
    }

    pub(super) fn render(
        &self,
        keys: &mut KeyAlloc,
        uid: &mut u32,
        components: &mut String,
        edges: &mut Vec<(u32, u32)>,
        structural_edges: &mut BTreeSet<(u32, u32)>,
    ) -> Result<String, MfdError> {
        let definition_uid = next_uid(uid);
        let input_uid = next_uid(uid);
        let output_uid = next_uid(uid);
        let call_uid = next_uid(uid);
        let call_input = keys.next();
        let call_output = keys.next();
        let source_name = xml_escape(&self.source_name);
        let target_name = xml_escape(&self.target_name);
        let directories = xml_escape(&self.directories);
        let _ = write!(
            components,
            "\t\t\t\t<component name=\"{FUNCTION_NAME}\" library=\"user\" uid=\"{call_uid}\" kind=\"19\">\n\
             \t\t\t\t\t<data><root><entry name=\"{source_name}\" componentid=\"{input_uid}\"><entry name=\"{source_name}\" inpkey=\"{call_input}\"/></entry></root>\
             <root rootindex=\"1\"><entry name=\"{target_name}\" componentid=\"{output_uid}\"><entry name=\"{target_name}\"><entry name=\"{directories}\" outkey=\"{call_output}\"/></entry></entry></root></data>\n\
             \t\t\t\t</component>\n"
        );
        let input_edge = (self.source_root, call_input);
        let output_edge = (call_output, self.target_root);
        edges.extend([input_edge, output_edge]);
        structural_edges.extend([input_edge, output_edge]);
        self.definition(keys, uid, definition_uid, input_uid, output_uid)
    }

    fn definition(
        &self,
        keys: &mut KeyAlloc,
        uid: &mut u32,
        definition_uid: u32,
        input_uid: u32,
        output_uid: u32,
    ) -> Result<String, MfdError> {
        let source_value = keys.next();
        let file_target = keys.next();
        let file_name_target = keys.next();
        let directory_target = keys.next();
        let directory_name_target = keys.next();
        let recursive_file_target = keys.next();
        let recursive_directory_target = keys.next();
        let contains_inputs = [keys.next(), keys.next()];
        let contains_output = keys.next();
        let separator_output = keys.next();
        let filter_inputs = [keys.next(), keys.next()];
        let filter_outputs = [keys.next(), keys.next()];
        let before_inputs = [keys.next(), keys.next()];
        let before_output = keys.next();
        let group_inputs = [keys.next(), keys.next()];
        let group_outputs = [keys.next(), keys.next()];
        let after_inputs = [keys.next(), keys.next()];
        let after_output = keys.next();
        let recursive_input = keys.next();
        let recursive_files = keys.next();
        let recursive_directories = keys.next();

        let mut children = String::new();
        let source_name = xml_escape(&self.source_name);
        let target_name = xml_escape(&self.target_name);
        let collection = xml_escape(&self.collection);
        let directories = xml_escape(&self.directories);
        let files = xml_escape(&self.files);
        let name = xml_escape(&self.name);
        let source_schema = xml_escape(&self.source_schema_file);
        let target_schema = xml_escape(&self.target_schema_file);
        let separator = xml_escape(&self.separator);
        let _ = write!(
            children,
            "\t\t\t\t<component name=\"{source_name}\" library=\"xml\" uid=\"{input_uid}\" kind=\"14\"><data>\
             <root><entry name=\"{source_name}\"><entry name=\"{collection}\" outkey=\"{source_value}\"/></entry></root>\
             <document schema=\"{source_schema}\" instanceroot=\"{{}}{source_name}\"/>\
             <parameter usageKind=\"input\" name=\"{source_name}\"/></data></component>\n\
             \t\t\t\t<component name=\"{target_name}\" library=\"xml\" uid=\"{output_uid}\" kind=\"14\"><data>\
             <root><entry name=\"{target_name}\">\
             <entry name=\"{files}\" inpkey=\"{file_target}\"><entry name=\"{name}\" type=\"attribute\" inpkey=\"{file_name_target}\"/></entry>\
             <entry name=\"{directories}\" inpkey=\"{directory_target}\"><entry name=\"{name}\" type=\"attribute\" inpkey=\"{directory_name_target}\"/><entry name=\"{files}\" inpkey=\"{recursive_file_target}\"/><entry name=\"{directories}\" inpkey=\"{recursive_directory_target}\"/></entry>\
             </entry></root><document schema=\"{target_schema}\" instanceroot=\"{{}}{target_name}\"/>\
             <parameter usageKind=\"output\" name=\"{target_name}\"/></data></component>\n"
        );
        core_function(
            &mut children,
            uid,
            "contains",
            5,
            &contains_inputs,
            &[contains_output],
        );
        let _ = writeln!(
            children,
            "\t\t\t\t<component name=\"constant\" library=\"core\" uid=\"{}\" kind=\"2\"><targets><datapoint pos=\"0\" key=\"{separator_output}\"/></targets><data><constant value=\"{separator}\" datatype=\"string\"/></data></component>",
            next_uid(uid)
        );
        core_function(
            &mut children,
            uid,
            &self.collection,
            3,
            &filter_inputs,
            &filter_outputs,
        );
        core_function(
            &mut children,
            uid,
            "substring-before",
            5,
            &before_inputs,
            &[before_output],
        );
        core_function(
            &mut children,
            uid,
            "group-by",
            5,
            &group_inputs,
            &group_outputs,
        );
        core_function(
            &mut children,
            uid,
            "substring-after",
            5,
            &after_inputs,
            &[after_output],
        );
        let self_uid = next_uid(uid);
        let _ = writeln!(
            children,
            "\t\t\t\t<component name=\"{FUNCTION_NAME}\" library=\"user\" uid=\"{self_uid}\" kind=\"19\"><data>\
             <root><entry name=\"{source_name}\" componentid=\"{input_uid}\"><entry name=\"{source_name}\"><entry name=\"{collection}\" inpkey=\"{recursive_input}\"/></entry></entry></root>\
             <root rootindex=\"1\"><entry name=\"{target_name}\" componentid=\"{output_uid}\"><entry name=\"{target_name}\"><entry name=\"{files}\" outkey=\"{recursive_files}\"/><entry name=\"{directories}\" outkey=\"{recursive_directories}\"/></entry></entry></root>\
             </data></component>"
        );

        let edges = vec![
            (source_value, contains_inputs[0]),
            (source_value, filter_inputs[0]),
            (source_value, before_inputs[0]),
            (source_value, after_inputs[0]),
            (source_value, file_name_target),
            (separator_output, contains_inputs[1]),
            (separator_output, before_inputs[1]),
            (separator_output, after_inputs[1]),
            (contains_output, filter_inputs[1]),
            (filter_outputs[0], group_inputs[0]),
            (filter_outputs[1], file_target),
            (before_output, group_inputs[1]),
            (group_outputs[0], directory_target),
            (group_outputs[1], directory_name_target),
            (after_output, recursive_input),
            (recursive_files, recursive_file_target),
            (recursive_directories, recursive_directory_target),
        ];
        let structural = BTreeSet::from([
            (recursive_files, recursive_file_target),
            (recursive_directories, recursive_directory_target),
        ]);
        let (edge_keys, edge_metadata) = render_edge_metadata(&structural, keys);
        let mut vertices = String::new();
        let mut grouped = BTreeMap::<u32, Vec<u32>>::new();
        for (from, to) in edges {
            grouped.entry(from).or_default().push(to);
        }
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
        Ok(format!(
            "\t<component name=\"{FUNCTION_NAME}\" library=\"user\" uid=\"{definition_uid}\" editable=\"1\">\n\
             \t\t<structure>\n\t\t\t<children>\n{children}\t\t\t</children>\n\
             \t\t\t<graph directed=\"1\">\n{edge_metadata}\t\t\t\t<vertices>\n{vertices}\t\t\t\t</vertices>\n\t\t\t</graph>\n\
             \t\t</structure>\n\t</component>\n"
        ))
    }
}

fn plain_string_attribute(node: &SchemaNode) -> bool {
    !node.repeating
        && node.attribute
        && node.xml_namespace.is_none()
        && matches!(
            node.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        )
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
