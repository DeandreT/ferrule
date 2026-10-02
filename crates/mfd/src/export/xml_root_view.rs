//! Native compatibility for connected fields in an unowned XML root view.

use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode};
use roxmltree::{Document, Node};

use crate::MfdError;

use super::schema::SideFormat;
use super::source::SourceExports;
use super::{ExportCompatibilityFeature, ExportCompatibilityIssue, TargetExport};

/// Child type views have separately allocated conditional ports. A document
/// root instead renders one merged entry: connected fields outside its concrete
/// declared view must not be certified as native before root-view lowering exists.
/// Inspect final edges so unused union fields and unrelated child views stay out
/// of this deliberately bounded finding.
pub(super) fn issues(
    xml: &str,
    sources: &SourceExports<'_>,
    targets: &[TargetExport<'_>],
) -> Result<Vec<ExportCompatibilityIssue>, MfdError> {
    let owners: BTreeMap<_, _> = sources
        .iter()
        .filter(|source| source.format == SideFormat::Xml)
        .map(|source| (source.component_uid, (source.schema, "outkey")))
        .chain(
            targets
                .iter()
                .filter(|target| target.format == SideFormat::Xml)
                .map(|target| (target.component_uid, (target.schema, "inpkey"))),
        )
        .collect();
    let document = Document::parse(xml)?;
    let mut findings = Vec::new();
    for component in document.descendants().filter(|node| {
        node.has_tag_name("component")
            && node.attribute("library") == Some("xml")
            && node.attribute("kind") == Some("14")
    }) {
        let Some(uid) = component.attribute("uid").and_then(|uid| uid.parse().ok()) else {
            continue;
        };
        let Some(&(schema, key_attribute)) = owners.get(&uid) else {
            continue;
        };
        let Some((identity, members)) = declared_members(schema) else {
            continue;
        };
        let Some(structure) = component
            .ancestors()
            .find(|node| node.has_tag_name("structure"))
        else {
            continue;
        };
        if !structure.parent().is_some_and(|owner| {
            owner.attribute("uid") == Some("1")
                && owner
                    .parent()
                    .is_some_and(|parent| parent.has_tag_name("mapping"))
        }) {
            continue;
        }
        let Some(graph) = child(structure, "graph") else {
            continue;
        };
        let connected: BTreeSet<_> = graph
            .descendants()
            .filter(|node| node.has_tag_name("edge"))
            .filter_map(|edge| {
                if key_attribute == "inpkey" {
                    edge.attribute("vertexkey")
                } else {
                    edge.ancestors()
                        .find(|ancestor| ancestor.has_tag_name("vertex"))
                        .and_then(|vertex| vertex.attribute("vertexkey"))
                }
            })
            .collect();
        // These levels are emitted structural wrappers. Do not descend based on
        // user-visible names: payloads may legitimately use the same spellings.
        let roots = child(component, "data")
            .and_then(|data| child(data, "root"))
            .into_iter()
            .flat_map(entries)
            .flat_map(entries)
            .flat_map(entries);
        let mut unowned = BTreeSet::new();
        for root in roots {
            if child(root, "condition").is_some() {
                continue;
            }
            for field in entries(root) {
                let Some(name) = field.attribute("name") else {
                    continue;
                };
                if members.iter().any(|member| member == name) || schema.child(name).is_none() {
                    continue;
                }
                if field.descendants().any(|node| {
                    node.attribute(key_attribute)
                        .is_some_and(|key| connected.contains(key))
                }) {
                    unowned.insert(name);
                }
            }
        }
        if !unowned.is_empty() {
            findings.push(ExportCompatibilityIssue {
                feature: ExportCompatibilityFeature::XmlRootTypeViewOwnership,
                component: component.attribute("name").unwrap_or(&schema.name).to_owned(),
                component_uid: Some(uid),
                message: format!(
                    "connected XML root fields [{}] are outside declared type `{identity}` and have no owned native root-view ports",
                    unowned.into_iter().collect::<Vec<_>>().join(", ")
                ),
            });
        }
    }
    Ok(findings)
}

fn declared_members(schema: &SchemaNode) -> Option<(&str, &[String])> {
    let identity = schema.xml_default_type.as_deref()?;
    let SchemaKind::Group { alternatives, .. } = &schema.kind else {
        return None;
    };
    if !schema.xml_type_alternatives || !schema.xml_default_type_is_valid() {
        return None;
    }
    alternatives
        .iter()
        .find(|alternative| alternative.name == identity)
        .map(|alternative| (identity, alternative.members.as_slice()))
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|child| child.has_tag_name(name))
}

fn entries<'a, 'input>(node: Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children().filter(|child| child.has_tag_name("entry"))
}
