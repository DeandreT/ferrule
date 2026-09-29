//! Bounded native XML target casting for direct date-time bindings.

use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaKind, SchemaNode, XML_ELEMENTS_FIELD};
use mapping::{Node, NodeId, Project, ScopeConstruction};
use roxmltree::{Document, Node as XmlNode};

use super::TargetExport;
use super::schema::{RenderedSchemaComponent, SideFormat};
use crate::MfdError;

#[derive(Default)]
pub(super) struct NativeDatetimeCasts {
    calls: BTreeMap<NodeId, NodeId>,
    fields_by_target: BTreeMap<usize, BTreeSet<String>>,
}

impl NativeDatetimeCasts {
    pub(super) fn plan(project: &Project, targets: &[TargetExport<'_>]) -> Self {
        let mut plan = Self::default();
        let mut bindings = Vec::new();
        for (target_index, target) in targets.iter().enumerate() {
            if target.format != SideFormat::Xml
                || target.root.construction != ScopeConstruction::Constructed
                || target.dynamic_json.is_some()
                || target.options.wsdl.is_some()
                || !closed_element_subtree(target.schema, true)
            {
                continue;
            }
            for (binding_index, binding) in target.root.bindings.iter().enumerate() {
                let Some(Node::Call { function, args }) = project.graph.nodes.get(&binding.node)
                else {
                    continue;
                };
                let [argument] = args.as_slice() else {
                    continue;
                };
                if function != "coerce_datetime"
                    || target
                        .root
                        .bindings
                        .iter()
                        .filter(|other| other.target_field == binding.target_field)
                        .count()
                        != 1
                    || target
                        .branches
                        .count(std::slice::from_ref(&binding.target_field))
                        .is_some()
                    || target
                        .schema
                        .child(&binding.target_field)
                        .is_none_or(|field| {
                            field.repeating
                                || field.attribute
                                || field.text
                                || !matches!(
                                    field.kind,
                                    SchemaKind::Scalar {
                                        ty: ScalarType::String
                                    }
                                )
                        })
                    || target
                        .ports
                        .key_for_abs(std::slice::from_ref(&binding.target_field))
                        .is_none()
                    || plan.calls.contains_key(&binding.node)
                {
                    continue;
                }
                plan.calls.insert(binding.node, *argument);
                plan.fields_by_target
                    .entry(target_index)
                    .or_default()
                    .insert(binding.target_field.clone());
                bindings.push((target_index, binding_index, binding.node, *argument));
            }
        }
        if plan.calls.is_empty() || plan.calls.values().any(|id| plan.calls.contains_key(id)) {
            return Self::default();
        }

        // Replacing the selected target bindings must make every call
        // unreachable. This checks graph consumers, controls, other targets,
        // and failure rules without guessing which expressions may use it.
        let mut without_casts = project.clone();
        for &(target_index, binding_index, _, argument) in &bindings {
            let root = if target_index == 0 {
                &mut without_casts.root
            } else {
                &mut without_casts.extra_targets[target_index - 1].root
            };
            root.bindings[binding_index].node = argument;
        }
        without_casts.prune_unreachable_nodes();
        if bindings
            .iter()
            .any(|&(_, _, call, _)| without_casts.graph.nodes.contains_key(&call))
        {
            return Self::default();
        }

        // The generated XSD must have one direct string element at each
        // selected path so its dateTime type can be restored exactly.
        for (&target_index, fields) in &plan.fields_by_target {
            let target = &targets[target_index];
            let Ok(schema) = format_xml::xsd::export_set(target.schema, "native-cast-check.xsd")
            else {
                return Self::default();
            };
            if patch_xsd(&schema.root, target.schema, fields).is_err() {
                return Self::default();
            }
        }
        plan
    }

    pub(super) fn calls(&self) -> &BTreeMap<NodeId, NodeId> {
        &self.calls
    }

    pub(super) fn apply_to_target(
        &self,
        target_index: usize,
        schema: &SchemaNode,
        rendered: &mut RenderedSchemaComponent,
    ) -> Result<(), MfdError> {
        let Some(fields) = self.fields_by_target.get(&target_index) else {
            return Ok(());
        };
        let document = Document::parse(&rendered.xml)?;
        let schema_file = document
            .descendants()
            .find(|node| node.has_tag_name("document"))
            .and_then(|node| node.attribute("schema"))
            .ok_or_else(|| unsupported("rendered XML target has no schema reference"))?;
        let sibling = rendered
            .siblings
            .iter_mut()
            .find(|sibling| {
                sibling
                    .path
                    .file_name()
                    .is_some_and(|name| name == schema_file)
            })
            .ok_or_else(|| unsupported("rendered XML target schema is missing"))?;
        sibling.contents = patch_xsd(&sibling.contents, schema, fields)?;
        rendered.xml = patch_document_entry(&rendered.xml)?;
        Ok(())
    }
}

// The document flag applies to the complete subtree. Limit its use to a
// closed element tree whose other leaves retain their ordinary XSD scalar
// types and XML-writer coercions. Defaults, fixed values, XML alternatives,
// and wildcards need more provenance than the project currently carries.
fn closed_element_subtree(schema: &SchemaNode, is_root: bool) -> bool {
    if (is_root && schema.repeating)
        || schema.attribute
        || schema.text
        || schema.fixed.is_some()
        || schema.default.is_some()
        || schema.value_generation.is_some()
        || schema.recursive_ref.is_some()
        || !schema.xml_name_alternatives.is_empty()
        || schema.xml_wildcard_namespace.is_some()
        || schema.xml_type_alternatives
        || !schema.xml_repeating_sequences.is_empty()
        || !schema.xml_repeating_choices.is_empty()
        || schema.name == XML_ELEMENTS_FIELD
    {
        return false;
    }
    match &schema.kind {
        SchemaKind::Scalar { .. } => true,
        SchemaKind::ScalarUnion { .. } => false,
        SchemaKind::Group {
            children,
            alternatives,
            xml_restricted_alternatives,
            dynamic,
            ..
        } => {
            alternatives.is_empty()
                && xml_restricted_alternatives.is_empty()
                && dynamic.is_none()
                && children
                    .iter()
                    .all(|child| closed_element_subtree(child, false))
        }
    }
}

fn unsupported(reason: &str) -> MfdError {
    MfdError::Unsupported(format!("native date-time target casting: {reason}"))
}

fn patch_document_entry(xml: &str) -> Result<String, MfdError> {
    let document = Document::parse(xml)?;
    let entries = document
        .descendants()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("document"))
        .collect::<Vec<_>>();
    let [entry] = entries.as_slice() else {
        return Err(unsupported("target document entry is not unique"));
    };
    if entry.attribute("casttotargettypemode").is_some() {
        return Err(unsupported("target document already declares a cast mode"));
    }
    let open_end = xml[entry.range()]
        .find('>')
        .ok_or_else(|| unsupported("target document entry is not closed"))?
        + entry.range().start;
    let mut patched = xml.to_string();
    patched.insert_str(open_end, " casttotargettypemode=\"cast-in-subtree\"");
    Ok(patched)
}

