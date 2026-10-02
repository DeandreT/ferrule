//! Bounded XML component trees and port utilities for scope variables.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::MfdError;

use super::super::schema::xml_escape;

#[derive(Clone)]
pub(super) struct Element {
    pub(super) name: String,
    pub(super) attributes: Vec<(String, String)>,
    pub(super) children: Vec<Element>,
    pub(super) text: String,
}

impl Element {
    pub(super) fn read(node: roxmltree::Node<'_, '_>) -> Self {
        let name = match node
            .tag_name()
            .namespace()
            .and_then(|uri| node.lookup_prefix(uri))
        {
            Some(prefix) => format!("{prefix}:{}", node.tag_name().name()),
            None => node.tag_name().name().to_string(),
        };
        let mut attributes = node
            .attributes()
            .map(|attribute| {
                let name = match attribute
                    .namespace()
                    .and_then(|uri| node.lookup_prefix(uri))
                {
                    Some(prefix) => format!("{prefix}:{}", attribute.name()),
                    None => attribute.name().to_string(),
                };
                (name, attribute.value().to_string())
            })
            .collect::<Vec<_>>();
        for namespace in node.namespaces() {
            let inherited = node.parent_element().is_some_and(|parent| {
                parent.namespaces().any(|candidate| {
                    candidate.name() == namespace.name() && candidate.uri() == namespace.uri()
                })
            });
            if !inherited {
                let name = namespace
                    .name()
                    .map_or_else(|| "xmlns".to_string(), |name| format!("xmlns:{name}"));
                attributes.push((name, namespace.uri().to_string()));
            }
        }
        Self {
            name,
            attributes,
            children: node
                .children()
                .filter(roxmltree::Node::is_element)
                .map(Self::read)
                .collect(),
            text: node
                .children()
                .filter(roxmltree::Node::is_text)
                .filter_map(|node| node.text())
                .collect(),
        }
    }

    pub(super) fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub(super) fn set(&mut self, name: &str, value: impl ToString) {
        let value = value.to_string();
        if let Some((_, old)) = self.attributes.iter_mut().find(|(key, _)| key == name) {
            *old = value;
        } else {
            self.attributes.push((name.to_string(), value));
        }
    }

    pub(super) fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|child| child.name == name)
    }

    pub(super) fn child_mut(&mut self, name: &str) -> Option<&mut Self> {
        self.children.iter_mut().find(|child| child.name == name)
    }

    pub(super) fn render(&self, output: &mut String) {
        let _ = write!(output, "<{}", self.name);
        for (key, value) in &self.attributes {
            let _ = write!(output, " {key}=\"{}\"", xml_escape(value));
        }
        if self.children.is_empty() && self.text.trim().is_empty() {
            output.push_str("/>");
            return;
        }
        output.push('>');
        if !self.text.trim().is_empty() {
            output.push_str(&xml_escape(&self.text));
        }
        for child in &self.children {
            child.render(output);
        }
        let _ = write!(output, "</{}>", self.name);
    }
}

pub(super) fn element(
    name: &str,
    attributes: &[(&str, String)],
    children: Vec<Element>,
) -> Element {
    Element {
        name: name.to_string(),
        attributes: attributes
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
        children,
        text: String::new(),
    }
}

pub(super) fn missing(what: &str) -> MfdError {
    MfdError::Unsupported(format!("XML sequence variable: missing {what}"))
}

pub(super) fn key(value: Option<&str>) -> Option<u32> {
    value.and_then(|value| value.parse().ok())
}

pub(super) fn find_entry_path(node: &Element, port: u32, path: &mut Vec<usize>) -> bool {
    if key(node.attr("outkey")) == Some(port) {
        return true;
    }
    for (index, child) in node.children.iter().enumerate() {
        path.push(index);
        if find_entry_path(child, port, path) {
            return true;
        }
        path.pop();
    }
    false
}

pub(super) fn at_path<'a>(mut node: &'a Element, path: &[usize]) -> &'a Element {
    for &index in path {
        node = &node.children[index];
    }
    node
}

/// Static XML sources have two protocol entry levels before their payload.
/// Verify those levels, then accept the payload's name and namespace unchanged.
pub(super) fn document_payload_path(
    root: &Element,
    collection_path: &[usize],
    protocol_namespace: usize,
) -> Option<Vec<usize>> {
    let namespace = protocol_namespace.to_string();
    let mut wrappers = ["FileInstance", "document"].into_iter();
    let mut prefix = Vec::new();
    for &index in collection_path {
        prefix.push(index);
        let entry = at_path(root, &prefix);
        if entry.name != "entry" {
            continue;
        }
        let Some(expected) = wrappers.next() else {
            return Some(prefix);
        };
        if entry.attr("name") != Some(expected) || entry.attr("ns") != Some(namespace.as_str()) {
            return None;
        }
    }
    None
}

pub(super) fn at_path_mut<'a>(mut node: &'a mut Element, path: &[usize]) -> &'a mut Element {
    for &index in path {
        node = &mut node.children[index];
    }
    node
}

pub(super) fn ports(component: &Element, side: &str) -> Vec<u32> {
    let mut result = component
        .child(side)
        .into_iter()
        .flat_map(|side| &side.children)
        .filter_map(|pin| key(pin.attr("key")))
        .collect::<Vec<_>>();
    fn entries(node: &Element, attribute: &str, result: &mut Vec<u32>) {
        if node.name == "entry"
            && let Some(port) = key(node.attr(attribute))
            && !result.contains(&port)
        {
            result.push(port);
        }
        for child in &node.children {
            entries(child, attribute, result);
        }
    }
    if let Some(data) = component.child("data") {
        entries(
            data,
            if side == "sources" {
                "inpkey"
            } else {
                "outkey"
            },
            &mut result,
        );
    }
    result
}

pub(super) fn replace_pin_keys(node: &mut Element, replacements: &BTreeMap<u32, u32>) {
    for attribute in if node.name == "datapoint" {
        &["key"][..]
    } else if node.name == "entry" {
        &["inpkey", "outkey"][..]
    } else {
        &[][..]
    } {
        if let Some(old) = key(node.attr(attribute))
            && let Some(&new) = replacements.get(&old)
        {
            node.set(attribute, new);
        }
    }
    for child in &mut node.children {
        replace_pin_keys(child, replacements);
    }
}
