use super::*;

mod alternatives;

use alternatives::AlternativeExportPlan;

mod set;

pub use set::{XsdExportArtifact, XsdExportSet, export_set};

#[derive(Clone)]
pub(super) struct ExternalReference {
    attribute: bool,
    namespace: String,
    name: String,
    prefix: String,
    location: String,
}

/// Renders a [`SchemaNode`] as XSD text -- the inverse of [`import`],
/// producing the same `xs:element`/`xs:complexType`/`xs:sequence` subset it
/// reads (repeating nodes get `maxOccurs="unbounded"`). Returns an error when
/// XML role flags describe a shape this subset cannot preserve.
pub fn export_namespace(schema: &SchemaNode) -> Result<Option<String>, XmlFormatError> {
    let alternatives = AlternativeExportPlan::build(schema, &[])?;
    export_target_namespace(schema, &alternatives)
}

pub fn export(schema: &SchemaNode) -> Result<String, XmlFormatError> {
    export_document(schema, &[])
}

pub(super) fn export_document(
    schema: &SchemaNode,
    external_references: &[ExternalReference],
) -> Result<String, XmlFormatError> {
    crate::instance::validate_namespace_siblings(schema)?;
    let recursive_anchors = recursive_export_anchors(schema)?;
    let mut alternatives = AlternativeExportPlan::build(schema, external_references)?;
    let namespace = export_target_namespace(schema, &alternatives)?;
    alternatives.set_export_namespace(namespace.clone());
    validate_namespace_tree(schema, namespace.as_deref(), &alternatives)?;
    validate_export_node(schema, true, &schema.name, &recursive_anchors)?;
    let legacy_name_namespace = if namespace.is_some() && has_legacy_xml_names(schema) {
        format!(" xmlns:ferruleName=\"{LEGACY_NAME_NAMESPACE}\"")
    } else {
        String::new()
    };
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"{}{legacy_name_namespace} elementFormDefault=\"unqualified\" attributeFormDefault=\"unqualified\">\n",
        alternatives.schema_attributes(),
    );
    alternatives.write_imports(&mut out);
    for (anchor, node) in &recursive_anchors {
        write_complex_type(
            node,
            1,
            Some(&recursive_type_name(anchor)),
            &schema.name,
            &recursive_anchors,
            &alternatives,
            &mut out,
        )?;
    }
    alternatives.write_definitions(&schema.name, &recursive_anchors, &mut out)?;
    write_element(
        schema,
        1,
        &schema.name,
        &recursive_anchors,
        &alternatives,
        &mut out,
    )?;
    out.push_str("</xs:schema>\n");
    Ok(out)
}

fn has_legacy_xml_names(node: &SchemaNode) -> bool {
    (!node.text && node.xml_namespace.is_none())
        || matches!(
            &node.kind,
            ir::SchemaKind::Group { children, .. }
                if children.iter().any(has_legacy_xml_names)
        )
}

fn export_target_namespace(
    schema: &SchemaNode,
    alternatives: &AlternativeExportPlan<'_>,
) -> Result<Option<String>, XmlFormatError> {
    let alternative_namespace = alternatives.namespace();
    match &schema.xml_namespace {
        Some(XmlNamespace::Qualified(namespace)) => {
            if let Some(alternative_namespace) = alternative_namespace
                && alternative_namespace != namespace.as_str()
            {
                return Err(namespace_export_error(
                    schema,
                    alternative_namespace,
                    namespace.as_str(),
                ));
            }
            Ok(Some(namespace.as_str().to_string()))
        }
        Some(XmlNamespace::Unqualified) if alternative_namespace.is_some() => Err(
            namespace_export_error(schema, "", alternative_namespace.unwrap_or_default()),
        ),
        Some(XmlNamespace::Unqualified) => Ok(None),
        None => Ok(alternative_namespace.map(str::to_string)),
    }
}

fn validate_namespace_tree(
    node: &SchemaNode,
    target_namespace: Option<&str>,
    alternatives: &AlternativeExportPlan<'_>,
) -> Result<(), XmlFormatError> {
    if let Some(XmlNamespace::Qualified(namespace)) = &node.xml_namespace
        && Some(namespace.as_str()) != target_namespace
    {
        if alternatives.external_prefix(node).is_some() {
            return Ok(());
        }
        return Err(namespace_export_error(
            node,
            namespace.as_str(),
            target_namespace.unwrap_or_default(),
        ));
    }
    if let ir::SchemaKind::Group { children, .. } = &node.kind {
        for child in children {
            validate_namespace_tree(child, target_namespace, alternatives)?;
        }
    }
    Ok(())
}

