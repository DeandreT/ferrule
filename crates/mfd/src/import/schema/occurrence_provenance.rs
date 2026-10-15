//! Import-only raw evidence for the bounded physical X12 -> JSON occurrence
//! adapter. Nothing here changes ordinary port or edge normalization.
use std::collections::{BTreeMap, BTreeSet};

use super::parse_u32;

#[derive(Clone, PartialEq, Eq)]
pub(in crate::import) struct RawOccurrenceOwner {
    pub(in crate::import) component: usize,
    pub(in crate::import) selected_children: usize,
    pub(in crate::import) inputs: BTreeMap<u32, usize>,
    pub(in crate::import) outputs: BTreeMap<u32, usize>,
    pub(in crate::import) feeds: BTreeMap<u32, u32>,
    pub(in crate::import) literals: BTreeSet<u32>,
    pub(in crate::import) components: BTreeSet<usize>,
    pub(in crate::import) scalar_outputs: BTreeSet<u32>,
    pub(in crate::import) group_outputs: BTreeSet<u32>,
    pub(in crate::import) ordinary_x12: bool,
    pub(in crate::import) ordinary_json: bool,
}

fn elements<'a, 'input>(node: roxmltree::Node<'a, 'input>) -> Vec<roxmltree::Node<'a, 'input>> {
    node.children()
        .filter(roxmltree::Node::is_element)
        .collect()
}

fn attributes(node: roxmltree::Node<'_, '_>, allowed: &[&str]) -> bool {
    node.tag_name().namespace().is_none()
        && node
            .attributes()
            .all(|attribute| attribute.namespace().is_none() && allowed.contains(&attribute.name()))
        && !node.children().any(|child| {
            child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty())
        })
}

fn exact_children(node: roxmltree::Node<'_, '_>, names: &[&str]) -> bool {
    let children = elements(node);
    children.len() == names.len()
        && names.iter().all(|name| {
            children
                .iter()
                .filter(|child| child.has_tag_name(*name))
                .count()
                == 1
        })
}

fn owner(node: roxmltree::Node<'_, '_>) -> Option<usize> {
    node.ancestors()
        .find(|parent| parent.has_tag_name("component"))
        .map(|component| component.range().start)
}

fn point_key(node: roxmltree::Node<'_, '_>, direction: &str) -> Option<u32> {
    (node.has_tag_name("datapoint")
        && node
            .parent()
            .is_some_and(|parent| parent.has_tag_name(direction)))
    .then(|| parse_u32(node.attribute("key")))
    .flatten()
}

fn literal(component: roxmltree::Node<'_, '_>, children: roxmltree::Node<'_, '_>) -> Option<u32> {
    if component.parent() != Some(children)
        || component.attribute("library") != Some("core")
        || component.attribute("kind") != Some("2")
        || component.attribute("name") != Some("constant")
        || !attributes(component, &["name", "library", "kind"])
        || !exact_children(component, &["targets", "data"])
    {
        return None;
    }
    let targets = component
        .children()
        .find(|node| node.has_tag_name("targets"))?;
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))?;
    if !attributes(targets, &[]) || !attributes(data, &[]) || !exact_children(data, &["constant"]) {
        return None;
    }
    let points = elements(targets);
    let [point] = points.as_slice() else {
        return None;
    };
    if !point.has_tag_name("datapoint")
        || !attributes(*point, &["key"])
        || !elements(*point).is_empty()
    {
        return None;
    }
    let value = elements(data)[0];
    if !attributes(value, &["value", "datatype"])
        || !elements(value).is_empty()
        || value.attribute("value").is_none()
        || !matches!(
            value.attribute("datatype"),
            Some("string" | "integer" | "boolean")
        )
    {
        return None;
    }
    parse_u32(point.attribute("key"))
}

fn ordinary(component: roxmltree::Node<'_, '_>, json: bool) -> bool {
    if !attributes(component, &["name", "library", "kind"])
        || component.attribute("library") != Some(if json { "json" } else { "text" })
        || component.attribute("kind") != Some(if json { "31" } else { "16" })
    {
        return false;
    }
    let children = elements(component);
    if !children
        .iter()
        .all(|node| node.has_tag_name("data") || json && node.has_tag_name("properties"))
        || children
            .iter()
            .filter(|node| node.has_tag_name("data"))
            .count()
            != 1
        || children
            .iter()
            .filter(|node| node.has_tag_name("properties"))
            .count()
            > 1
    {
        return false;
    }
    if children
        .iter()
        .filter(|node| node.has_tag_name("properties"))
        .any(|node| {
            !attributes(*node, &["XSLTDefaultOutput"])
                || node.attribute("XSLTDefaultOutput") != Some("1")
                || !elements(*node).is_empty()
        })
    {
        return false;
    }
    let data = children
        .iter()
        .find(|node| node.has_tag_name("data"))
        .copied()
        .unwrap();
    let format = if json { "json" } else { "text" };
    if !attributes(data, &[]) || !exact_children(data, &["root", format]) {
        return false;
    }
    let root = data
        .children()
        .find(|node| node.has_tag_name("root"))
        .unwrap();
    let text = data
        .children()
        .find(|node| node.has_tag_name(format))
        .unwrap();
    if !attributes(root, &[])
        || elements(root).len() != 1
        || !root
            .descendants()
            .filter(roxmltree::Node::is_element)
            .all(|node| {
                node == root
                    || node.has_tag_name("entry")
                        && attributes(
                            node,
                            if json {
                                &["name", "type", "inpkey", "clone"]
                            } else {
                                &["name", "outkey"]
                            },
                        )
                        && (!json
                            || matches!(
                                node.attribute("type"),
                                None | Some("json-property" | "json-item")
                            ) && matches!(node.attribute("clone"), None | Some("1")))
            })
    {
        return false;
    }
    if json {
        attributes(text, &["schema", "outputinstance"])
            && text
                .attribute("schema")
                .is_some_and(|name| !name.is_empty())
            && elements(text).is_empty()
    } else {
        attributes(text, &["type", "kind", "config", "inputinstance"])
            && text.attribute("type") == Some("edi")
            && text.attribute("kind") == Some("EDIX12")
            && text
                .attribute("config")
                .is_some_and(|name| !name.is_empty())
            && elements(text).is_empty()
    }
}

