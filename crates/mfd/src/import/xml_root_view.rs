//! Diagnose document-root views without changing repair-oriented projection.
//!
//! Pin inventory precedes the first-entry schema reader. Descendant child views
//! stay with their existing lowering; a connected root condition instead needs
//! an explicit source predicate or target construction proof that we do not have.

use std::collections::{BTreeMap, BTreeSet};

use ir::SchemaNode;
use roxmltree::Node;

use super::schema::{is_xml_document_wrapper, xml_document_wrapper};

#[cfg_attr(test, derive(Debug, PartialEq))]
pub(super) struct Inventory {
    connected: BTreeSet<u32>,
    owners: BTreeMap<u32, usize>,
    uids: BTreeMap<String, usize>,
    feeds: Vec<(u32, u32)>,
    malformed_feeds: bool,
    scoped_owners: BTreeMap<u32, Vec<PortOwner>>,
    scoped_uids: BTreeMap<u32, usize>,
    malformed_components: BTreeSet<usize>,
    variable_components: BTreeSet<usize>,
    xml_document_roles: BTreeMap<usize, Option<bool>>,
}

#[cfg_attr(test, derive(Debug, PartialEq))]
struct PortOwner {
    component: usize,
    uid: Option<u32>,
    kind: &'static str,
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
        let mut feeds = Vec::new();
        let mut malformed_feeds = structure
            .children()
            .filter(|node| node.has_tag_name("graph"))
            .count()
            != 1;
        if let Some(graph) = structure.children().find(|node| node.has_tag_name("graph"))
            && let Some(vertices) = graph.children().find(|node| node.has_tag_name("vertices"))
        {
            malformed_feeds |= graph.attribute("directed") != Some("1")
                || graph
                    .children()
                    .filter(|node| node.has_tag_name("vertices"))
                    .count()
                    != 1;
            for vertex in vertices
                .children()
                .filter(|node| node.has_tag_name("vertex"))
            {
                let source = vertex
                    .attribute("vertexkey")
                    .and_then(|key| key.parse::<u32>().ok());
                let Some(edges) = vertex.children().find(|node| node.has_tag_name("edges")) else {
                    continue;
                };
                malformed_feeds |= vertex
                    .children()
                    .filter(|node| node.has_tag_name("edges"))
                    .count()
                    != 1;
                for edge in edges.children().filter(|node| node.has_tag_name("edge")) {
                    let target = edge
                        .attribute("vertexkey")
                        .and_then(|key| key.parse::<u32>().ok());
                    if let (Some(source), Some(target)) = (source, target) {
                        connected.extend([source, target]);
                        feeds.push((source, target));
                    } else {
                        malformed_feeds = true;
                    }
                }
            }
        }
        let mut owners = BTreeMap::new();
        let mut uids = BTreeMap::new();
        let mut scoped_owners = BTreeMap::<u32, Vec<PortOwner>>::new();
        let mut scoped_uids = BTreeMap::new();
        let mut malformed_components = BTreeSet::new();
        let mut variable_components = BTreeSet::new();
        let mut xml_document_roles = BTreeMap::new();
        // One document-order pass carries what an ancestor search per element
        // would find: whether the nearest `structure` is this one, and the
        // nearest `component`, each counting the element itself. A nested
        // structure leaves its whole subtree out of scope.
        let mut pending = vec![(
            structure,
            true,
            structure
                .ancestors()
                .find(|ancestor| ancestor.has_tag_name("component")),
        )];
        while let Some((node, in_scope, enclosing)) = pending.pop() {
            let in_scope = in_scope && (node == structure || !node.has_tag_name("structure"));
            let enclosing = if node.has_tag_name("component") {
                Some(node)
            } else {
                enclosing
            };
            pending.extend(
                node.children()
                    .rev()
                    .filter(Node::is_element)
                    .map(|child| (child, in_scope, enclosing)),
            );
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
            if !in_scope {
                continue;
            }
            if node.has_tag_name("component")
                && let Some(uid) = node
                    .attribute("uid")
                    .and_then(|uid| uid.parse::<u32>().ok())
            {
                *scoped_uids.entry(uid).or_default() += 1;
            }
            if node.has_tag_name("component") && variable_component(node) {
                variable_components.insert(node.range().start);
            }
            if node.has_tag_name("component") && node.attribute("library") == Some("xml") {
                xml_document_roles.insert(node.range().start, document_role(node));
            }
            let Some(component) = enclosing else {
                continue;
            };
            for kind in ["inpkey", "outkey"] {
                if let Some(raw) = node.attribute(kind) {
                    if let Ok(key) = raw.parse::<u32>() {
                        scoped_owners.entry(key).or_default().push(PortOwner {
                            component: component.range().start,
                            uid: component.attribute("uid").and_then(|uid| uid.parse().ok()),
                            kind,
                        });
                    } else {
                        malformed_components.insert(component.range().start);
                    }
                }
            }
        }
        Self {
            connected,
            owners,
            uids,
            feeds,
            malformed_feeds,
            scoped_owners,
            scoped_uids,
            malformed_components,
            variable_components,
            xml_document_roles,
        }
    }

    /// Recover a diagnostic role only; sibling view ports remain unprojected.
    pub(super) fn diagnostic_role(
        &self,
        component: Node<'_, '_>,
        has_connected_root_view: bool,
    ) -> Option<bool> {
        if !has_connected_root_view || self.malformed_feeds {
            return None;
        }
        let uid = component.attribute("uid")?.parse::<u32>().ok()?;
        if self.scoped_uids.get(&uid) != Some(&1)
            || self.malformed_components.contains(&component.range().start)
        {
            return None;
        }
        if self.variable_components.contains(&component.range().start) {
            return None;
        }
        let is_source = document_role(component)?;
        let mut found = false;
        let mut targets = BTreeSet::new();
        for &(source, target) in &self.feeds {
            if !targets.insert(target) {
                return None;
            }
            let source = self.unique_owner(source)?;
            let target = self.unique_owner(target)?;
            if source.kind != "outkey" || target.kind != "inpkey" {
                return None;
            }
            if source.component == component.range().start {
                if !is_source {
                    return None;
                }
                found = true;
            }
            if target.component == component.range().start {
                if is_source {
                    return None;
                }
                found = true;
            }
        }
        found.then_some(is_source)
    }

    fn unique_owner(&self, key: u32) -> Option<&PortOwner> {
        let owners = self.scoped_owners.get(&key)?;
        let [owner] = owners.as_slice() else {
            return None;
        };
        if let Some(role) = self.xml_document_roles.get(&owner.component)
            && *role != Some(owner.kind == "outkey")
        {
            return None;
        }
        (self.scoped_uids.get(&owner.uid?) == Some(&1)
            && !self.malformed_components.contains(&owner.component)
            && !self.variable_components.contains(&owner.component))
        .then_some(owner)
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
                // Payloads share their container's ancestry, so search it once
                // rather than once per payload.
                let conditioned_ancestor = wrapper
                    .ancestors()
                    .take_while(|ancestor| *ancestor != data)
                    .find(|ancestor| has_condition(*ancestor));
                wrapper
                    .children()
                    .filter(|node| node.has_tag_name("entry") && !decoration(*node))
                    .map(move |entry| (entry, conditioned_ancestor))
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
        for (entry, conditioned_ancestor) in &payloads {
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
            let conditioned_ancestor = *conditioned_ancestor;
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

fn document_role(component: Node<'_, '_>) -> Option<bool> {
    let mut data = component
        .children()
        .filter(|node| node.has_tag_name("data"));
    let data_node = data.next()?;
    if data.next().is_some() {
        return None;
    }
    let mut documents = data_node
        .children()
        .filter(|node| node.has_tag_name("document"));
    let document = documents.next()?;
    if documents.next().is_some() {
        return None;
    }
    match (
        document
            .attribute("inputinstance")
            .is_some_and(|s| !s.is_empty()),
        document
            .attribute("outputinstance")
            .is_some_and(|s| !s.is_empty()),
    ) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

fn variable_component(component: Node<'_, '_>) -> bool {
    component
        .children()
        .any(|node| node.has_tag_name("properties") && node.attribute("PassThrough") == Some("1"))
        || component
            .children()
            .find(|node| node.has_tag_name("data"))
            .is_some_and(|data| {
                data.descendants().any(|node| {
                    node.has_tag_name("parameter")
                        && node.attribute("usageKind") == Some("variable")
                })
            })
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
            "{}. Best-effort projection is retained; executable import requires explicit root-view ownership and condition/construction support",
            finding.message(default),
        ));
    }
}