fn namespace_export_error(
    node: &SchemaNode,
    namespace: &str,
    target_namespace: &str,
) -> XmlFormatError {
    XmlFormatError::UnsupportedNamespaceExport {
        node: node.name.clone(),
        namespace: namespace.to_string(),
        target_namespace: target_namespace.to_string(),
    }
}

fn recursive_export_anchors(
    schema: &SchemaNode,
) -> Result<BTreeMap<String, &SchemaNode>, XmlFormatError> {
    let mut references = BTreeMap::new();
    collect_recursive_references(schema, &schema.name, &mut references);
    let mut anchors = BTreeMap::new();
    for (anchor, node) in references {
        let mut candidates = Vec::new();
        collect_concrete_anchors(schema, &anchor, &mut candidates);
        let Some(candidate) = candidates.first().copied() else {
            return Err(XmlFormatError::UnsupportedRecursiveAnchor { node, anchor });
        };
        if !candidates
            .iter()
            .skip(1)
            .all(|other| same_recursive_anchor_definition(candidate, other))
        {
            return Err(XmlFormatError::UnsupportedRecursiveAnchor { node, anchor });
        }
        anchors.insert(anchor, candidate);
    }
    Ok(anchors)
}

fn same_recursive_anchor_definition(left: &SchemaNode, right: &SchemaNode) -> bool {
    left.name == right.name
        && left.xml_namespace == right.xml_namespace
        && left.recursive_ref == right.recursive_ref
        && left.attribute == right.attribute
        && left.text == right.text
        && left.fixed == right.fixed
        && left.value_generation == right.value_generation
        && left.alternative_mode == right.alternative_mode
        && left.xml_repeating_sequences == right.xml_repeating_sequences
        && left.kind == right.kind
}

fn collect_recursive_references(
    node: &SchemaNode,
    root_name: &str,
    references: &mut BTreeMap<String, String>,
) {
    if let Some(anchor) = &node.recursive_ref {
        if anchor != root_name {
            references
                .entry(anchor.clone())
                .or_insert_with(|| node.name.clone());
        }
        return;
    }
    if let ir::SchemaKind::Group { children, .. } = &node.kind {
        for child in children {
            collect_recursive_references(child, root_name, references);
        }
    }
}

fn collect_concrete_anchors<'a>(
    node: &'a SchemaNode,
    anchor: &str,
    candidates: &mut Vec<&'a SchemaNode>,
) {
    if node.recursive_ref.is_some() {
        return;
    }
    let ir::SchemaKind::Group { children, .. } = &node.kind else {
        return;
    };
    if node.name == anchor {
        candidates.push(node);
    }
    for child in children {
        collect_concrete_anchors(child, anchor, candidates);
    }
}

fn recursive_type_name(anchor: &str) -> String {
    format!("{anchor}Type")
}