fn patch_xsd(
    xml: &str,
    schema: &SchemaNode,
    fields: &BTreeSet<String>,
) -> Result<String, MfdError> {
    let document = Document::parse(xml)?;
    let roots = document
        .root_element()
        .children()
        .filter(|node| {
            is_xsd(*node, "element") && node.attribute("name") == Some(schema.name.as_str())
        })
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(unsupported("generated XML Schema root is not unique"));
    };
    let sequence = child(*root, "complexType")
        .and_then(|node| child(node, "sequence"))
        .ok_or_else(|| unsupported("generated XML Schema root has no direct sequence"))?;
    let mut replacements = Vec::new();
    for field in fields {
        let entries = sequence
            .children()
            .filter(|node| {
                is_xsd(*node, "element") && node.attribute("name") == Some(field.as_str())
            })
            .collect::<Vec<_>>();
        let [entry] = entries.as_slice() else {
            return Err(unsupported("generated date-time element is not unique"));
        };
        let attribute = entry
            .attributes()
            .find(|attribute| attribute.name() == "type" && attribute.value() == "xs:string")
            .ok_or_else(|| unsupported("generated date-time element is not a string scalar"))?;
        replacements.push(attribute.range());
    }
    let mut patched = xml.to_string();
    replacements.sort_by_key(|range| std::cmp::Reverse(range.start));
    for range in replacements {
        patched.replace_range(range, "type=\"xs:dateTime\"");
    }
    Ok(patched)
}

fn child<'a, 'input>(node: XmlNode<'a, 'input>, name: &str) -> Option<XmlNode<'a, 'input>> {
    node.children().find(|child| is_xsd(*child, name))
}

fn is_xsd(node: XmlNode<'_, '_>, name: &str) -> bool {
    node.is_element()
        && node.tag_name().namespace() == Some("http://www.w3.org/2001/XMLSchema")
        && node.tag_name().name() == name
}
