use std::collections::{BTreeMap, BTreeSet};

use ir::{GroupAlternative, SchemaKind, SchemaNode};

use crate::XmlFormatError;

use super::{write_attribute, write_element_required};

pub(super) struct AlternativeExportPlan<'a> {
    namespace: Option<String>,
    saw_unqualified: bool,
    groups: BTreeMap<usize, String>,
    group_views: BTreeMap<usize, Vec<String>>,
    alternatives_by_base: BTreeMap<String, BTreeSet<String>>,
    definitions: BTreeMap<String, TypeDefinition<'a>>,
}

struct TypeDefinition<'a> {
    base: Option<String>,
    abstract_type: bool,
    members: Vec<&'a SchemaNode>,
    required: BTreeSet<String>,
}

impl PartialEq for TypeDefinition<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base
            && self.abstract_type == other.abstract_type
            && self.required == other.required
            && self.members == other.members
    }
}

impl<'a> AlternativeExportPlan<'a> {
    pub(super) fn build(schema: &'a SchemaNode) -> Result<Self, XmlFormatError> {
        let mut reserved = BTreeSet::new();
        collect_type_names(schema, &mut reserved)?;
        let mut plan = Self {
            namespace: None,
            saw_unqualified: false,
            groups: BTreeMap::new(),
            group_views: BTreeMap::new(),
            alternatives_by_base: BTreeMap::new(),
            definitions: BTreeMap::new(),
        };
        plan.collect(schema, &reserved)?;
        Ok(plan)
    }

    pub(super) fn schema_attributes(&self) -> String {
        let mut attributes = self
            .namespace
            .as_ref()
            .map_or_else(String::new, |namespace| {
                format!(
                    " xmlns:tns=\"{}\" targetNamespace=\"{}\"",
                    xml_escape(namespace),
                    xml_escape(namespace)
                )
            });
        if self.has_restricted_views() {
            attributes.push_str(&format!(
                " xmlns:ferrule=\"{}\"",
                super::ALTERNATIVE_VIEW_NAMESPACE
            ));
        }
        attributes
    }

    pub(super) fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    pub(super) fn type_for(&self, node: &SchemaNode) -> Option<String> {
        self.groups
            .get(&node_key(node))
            .map(|name| self.qualified(name))
    }

    pub(super) fn restricted_view_for(&self, node: &SchemaNode) -> Option<&[String]> {
        let key = node_key(node);
        let base = self.groups.get(&key)?;
        let view = self.group_views.get(&key)?;
        let complete = self.alternatives_by_base.get(base)?;
        let view_set = view.iter().cloned().collect::<BTreeSet<_>>();
        (view_set != *complete).then_some(view.as_slice())
    }

    pub(super) fn write_definitions(
        &self,
        root_name: &str,
        recursive_anchors: &BTreeMap<String, &SchemaNode>,
        out: &mut String,
    ) -> Result<(), XmlFormatError> {
        for (name, definition) in &self.definitions {
            let abstract_attr = if definition.abstract_type {
                " abstract=\"true\""
            } else {
                ""
            };
            if let Some(base) = &definition.base {
                out.push_str(&format!(
                    "  <xs:complexType name=\"{}\">\n    <xs:complexContent>\n      <xs:extension base=\"{}\">\n",
                    xml_escape(name),
                    self.qualified(base)
                ));
                write_members(definition, 4, root_name, recursive_anchors, self, out)?;
                out.push_str(
                    "      </xs:extension>\n    </xs:complexContent>\n  </xs:complexType>\n",
                );
            } else {
                out.push_str(&format!(
                    "  <xs:complexType name=\"{}\"{abstract_attr}>\n",
                    xml_escape(name)
                ));
                write_members(definition, 2, root_name, recursive_anchors, self, out)?;
                out.push_str("  </xs:complexType>\n");
            }
        }
        Ok(())
    }