fn validate_export_node(
    node: &SchemaNode,
    is_root: bool,
    root_name: &str,
    recursive_anchors: &BTreeMap<String, &SchemaNode>,
) -> Result<(), XmlFormatError> {
    if !node.xml_repeating_sequences_are_valid() {
        return Err(XmlFormatError::InvalidRepeatingSequenceSchema {
            group: node.name.clone(),
        });
    }
    if node.attribute && node.text {
        return Err(XmlFormatError::ConflictingSchemaRoles {
            node: node.name.clone(),
        });
    }
    let role = if node.attribute {
        Some("attribute")
    } else if node.text {
        Some("text")
    } else {
        None
    };
    if let Some(role) = role {
        if is_root {
            return Err(XmlFormatError::UnsupportedSchemaRole {
                node: node.name.clone(),
                role,
                kind: "document root",
            });
        }
        if matches!(node.kind, ir::SchemaKind::Group { .. }) {
            return Err(XmlFormatError::UnsupportedSchemaRole {
                node: node.name.clone(),
                role,
                kind: "group",
            });
        }
        if node.repeating {
            return Err(XmlFormatError::RepeatingSchemaRole {
                node: node.name.clone(),
                role,
            });
        }
    }
    if let Some(anchor) = &node.recursive_ref {
        return if !is_root && (anchor == root_name || recursive_anchors.contains_key(anchor)) {
            Ok(())
        } else {
            Err(XmlFormatError::UnsupportedRecursiveAnchor {
                node: node.name.clone(),
                anchor: anchor.clone(),
            })
        };
    }
    let ir::SchemaKind::Group { children, .. } = &node.kind else {
        return Ok(());
    };
    if node.name == XML_ELEMENTS_FIELD {
        return if node.repeating {
            Ok(())
        } else {
            Err(XmlFormatError::UnsupportedSchemaRole {
                node: node.name.clone(),
                role: "generic elements",
                kind: "non-repeating group",
            })
        };
    }
    for child in children {
        validate_export_node(child, false, root_name, recursive_anchors)?;
    }
    let text_count = children.iter().filter(|child| child.text).count();
    if text_count > 1 {
        return Err(XmlFormatError::MultipleTextFields {
            group: node.name.clone(),
            count: text_count,
        });
    }
    if text_count == 1
        && children.iter().any(|child| !child.attribute && !child.text)
        && children.iter().find(|child| child.text).is_none_or(|text| {
            !matches!(
                text.kind,
                ir::SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            )
        })
    {
        return Err(XmlFormatError::MixedContent {
            group: node.name.clone(),
        });
    }
    Ok(())
}

