//! A single invocation-local census. No normalized winner supplies raw identity.
use super::*;
use crate::import::schema::parse_u32;

/// Account decoded metadata before looking up, parsing or normalizing a value.
/// Attribute counts alone do not bound a padded numeric key's scan cost.
pub(super) fn metadata(
    node: roxmltree::Node<'_, '_>,
    budget: &CountConstruction,
) -> Result<(), String> {
    let mut bytes = node.tag_name().name().len();
    bytes = bytes
        .checked_add(node.tag_name().namespace().map_or(0, str::len))
        .ok_or("Count metadata text overflow")?;
    for attribute in node.attributes() {
        for size in [
            attribute.name().len(),
            attribute.value().len(),
            attribute.namespace().map_or(0, str::len),
        ] {
            bytes = bytes
                .checked_add(size)
                .ok_or("Count metadata text overflow")?;
        }
    }
    budget.claim(1 + node.attributes().len(), bytes, 0)
}

#[derive(Clone, Copy)]
pub(super) struct Pin<'a, 'xml> {
    pub owner: roxmltree::Node<'a, 'xml>,
    pub node: roxmltree::Node<'a, 'xml>,
    pub output: bool,
}
pub(super) struct Raw<'a, 'xml> {
    pins: BTreeMap<u32, Vec<Pin<'a, 'xml>>>,
    pub outgoing: BTreeMap<u32, Vec<u32>>,
    pub counts: BTreeMap<usize, (RawOwner, Vec<u32>)>,
}

impl<'a, 'xml> Raw<'a, 'xml> {
    pub fn build(
        structure: roxmltree::Node<'a, 'xml>,
        edges: &crate::import::graph::EdgeInputs,
        budget: &CountConstruction,
    ) -> Result<Self, String> {
        count_visit(Counter::RawBuild);
        let mut raw = Self {
            pins: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            counts: BTreeMap::new(),
        };
        for node in structure.descendants().filter(roxmltree::Node::is_element) {
            metadata(node, budget)?;
            count_visit(Counter::RawElement);
            let (owner, nearest_structure) = ownership(node, budget)?;
            if nearest_structure != Some(structure) {
                continue;
            }
            let Some(owner) = owner else {
                continue;
            };
            if owner
                .parent()
                .is_none_or(|p| !p.has_tag_name("children") || p.parent() != Some(structure))
            {
                continue;
            }
            for (attribute, output) in [("outkey", true), ("inpkey", false)] {
                if let Some(key) = parse_u32(node.attribute(attribute)) {
                    raw.add(
                        key,
                        Pin {
                            owner,
                            node,
                            output,
                        },
                        budget,
                    )?;
                }
            }
            if node.has_tag_name("datapoint")
                && let Some(container) = node.parent()
                && (container.has_tag_name("sources") || container.has_tag_name("targets"))
                && let Some(key) = parse_u32(node.attribute("key"))
            {
                raw.add(
                    key,
                    Pin {
                        owner,
                        node,
                        output: container.has_tag_name("targets"),
                    },
                    budget,
                )?;
            }
        }
        // Audit every saved container against the already-selected feeds. This
        // deliberately builds no second input->feed index.
        for parent in std::iter::once(structure).chain(structure.parent()) {
            for container in parent
                .children()
                .filter(|n| n.has_tag_name("connections") || n.has_tag_name("graph"))
            {
                metadata(container, budget)?;
                if container.has_tag_name("connections") {
                    if container.attributes().len() != 0 {
                        return Err("unsupported edge container".into());
                    }
                    for edge in container.children().filter(roxmltree::Node::is_element) {
                        metadata(edge, budget)?;
                        if !edge.has_tag_name("edge")
                            || edge
                                .attributes()
                                .any(|a| !matches!(a.name(), "from" | "to"))
                            || edge.children().any(|n| n.is_element())
                        {
                            return Err("unsupported edge record".into());
                        }
                        let from =
                            parse_u32(edge.attribute("from")).ok_or("invalid edge source")?;
                        let to = parse_u32(edge.attribute("to")).ok_or("invalid edge target")?;
                        raw.edge(from, to, edges, budget)?;
                    }
                } else {
                    for vertex in container.descendants().filter(|n| n.has_tag_name("vertex")) {
                        metadata(vertex, budget)?;
                        let from = parse_u32(vertex.attribute("vertexkey"))
                            .ok_or("invalid graph vertex")?;
                        for edge in vertex.descendants().filter(|n| n.has_tag_name("edge")) {
                            metadata(edge, budget)?;
                            if edge
                                .attributes()
                                .any(|a| !matches!(a.name(), "vertexkey" | "edgekey"))
                                || edge.children().any(|n| n.is_element())
                            {
                                return Err("unsupported graph edge".into());
                            }
                            let to = parse_u32(edge.attribute("vertexkey"))
                                .ok_or("invalid graph endpoint")?;
                            raw.edge(from, to, edges, budget)?;
                        }
                    }
                }
            }
        }
        if !edges.complete || !edges.conflicts.is_empty() {
            return Err("incomplete or conflicting saved edges".into());
        }
        Ok(raw)
    }
    fn add(
        &mut self,
        key: u32,
        pin: Pin<'a, 'xml>,
        budget: &CountConstruction,
    ) -> Result<(), String> {
        budget.claim(2, 0, 0)?;
        count_visit(Counter::RawPin);
        if pin.output
            && pin.owner.attribute("name") == Some("count")
            && !non_count_header(pin.owner)
        {
            budget.claim(2, 0, 0)?;
            self.counts
                .entry(pin.owner.range().start)
                .or_insert_with(|| (RawOwner::of(pin.owner), Vec::new()))
                .1
                .push(key);
        }
        self.pins.entry(key).or_default().push(pin);
        Ok(())
    }
    fn edge(
        &mut self,
        from: u32,
        to: u32,
        edges: &crate::import::graph::EdgeInputs,
        budget: &CountConstruction,
    ) -> Result<(), String> {
        budget.claim(2, 0, 0)?;
        count_visit(Counter::Edge);
        if edges.feeds.get(&to) != Some(&from) {
            return Err("alternate edge disagrees with selected feed".into());
        }
        let uses = self.outgoing.entry(from).or_default();
        budget.claim(uses.len(), 0, 0)?;
        if uses.contains(&to) {
            return Err("duplicate saved edge".into());
        }
        uses.push(to);
        Ok(())
    }
    pub fn pin(
        &self,
        key: u32,
        output: bool,
        budget: &CountConstruction,
    ) -> Result<Pin<'a, 'xml>, String> {
        budget.claim(1, 0, 0)?;
        let [pin] = self.pins.get(&key).ok_or("missing raw pin")?.as_slice() else {
            return Err("ambiguous raw pin ownership".into());
        };
        if pin.output != output {
            return Err("opposite-direction raw pin".into());
        }
        Ok(*pin)
    }
    pub fn function(
        &self,
        function: &FnComponent,
        origin: Origin,
        budget: &CountConstruction,
    ) -> Result<(), String> {
        for &key in function
            .outputs
            .iter()
            .chain(&function.declared_output_keys)
        {
            let pin = self.pin(key, true, budget)?;
            if origin != Origin::ActiveMain(RawOwner::of(pin.owner)) {
                return Err("function raw/final identity mismatch".into());
            }
        }
        for &key in function.inputs.iter().flatten() {
            let pin = self.pin(key, false, budget)?;
            if origin != Origin::ActiveMain(RawOwner::of(pin.owner)) {
                return Err("function input ownership mismatch".into());
            }
        }
        Ok(())
    }
}

