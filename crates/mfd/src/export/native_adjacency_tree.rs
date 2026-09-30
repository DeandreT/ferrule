//! Guarded native recursive user-function export for a flat adjacency catalog.
//!
//! One repeated row group supplies a string key and optional string parent.
//! The generated function selects the root with no parent, then recurses over
//! rows whose parent equals the current key. Other adjacency plans retain the
//! portable Ferrule component rather than claiming native compatibility.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{FormatOptions, IterationOutput, Project, ScopeConstruction, ScopeIteration};

use super::TargetExport;
use super::mapped_sequence::render_edge_metadata;
use super::schema::{KeyAlloc, SideFormat, xml_escape};
use super::source::SourceExports;

const FUNCTION_NAME: &str = "BuildAdjacencyTree";
const BASE_NAME: &str = "base";

pub(super) struct NativeAdjacencyTree {
    source_root: u32,
    target_root: u32,
    source_name: String,
    target_name: String,
    collection: String,
    key: String,
    parent: String,
    target_key: String,
    target_children: String,
    source_schema_file: String,
    target_schema_file: String,
}

impl NativeAdjacencyTree {
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
        let ScopeConstruction::AdjacencyTree { plan } = &project.root.construction else {
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
            || plan.root().is_some()
            || plan.collection().len() != 1
            || plan.key().len() != 1
            || plan.parent().len() != 1
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
        let [rows] = source_fields.as_slice() else {
            return None;
        };
        let SchemaKind::Group {
            children: row_fields,
            ..
        } = &rows.kind
        else {
            return None;
        };
        let [key, parent] = row_fields.as_slice() else {
            return None;
        };
        if source.schema.xml_namespace.is_some()
            || rows.xml_namespace.is_some()
            || !rows.repeating
            || rows.recursive_ref.is_some()
            || rows.name != plan.collection()[0]
            || key.name != plan.key()[0]
            || parent.name != plan.parent()[0]
            || !plain_string_attribute(key)
            || !plain_string_attribute(parent)
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
        if target.schema.xml_namespace.is_some() || target_fields.len() != 2 {
            return None;
        }
        let target_key = target.schema.child(plan.target_key())?;
        let target_children = target.schema.child(plan.target_children())?;
        if !plain_string_attribute(target_key)
            || !target_children.repeating
            || !matches!(target_children.kind, SchemaKind::Group { .. })
            || target_children.recursive_ref.as_deref() != Some(target.schema.name.as_str())
            || target_children.xml_namespace.is_some()
        {
            return None;
        }
        let stem = mfd_path.file_stem()?.to_str()?;
        Some(Self {
            source_root: source.ports.key_for_abs(&[])?,
            target_root: target.ports.key_for_abs(&[])?,
            source_name: source.schema.name.clone(),
            target_name: target.schema.name.clone(),
            collection: rows.name.clone(),
            key: key.name.clone(),
            parent: parent.name.clone(),
            target_key: target_key.name.clone(),
            target_children: target_children.name.clone(),
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
    ) -> String {
        let definition_uid = next_uid(uid);
        let input_uid = next_uid(uid);
        let output_uid = next_uid(uid);
        let base_uid = next_uid(uid);
        let call_uid = next_uid(uid);
        let call_catalog = keys.next();
        let call_output = keys.next();
        let source_name = xml_escape(&self.source_name);
        let target_name = xml_escape(&self.target_name);
        let _ = writeln!(
            components,
            "\t\t\t\t<component name=\"{FUNCTION_NAME}\" library=\"user\" uid=\"{call_uid}\" kind=\"19\"><data>\
             <root><entry name=\"{source_name}\" componentid=\"{input_uid}\" inpkey=\"{call_catalog}\"/></root>\
             <root><entry name=\"{BASE_NAME}\" componentid=\"{base_uid}\"/></root>\
             <root rootindex=\"2\"><entry name=\"{target_name}\" componentid=\"{output_uid}\" outkey=\"{call_output}\"/></root>\
             </data></component>"
        );
        let input_edge = (self.source_root, call_catalog);
        let output_edge = (call_output, self.target_root);
        edges.extend([input_edge, output_edge]);
        structural_edges.extend([input_edge, output_edge]);
        self.definition(keys, uid, definition_uid, input_uid, output_uid, base_uid)
    }

    fn definition(
        &self,
        keys: &mut KeyAlloc,
        uid: &mut u32,
        definition_uid: u32,
        input_uid: u32,
        output_uid: u32,
        base_uid: u32,
    ) -> String {
        let catalog = keys.next();
        let rows = keys.next();
        let key = keys.next();
        let parent = keys.next();
        let target_root = keys.next();
        let target_key = keys.next();
        let target_children = keys.next();
        let base = keys.next();
        let self_catalog = keys.next();
        let self_base = keys.next();
        let self_output = keys.next();
        let exists_input = keys.next();
        let exists_output = keys.next();
        let equal_inputs = [keys.next(), keys.next()];
        let equal_output = keys.next();
        let not_exists_input = keys.next();
        let not_exists_output = keys.next();
        let choose_inputs = [keys.next(), keys.next(), keys.next()];
        let choose_output = keys.next();
        let filter_inputs = [keys.next(), keys.next()];
        let filter_output = keys.next();

        let source_name = xml_escape(&self.source_name);
        let target_name = xml_escape(&self.target_name);
        let collection = xml_escape(&self.collection);
        let key_name = xml_escape(&self.key);
        let parent_name = xml_escape(&self.parent);
        let target_key_name = xml_escape(&self.target_key);
        let target_children_name = xml_escape(&self.target_children);
        let source_schema = xml_escape(&self.source_schema_file);
        let target_schema = xml_escape(&self.target_schema_file);
        let mut children = String::new();
        let _ = writeln!(
            children,
            "\t\t\t\t<component name=\"{source_name}\" library=\"xml\" uid=\"{input_uid}\" kind=\"14\"><data>\
             <root><entry name=\"{source_name}\" outkey=\"{catalog}\"><entry name=\"{collection}\" outkey=\"{rows}\"><entry name=\"{key_name}\" type=\"attribute\" outkey=\"{key}\"/><entry name=\"{parent_name}\" type=\"attribute\" outkey=\"{parent}\"/></entry></entry></root>\
             <document schema=\"{source_schema}\" instanceroot=\"{{}}{source_name}\"/><parameter usageKind=\"input\" name=\"{source_name}\"/>\
             </data></component>"
        );
        let _ = writeln!(
            children,
            "\t\t\t\t<component name=\"{target_name}\" library=\"xml\" uid=\"{output_uid}\" kind=\"14\"><data>\
             <root><entry name=\"{target_name}\" inpkey=\"{target_root}\"><entry name=\"{target_key_name}\" type=\"attribute\" inpkey=\"{target_key}\"/><entry name=\"{target_children_name}\" inpkey=\"{target_children}\"/></entry></root>\
             <document schema=\"{target_schema}\" instanceroot=\"{{}}{target_name}\"/><parameter usageKind=\"output\" name=\"{target_name}\"/>\
             </data></component>"
        );
        let _ = writeln!(
            children,
            "\t\t\t\t<component name=\"{BASE_NAME}\" library=\"core\" uid=\"{base_uid}\" kind=\"6\"><targets><datapoint pos=\"0\" key=\"{base}\"/></targets><data><input datatype=\"string\"/><parameter usageKind=\"input\" name=\"{BASE_NAME}\" optional=\"1\"/></data></component>"
        );
        core_function(
            &mut children,
            uid,
            "exists",
            5,
            &[exists_input],
            &[exists_output],
        );
        core_function(
            &mut children,
            uid,
            "equal",
            5,
            &equal_inputs,
            &[equal_output],
        );
        core_function(
            &mut children,
            uid,
            "not-exists",
            5,
            &[not_exists_input],
            &[not_exists_output],
        );
        core_function(
            &mut children,
            uid,
            "if-else",
            4,
            &choose_inputs,
            &[choose_output],
        );
        core_function(
            &mut children,
            uid,
            &self.collection,
            3,
            &filter_inputs,
            &[filter_output],
        );
        let self_uid = next_uid(uid);
        let _ = writeln!(
            children,
            "\t\t\t\t<component name=\"{FUNCTION_NAME}\" library=\"user\" uid=\"{self_uid}\" kind=\"19\"><data>\
             <root><entry name=\"{source_name}\" componentid=\"{input_uid}\" inpkey=\"{self_catalog}\"/></root>\
             <root><entry name=\"{BASE_NAME}\" componentid=\"{base_uid}\" inpkey=\"{self_base}\"/></root>\
             <root rootindex=\"2\"><entry name=\"{target_name}\" componentid=\"{output_uid}\" outkey=\"{self_output}\"/></root>\
             </data></component>"
        );

        let edges = [
            (catalog, self_catalog),
            (rows, filter_inputs[0]),
            (key, self_base),
            (key, target_key),
            (parent, equal_inputs[1]),
            (parent, not_exists_input),
            (base, exists_input),
            (base, equal_inputs[0]),
            (exists_output, choose_inputs[0]),
            (equal_output, choose_inputs[1]),
            (not_exists_output, choose_inputs[2]),
            (choose_output, filter_inputs[1]),
            (filter_output, target_root),
            (self_output, target_children),
        ];
        let structural = BTreeSet::from([
            (catalog, self_catalog),
            (filter_output, target_root),
            (self_output, target_children),
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
             \t\t<structure>\n\t\t\t<children>\n{children}\t\t\t</children>\n\
             \t\t\t<graph directed=\"1\">\n{edge_metadata}\t\t\t\t<vertices>\n{vertices}\t\t\t\t</vertices>\n\t\t\t</graph>\n\
             \t\t</structure>\n\t</component>\n"
        )
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