pub(super) fn capture(component: roxmltree::Node<'_, '_>) -> Option<RawOccurrenceOwner> {
    if !matches!(component.attribute("library"), Some("json" | "text")) {
        return None;
    }
    let document = component.document();
    let root = document.root_element();
    if !root.has_tag_name("mapping") || root.tag_name().namespace().is_some() {
        return None;
    }
    // Bind exactly the same first wrapper/structure/children selection as import.
    let wrapper = root
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "component")?;
    let structure = wrapper
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "structure")?;
    let children = structure
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "children")?;
    if component.parent() != Some(children)
        || [wrapper, structure, children]
            .iter()
            .any(|node| node.tag_name().namespace().is_some())
    {
        return None;
    }
    if elements(children).len() > 258
        || !ordinary(component, component.attribute("library") == Some("json"))
    {
        return None;
    }
    let nodes = root
        .descendants()
        .filter(roxmltree::Node::is_element)
        .take(16_385)
        .collect::<Vec<_>>();
    if nodes.len() > 16_384 {
        return None;
    }
    let mut inputs = BTreeMap::new();
    let mut outputs = BTreeMap::new();
    let mut feeds = BTreeMap::new();
    let mut literals = BTreeSet::new();
    let mut scalar_outputs = BTreeSet::new();
    let mut group_outputs = BTreeSet::new();
    for node in &nodes {
        if node.tag_name().namespace().is_some() {
            return None;
        }
        if ["outkey", "inpkey"].iter().any(|name| {
            node.attribute(*name).is_some() && parse_u32(node.attribute(*name)).is_none()
        }) {
            return None;
        }
        if node.has_tag_name("datapoint")
            && node.attribute("key").is_some()
            && parse_u32(node.attribute("key")).is_none()
        {
            return None;
        }
        if node.attributes().any(|attribute| {
            attribute.namespace().is_some()
                && matches!(
                    attribute.name(),
                    "outkey" | "inpkey" | "key" | "from" | "to" | "vertexkey"
                )
        }) {
            return None;
        }
        let output = parse_u32(node.attribute("outkey")).or_else(|| point_key(*node, "targets"));
        let input = parse_u32(node.attribute("inpkey")).or_else(|| point_key(*node, "sources"));
        for (key, direction) in [(output, &mut outputs), (input, &mut inputs)] {
            if let Some(key) = key
                && direction.insert(key, owner(*node)?).is_some()
            {
                return None;
            }
        }
        if node.has_tag_name("component")
            && let Some(key) = literal(*node, children)
        {
            literals.insert(key);
        }
        if node.has_tag_name("entry")
            && let Some(key) = output
            && owner(*node) == Some(component.range().start)
        {
            if elements(*node).is_empty() {
                scalar_outputs.insert(key);
            } else {
                group_outputs.insert(key);
            }
        }
        if node.has_tag_name("edge") {
            let (from, to) = if let (Some(from), Some(to)) = (
                parse_u32(node.attribute("from")),
                parse_u32(node.attribute("to")),
            ) {
                if !attributes(*node, &["from", "to"]) || !elements(*node).is_empty() {
                    return None;
                }
                if node.parent().is_none_or(|parent| {
                    !parent.has_tag_name("connections") || parent.parent() != Some(structure)
                }) {
                    return None;
                }
                (from, to)
            } else {
                let to = parse_u32(node.attribute("vertexkey"))?;
                if !attributes(*node, &["vertexkey"]) || !elements(*node).is_empty() {
                    return None;
                }
                let vertex = node
                    .ancestors()
                    .find(|parent| parent.has_tag_name("vertex"))?;
                if vertex
                    .ancestors()
                    .find(|parent| parent.has_tag_name("structure"))
                    != Some(structure)
                {
                    return None;
                }
                (parse_u32(vertex.attribute("vertexkey"))?, to)
            };
            if feeds.insert(to, from).is_some() {
                return None;
            }
        }
    }
    if inputs.keys().any(|key| outputs.contains_key(key)) {
        return None;
    }
    if feeds
        .iter()
        .any(|(input, output)| !inputs.contains_key(input) || !outputs.contains_key(output))
    {
        return None;
    }
    let components = elements(children)
        .into_iter()
        .map(|node| node.has_tag_name("component").then_some(node.range().start))
        .collect::<Option<BTreeSet<_>>>()?;
    if nodes
        .iter()
        .filter(|node| node.has_tag_name("component"))
        .count()
        != components.len() + 1
    {
        return None;
    }
    Some(RawOccurrenceOwner {
        component: component.range().start,
        selected_children: children.range().start,
        inputs,
        outputs,
        feeds,
        literals,
        components,
        scalar_outputs,
        group_outputs,
        ordinary_x12: ordinary(component, false),
        ordinary_json: ordinary(component, true),
    })
}
