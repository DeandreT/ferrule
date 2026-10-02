//! Diagnose document-root views without changing repair-oriented projection.
//!
//! Pin inventory precedes the first-entry schema reader. Descendant child views
//! stay with their existing lowering; a connected root condition instead needs
//! an explicit source predicate or target construction proof that we do not have.

use std::collections::{BTreeMap, BTreeSet};

use ir::SchemaNode;
use roxmltree::Node;

use super::schema::{is_xml_document_wrapper, xml_document_wrapper};

pub(super) struct Inventory {
    connected: BTreeSet<u32>,
    owners: BTreeMap<u32, usize>,
    uids: BTreeMap<String, usize>,
}

pub(super) struct Finding {
    uid: String,
    name: String,
    role: &'static str,
    root: String,
    declared_root: String,
    type_condition: String,
    ports: String,
    reason: &'static str,
    ownership: String,
}

impl Inventory {
    pub(super) fn read(structure: Node<'_, '_>) -> Self {
        let mut connected = BTreeSet::new();
        if let Some(vertices) = structure
            .children()
            .find(|node| node.has_tag_name("graph"))
            .and_then(|graph| graph.children().find(|node| node.has_tag_name("vertices")))
        {
            for vertex in vertices
                .children()
                .filter(|node| node.has_tag_name("vertex"))
            {
                let Some(source) = vertex
                    .attribute("vertexkey")
                    .and_then(|key| key.parse::<u32>().ok())
                else {
                    continue;
                };
                let Some(edges) = vertex.children().find(|node| node.has_tag_name("edges")) else {
                    continue;
                };
                for edge in edges.children().filter(|node| node.has_tag_name("edge")) {
                    if let Some(target) = edge
                        .attribute("vertexkey")
                        .and_then(|key| key.parse::<u32>().ok())
                    {
                        connected.extend([source, target]);
                    }
                }
            }
        }
        let mut owners = BTreeMap::new();
        let mut uids = BTreeMap::new();
        for node in structure.descendants().filter(Node::is_element) {
            for attribute in ["inpkey", "outkey"] {
                if let Some(key) = node
                    .attribute(attribute)
                    .and_then(|key| key.parse::<u32>().ok())
                {
                    *owners.entry(key).or_default() += 1;
                }
            }
            if node.has_tag_name("component")
                && let Some(uid) = node.attribute("uid")
            {
                *uids.entry(uid.to_string()).or_default() += 1;
            }
        }
        Self {
            connected,
            owners,
            uids,
        }
    }

