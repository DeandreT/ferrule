use std::collections::{BTreeMap, BTreeSet};

use ir::{GroupAlternative, SchemaKind, SchemaNode, XmlAlternativeKind, XmlNamespace};

use crate::XmlFormatError;

use super::{AlternativeExportPlan, alternatives};

pub(super) struct SubstitutionExportPlan<'a> {
    groups: BTreeMap<String, &'a SchemaNode>,
}

impl<'a> SubstitutionExportPlan<'a> {
    pub(super) fn build(
        schema: &'a SchemaNode,
        alternatives: &AlternativeExportPlan<'_>,
    ) -> Result<Self, XmlFormatError> {
        Self::build_with_partitioned_substitutions(schema, alternatives, false)
    }

    pub(super) fn build_set(
        schema: &'a SchemaNode,
        alternatives: &AlternativeExportPlan<'_>,
    ) -> Result<Self, XmlFormatError> {
        Self::build_with_partitioned_substitutions(schema, alternatives, true)
    }

    fn build_with_partitioned_substitutions(
        schema: &'a SchemaNode,
        alternatives: &AlternativeExportPlan<'_>,
        partition_cross_substitutions: bool,
    ) -> Result<Self, XmlFormatError> {
        let mut plan = Self {
            groups: BTreeMap::new(),
        };
        let mut declarations = BTreeMap::<String, &'a SchemaNode>::new();
        plan.collect(
            schema,
            alternatives,
            &mut declarations,
            partition_cross_substitutions,
        )?;
        Ok(plan)
    }

    pub(super) fn contains(&self, node: &SchemaNode) -> bool {
        self.groups.contains_key(&node_identity(node))
    }

    pub(super) fn write_declarations(
        &self,
        alternatives: &AlternativeExportPlan<'_>,
        out: &mut String,
    ) -> Result<(), XmlFormatError> {
        for node in self.groups.values() {
            let head = node_identity(node);
            let head_type = alternatives.type_for(node).ok_or_else(|| {
                unsupported(
                    node,
                    &head,
                    "substitution head has no exportable complex type",
                )
            })?;
            let qualified_head = qualified_name(node, &node.name);
            let abstract_head = !node
                .alternatives()
                .iter()
                .any(|alternative| alternative.name == head);
            let abstract_attribute = if abstract_head {
                " abstract=\"true\""
            } else {
                ""
            };
            out.push_str(&format!(
                "  <xs:element name=\"{}\" type=\"{}\"{abstract_attribute}/>\n",
                alternatives::xml_escape(&node.name),
                alternatives::xml_escape(&head_type),
            ));
            for alternative in node
                .alternatives()
                .iter()
                .filter(|alternative| alternative.name != head)
            {
                let (namespace, local) = split_identity(&alternative.name).ok_or_else(|| {
                    unsupported(
                        node,
                        &alternative.name,
                        "member identity is not a valid expanded XML name",
                    )
                })?;
                if namespace != node_namespace(node) {
                    return Err(unsupported(
                        node,
                        &alternative.name,
                        "member and head namespaces differ",
                    ));
                }
                let member_type = alternatives
                    .type_for_alternative(node, &alternative.name)
                    .ok_or_else(|| {
                        unsupported(
                            node,
                            &alternative.name,
                            "member has no exportable complex type",
                        )
                    })?;
                out.push_str(&format!(
                    "  <xs:element name=\"{}\" type=\"{}\" substitutionGroup=\"{}\"/>\n",
                    alternatives::xml_escape(local),
                    alternatives::xml_escape(&member_type),
                    alternatives::xml_escape(&qualified_head),
                ));
            }
        }
        Ok(())
    }

    fn collect(
        &mut self,
        node: &'a SchemaNode,
        alternatives: &AlternativeExportPlan<'_>,
        declarations: &mut BTreeMap<String, &'a SchemaNode>,
        partition_cross_substitutions: bool,
    ) -> Result<(), XmlFormatError> {
        if alternatives.external_prefix(node).is_some() {
            return Ok(());
        }
        if partition_cross_substitutions && requires_partition(node) {
            return Ok(());
        }
        if node.xml_alternative_kind == XmlAlternativeKind::SubstitutionGroup {
            let head = node_identity(node);
            if node.recursive_ref.is_some() {
                return Err(unsupported(
                    node,
                    &head,
                    "recursive substitution heads cannot be exported",
                ));
            }
            if let Some(existing) = declarations.get(&head) {
                if *existing != node {
                    return Err(XmlFormatError::ConflictingSubstitutionMember {
                        head: head.clone(),
                        member: head,
                    });
                }
            } else {
                declarations.insert(head.clone(), node);
            }
            self.groups.entry(head).or_insert(node);
        }
        if let SchemaKind::Group { children, .. } = &node.kind {
            for child in children {
                self.collect(
                    child,
                    alternatives,
                    declarations,
                    partition_cross_substitutions,
                )?;
            }
        }
        Ok(())
    }
}

