//! Restore native XML-valued database columns for a direct group-to-column wire.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{IterationOutput, Node, NodeId, Project, ScopeConstruction, ScopeIteration};
use roxmltree::Document;

use super::TargetExport;
use super::schema::{
    DbLayout, GeneratedSibling, PortMatch, RenderedSchemaComponent, SideFormat, db_layout,
    xml_escape,
};
use super::source::SourceExports;
use crate::MfdError;

struct DirectColumn {
    node: NodeId,
    name: String,
    source_port: u32,
    target_port: u32,
    schema: SchemaNode,
}

#[derive(Default)]
pub(super) struct DirectColumns {
    by_target: BTreeMap<usize, Vec<DirectColumn>>,
    source_ports: BTreeMap<NodeId, u32>,
}

impl DirectColumns {
    pub(super) fn plan(
        project: &Project,
        sources: &SourceExports<'_>,
        targets: &[TargetExport<'_>],
        mixed_database_pairs: &[(usize, usize)],
    ) -> Self {
        let mut plan = Self::default();
        for (target_index, target) in targets.iter().enumerate() {
            if target.format != SideFormat::Db
                || target.dynamic_json.is_some()
                || !plain_row_scope(target.root)
                || target.branches.count(&[]).is_some()
                || mixed_database_pairs
                    .iter()
                    .any(|(_, paired_target)| *paired_target == target_index)
                || !matches!(db_layout(target.schema), Some(DbLayout::Table(_)))
            {
                continue;
            }
            for (binding_index, binding) in target.root.bindings.iter().enumerate() {
                let Some(Node::XmlSerialize {
                    path,
                    frame,
                    schema,
                    declaration: false,
                    indent: false,
                    namespace: None,
                }) = project.graph.nodes.get(&binding.node)
                else {
                    continue;
                };
                let Some(field) = target.schema.child(&binding.target_field) else {
                    continue;
                };
                if !plain_string_column(field)
                    || target
                        .root
                        .bindings
                        .iter()
                        .filter(|other| other.target_field == binding.target_field)
                        .count()
                        != 1
                    || plan.source_ports.contains_key(&binding.node)
                {
                    continue;
                }
                let column_path = [binding.target_field.clone()];
                let Some(target_port) = target.ports.key_for_abs(&column_path) else {
                    continue;
                };
                let mut source_path = frame.clone().unwrap_or_default();
                source_path.extend(path.iter().cloned());
                if source_path.is_empty() {
                    continue;
                }
                let PortMatch::Unique(source_port) = sources.match_sequence(&source_path) else {
                    continue;
                };
                if let ScopeIteration::Source(scope_source) = &target.root.iteration {
                    if scope_source.is_empty() {
                        let Some(root_schema) = sources.schema_node_at(&[]) else {
                            continue;
                        };
                        let SchemaKind::Group { children, .. } = &root_schema.kind else {
                            continue;
                        };
                        if source_path.len() != 1
                            || children.len() != 1
                            || children[0].name != source_path[0]
                            || !children[0].repeating
                            || !matches!(children[0].kind, SchemaKind::Group { .. })
                        {
                            continue;
                        }
                    } else if sources.key_for_abs(scope_source) != Some(source_port) {
                        continue;
                    }
                }
                let Some(source_schema) = sources.schema_node_at(&source_path) else {
                    continue;
                };
                let mut source_schema = source_schema.clone();
                source_schema.repeating = false;
                if source_schema != **schema {
                    continue;
                }
                if format_xml::xsd::export_set(schema, "native-database-xml-check.xsd")
                    .map_or(true, |exported| exported.namespace.is_some())
                {
                    continue;
                }

                // The serializer may also feed a graph expression, control,
                // failure rule, or another output. A direct XML column can
                // replace it only when this one binding is its sole consumer.
                let mut without_binding = project.clone();
                let root = if target_index == 0 {
                    &mut without_binding.root
                } else {
                    &mut without_binding.extra_targets[target_index - 1].root
                };
                root.bindings.remove(binding_index);
                without_binding.prune_unreachable_nodes();
                if without_binding.graph.nodes.contains_key(&binding.node) {
                    continue;
                }

                plan.source_ports.insert(binding.node, source_port);
                plan.by_target
                    .entry(target_index)
                    .or_default()
                    .push(DirectColumn {
                        node: binding.node,
                        name: binding.target_field.clone(),
                        source_port,
                        target_port,
                        schema: (**schema).clone(),
                    });
            }
        }
        plan
    }

    pub(super) fn source_port(&self, node: NodeId) -> Option<u32> {
        self.source_ports.get(&node).copied()
    }

    pub(super) fn mark_structural_edges(
        &self,
        wired_edges: &[(u32, u32)],
        structural_edges: &mut BTreeSet<(u32, u32)>,
    ) -> Result<(), MfdError> {
        for column in self.by_target.values().flatten() {
            let edge = (column.source_port, column.target_port);
            if !wired_edges.contains(&edge) {
                return Err(MfdError::Unsupported(format!(
                    "native database XML column `{}` lost its direct source connection",
                    column.name
                )));
            }
            structural_edges.insert(edge);
        }
        Ok(())
    }

    pub(super) fn apply_to_target(
        &self,
        target_index: usize,
        mfd_path: &Path,
        sibling_suffix: &str,
        rendered: &mut RenderedSchemaComponent,
    ) -> Result<(), MfdError> {
        let Some(columns) = self.by_target.get(&target_index) else {
            return Ok(());
        };
        let stem = mfd_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("mapping");
        for column in columns {
            let schema_file = format!("{stem}-{sibling_suffix}-database-xml-{}.xsd", column.node);
            let exported = format_xml::xsd::export_set(&column.schema, &schema_file)?;
            if exported.namespace.is_some() {
                return Err(MfdError::Unsupported(format!(
                    "native database XML column `{}` has an unexpected namespace",
                    column.name
                )));
            }
            rendered.xml = patch_column(&rendered.xml, column, &schema_file)?;
            let directory = mfd_path.parent().unwrap_or_else(|| Path::new("."));
            rendered.siblings.push(GeneratedSibling {
                path: directory.join(&schema_file),
                contents: exported.root,
            });
            rendered
                .siblings
                .extend(
                    exported
                        .dependencies
                        .into_iter()
                        .map(|dependency| GeneratedSibling {
                            path: directory.join(dependency.filename),
                            contents: dependency.contents,
                        }),
                );
        }
        Ok(())
    }
}

fn plain_row_scope(scope: &mapping::Scope) -> bool {
    matches!(scope.iteration, ScopeIteration::Source(_))
        && scope.construction == ScopeConstruction::Constructed
        && scope.iteration_output == IterationOutput::Repeated
        && scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && scope.children.is_empty()
        && !scope.merge_dynamic_fields
}

fn plain_string_column(field: &SchemaNode) -> bool {
    !field.repeating
        && !field.attribute
        && !field.text
        && !field.nillable
        && field.fixed.is_none()
        && field.default.is_none()
        && field.value_generation.is_none()
        && field.recursive_ref.is_none()
        && matches!(
            field.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        )
}

fn patch_column(xml: &str, column: &DirectColumn, schema_file: &str) -> Result<String, MfdError> {
    let document = Document::parse(xml)?;
    let port = column.target_port.to_string();
    let matches = document
        .descendants()
        .filter(|entry| {
            entry.has_tag_name("entry") && entry.attribute("inpkey") == Some(port.as_str())
        })
        .collect::<Vec<_>>();
    let [entry] = matches.as_slice() else {
        return Err(MfdError::Unsupported(format!(
            "native database XML column `{}` has no unique target port",
            column.name
        )));
    };
    if entry.attribute("name") != Some(column.name.as_str())
        || entry.attribute("datatype") != Some("string")
        || entry.children().any(|child| child.is_element())
        || entry.parent().and_then(|parent| parent.attribute("type")) != Some("table")
    {
        return Err(MfdError::Unsupported(format!(
            "native database XML column `{}` does not have a plain table entry",
            column.name
        )));
    }
    let root = xml_escape(&column.schema.name);
    let replacement = format!(
        "<entry name=\"{}\" datatype=\"string\">\n\
         \t\t\t\t\t\t\t\t\t\t\t<entry name=\"document\" type=\"doc-xml\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t\t\t\t<document schemafile=\"{}\" root=\"{root}\" encoding=\"UTF-8\"/>\n\
         \t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"{root}\" inpkey=\"{}\"/>\n\
         \t\t\t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t\t\t</entry>",
        xml_escape(&column.name),
        xml_escape(schema_file),
        column.target_port,
    );
    let mut patched = xml.to_string();
    patched.replace_range(entry.range(), &replacement);
    Ok(patched)
}
