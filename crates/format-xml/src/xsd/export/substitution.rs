use std::collections::BTreeMap;

use ir::{SchemaKind, SchemaNode, XmlAlternativeKind, XmlNamespace};

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
        let mut plan = Self {
            groups: BTreeMap::new(),
        };
        let mut declarations = BTreeMap::<String, &'a SchemaNode>::new();
        plan.collect(schema, alternatives, &mut declarations)?;
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
    ) -> Result<(), XmlFormatError> {
        if alternatives.external_prefix(node).is_some() {
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
                self.collect(child, alternatives, declarations)?;
            }
        }
        Ok(())
    }
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