fn ownership<'a, 'xml>(
    node: roxmltree::Node<'a, 'xml>,
    budget: &CountConstruction,
) -> Result<
    (
        Option<roxmltree::Node<'a, 'xml>>,
        Option<roxmltree::Node<'a, 'xml>>,
    ),
    String,
> {
    let mut owner = None;
    for (depth, ancestor) in node.ancestors().enumerate() {
        budget.claim(1, 0, depth)?;
        if owner.is_none() && ancestor.has_tag_name("component") {
            owner = Some(ancestor);
        }
        if ancestor.has_tag_name("structure") {
            return Ok((owner, Some(ancestor)));
        }
    }
    Ok((owner, None))
}

pub(super) fn path(
    pin: Pin<'_, '_>,
    budget: &CountConstruction,
    expected_root: &str,
) -> Result<Vec<String>, String> {
    let mut selected = None;
    for (depth, ancestor) in pin.node.ancestors().enumerate() {
        budget.claim(1, 0, depth)?;
        metadata(ancestor, budget)?;
        if ancestor.has_tag_name("root") {
            selected = Some(ancestor);
            break;
        }
    }
    let root = selected.ok_or("pin has no raw schema root")?;
    let mut top = pin
        .node
        .ancestors()
        .take_while(|n| *n != root)
        .last()
        .ok_or("missing schema root entry")?;
    while matches!(top.attribute("name"), Some("document" | "FileInstance")) {
        metadata(top, budget)?;
        let mut entries = top.children().filter(|n| n.has_tag_name("entry"));
        top = entries.next().ok_or("empty raw document wrapper")?;
        if entries.next().is_some() {
            return Err("ambiguous raw document wrapper".into());
        }
    }
    let json = pin.owner.attribute("library") == Some("json");
    if pin.owner.attribute("library") == Some("xml") {
        metadata(top, budget)?;
        budget.claim(1, expected_root.len(), 0)?;
        let (name, attribute) = crate::import::schema::normalize_xml_entry_name(
            top.attribute("name").unwrap_or_default(),
        );
        if expected_root.len() > 4096
            || attribute
            || name != expected_root
            || !plain_xml_name(top)
            || top.attribute("type").is_some_and(|ty| ty != "element")
        {
            return Err("unproved XML Count root identity".into());
        }
        if let Some(document) = root
            .parent()
            .and_then(|data| data.children().find(|n| n.has_tag_name("document")))
            && let Some(instance_root) = document.attribute("instanceroot")
        {
            metadata(document, budget)?;
            budget.claim(1, instance_root.len(), 0)?;
            if instance_root.strip_prefix("{}") != Some(expected_root) {
                return Err("unproved XML Count document root".into());
            }
        }
        let mut headers = root.children().filter(|n| n.has_tag_name("header"));
        if let Some(header) = headers.next() {
            metadata(header, budget)?;
            if headers.next().is_some() {
                return Err("ambiguous XML Count namespace header".into());
            }
            let mut containers = header.children().filter(|n| n.has_tag_name("namespaces"));
            let namespace = containers
                .next()
                .ok_or("missing XML Count namespace authority")?;
            metadata(namespace, budget)?;
            if containers.next().is_some() {
                return Err("duplicate XML Count namespace containers".into());
            }
            let first = namespace
                .children()
                .find(|n| n.has_tag_name("namespace"))
                .ok_or("missing XML Count namespace zero")?;
            metadata(first, budget)?;
            if first.attribute("uid").is_some_and(|uri| !uri.is_empty()) {
                return Err("qualified XML Count namespace is unsupported".into());
            }
        }
    }
    let mut path = Vec::new();
    let mut bytes = 0usize;
    for entry in pin.node.ancestors().take_while(|n| *n != top) {
        budget.claim(1, 0, path.len())?;
        metadata(entry, budget)?;
        if !entry.has_tag_name("entry")
            || entry.children().any(|n| n.has_tag_name("condition"))
            || entry.attribute("ns").is_some_and(|n| n != "0")
        {
            return Err("unsupported raw schema entry".into());
        }
        if pin.owner.attribute("library") == Some("xml") && !plain_xml_name(entry) {
            return Err("unproved XML Count namespace slot".into());
        }
        if json && entry.attribute("type") != Some("json-property") {
            if entry.attribute("type").is_some_and(|ty| ty != "json-item")
                || !matches!(
                    entry.attribute("name"),
                    Some("object" | "array" | "item" | "number" | "string" | "boolean")
                )
            {
                return Err("unsupported raw JSON boundary path".into());
            }
            continue;
        }
        let (name, attribute) = crate::import::schema::normalize_xml_entry_name(
            entry.attribute("name").unwrap_or_default(),
        );
        if name.is_empty() || name.contains(':') || (entry != pin.node && attribute) {
            return Err("unproved raw schema path".into());
        }
        bytes = bytes
            .checked_add(name.len())
            .ok_or("raw Count path overflow")?;
        budget.claim(1, name.len(), path.len() + 1)?;
        if bytes > 4096 {
            return Err("raw Count path exceeds 4 KiB".into());
        }
        path.push(name.to_owned());
    }
    path.reverse();
    checked_path(&path, budget)?;
    Ok(path)
}

fn plain_xml_name(entry: roxmltree::Node<'_, '_>) -> bool {
    let name = entry.attribute("name").unwrap_or_default();
    name.len() <= 4096
        && name.split_once(':').is_none_or(|(prefix, _)| {
            !prefix.is_empty()
                && prefix.bytes().all(|byte| byte.is_ascii_digit())
                && prefix.parse::<u32>() == Ok(0)
        })
        && entry.attribute("ns").is_none_or(|slot| slot == "0")
}

pub(super) fn attribute(pin: Pin<'_, '_>, budget: &CountConstruction) -> Result<bool, String> {
    metadata(pin.node, budget)?;
    Ok(crate::import::schema::normalize_xml_entry_name(
        pin.node.attribute("name").unwrap_or_default(),
    )
    .1 || pin.node.attribute("type") == Some("attribute"))
}