fn write_element(
    node: &SchemaNode,
    depth: usize,
    root_name: &str,
    recursive_anchors: &BTreeMap<String, &SchemaNode>,
    alternatives: &AlternativeExportPlan<'_>,
    out: &mut String,
) -> Result<(), XmlFormatError> {
    write_element_required(
        node,
        depth,
        ElementOccurrence::Required,
        root_name,
        recursive_anchors,
        alternatives,
        out,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ElementOccurrence {
    Required,
    Optional,
    RepeatingRequired,
}

fn write_element_required(
    node: &SchemaNode,
    depth: usize,
    occurrence: ElementOccurrence,
    root_name: &str,
    recursive_anchors: &BTreeMap<String, &SchemaNode>,
    alternatives: &AlternativeExportPlan<'_>,
    out: &mut String,
) -> Result<(), XmlFormatError> {
    let pad = "  ".repeat(depth);
    if node.name == XML_ELEMENTS_FIELD {
        out.push_str(&format!(
            "{pad}<xs:any minOccurs=\"0\" maxOccurs=\"unbounded\" processContents=\"lax\"/>\n"
        ));
        return Ok(());
    }
    let occurs = if node.repeating && occurrence == ElementOccurrence::RepeatingRequired {
        " maxOccurs=\"unbounded\""
    } else if node.repeating {
        " minOccurs=\"0\" maxOccurs=\"unbounded\""
    } else if occurrence == ElementOccurrence::Optional {
        " minOccurs=\"0\""
    } else {
        ""
    };
    if let Some(prefix) = alternatives.external_prefix(node) {
        out.push_str(&format!(
            "{pad}<xs:element ref=\"{prefix}:{}\"{occurs}/>\n",
            node.name
        ));
        return Ok(());
    }
    let nillable = if node.nillable {
        " nillable=\"true\""
    } else {
        ""
    };
    let fixed = element_fixed(node).map_or_else(String::new, |value| {
        format!(" fixed=\"{}\"", alternatives::xml_escape(value))
    });
    let form = if depth > 1 && matches!(node.xml_namespace, Some(XmlNamespace::Qualified(_))) {
        " form=\"qualified\""
    } else {
        ""
    };
    let legacy_name = if node.xml_namespace.is_none() && alternatives.needs_legacy_name_markers() {
        " ferruleName:namespace=\"legacy\""
    } else {
        ""
    };
    if let Some(anchor) = node.recursive_ref.as_deref() {
        if anchor == root_name {
            let reference = if matches!(node.xml_namespace, Some(XmlNamespace::Qualified(_))) {
                format!("tns:{root_name}")
            } else {
                root_name.to_string()
            };
            out.push_str(&format!(
                "{pad}<xs:element ref=\"{reference}\"{legacy_name}{occurs}{nillable}{fixed}/>\n"
            ));
        } else {
            out.push_str(&format!(
                "{pad}<xs:element name=\"{}\" type=\"{}\"{form}{legacy_name}{occurs}{nillable}{fixed}/>\n",
                node.name,
                recursive_type_name(anchor)
            ));
        }
        return Ok(());
    }
    if node.name != root_name && recursive_anchors.contains_key(&node.name) {
        out.push_str(&format!(
            "{pad}<xs:element name=\"{}\" type=\"{}\"{form}{legacy_name}{occurs}{nillable}{fixed}/>\n",
            node.name,
            recursive_type_name(&node.name)
        ));
        return Ok(());
    }
    if let Some(type_name) = alternatives.type_for(node) {
        if let Some(view) = alternatives.restricted_view_for(node) {
            out.push_str(&format!(
                "{pad}<xs:element name=\"{}\" type=\"{type_name}\"{form}{legacy_name}{occurs}{nillable}{fixed}>\n{pad}  <xs:annotation>\n{pad}    <xs:appinfo source=\"{ALTERNATIVE_VIEW_NAMESPACE}\">\n",
                node.name
            ));
            for name in view {
                out.push_str(&format!(
                    "{pad}      <ferrule:type name=\"{}\"/>\n",
                    alternatives::xml_escape(name)
                ));
            }
            out.push_str(&format!(
                "{pad}    </xs:appinfo>\n{pad}  </xs:annotation>\n{pad}</xs:element>\n"
            ));
        } else {
            out.push_str(&format!(
                "{pad}<xs:element name=\"{}\" type=\"{type_name}\"{form}{legacy_name}{occurs}{nillable}{fixed}/>\n",
                node.name
            ));
        }
        return Ok(());
    }
    match &node.kind {
        ir::SchemaKind::Scalar { ty } => {
            out.push_str(&format!(
                "{pad}<xs:element name=\"{}\" type=\"{}\"{form}{legacy_name}{occurs}{nillable}{fixed}/>\n",
                node.name,
                xsd_type_name(ty)
            ));
        }
        ir::SchemaKind::Group { .. } => {
            out.push_str(&format!(
                "{pad}<xs:element name=\"{}\"{form}{legacy_name}{occurs}{nillable}{fixed}>\n",
                node.name
            ));
            write_complex_type(
                node,
                depth + 1,
                None,
                root_name,
                recursive_anchors,
                alternatives,
                out,
            )?;
            out.push_str(&format!("{pad}</xs:element>\n"));
        }
    }
    Ok(())
}

fn element_fixed(node: &SchemaNode) -> Option<&str> {
    node.fixed.as_deref().or_else(|| {
        let ir::SchemaKind::Group { children, .. } = &node.kind else {
            return None;
        };
        children
            .iter()
            .find(|child| child.text)
            .and_then(|child| child.fixed.as_deref())
    })
}

fn write_complex_type(
    node: &SchemaNode,
    depth: usize,
    name: Option<&str>,
    root_name: &str,
    recursive_anchors: &BTreeMap<String, &SchemaNode>,
    alternatives: &AlternativeExportPlan<'_>,
    out: &mut String,
) -> Result<(), XmlFormatError> {
    let ir::SchemaKind::Group { children, .. } = &node.kind else {
        return Err(XmlFormatError::UnsupportedSchemaRole {
            node: node.name.clone(),
            role: "named recursive type",
            kind: "scalar",
        });
    };
    let pad = "  ".repeat(depth);
    let name = name.map_or_else(String::new, |name| format!(" name=\"{name}\""));
    let (attrs, elements): (Vec<_>, Vec<_>) = children.iter().partition(|child| child.attribute);
    let text = elements.iter().find(|child| child.text);
    let nested_elements = elements
        .iter()
        .filter(|child| !child.text)
        .copied()
        .collect::<Vec<_>>();
    if let Some(text) = text
        && nested_elements.is_empty()
    {
        let ir::SchemaKind::Scalar { ty } = &text.kind else {
            return Err(XmlFormatError::UnsupportedSchemaRole {
                node: text.name.clone(),
                role: "text",
                kind: "group",
            });
        };
        out.push_str(&format!(
            "{pad}<xs:complexType{name}>\n{pad}  <xs:simpleContent>\n{pad}    <xs:extension base=\"{}\">\n",
            xsd_type_name(ty)
        ));
        for attr in attrs {
            write_attribute(attr, depth + 3, alternatives, out)?;
        }
        out.push_str(&format!(
            "{pad}    </xs:extension>\n{pad}  </xs:simpleContent>\n{pad}</xs:complexType>\n"
        ));
        return Ok(());
    }
    let mixed = if text.is_some() {
        " mixed=\"true\""
    } else {
        ""
    };
    out.push_str(&format!(
        "{pad}<xs:complexType{name}{mixed}>\n{pad}  <xs:sequence>\n"
    ));
    write_nested_elements(
        node,
        &nested_elements,
        depth + 2,
        root_name,
        recursive_anchors,
        alternatives,
        out,
    )?;
    out.push_str(&format!("{pad}  </xs:sequence>\n"));
    for attr in attrs {
        write_attribute(attr, depth + 1, alternatives, out)?;
    }
    out.push_str(&format!("{pad}</xs:complexType>\n"));
    Ok(())
}

fn write_nested_elements(
    group: &SchemaNode,
    children: &[&SchemaNode],
    depth: usize,
    root_name: &str,
    recursive_anchors: &BTreeMap<String, &SchemaNode>,
    alternatives: &AlternativeExportPlan<'_>,
    out: &mut String,
) -> Result<(), XmlFormatError> {
    for child in children {
        let sequence = group.xml_repeating_sequences.iter().find(|sequence| {
            sequence
                .members
                .first()
                .is_some_and(|member| member.name == child.name)
        });
        if let Some(sequence) = sequence {
            let pad = "  ".repeat(depth);
            let min_occurs = if sequence.required {
                ""
            } else {
                " minOccurs=\"0\""
            };
            out.push_str(&format!(
                "{pad}<xs:sequence{min_occurs} maxOccurs=\"unbounded\">\n"
            ));
            for member in &sequence.members {
                let child = children
                    .iter()
                    .find(|child| child.name == member.name)
                    .ok_or_else(|| XmlFormatError::UnsupportedSchemaRole {
                        node: group.name.clone(),
                        role: "repeating sequence with a missing member",
                        kind: "group",
                    })?;
                let mut occurrence = (*child).clone();
                occurrence.repeating = member.repeating;
                let requirement = if member.required && member.repeating {
                    ElementOccurrence::RepeatingRequired
                } else if member.required {
                    ElementOccurrence::Required
                } else {
                    ElementOccurrence::Optional
                };
                write_element_required(
                    &occurrence,
                    depth + 1,
                    requirement,
                    root_name,
                    recursive_anchors,
                    alternatives,
                    out,
                )?;
            }
            out.push_str(&format!("{pad}</xs:sequence>\n"));
            continue;
        }
        if group.xml_repeating_sequences.iter().any(|sequence| {
            sequence
                .members
                .iter()
                .skip(1)
                .any(|member| member.name == child.name)
        }) {
            continue;
        }
        write_element(
            child,
            depth,
            root_name,
            recursive_anchors,
            alternatives,
            out,
        )?;
    }
    Ok(())
}

fn write_attribute(
    attribute: &SchemaNode,
    depth: usize,
    alternatives: &AlternativeExportPlan<'_>,
    out: &mut String,
) -> Result<(), XmlFormatError> {
    if let Some(prefix) = alternatives.external_prefix(attribute) {
        let pad = "  ".repeat(depth);
        out.push_str(&format!(
            "{pad}<xs:attribute ref=\"{prefix}:{}\"/>\n",
            attribute.name
        ));
        return Ok(());
    }
    let ir::SchemaKind::Scalar { ty } = &attribute.kind else {
        return Err(XmlFormatError::UnsupportedSchemaRole {
            node: attribute.name.clone(),
            role: "attribute",
            kind: "group",
        });
    };
    let pad = "  ".repeat(depth);
    let fixed = attribute
        .fixed
        .as_deref()
        .map_or_else(String::new, |value| {
            format!(" fixed=\"{}\"", alternatives::xml_escape(value))
        });
    let form = if matches!(attribute.xml_namespace, Some(XmlNamespace::Qualified(_))) {
        " form=\"qualified\""
    } else {
        ""
    };
    let legacy_name =
        if attribute.xml_namespace.is_none() && alternatives.needs_legacy_name_markers() {
            " ferruleName:namespace=\"legacy\""
        } else {
            ""
        };
    out.push_str(&format!(
        "{pad}<xs:attribute name=\"{}\" type=\"{}\"{form}{legacy_name}{fixed}/>\n",
        attribute.name,
        xsd_type_name(ty)
    ));
    Ok(())
}