    pub(super) fn inspect(&self, component: Node<'_, '_>) -> Vec<Finding> {
        let Some(data) = component.children().find(|node| node.has_tag_name("data")) else {
            return Vec::new();
        };
        let Some(root) = data.children().find(|node| node.has_tag_name("root")) else {
            return Vec::new();
        };
        let document = data.children().find(|node| node.has_tag_name("document"));
        let wrappers: Vec<_> = root
            .descendants()
            .filter(|entry| is_xml_document_wrapper(entry, root))
            .collect();
        let first = xml_document_wrapper(root)
            .and_then(|wrapper| {
                wrapper
                    .children()
                    .find(|node| node.has_tag_name("entry") && !decoration(*node))
            })
            .or_else(|| root.children().find(|node| node.has_tag_name("entry")));
        let containers = if wrappers.is_empty() {
            vec![root]
        } else {
            wrappers
        };
        let payloads: Vec<_> = containers
            .into_iter()
            .flat_map(|wrapper| {
                wrapper
                    .children()
                    .filter(|node| node.has_tag_name("entry") && !decoration(*node))
            })
            .collect();
        let uid = component.attribute("uid").unwrap_or("<missing>");
        let input = document
            .and_then(|node| node.attribute("inputinstance"))
            .filter(|value| !value.is_empty());
        let output = document
            .and_then(|node| node.attribute("outputinstance"))
            .filter(|value| !value.is_empty());
        let variable = data.descendants().any(|node| {
            node.has_tag_name("parameter") && node.attribute("usageKind") == Some("variable")
        }) || component.children().any(|node| {
            node.has_tag_name("properties") && node.attribute("PassThrough") == Some("1")
        });
        let role = match (variable, input.is_some(), output.is_some()) {
            (true, _, _) => "variable/pass-through",
            (false, true, false) => "source",
            (false, false, true) => "target",
            _ => "ambiguous",
        };
        let mut findings = Vec::new();
        for entry in &payloads {
            let ports: Vec<_> = entry
                .descendants()
                .filter(Node::is_element)
                .flat_map(|node| {
                    ["inpkey", "outkey"].into_iter().filter_map(move |kind| {
                        let key = node.attribute(kind)?.parse::<u32>().ok()?;
                        self.connected.contains(&key).then_some((node, kind, key))
                    })
                })
                .collect();
            if ports.is_empty() {
                continue;
            }
            let conditioned_ancestor = entry
                .ancestors()
                .skip(1)
                .take_while(|ancestor| *ancestor != data)
                .find(|ancestor| has_condition(*ancestor));
            let reason = if has_condition(*entry) {
                "document-root condition has no proved source predicate/target construction lowering"
            } else if conditioned_ancestor.is_some() {
                "document-root wrapper ancestor condition has no proved lowering"
            } else if Some(*entry) != first {
                "connected document-root sibling ownership/construction is not proved"
            } else {
                continue;
            };
            let condition_owner = if has_condition(*entry) {
                *entry
            } else {
                conditioned_ancestor.unwrap_or(*entry)
            };
            let type_condition = condition_owner
                .children()
                .filter(|node| matches!(node.tag_name().name(), "condition" | "conditions"))
                .flat_map(|node| node.descendants())
                .find(|node| {
                    node.has_tag_name("constant") && node.attribute("datatype") == Some("QName")
                })
                .and_then(|node| node.attribute("value"))
                .unwrap_or("<unresolved/non-type condition>");
            let mut ownership = Vec::new();
            if self.uids.get(uid).is_some_and(|count| *count > 1) {
                ownership.push("duplicate component UID");
            }
            if role == "ambiguous" || role == "variable/pass-through" {
                ownership.push("unproved document role");
            }
            if ports
                .iter()
                .any(|(_, _, key)| self.owners.get(key).is_some_and(|count| *count != 1))
            {
                ownership.push("nonunique raw port owner");
            }
            if ports.iter().any(|(_, kind, _)| {
                matches!((role, *kind), ("source", "inpkey") | ("target", "outkey"))
            }) {
                ownership.push("raw port direction conflicts with explicit document role");
            }
            let port_preview = ports
                .iter()
                .take(8)
                .map(|(node, kind, key)| {
                    let path = node
                        .ancestors()
                        .take_while(|ancestor| *ancestor != *entry)
                        .filter(|ancestor| ancestor.has_tag_name("entry"))
                        .filter_map(|ancestor| ancestor.attribute("name"))
                        .collect::<Vec<_>>();
                    let path = path.into_iter().rev().collect::<Vec<_>>().join("/");
                    format!(
                        "{kind}={key} at {}",
                        if path.is_empty() {
                            "<root>".to_string()
                        } else {
                            excerpt(&path)
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            findings.push(Finding {
                uid: excerpt(uid),
                name: excerpt(component.attribute("name").unwrap_or("<unnamed>")),
                role,
                root: expanded_root(*entry, root),
                declared_root: excerpt(
                    document
                        .and_then(|node| node.attribute("instanceroot"))
                        .unwrap_or("<missing>"),
                ),
                type_condition: excerpt(type_condition),
                ports: port_preview,
                reason,
                ownership: if ownership.is_empty() {
                    "branch ancestry/condition is unrepresented".to_string()
                } else {
                    ownership.join(", ")
                },
            });
        }
        findings
    }
}

pub(super) fn warn(
    findings: Vec<Finding>,
    schema: Option<&SchemaNode>,
    warnings: &mut Vec<String>,
) {
    let default = schema
        .and_then(|schema| schema.xml_default_type.as_deref())
        .unwrap_or("<none/unresolved>");
    for finding in findings {
        warnings.push(format!(
            "component `{}` UID {}: unproved XML document-root view `{}` (declared `{}`, role {}, type condition `{}`, default `{}`); {} on {}; {}. Best-effort projection is retained; executable import requires explicit root-view ownership and condition/construction support",
            finding.name, finding.uid, finding.root, finding.declared_root, finding.role,
            finding.type_condition, excerpt(default), finding.reason, finding.ports, finding.ownership,
        ));
    }
}

fn has_condition(node: Node<'_, '_>) -> bool {
    node.children()
        .any(|child| matches!(child.tag_name().name(), "condition" | "conditions"))
}

fn decoration(node: Node<'_, '_>) -> bool {
    matches!(
        node.attribute("type"),
        Some(
            "comment-before"
                | "comment-after"
                | "processing-instruction-before"
                | "processing-instruction-after"
        )
    )
}

fn expanded_root(entry: Node<'_, '_>, root: Node<'_, '_>) -> String {
    let namespace = entry
        .attribute("ns")
        .unwrap_or("0")
        .parse::<usize>()
        .ok()
        .and_then(|slot| {
            root.children()
                .find(|node| node.has_tag_name("header"))
                .and_then(|header| {
                    header
                        .children()
                        .find(|node| node.has_tag_name("namespaces"))
                })
                .and_then(|namespaces| {
                    namespaces
                        .children()
                        .filter(|node| node.has_tag_name("namespace"))
                        .nth(slot)
                })
        })
        .and_then(|node| node.attribute("uid"));
    let namespace = namespace.unwrap_or(if entry.attribute("ns").is_none() {
        ""
    } else {
        "<unresolved namespace>"
    });
    excerpt(&format!(
        "{{{namespace}}}{}",
        entry.attribute("name").unwrap_or("<missing>")
    ))
}

fn excerpt(value: &str) -> String {
    let mut chars = value.chars();
    let result: String = chars.by_ref().take(160).collect();
    if chars.next().is_some() {
        format!("{result}…")
    } else {
        result
    }
}
