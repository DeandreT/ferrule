//! The explicit array and item edges must agree on both physical owners.
use std::collections::{BTreeMap, BTreeSet};

use roxmltree::Node;

pub(super) struct Receiver {
    pub frame: Vec<String>,
    // Scalar inputs directly under the selected target item, by physical pin.
    targets: BTreeMap<u32, Vec<String>>,
    edges: BTreeMap<u32, u32>,
}

fn one<'a, 'i: 'a>(mut nodes: impl Iterator<Item = Node<'a, 'i>>) -> Option<Node<'a, 'i>> {
    let first = nodes.next()?;
    nodes.next().is_none().then_some(first)
}

fn named<'a, 'i>(p: Node<'a, 'i>, tag: &str) -> Option<Node<'a, 'i>> {
    one(p.children().filter(|n| n.has_tag_name(tag)))
}

fn entry<'a, 'i>(p: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    one(p
        .children()
        .filter(|n| n.has_tag_name("entry") && n.attribute("name") == Some(name)))
}

fn arrays<'a, 'i>(component: Node<'a, 'i>, pin: &str) -> Option<(String, u32, u32, Node<'a, 'i>)> {
    let data = named(component, "data")?;
    let object = entry(
        entry(
            entry(entry(named(data, "root")?, "FileInstance")?, "document")?,
            "root",
        )?,
        "object",
    )?;
    let mut candidates = object.children().filter_map(|p| {
        if !p.has_tag_name("entry") || p.attribute("type") != Some("json-property") {
            return None;
        }
        let array = entry(p, "array")?;
        let item = entry(array, "item")?;
        if item.attribute("type") != Some("json-item") {
            return None;
        }
        let obj = entry(item, "object")?;
        Some((
            p.attribute("name")?.to_owned(),
            array.attribute(pin)?.parse().ok()?,
            obj.attribute(pin)?.parse().ok()?,
            obj,
        ))
    });
    let result = candidates.next()?;
    candidates.next().is_none().then_some(result)
}

impl Receiver {
    pub(super) fn read(parser: Node<'_, '_>) -> Option<Self> {
        let children = parser.parent()?;
        let structure = children.parent()?;
        let components: Vec<_> = children
            .children()
            .filter(|n| n.has_tag_name("component"))
            .collect();
        let json: Vec<_> = components
            .iter()
            .copied()
            .filter(|n| n.attribute("library") == Some("json"))
            .collect();
        if json.len() != 3
            || components
                .iter()
                .any(|n| !matches!(n.attribute("library"), Some("json" | "core")))
        {
            return None;
        }
        let physical: Vec<_> = json.into_iter().filter(|n| *n != parser).collect();
        let (frame, src_array, src_item, _) = physical.iter().find_map(|n| arrays(*n, "outkey"))?;
        let (result, dst_array, dst_item, obj) =
            physical.iter().find_map(|n| arrays(*n, "inpkey"))?;
        let mut targets = BTreeMap::new();
        for p in obj.children().filter(|n| n.has_tag_name("entry")) {
            if p.attribute("type") != Some("json-property") {
                return None;
            }
            let scalar = one(p.children().filter(|n| n.has_tag_name("entry")))?;
            if !matches!(scalar.attribute("name"), Some("string" | "number")) {
                return None;
            }
            if scalar.attribute("name") == Some("string")
                && let Some(pin) = scalar.attribute("inpkey")
            {
                targets.insert(
                    pin.parse().ok()?,
                    vec![result.clone(), p.attribute("name")?.to_owned()],
                );
            }
        }
        // Normalized feeds can hide competing wires and duplicate pins.
        let mut pins = BTreeSet::<u32>::new();
        for n in components
            .iter()
            .flat_map(|n| n.descendants())
            .filter(Node::is_element)
        {
            if n.has_tag_name("component") && n.parent() != Some(children) {
                return None;
            }
            if matches!(n.attribute("name"), Some("compute-when" | "parent-context")) {
                return None;
            }
            for a in ["inpkey", "outkey", "key"] {
                if let Some(v) = n.attribute(a)
                    && !pins.insert(v.parse().ok()?)
                {
                    return None;
                }
            }
        }
        let graph = named(structure, "graph")?;
        if graph
            .children()
            .filter(|n| n.has_tag_name("edges"))
            .any(|n| n.children().any(|e| e.is_element()))
        {
            return None;
        }
        let mut edges = BTreeMap::new();
        for v in named(graph, "vertices")?
            .children()
            .filter(Node::is_element)
        {
            if !v.has_tag_name("vertex") {
                return None;
            }
            let src = v.attribute("vertexkey")?.parse::<u32>().ok()?;
            for e in named(v, "edges")?.children().filter(Node::is_element) {
                if !e.has_tag_name("edge") || e.attributes().len() != 1 {
                    return None;
                }
                let dst = e.attribute("vertexkey")?.parse::<u32>().ok()?;
                if !pins.contains(&src) || !pins.contains(&dst) || edges.insert(dst, src).is_some()
                {
                    return None;
                }
            }
        }
        if edges.get(&dst_array) != Some(&src_array) || edges.get(&dst_item) != Some(&src_item) {
            return None;
        }
        Some(Self {
            frame: vec![frame],
            targets,
            edges,
        })
    }

    pub(super) fn target(&self, feed: u32) -> Option<&[String]> {
        let mut targets = self
            .targets
            .iter()
            .filter(|(pin, _)| self.edges.get(pin) == Some(&feed));
        let path = targets.next()?.1;
        targets.next().is_none().then_some(path)
    }

    pub(super) fn uses(&self, feed: u32) -> usize {
        self.edges.values().filter(|&&v| v == feed).count()
    }
}