    fn collect(
        &mut self,
        node: &'a SchemaNode,
        reserved: &BTreeSet<String>,
    ) -> Result<(), XmlFormatError> {
        let SchemaKind::Group {
            children,
            alternatives,
            ..
        } = &node.kind
        else {
            return Ok(());
        };
        if !alternatives.is_empty() {
            self.collect_group(node, children, alternatives, reserved)?;
        }
        for child in children {
            self.collect(child, reserved)?;
        }
        Ok(())
    }

    fn collect_group(
        &mut self,
        node: &'a SchemaNode,
        children: &'a [SchemaNode],
        alternatives: &[GroupAlternative],
        reserved: &BTreeSet<String>,
    ) -> Result<(), XmlFormatError> {
        if alternatives
            .iter()
            .any(|alternative| !alternative.required.is_empty())
        {
            return Err(unsupported(node));
        }
        let identities = alternatives
            .iter()
            .map(|alternative| split_identity(&alternative.name))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| unsupported(node))?;
        let group_namespace = &identities[0].0;
        if identities
            .iter()
            .any(|(namespace, _)| namespace != group_namespace)
        {
            return Err(unsupported(node));
        }
        match group_namespace {
            Some(namespace) if self.saw_unqualified => return Err(unsupported(node)),
            Some(namespace) => match &self.namespace {
                Some(existing) if existing != namespace => return Err(unsupported(node)),
                Some(_) => {}
                None => self.namespace = Some(namespace.clone()),
            },
            None if self.namespace.is_some() => return Err(unsupported(node)),
            None => self.saw_unqualified = true,
        }

        let common = children
            .iter()
            .filter(|child| {
                alternatives
                    .iter()
                    .all(|alternative| alternative.members.contains(&child.name))
            })
            .collect::<Vec<_>>();
        let common_names = common
            .iter()
            .map(|child| child.name.as_str())
            .collect::<BTreeSet<_>>();
        if !alternatives
            .iter()
            .all(|alternative| extension_prefix_is_valid(children, alternative, &common_names))
        {
            return Err(unsupported(node));
        }

        let base_index = alternatives.iter().position(|alternative| {
            alternative.members.len() == common.len()
                && alternative
                    .members
                    .iter()
                    .all(|member| common_names.contains(member.as_str()))
        });
        let (base_name, abstract_type) = match base_index {
            Some(index) => (identities[index].1.clone(), false),
            None => (synthetic_base_name(&identities[0].1, reserved), true),
        };
        let identity_set = identities
            .iter()
            .map(|(_, local)| local.clone())
            .collect::<BTreeSet<_>>();
        self.alternatives_by_base
            .entry(base_name.clone())
            .or_default()
            .extend(identity_set);
        self.insert_definition(
            node,
            base_name.clone(),
            TypeDefinition {
                base: None,
                abstract_type,
                members: common,
                required: BTreeSet::new(),
            },
        )?;

        for (index, alternative) in alternatives.iter().enumerate() {
            if Some(index) == base_index {
                continue;
            }
            let members = alternative
                .members
                .iter()
                .filter(|member| !common_names.contains(member.as_str()))
                .filter_map(|member| children.iter().find(|child| child.name == *member))
                .collect::<Vec<_>>();
            if members.len() + common_names.len() != alternative.members.len() {
                return Err(unsupported(node));
            }
            let required = alternative
                .required
                .iter()
                .filter(|member| !common_names.contains(member.as_str()))
                .cloned()
                .collect();
            self.insert_definition(
                node,
                identities[index].1.clone(),
                TypeDefinition {
                    base: Some(base_name.clone()),
                    abstract_type: false,
                    members,
                    required,
                },
            )?;
        }
        let key = node_key(node);
        self.groups.insert(key, base_name);
        self.group_views.insert(
            key,
            alternatives
                .iter()
                .map(|alternative| alternative.name.clone())
                .collect(),
        );
        Ok(())
    }

    fn insert_definition(
        &mut self,
        node: &SchemaNode,
        name: String,
        definition: TypeDefinition<'a>,
    ) -> Result<(), XmlFormatError> {
        if let Some(existing) = self.definitions.get(&name) {
            if existing != &definition {
                return Err(unsupported(node));
            }
        } else {
            self.definitions.insert(name, definition);
        }
        Ok(())
    }

    fn qualified(&self, name: &str) -> String {
        if self.namespace.is_some() {
            format!("tns:{name}")
        } else {
            name.to_string()
        }
    }

    fn has_restricted_views(&self) -> bool {
        self.groups.iter().any(|(key, base)| {
            let Some(view) = self.group_views.get(key) else {
                return false;
            };
            let Some(complete) = self.alternatives_by_base.get(base) else {
                return false;
            };
            view.iter().cloned().collect::<BTreeSet<_>>() != *complete
        })
    }
}