pub(super) fn requires_partition(node: &SchemaNode) -> bool {
    node.xml_alternative_kind == XmlAlternativeKind::SubstitutionGroup
        && node.alternatives().iter().any(|alternative| {
            split_identity(&alternative.name)
                .is_some_and(|(namespace, _)| namespace != node_namespace(node))
        })
}

pub(super) fn member_identities(
    node: &SchemaNode,
) -> Result<Vec<(String, String)>, XmlFormatError> {
    if !requires_partition(node) {
        return Ok(Vec::new());
    }
    let mut members = Vec::new();
    for alternative in node.alternatives() {
        let (namespace, local) = split_identity(&alternative.name).ok_or_else(|| {
            unsupported(
                node,
                &alternative.name,
                "member identity is not a valid expanded XML name",
            )
        })?;
        if namespace != node_namespace(node) {
            let Some(namespace) = namespace else {
                return Err(unsupported(
                    node,
                    &alternative.name,
                    "member and head namespace qualification differ",
                ));
            };
            members.push((namespace.to_string(), local.to_string()));
        }
    }
    Ok(members)
}

pub(super) fn write_partitioned_heads(
    schema: &SchemaNode,
    target_namespace: Option<&str>,
    root_name: &str,
    recursive_anchors: &BTreeMap<String, &SchemaNode>,
    alternatives: &AlternativeExportPlan<'_>,
    out: &mut String,
) -> Result<(), XmlFormatError> {
    let mut declarations = BTreeMap::<String, &SchemaNode>::new();
    collect_partitioned_heads(schema, target_namespace, &mut declarations)?;
    for head in declarations.values() {
        let shape = PartitionedShape::new(head)?;
        let base = projection(head, &shape.base_members);
        super::write_complex_type(
            &base,
            1,
            Some(&shape.head_type),
            root_name,
            recursive_anchors,
            alternatives,
            out,
        )?;
        let abstract_attribute = if shape.abstract_head {
            " abstract=\"true\""
        } else {
            ""
        };
        out.push_str(&format!(
            "  <xs:element name=\"{}\" type=\"tns:{}\"{abstract_attribute}/>\n",
            alternatives::xml_escape(&head.name),
            alternatives::xml_escape(&shape.head_type),
        ));
        for alternative in head.alternatives() {
            let (namespace, _) = parsed_member(head, alternative)?;
            if namespace == node_namespace(head) && alternative.name != node_identity(head) {
                write_partitioned_member_inner(
                    head,
                    alternative,
                    &shape,
                    "tns",
                    root_name,
                    recursive_anchors,
                    alternatives,
                    out,
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn write_partitioned_member(
    head: &SchemaNode,
    member_identity: &str,
    root_name: &str,
    recursive_anchors: &BTreeMap<String, &SchemaNode>,
    alternatives: &AlternativeExportPlan<'_>,
    out: &mut String,
) -> Result<(), XmlFormatError> {
    let alternative = head
        .alternatives()
        .iter()
        .find(|alternative| alternative.name == member_identity)
        .ok_or_else(|| {
            unsupported(
                head,
                member_identity,
                "partitioned member is not declared by the head",
            )
        })?;
    let Some(head_prefix) = alternatives.external_prefix(head) else {
        return Err(unsupported(
            head,
            member_identity,
            "partitioned member cannot resolve its head declaration",
        ));
    };
    let shape = PartitionedShape::new(head)?;
    write_partitioned_member_inner(
        head,
        alternative,
        &shape,
        head_prefix,
        root_name,
        recursive_anchors,
        alternatives,
        out,
    )
}

pub(super) fn head_document_projection(head: &SchemaNode) -> Result<SchemaNode, XmlFormatError> {
    let shape = PartitionedShape::new(head)?;
    let head_namespace = node_namespace(head);
    let mut members = shape.base_members;
    for alternative in head.alternatives() {
        let (namespace, _) = parsed_member(head, alternative)?;
        if namespace == head_namespace {
            for member in &alternative.members {
                if !members.contains(member) {
                    members.push(member.clone());
                }
            }
        }
    }
    Ok(projection(head, &members))
}

pub(super) fn member_extension_projection(
    head: &SchemaNode,
    member_identity: &str,
) -> Result<SchemaNode, XmlFormatError> {
    let shape = PartitionedShape::new(head)?;
    let alternative = head
        .alternatives()
        .iter()
        .find(|alternative| alternative.name == member_identity)
        .ok_or_else(|| {
            unsupported(
                head,
                member_identity,
                "partitioned member is not declared by the head",
            )
        })?;
    let extras = alternative
        .members
        .iter()
        .filter(|member| !shape.base_members.contains(member))
        .cloned()
        .collect::<Vec<_>>();
    Ok(projection(head, &extras))
}

fn collect_partitioned_heads<'a>(
    node: &'a SchemaNode,
    target_namespace: Option<&str>,
    declarations: &mut BTreeMap<String, &'a SchemaNode>,
) -> Result<(), XmlFormatError> {
    if requires_partition(node) && node_namespace(node) == target_namespace {
        let identity = node_identity(node);
        if let Some(existing) = declarations.get(&identity) {
            if *existing != node {
                return Err(XmlFormatError::ConflictingSubstitutionMember {
                    head: identity.clone(),
                    member: identity,
                });
            }
        } else {
            declarations.insert(identity, node);
        }
        return Ok(());
    }
    if let SchemaKind::Group { children, .. } = &node.kind {
        for child in children {
            collect_partitioned_heads(child, target_namespace, declarations)?;
        }
    }
    Ok(())
}

struct PartitionedShape {
    abstract_head: bool,
    base_members: Vec<String>,
    head_type: String,
}

impl PartitionedShape {
    fn new(head: &SchemaNode) -> Result<Self, XmlFormatError> {
        if head.nillable || head.fixed.is_some() || head.default.is_some() {
            return Err(unsupported(
                head,
                &node_identity(head),
                "partitioned complex heads cannot carry value constraints or nillability",
            ));
        }
        if head.alternatives().iter().any(|alternative| {
            !alternative.required.is_empty() || !alternative.constraints.is_empty()
        }) {
            return Err(unsupported(
                head,
                &node_identity(head),
                "partitioned substitution alternatives cannot carry target-only constraints",
            ));
        }
        let identity = node_identity(head);
        let head_alternative = head
            .alternatives()
            .iter()
            .find(|alternative| alternative.name == identity);
        let abstract_head = head_alternative.is_none();
        let base_members = if abstract_head && head.alternatives().len() == 1 {
            Vec::new()
        } else {
            head_alternative.map_or_else(
                || {
                    let mut common = head
                        .alternatives()
                        .first()
                        .map(|alternative| alternative.members.clone())
                        .unwrap_or_default();
                    common.retain(|member| {
                        head.alternatives()
                            .iter()
                            .skip(1)
                            .all(|alternative| alternative.members.contains(member))
                    });
                    common
                },
                |alternative| alternative.members.clone(),
            )
        };
        let base_element_names = selected_element_names(head, &base_members);
        for alternative in head.alternatives() {
            parsed_member(head, alternative)?;
            let alternative_elements = selected_element_names(head, &alternative.members);
            if !alternative_elements.starts_with(&base_element_names) {
                return Err(unsupported(
                    head,
                    &alternative.name,
                    "member elements do not extend the head sequence",
                ));
            }
        }
        Ok(Self {
            abstract_head,
            base_members,
            head_type: format!("{}FerruleSubstitutionType", head.name),
        })
    }
}

fn selected_element_names<'a>(head: &'a SchemaNode, members: &[String]) -> Vec<&'a str> {
    let SchemaKind::Group { children, .. } = &head.kind else {
        return Vec::new();
    };
    children
        .iter()
        .filter(|child| {
            !child.attribute && !child.text && members.iter().any(|member| member == &child.name)
        })
        .map(|child| child.name.as_str())
        .collect()
}

fn projection(head: &SchemaNode, members: &[String]) -> SchemaNode {
    let mut projected = head.clone();
    if let SchemaKind::Group {
        children,
        alternatives,
        xml_restricted_alternatives,
        dynamic,
    } = &mut projected.kind
    {
        children.retain(|child| members.iter().any(|member| member == &child.name));
        alternatives.clear();
        xml_restricted_alternatives.clear();
        *dynamic = None;
    }
    let selected = members.iter().map(String::as_str).collect::<BTreeSet<_>>();
    projected.xml_repeating_sequences.retain(|sequence| {
        sequence
            .members
            .iter()
            .all(|member| selected.contains(member.name.as_str()))
    });
    projected.xml_repeating_choices.retain(|choice| {
        choice
            .members
            .iter()
            .all(|member| selected.contains(member.as_str()))
    });
    projected.xml_alternative_kind = XmlAlternativeKind::XsiType;
    projected.repeating = false;
    projected
}

#[allow(clippy::too_many_arguments)]
fn write_partitioned_member_inner(
    head: &SchemaNode,
    alternative: &GroupAlternative,
    shape: &PartitionedShape,
    head_prefix: &str,
    root_name: &str,
    recursive_anchors: &BTreeMap<String, &SchemaNode>,
    alternatives: &AlternativeExportPlan<'_>,
    out: &mut String,
) -> Result<(), XmlFormatError> {
    let (_, local) = parsed_member(head, alternative)?;
    let extras = alternative
        .members
        .iter()
        .filter(|member| !shape.base_members.contains(member))
        .cloned()
        .collect::<Vec<_>>();
    let member = projection(head, &extras);
    if contains_recursive_reference(&member) {
        return Err(unsupported(
            head,
            &alternative.name,
            "recursive member extensions cannot be partitioned",
        ));
    }
    let member_type = format!("{local}FerruleSubstitutionType");
    let pad = "  ";
    out.push_str(&format!(
        "{pad}<xs:complexType name=\"{}\">\n{pad}  <xs:complexContent>\n{pad}    <xs:extension base=\"{}:{}\">\n",
        alternatives::xml_escape(&member_type),
        alternatives::xml_escape(head_prefix),
        alternatives::xml_escape(&shape.head_type),
    ));
    let SchemaKind::Group { children, .. } = &member.kind else {
        return Err(unsupported(
            head,
            &alternative.name,
            "only complex substitution members can be partitioned",
        ));
    };
    let (attributes, elements): (Vec<_>, Vec<_>) =
        children.iter().partition(|child| child.attribute);
    if elements.iter().any(|child| child.text) {
        return Err(unsupported(
            head,
            &alternative.name,
            "mixed-content member extensions cannot be partitioned",
        ));
    }
    out.push_str(&format!("{pad}      <xs:sequence>\n"));
    let nested = elements.to_vec();
    super::write_nested_elements(
        &member,
        &nested,
        4,
        root_name,
        recursive_anchors,
        alternatives,
        out,
    )?;
    out.push_str(&format!("{pad}      </xs:sequence>\n"));
    for attribute in attributes {
        super::write_attribute(attribute, 3, alternatives, out)?;
    }
    out.push_str(&format!(
        "{pad}    </xs:extension>\n{pad}  </xs:complexContent>\n{pad}</xs:complexType>\n"
    ));
    out.push_str(&format!(
        "{pad}<xs:element name=\"{}\" type=\"tns:{}\" substitutionGroup=\"{}:{}\"/>\n",
        alternatives::xml_escape(local),
        alternatives::xml_escape(&member_type),
        alternatives::xml_escape(head_prefix),
        alternatives::xml_escape(&head.name),
    ));
    Ok(())
}

fn parsed_member<'a>(
    head: &SchemaNode,
    alternative: &'a GroupAlternative,
) -> Result<(Option<&'a str>, &'a str), XmlFormatError> {
    split_identity(&alternative.name).ok_or_else(|| {
        unsupported(
            head,
            &alternative.name,
            "member identity is not a valid expanded XML name",
        )
    })
}

fn contains_recursive_reference(node: &SchemaNode) -> bool {
    node.recursive_ref.is_some()
        || matches!(
            &node.kind,
            SchemaKind::Group { children, .. }
                if children.iter().any(contains_recursive_reference)
        )
}

fn node_identity(node: &SchemaNode) -> String {
    match node_namespace(node) {
        Some(namespace) => format!("{{{namespace}}}{}", node.name),
        None => node.name.clone(),
    }
}

fn node_namespace(node: &SchemaNode) -> Option<&str> {
    match &node.xml_namespace {
        Some(XmlNamespace::Qualified(namespace)) => Some(namespace.as_str()),
        Some(XmlNamespace::Unqualified) | None => None,
    }
}

fn qualified_name(node: &SchemaNode, local: &str) -> String {
    if node_namespace(node).is_some() {
        format!("tns:{local}")
    } else {
        local.to_string()
    }
}

fn split_identity(identity: &str) -> Option<(Option<&str>, &str)> {
    if let Some(rest) = identity.strip_prefix('{') {
        let (namespace, local) = rest.split_once('}')?;
        (!namespace.is_empty() && !local.is_empty()).then_some((Some(namespace), local))
    } else {
        (!identity.is_empty() && !identity.contains(['{', '}'])).then_some((None, identity))
    }
}

fn unsupported(node: &SchemaNode, member: &str, reason: &'static str) -> XmlFormatError {
    XmlFormatError::UnsupportedSubstitutionGroup {
        head: node_identity(node),
        member: member.to_string(),
        reason,
    }
}