pub(super) fn evidence(
    findings: &[Finding],
    schema: Option<&SchemaNode>,
    evidence: &mut Vec<String>,
) {
    let default = schema
        .and_then(|schema| schema.xml_default_type.as_deref())
        .unwrap_or("<none/unresolved>");
    evidence.extend(findings.iter().map(|finding| finding.message(default)));
}

impl Finding {
    fn message(&self, default: &str) -> String {
        format!(
            "component `{}` UID {}: unproved XML document-root view `{}` (declared `{}`, role {}, type condition `{}`, default `{}`); {} on {}; {}",
            self.name,
            self.uid,
            self.root,
            self.declared_root,
            self.role,
            self.type_condition,
            excerpt(default),
            self.reason,
            self.ports,
            self.ownership,
        )
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use roxmltree::Document;

    use super::*;

    fn top_structure<'a, 'input>(document: &'a Document<'input>) -> Node<'a, 'input> {
        document
            .descendants()
            .find(|node| node.has_tag_name("structure"))
            .unwrap()
    }

    fn component_start(document: &Document<'_>, uid: &str) -> usize {
        document
            .descendants()
            .find(|node| node.has_tag_name("component") && node.attribute("uid") == Some(uid))
            .unwrap()
            .range()
            .start
    }

    /// The per-element ancestor search `Inventory::read` replaced, kept to
    /// pin that the single pass finds the same owners.
    fn ancestor_search(structure: Node<'_, '_>) -> Inventory {
        let mut inventory = Inventory::read(structure);
        inventory.owners.clear();
        inventory.uids.clear();
        inventory.scoped_owners.clear();
        inventory.scoped_uids.clear();
        inventory.malformed_components.clear();
        inventory.variable_components.clear();
        inventory.xml_document_roles.clear();
        for node in structure.descendants().filter(Node::is_element) {
            for attribute in ["inpkey", "outkey"] {
                if let Some(key) = node
                    .attribute(attribute)
                    .and_then(|key| key.parse::<u32>().ok())
                {
                    *inventory.owners.entry(key).or_default() += 1;
                }
            }
            if node.has_tag_name("component")
                && let Some(uid) = node.attribute("uid")
            {
                *inventory.uids.entry(uid.to_string()).or_default() += 1;
            }
            if node
                .ancestors()
                .find(|ancestor| ancestor.has_tag_name("structure"))
                != Some(structure)
            {
                continue;
            }
            if node.has_tag_name("component")
                && let Some(uid) = node
                    .attribute("uid")
                    .and_then(|uid| uid.parse::<u32>().ok())
            {
                *inventory.scoped_uids.entry(uid).or_default() += 1;
            }
            if node.has_tag_name("component") && variable_component(node) {
                inventory.variable_components.insert(node.range().start);
            }
            if node.has_tag_name("component") && node.attribute("library") == Some("xml") {
                inventory
                    .xml_document_roles
                    .insert(node.range().start, document_role(node));
            }
            let Some(component) = node
                .ancestors()
                .find(|ancestor| ancestor.has_tag_name("component"))
            else {
                continue;
            };
            for kind in ["inpkey", "outkey"] {
                if let Some(raw) = node.attribute(kind) {
                    if let Ok(key) = raw.parse::<u32>() {
                        inventory
                            .scoped_owners
                            .entry(key)
                            .or_default()
                            .push(PortOwner {
                                component: component.range().start,
                                uid: component.attribute("uid").and_then(|uid| uid.parse().ok()),
                                kind,
                            });
                    } else {
                        inventory
                            .malformed_components
                            .insert(component.range().start);
                    }
                }
            }
        }
        inventory
    }

    #[test]
    fn nested_structures_and_components_keep_their_owners() {
        let text = r#"<mapping xmlns:x="urn:neutral">
            <component name="outer" uid="9">
              <structure>
                <graph directed="1"><vertices><vertex vertexkey="1"><edges><edge vertexkey="8"/></edges></vertex></vertices></graph>
                <port outkey="40"/>
                <children>
                  <component name="a" uid="1" library="xml">
                    <properties PassThrough="1"/>
                    <data><document inputinstance="in.xml"/><root><entry name="x" outkey="1"><entry inpkey="bad"/></entry></root></data>
                    <component name="inner" uid="2"><entry inpkey="3"/></component>
                    <entry outkey="4"/>
                  </component>
                  <component name="b" uid="1">
                    <structure>
                      <component name="hidden" uid="5" library="xml"><data><document outputinstance="o.xml"/></data><entry outkey="6"/></component>
                      <n inpkey="7"/>
                    </structure>
                    <entry inpkey="8"/>
                    <x:structure><entry inpkey="10"/></x:structure>
                    <x:component uid="11"><entry inpkey="12"/></x:component>
                  </component>
                  <component name="target" uid="3" library="xml"><data><document outputinstance="out.xml"/></data></component>
                  <n inpkey="13" outkey="nope"/>
                </children>
              </structure>
            </component>
            <structure><component uid="20"><entry inpkey="21"/></component></structure>
        </mapping>"#;
        let document = Document::parse(text).unwrap();
        let structure = top_structure(&document);
        let inventory = Inventory::read(structure);
        assert_eq!(inventory, ancestor_search(structure));

        let start = |name: &str| {
            document
                .descendants()
                .find(|node| node.attribute("name") == Some(name))
                .unwrap()
                .range()
                .start
        };
        let owner = |key: u32| {
            let owners = &inventory.scoped_owners[&key];
            assert_eq!(owners.len(), 1, "{key}: {owners:?}");
            (owners[0].component, owners[0].uid, owners[0].kind)
        };
        // A port directly below the structure belongs to the component that
        // encloses the structure itself.
        assert_eq!(owner(40), (start("outer"), Some(9), "outkey"));
        assert_eq!(owner(1), (start("a"), Some(1), "outkey"));
        assert_eq!(owner(3), (start("inner"), Some(2), "inpkey"));
        assert_eq!(owner(4), (start("a"), Some(1), "outkey"));
        // Leaving a nested structure returns to its enclosing component.
        assert_eq!(owner(8), (start("b"), Some(1), "inpkey"));
        assert_eq!(
            owner(12),
            (component_start(&document, "11"), Some(11), "inpkey")
        );
        assert_eq!(owner(13), (start("outer"), Some(9), "inpkey"));
        // Nested structures, prefixed or not, and sibling structures are out
        // of scope but still count as raw owners.
        for hidden in [6, 7, 10, 21] {
            assert!(!inventory.scoped_owners.contains_key(&hidden), "{hidden}");
        }
        for raw in [6, 7, 10, 13, 40] {
            assert_eq!(inventory.owners.get(&raw), Some(&1), "{raw}");
        }
        assert!(!inventory.owners.contains_key(&21));
        assert_eq!(
            inventory.scoped_uids,
            BTreeMap::from([(1, 2), (2, 1), (3, 1), (11, 1)])
        );
        assert_eq!(inventory.uids.get("5"), Some(&1));
        assert_eq!(
            inventory.malformed_components,
            BTreeSet::from([start("a"), start("outer")])
        );
        assert_eq!(inventory.variable_components, BTreeSet::from([start("a")]));
        assert_eq!(
            inventory.xml_document_roles,
            BTreeMap::from([(start("a"), Some(true)), (start("target"), Some(false))])
        );
    }

    /// The per-element ancestor search was quadratic in depth: 60,000 levels
    /// took about two minutes in a debug build.
    #[test]
    fn deep_component_chain_is_read_in_one_pass() {
        const LEVELS: usize = 60_000;
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let text = format!(
                "<mapping><structure><children><component uid=\"1\">{}<n outkey=\"2\"/>{}</component></children></structure></mapping>",
                "<n>".repeat(LEVELS),
                "</n>".repeat(LEVELS)
            );
            let document = Document::parse(&text).unwrap();
            let inventory = Inventory::read(top_structure(&document));
            let owners = &inventory.scoped_owners[&2];
            let _ = sender.send((owners.len(), owners[0].uid, owners[0].kind));
        });
        match receiver.recv_timeout(Duration::from_secs(20)) {
            Ok(read) => assert_eq!(read, (1, Some(1), "outkey")),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("the inventory pass did not finish in time")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the inventory pass panicked"),
        }
    }
}