fn write_members(
    definition: &TypeDefinition<'_>,
    depth: usize,
    root_name: &str,
    recursive_anchors: &BTreeMap<String, &SchemaNode>,
    alternatives: &AlternativeExportPlan<'_>,
    out: &mut String,
) -> Result<(), XmlFormatError> {
    let (attributes, elements): (Vec<&SchemaNode>, Vec<&SchemaNode>) = definition
        .members
        .iter()
        .copied()
        .partition(|child| child.attribute);
    if elements.iter().any(|child| child.text) {
        return Err(unsupported_node_name("mixed alternative content"));
    }
    let pad = "  ".repeat(depth);
    out.push_str(&format!("{pad}<xs:sequence>\n"));
    for child in elements {
        write_element_required(
            child,
            depth + 1,
            definition.required.contains(&child.name),
            root_name,
            recursive_anchors,
            alternatives,
            out,
        )?;
    }
    out.push_str(&format!("{pad}</xs:sequence>\n"));
    for attribute in attributes {
        write_attribute(attribute, depth, out)?;
    }
    Ok(())
}

fn collect_type_names(node: &SchemaNode, out: &mut BTreeSet<String>) -> Result<(), XmlFormatError> {
    let SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &node.kind
    else {
        return Ok(());
    };
    for alternative in alternatives {
        let (_, local) = split_identity(&alternative.name).ok_or_else(|| unsupported(node))?;
        if !out.insert(local) {
            // Reuse is allowed only after the full definitions are compared.
        }
    }
    for child in children {
        collect_type_names(child, out)?;
    }
    Ok(())
}

fn extension_prefix_is_valid(
    children: &[SchemaNode],
    alternative: &GroupAlternative,
    common: &BTreeSet<&str>,
) -> bool {
    let base_elements = children
        .iter()
        .filter(|child| !child.attribute && common.contains(child.name.as_str()))
        .map(|child| child.name.as_str())
        .collect::<Vec<_>>();
    let alternative_elements = alternative
        .members
        .iter()
        .filter_map(|member| {
            children
                .iter()
                .find(|child| child.name == *member && !child.attribute)
                .map(|child| child.name.as_str())
        })
        .collect::<Vec<_>>();
    alternative_elements.starts_with(&base_elements)
}

fn synthetic_base_name(first_alternative: &str, reserved: &BTreeSet<String>) -> String {
    let stem = format!("{first_alternative}BaseType");
    let mut candidate = stem.clone();
    let mut suffix = 2;
    while reserved.contains(&candidate) {
        candidate = format!("{stem}{suffix}");
        suffix += 1;
    }
    candidate
}

fn split_identity(identity: &str) -> Option<(Option<String>, String)> {
    if let Some(rest) = identity.strip_prefix('{') {
        let (namespace, local) = rest.split_once('}')?;
        return (!namespace.is_empty() && !local.is_empty())
            .then(|| (Some(namespace.to_string()), local.to_string()));
    }
    (!identity.is_empty() && !identity.contains(':')).then(|| (None, identity.to_string()))
}

fn node_key(node: &SchemaNode) -> usize {
    std::ptr::from_ref(node).addr()
}

fn unsupported(node: &SchemaNode) -> XmlFormatError {
    XmlFormatError::UnsupportedGroupAlternatives {
        group: node.name.clone(),
    }
}

fn unsupported_node_name(name: &str) -> XmlFormatError {
    XmlFormatError::UnsupportedGroupAlternatives {
        group: name.to_string(),
    }
}

pub(super) fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
