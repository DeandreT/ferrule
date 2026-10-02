//! Typed XML sequence boundaries for position-dependent scope expressions.
//!
//! A control function carries a sequence but does not establish the native
//! XML payload context by itself. Materializing its output into a typed
//! variable establishes that context without changing the raw predicate DAG.

use std::collections::{BTreeMap, BTreeSet};

use crate::MfdError;

use super::schema::KeyAlloc;

mod selectors;
mod xml;

#[cfg(test)]
mod tests;

pub(super) use selectors::selector_context_warnings;
use xml::{
    Element, at_path, at_path_mut, document_payload_path, element, find_entry_path, key, missing,
    ports, replace_pin_keys,
};

pub(super) struct Request {
    pub(super) source: super::source::XmlSequenceIdentity,
    pub(super) sequence_port: u32,
    pub(super) compute_target: Option<u32>,
    pub(super) owner_target: u32,
    pub(super) position_inputs: BTreeSet<u32>,
    /// Target bindings and downstream predicates evaluated in this context.
    /// Raw predicates and parent-context window bounds are deliberately absent.
    pub(super) payload_sinks: BTreeSet<u32>,
}

fn preserves_xml_items(component: &Element) -> bool {
    component.attr("library") == Some("core")
        && matches!(
            component.attr("name"),
            Some(
                "filter"
                    | "sort"
                    | "skip-first-items"
                    | "first-items"
                    | "items-from"
                    | "items-from-to"
                    | "last-items"
            )
        )
}

fn incoming_edges(graph: &Element) -> Result<BTreeMap<u32, Vec<u32>>, MfdError> {
    let mut incoming = BTreeMap::<u32, Vec<u32>>::new();
    for vertex in graph
        .child("vertices")
        .into_iter()
        .flat_map(|vertices| &vertices.children)
    {
        let from = key(vertex.attr("vertexkey")).ok_or_else(|| missing("source vertex key"))?;
        for edge in vertex
            .child("edges")
            .into_iter()
            .flat_map(|edges| &edges.children)
        {
            let to = key(edge.attr("vertexkey")).ok_or_else(|| missing("target vertex key"))?;
            incoming.entry(to).or_default().push(from);
        }
    }
    Ok(incoming)
}

fn source_origin(
    mut port: u32,
    outputs: &BTreeMap<u32, &Element>,
    incoming: &BTreeMap<u32, Vec<u32>>,
) -> Option<u32> {
    let mut visited = BTreeSet::new();
    while let Some(component) = outputs
        .get(&port)
        .filter(|component| preserves_xml_items(component))
    {
        if !visited.insert(port) || visited.len() > 256 {
            return None;
        }
        let input = *ports(component, "sources").first()?;
        let [source] = incoming.get(&input)?.as_slice() else {
            return None;
        };
        port = *source;
    }
    Some(port)
}

fn plan_with_identity(
    xml: &str,
    mut identity: impl FnMut(u32) -> Result<Option<super::source::XmlSequenceIdentity>, MfdError>,
) -> Result<Vec<Request>, MfdError> {
    let document = roxmltree::Document::parse(xml)?;
    let mapping = Element::read(document.root_element());
    let structure = mapping
        .child("component")
        .and_then(|component| component.child("structure"))
        .ok_or_else(|| missing("mapping structure"))?;
    let children = &structure
        .child("children")
        .ok_or_else(|| missing("components"))?
        .children;
    let incoming = incoming_edges(
        structure
            .child("graph")
            .ok_or_else(|| missing("mapping graph"))?,
    )?;
    let outputs = children
        .iter()
        .flat_map(|component| {
            ports(component, "targets")
                .into_iter()
                .map(move |port| (port, component))
        })
        .collect::<BTreeMap<_, _>>();
    let positions = children
        .iter()
        .filter(|component| {
            component.attr("library") == Some("core") && component.attr("name") == Some("position")
        })
        .filter_map(|position| {
            let input = *ports(position, "sources").first()?;
            let [context] = incoming.get(&input)?.as_slice() else {
                return None;
            };
            let origin = source_origin(*context, &outputs, &incoming)?;
            Some((position, input, *context, origin))
        })
        .collect::<Vec<_>>();
    let position_origins = positions
        .iter()
        .flat_map(|(position, _, _, origin)| {
            ports(position, "targets")
                .into_iter()
                .map(move |output| (output, *origin))
        })
        .collect::<BTreeMap<_, _>>();
    fn position_depends(
        port: u32,
        origin: u32,
        outputs: &BTreeMap<u32, &Element>,
        incoming: &BTreeMap<u32, Vec<u32>>,
        positions: &BTreeMap<u32, u32>,
        visited: &mut BTreeSet<u32>,
    ) -> bool {
        if !visited.insert(port) || visited.len() > 4_096 {
            return false;
        }
        if let Some(position_origin) = positions.get(&port) {
            return *position_origin == origin;
        }
        outputs.get(&port).is_some_and(|component| {
            ports(component, "sources")
                .into_iter()
                .flat_map(|input| incoming.get(&input).into_iter().flatten())
                .any(|source| {
                    position_depends(*source, origin, outputs, incoming, positions, visited)
                })
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn target_contexts(
        node: &Element,
        outputs: &BTreeMap<u32, &Element>,
        incoming: &BTreeMap<u32, Vec<u32>>,
        positions: &BTreeMap<u32, u32>,
        candidates: &mut BTreeMap<u32, BTreeSet<u32>>,
    ) {
        if let Some(input) = key(node.attr("inpkey"))
            && let Some([context]) = incoming.get(&input).map(Vec::as_slice)
            && outputs
                .get(context)
                .is_some_and(|component| preserves_xml_items(component))
            && let Some(origin) = source_origin(*context, outputs, incoming)
        {
            fn bound_position(
                node: &Element,
                origin: u32,
                outputs: &BTreeMap<u32, &Element>,
                incoming: &BTreeMap<u32, Vec<u32>>,
                positions: &BTreeMap<u32, u32>,
            ) -> bool {
                key(node.attr("inpkey"))
                    .and_then(|input| incoming.get(&input))
                    .is_some_and(|sources| {
                        sources.iter().any(|source| {
                            position_depends(
                                *source,
                                origin,
                                outputs,
                                incoming,
                                positions,
                                &mut BTreeSet::new(),
                            )
                        })
                    })
                    || node
                        .children
                        .iter()
                        .any(|child| bound_position(child, origin, outputs, incoming, positions))
            }
            if bound_position(node, origin, outputs, incoming, positions) {
                candidates.entry(*context).or_default();
            }
        }
        for child in &node.children {
            target_contexts(child, outputs, incoming, positions, candidates);
        }
    }
    let mut candidates = BTreeMap::<u32, BTreeSet<u32>>::new();
    for (_, input, context, _) in &positions {
        if outputs
            .get(context)
            .is_some_and(|component| preserves_xml_items(component))
        {
            candidates.entry(*context).or_default().insert(*input);
        }
    }
    for component in children.iter().filter(|component| {
        component.attr("kind") == Some("14") && component.attr("library") == Some("xml")
    }) {
        target_contexts(
            component,
            &outputs,
            &incoming,
            &position_origins,
            &mut candidates,
        );
    }
    let mut requests = BTreeMap::<u32, Request>::new();
    for (context, position_inputs) in candidates {
        let mut upstream = context;
        let mut visited = BTreeSet::new();
        while let Some(component) = outputs
            .get(&upstream)
            .filter(|component| preserves_xml_items(component))
        {
            if visited.len() >= 256 || !visited.insert(upstream) {
                return Err(MfdError::Unsupported(
                    "XML sequence variable control chain is cyclic or exceeds 256 stages"
                        .to_string(),
                ));
            }
            let Some(payload_input) = ports(component, "sources").first().copied() else {
                break;
            };
            let Some([source]) = incoming.get(&payload_input).map(Vec::as_slice) else {
                break;
            };
            upstream = *source;
        }
        let Some(source) = identity(upstream)? else {
            continue;
        };
        let mut payload_sinks = BTreeSet::new();
        let mut compute_target = None;
        let mut owner_target = 0;
        let mut sequences = BTreeSet::from([context]);
        loop {
            let mut added = false;
            for component in children
                .iter()
                .filter(|component| preserves_xml_items(component))
            {
                let inputs = ports(component, "sources");
                if inputs.first().is_some_and(|input| {
                    incoming
                        .get(input)
                        .is_some_and(|values| values.len() == 1 && sequences.contains(&values[0]))
                }) {
                    payload_sinks.extend(inputs.first().copied());
                    // Filter predicates and sort keys run once per current item.
                    // Window bounds retain their existing parent-context graph.
                    if matches!(component.attr("name"), Some("filter" | "sort")) {
                        payload_sinks.extend(inputs.iter().skip(1).copied());
                    }
                    for output in ports(component, "targets") {
                        added |= sequences.insert(output);
                    }
                }
            }
            if !added {
                break;
            }
            if sequences.len() > 4_096 {
                return Err(MfdError::Unsupported(
                    "XML sequence variable downstream chain exceeds 4096 stages".to_string(),
                ));
            }
        }
        #[allow(clippy::too_many_arguments)]
        fn target_sinks(
            node: &Element,
            context: u32,
            sequences: &BTreeSet<u32>,
            incoming: &BTreeMap<u32, Vec<u32>>,
            outputs: &BTreeMap<u32, &Element>,
            parents: &[u32],
            parent: Option<u32>,
            active: bool,
            sinks: &mut BTreeSet<u32>,
            compute: &mut Option<u32>,
        ) {
            let port = key(node.attr("inpkey"));
            let values = port.and_then(|input| incoming.get(&input));
            let starts =
                values.is_some_and(|values| values.len() == 1 && sequences.contains(&values[0]));
            if !active && starts {
                *compute = parent;
            }
            let active = active || starts;
            if active
                && let Some(input) = port
                && !values.is_some_and(|values| {
                    values.len() == 1 && values[0] != context && sequences.contains(&values[0])
                })
            {
                sinks.insert(input);
            }
            let parent = if !active
                && port.is_some()
                && values.is_some_and(|values| {
                    values.len() == 1
                        && source_origin(values[0], outputs, incoming)
                            .is_some_and(|origin| parents.contains(&origin))
                }) {
                port
            } else {
                parent
            };
            for child in &node.children {
                target_sinks(
                    child, context, sequences, incoming, outputs, parents, parent, active, sinks,
                    compute,
                );
            }
        }
        for component in children {
            if component.attr("kind") == Some("14")
                && component.attr("library") == Some("xml")
                && !component.child("data").is_some_and(|data| {
                    data.children.iter().any(|child| {
                        child.name == "parameter" && child.attr("usageKind") == Some("variable")
                    })
                })
            {
                let before = payload_sinks.len();
                target_sinks(
                    component,
                    context,
                    &sequences,
                    &incoming,
                    &outputs,
                    &source.parent_ports,
                    None,
                    false,
                    &mut payload_sinks,
                    &mut compute_target,
                );
                if payload_sinks.len() > before {
                    owner_target = key(component.attr("uid")).unwrap_or(0);
                }
            }
        }
        requests.insert(
            context,
            Request {
                source,
                sequence_port: context,
                compute_target,
                owner_target,
                position_inputs,
                payload_sinks,
            },
        );
    }
    // Earlier sequence stages must establish aliases before later stages.
    // Export allocates each control's output after its upstream controls.
    Ok(requests.into_values().collect())
}

pub(super) fn rewrite_controlled_positions(
    xml: &str,
    project: &mapping::Project,
    sources: &super::source::SourceExports<'_>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
) -> Result<(String, BTreeSet<u32>, Vec<String>), MfdError> {
    let requests = plan_with_identity(xml, |port| sources.xml_sequence_identity_for_port(port))?;
    let warnings = selector_context_warnings(project, sources, !requests.is_empty());
    if !warnings.is_empty() {
        return Ok((xml.to_string(), BTreeSet::new(), warnings));
    }
    let resolved = resolved_position_inputs(xml, &requests)?;
    Ok((rewrite(xml, &requests, keys, uid)?, resolved, Vec::new()))
}

/// A first-context warning is obsolete only when every consuming leaf has a
/// known raw or materialized XML sequence context. Unknown consumers retain
/// their existing compatibility diagnostics.
fn resolved_position_inputs(xml: &str, requests: &[Request]) -> Result<BTreeSet<u32>, MfdError> {
    if requests.is_empty() {
        return Ok(BTreeSet::new());
    }
    let document = roxmltree::Document::parse(xml)?;
    let mapping = Element::read(document.root_element());
    let structure = mapping
        .child("component")
        .and_then(|component| component.child("structure"))
        .ok_or_else(|| missing("mapping structure"))?;
    let children = &structure
        .child("children")
        .ok_or_else(|| missing("components"))?
        .children;
    let incoming = incoming_edges(
        structure
            .child("graph")
            .ok_or_else(|| missing("mapping graph"))?,
    )?;
    let outgoing = incoming
        .iter()
        .flat_map(|(&to, values)| values.iter().map(move |&from| (from, to)))
        .fold(
            BTreeMap::<u32, Vec<u32>>::new(),
            |mut outgoing, (from, to)| {
                outgoing.entry(from).or_default().push(to);
                outgoing
            },
        );
    let outputs = children
        .iter()
        .flat_map(|component| {
            ports(component, "targets")
                .into_iter()
                .map(move |port| (port, component))
        })
        .collect::<BTreeMap<_, _>>();
    let input_owners = children
        .iter()
        .flat_map(|component| {
            ports(component, "sources")
                .into_iter()
                .map(move |port| (port, component))
        })
        .collect::<BTreeMap<_, _>>();
    let mut target_contexts = BTreeMap::new();
    fn target_context(
        node: &Element,
        context: Option<u32>,
        incoming: &BTreeMap<u32, Vec<u32>>,
        result: &mut BTreeMap<u32, Option<u32>>,
    ) {
        let input = key(node.attr("inpkey"));
        let own = input
            .and_then(|input| incoming.get(&input))
            .filter(|values| values.len() == 1)
            .map(|values| values[0]);
        let context = if node.children.iter().any(|child| child.name == "entry") {
            own.or(context)
        } else {
            context
        };
        if let Some(input) = input {
            result.insert(input, context);
        }
        for child in &node.children {
            target_context(child, context, incoming, result);
        }
    }
    for component in children.iter().filter(|component| {
        component.attr("kind") == Some("14") && component.attr("library") == Some("xml")
    }) {
        target_context(component, None, &incoming, &mut target_contexts);
    }
    struct Contexts<'a> {
        origin: u32,
        original: u32,
        requests: &'a [Request],
        outgoing: &'a BTreeMap<u32, Vec<u32>>,
        incoming: &'a BTreeMap<u32, Vec<u32>>,
        outputs: &'a BTreeMap<u32, &'a Element>,
        owners: &'a BTreeMap<u32, &'a Element>,
        targets: &'a BTreeMap<u32, Option<u32>>,
    }
    impl Contexts<'_> {
        fn context(&self, context: Option<u32>) -> bool {
            context.is_some_and(|context| {
                source_origin(context, self.outputs, self.incoming) == Some(self.origin)
                    && (context == self.original
                        || self.requests.iter().any(|request| {
                            request.source.collection_port == self.origin
                                && request.sequence_port == context
                        }))
            })
        }
        fn consumers(&self, output: u32, active: &mut BTreeSet<u32>) -> bool {
            if active.len() >= 256 || !active.insert(output) {
                return false;
            }
            let result = self.outgoing.get(&output).is_some_and(|consumers| {
                !consumers.is_empty()
                    && consumers.iter().all(|input| {
                        if let Some(context) = self.targets.get(input) {
                            return self.context(*context);
                        }
                        let Some(component) = self.owners.get(input) else {
                            return false;
                        };
                        if preserves_xml_items(component) {
                            let inputs = ports(component, "sources");
                            if !matches!(component.attr("name"), Some("filter" | "sort"))
                                || inputs.first() == Some(input)
                            {
                                return false;
                            }
                            return self.context(
                                inputs
                                    .first()
                                    .and_then(|input| self.incoming.get(input))
                                    .filter(|values| values.len() == 1)
                                    .map(|values| values[0]),
                            );
                        }
                        if !matches!(component.attr("kind"), Some("2" | "4" | "5" | "19" | "23")) {
                            return false;
                        }
                        let outputs = ports(component, "targets");
                        !outputs.is_empty()
                            && outputs
                                .into_iter()
                                .all(|output| self.consumers(output, active))
                    })
            });
            active.remove(&output);
            result
        }
    }
    let mut resolved = BTreeSet::new();
    for position in children.iter().filter(|component| {
        component.attr("library") == Some("core") && component.attr("name") == Some("position")
    }) {
        let Some(input) = ports(position, "sources").first().copied() else {
            continue;
        };
        let Some([original]) = incoming.get(&input).map(Vec::as_slice) else {
            continue;
        };
        let Some(origin) = source_origin(*original, &outputs, &incoming) else {
            continue;
        };
        if !requests
            .iter()
            .any(|request| request.source.collection_port == origin)
        {
            continue;
        }
        let contexts = Contexts {
            origin,
            original: *original,
            requests,
            outgoing: &outgoing,
            incoming: &incoming,
            outputs: &outputs,
            owners: &input_owners,
            targets: &target_contexts,
        };
        let position_outputs = ports(position, "targets");
        if !position_outputs.is_empty()
            && position_outputs
                .into_iter()
                .all(|output| contexts.consumers(output, &mut BTreeSet::new()))
        {
            resolved.insert(input);
        }
    }
    Ok(resolved)
}

struct Expressions<'a> {
    original: &'a [Element],
    incoming: &'a BTreeMap<u32, Vec<u32>>,
    replacements: &'a BTreeMap<u32, u32>,
    memo: BTreeMap<u32, u32>,
    active: BTreeSet<u32>,
    components: Vec<Element>,
    retired_components: BTreeSet<u32>,
    edges: Vec<(u32, u32, Option<u32>)>,
    keys: &'a mut KeyAlloc,
    uid: &'a mut u32,
}

impl Expressions<'_> {
    fn output(&mut self, port: u32) -> Result<u32, MfdError> {
        if let Some(&new) = self
            .replacements
            .get(&port)
            .or_else(|| self.memo.get(&port))
        {
            return Ok(new);
        }
        if self.active.len() >= 256 || !self.active.insert(port) {
            return Err(MfdError::Unsupported(
                "XML sequence variable expression is cyclic or exceeds 256 levels".to_string(),
            ));
        }
        let result = self.output_inner(port);
        self.active.remove(&port);
        result
    }

    fn output_inner(&mut self, port: u32) -> Result<u32, MfdError> {
        let Some(component) = self
            .original
            .iter()
            .find(|component| ports(component, "targets").contains(&port))
            .cloned()
        else {
            return Ok(port);
        };
        let inputs = ports(&component, "sources");
        let mut mapped_inputs = Vec::new();
        let mut changed = false;
        for input in &inputs {
            let incoming = self.incoming.get(input).cloned().unwrap_or_default();
            let mut mapped = Vec::with_capacity(incoming.len());
            for old in incoming {
                let new = self.output(old)?;
                changed |= old != new;
                mapped.push(new);
            }
            mapped_inputs.push(mapped);
        }
        if !changed {
            self.memo.insert(port, port);
            return Ok(port);
        }
        if !matches!(
            component.attr("kind"),
            Some("2" | "3" | "4" | "5" | "19" | "23" | "30")
        ) {
            return Err(MfdError::Unsupported(
                "XML sequence variable expression requires a non-scalar component context"
                    .to_string(),
            ));
        }
        if self.components.len() >= 4_096 {
            return Err(MfdError::Unsupported(
                "XML sequence variable expression exceeds 4096 components".to_string(),
            ));
        }
        let outputs = ports(&component, "targets");
        let replacements = inputs
            .iter()
            .chain(&outputs)
            .map(|&old| (old, self.keys.next()))
            .collect::<BTreeMap<_, _>>();
        for (input, upstream) in inputs.iter().zip(mapped_inputs) {
            self.edges.extend(
                upstream
                    .into_iter()
                    .map(|from| (from, replacements[input], None)),
            );
        }
        for output in outputs {
            self.memo.insert(output, replacements[&output]);
        }
        let new = replacements[&port];
        if let Some(original_uid) = key(component.attr("uid")) {
            self.retired_components.insert(original_uid);
        }
        let mut clone = component;
        *self.uid += 1;
        clone.set("uid", *self.uid);
        replace_pin_keys(&mut clone, &replacements);
        self.components.push(clone);
        Ok(new)
    }
}

/// Rewrites only the selected context sinks. Components retained for raw
/// predicates keep their original ports and connections; shared scalar
/// expression components are cloned lazily when a payload field changes.
pub(super) fn rewrite(
    xml: &str,
    requests: &[Request],
    keys: &mut KeyAlloc,
    uid: &mut u32,
) -> Result<String, MfdError> {
    if requests.is_empty() {
        return Ok(xml.to_string());
    }
    let document = roxmltree::Document::parse(xml)?;
    let mut mapping = Element::read(document.root_element());
    let original_structure = document
        .root_element()
        .children()
        .find(|node| node.has_tag_name("component"))
        .and_then(|component| {
            component
                .children()
                .find(|node| node.has_tag_name("structure"))
        })
        .ok_or_else(|| missing("mapping structure"))?;
    let original_children = original_structure
        .children()
        .find(|node| node.has_tag_name("children"))
        .ok_or_else(|| missing("components"))?;
    let original_graph = original_structure
        .children()
        .find(|node| node.has_tag_name("graph"))
        .ok_or_else(|| missing("mapping graph"))?;
    let original_uids = original_children
        .children()
        .filter(roxmltree::Node::is_element)
        .filter_map(|node| key(node.attribute("uid")))
        .collect::<BTreeSet<_>>();
    let mut changed_sources = BTreeSet::new();
    let mut retired_components = BTreeSet::new();
    let mut port_aliases = BTreeMap::new();
    let structure = mapping
        .child_mut("component")
        .and_then(|component| component.child_mut("structure"))
        .ok_or_else(|| missing("mapping structure"))?;
    let graph = structure
        .child("graph")
        .ok_or_else(|| missing("mapping graph"))?;
    let mut edges = Vec::new();
    for vertex in graph
        .child("vertices")
        .into_iter()
        .flat_map(|vertices| &vertices.children)
    {
        let from = key(vertex.attr("vertexkey")).ok_or_else(|| missing("source vertex key"))?;
        for edge in vertex
            .child("edges")
            .into_iter()
            .flat_map(|edges| &edges.children)
        {
            let to = key(edge.attr("vertexkey")).ok_or_else(|| missing("target vertex key"))?;
            edges.push((from, to, key(edge.attr("edgekey"))));
        }
    }
    let mut edge_metadata = graph
        .child("edges")
        .cloned()
        .unwrap_or_else(|| element("edges", &[], Vec::new()));
    let children = structure
        .child_mut("children")
        .ok_or_else(|| missing("components"))?;
    let mut lineage = BTreeMap::<u32, u32>::new();
    for request in requests {
        let sequence_port = port_aliases
            .get(&(request.owner_target, request.sequence_port))
            .copied()
            .unwrap_or(request.sequence_port);
        let source = children
            .children
            .iter_mut()
            .find(|component| key(component.attr("uid")) == Some(request.source.source_uid))
            .ok_or_else(|| missing("source component"))?;
        let data = source
            .child_mut("data")
            .ok_or_else(|| missing("source data"))?;
        let identity = data
            .child("document")
            .ok_or_else(|| missing("source schema identity"))?
            .clone();
        let schema = identity
            .attr("schema")
            .ok_or_else(|| missing("source schema filename"))?;
        let root = data
            .child_mut("root")
            .ok_or_else(|| missing("source entry tree"))?;
        let mut path = Vec::new();
        if !find_entry_path(root, request.source.collection_port, &mut path) {
            return Err(missing("source sequence entry"));
        }
        let mut header = root
            .child("header")
            .ok_or_else(|| missing("source namespace header"))?
            .clone();
        let namespaces = header
            .child("namespaces")
            .ok_or_else(|| missing("source namespaces"))?;
        let native_namespace = namespaces
            .children
            .iter()
            .position(|namespace| namespace.attr("uid") == Some("http://www.altova.com/mapforce"))
            .ok_or_else(|| missing("control namespace"))?;
        let source_root_path = document_payload_path(root, &path, native_namespace)
            .ok_or_else(|| missing("source document root"))?;
        let source_root = at_path_mut(root, &source_root_path);
        let document_trigger = key(source_root.attr("outkey")).unwrap_or_else(|| {
            let trigger = keys.next();
            changed_sources.insert(request.source.source_uid);
            source_root.set("outkey", trigger);
            trigger
        });
        let trigger = request
            .compute_target
            .and_then(|target| {
                edges
                    .iter()
                    .find(|edge| edge.1 == target)
                    .map(|edge| edge.0)
            })
            .unwrap_or(document_trigger);
        let mut payload = at_path(root, &path).clone();
        let compute = keys.next();
        let input = keys.next();
        // The generated XSD is authoritative for exact qualified child names.
        // Display entry trees need not carry every native namespace index.
        let mut namespace_indexes = BTreeMap::new();
        for (index, namespace) in namespaces.children.iter().enumerate() {
            namespace_indexes.insert(namespace.attr("uid").unwrap_or_default().to_string(), index);
        }
        for namespace in request.source.namespaces.values() {
            let uri = namespace.as_deref().unwrap_or_default();
            if !namespace_indexes.contains_key(uri) {
                let index = header
                    .child("namespaces")
                    .ok_or_else(|| missing("variable namespaces"))?
                    .children
                    .len();
                namespace_indexes.insert(uri.to_string(), index);
                header
                    .child_mut("namespaces")
                    .ok_or_else(|| missing("variable namespaces"))?
                    .children
                    .push(element(
                        "namespace",
                        &[("uid", uri.to_string())],
                        Vec::new(),
                    ));
            }
        }
        fn qualify(
            payload: &mut Element,
            namespaces: &BTreeMap<u32, Option<String>>,
            indexes: &BTreeMap<String, usize>,
        ) {
            if let Some(old) = key(payload.attr("outkey"))
                && let Some(namespace) = namespaces.get(&old)
            {
                payload.set("ns", indexes[namespace.as_deref().unwrap_or_default()]);
            }
            for child in &mut payload.children {
                qualify(child, namespaces, indexes);
            }
        }
        qualify(&mut payload, &request.source.namespaces, &namespace_indexes);
        let mut replacements = BTreeMap::new();
        fn rekey(
            payload: &mut Element,
            keys: &mut KeyAlloc,
            replacements: &mut BTreeMap<u32, u32>,
            lineage: &mut BTreeMap<u32, u32>,
        ) {
            payload.attributes.retain(|(name, _)| name != "inpkey");
            if let Some(old) = key(payload.attr("outkey")) {
                let new = keys.next();
                payload.set("outkey", new);
                replacements.insert(old, new);
                lineage.insert(new, old);
            }
            for child in &mut payload.children {
                rekey(child, keys, replacements, lineage);
            }
        }
        rekey(&mut payload, keys, &mut replacements, &mut lineage);
        payload.set("inpkey", input);
        let output = key(payload.attr("outkey")).ok_or_else(|| missing("variable output port"))?;
        replacements.insert(request.sequence_port, output);
        replacements.insert(sequence_port, output);
        for (&alias, &original) in &lineage {
            if let Some(&new) = replacements.get(&original) {
                replacements.insert(alias, new);
            }
        }
        *uid += 1;
        let variable = element(
            "component",
            &[
                ("name", "scope-sequence".to_string()),
                ("library", "xml".to_string()),
                ("uid", uid.to_string()),
                ("kind", "14".to_string()),
            ],
            vec![
                element(
                    "view",
                    &[
                        ("ltx", "400".to_string()),
                        ("lty", "80".to_string()),
                        ("rbx", "570".to_string()),
                        ("rby", "220".to_string()),
                    ],
                    Vec::new(),
                ),
                element(
                    "data",
                    &[],
                    vec![
                        element(
                            "root",
                            &[],
                            vec![
                                header,
                                element(
                                    "entry",
                                    &[
                                        ("name", "compute-when".to_string()),
                                        ("inpkey", compute.to_string()),
                                        ("ns", native_namespace.to_string()),
                                    ],
                                    Vec::new(),
                                ),
                                element(
                                    "entry",
                                    &[
                                        ("name", "document".to_string()),
                                        ("ns", native_namespace.to_string()),
                                        ("expanded", "1".to_string()),
                                        ("casttotargettypemode", "cast-in-subtree".to_string()),
                                    ],
                                    vec![payload],
                                ),
                            ],
                        ),
                        element(
                            "document",
                            &[
                                ("schema", schema.to_string()),
                                ("instanceroot", request.source.instance_root.clone()),
                            ],
                            Vec::new(),
                        ),
                        element("wsdl", &[], Vec::new()),
                        element(
                            "parameter",
                            &[("usageKind", "variable".to_string())],
                            Vec::new(),
                        ),
                    ],
                ),
            ],
        );
        let incoming = edges.iter().fold(
            BTreeMap::<u32, Vec<u32>>::new(),
            |mut incoming, &(from, to, _)| {
                incoming.entry(to).or_default().push(from);
                incoming
            },
        );
        let original = children.children.clone();
        let mut expressions = Expressions {
            original: &original,
            incoming: &incoming,
            replacements: &replacements,
            memo: BTreeMap::new(),
            active: BTreeSet::new(),
            components: Vec::new(),
            retired_components: BTreeSet::new(),
            edges: Vec::new(),
            keys,
            uid,
        };
        for edge in &mut edges {
            if request.position_inputs.contains(&edge.1) && edge.0 == sequence_port {
                edge.0 = output;
            } else if request.payload_sinks.contains(&edge.1) {
                edge.0 = expressions.output(edge.0)?;
            }
        }
        let copy_edge = expressions.keys.next();
        edge_metadata.children.push(element(
            "edge",
            &[("edgekey", copy_edge.to_string())],
            vec![element(
                "data",
                &[],
                vec![element(
                    "dataconnection",
                    &[("type", "2".to_string())],
                    Vec::new(),
                )],
            )],
        ));
        retired_components.extend(expressions.retired_components);
        for (&old, &new) in &expressions.memo {
            if old != new {
                port_aliases.insert((request.owner_target, old), new);
            }
        }
        port_aliases.insert((request.owner_target, request.sequence_port), output);
        edges.extend(expressions.edges);
        edges.extend([
            (trigger, compute, None),
            (sequence_port, input, Some(copy_edge)),
        ]);
        children.children.push(variable);
        children.children.extend(expressions.components);
    }
    // Retire only components cloned by this rewrite and no longer consumed.
    // Repeat because removing an unused wrapper can orphan its old position DAG.
    let mut removed_components = BTreeSet::new();
    loop {
        let mut removed_inputs = BTreeSet::new();
        children.children.retain(|component| {
            let Some(original_uid) = key(component.attr("uid")) else {
                return true;
            };
            if retired_components.contains(&original_uid)
                && ports(component, "targets")
                    .iter()
                    .all(|output| !edges.iter().any(|edge| edge.0 == *output))
            {
                removed_components.insert(original_uid);
                removed_inputs.extend(ports(component, "sources"));
                false
            } else {
                true
            }
        });
        if removed_inputs.is_empty() {
            break;
        }
        edges.retain(|edge| !removed_inputs.contains(&edge.1));
    }
    let live_edge_keys = edges
        .iter()
        .filter_map(|edge| edge.2)
        .collect::<BTreeSet<_>>();
    edge_metadata
        .children
        .retain(|edge| key(edge.attr("edgekey")).is_none_or(|key| live_edge_keys.contains(&key)));
    let vertices = edges.into_iter().fold(
        BTreeMap::<u32, Vec<Element>>::new(),
        |mut vertices, (from, to, edgekey)| {
            let mut attributes = vec![("vertexkey", to.to_string())];
            if let Some(edgekey) = edgekey {
                attributes.push(("edgekey", edgekey.to_string()));
            }
            vertices
                .entry(from)
                .or_default()
                .push(element("edge", &attributes, Vec::new()));
            vertices
        },
    );
    let graph = structure
        .child_mut("graph")
        .ok_or_else(|| missing("mapping graph"))?;
    graph.children = vec![
        edge_metadata,
        element(
            "vertices",
            &[],
            vertices
                .into_iter()
                .map(|(from, edges)| {
                    element(
                        "vertex",
                        &[("vertexkey", from.to_string())],
                        vec![element("edges", &[], edges)],
                    )
                })
                .collect(),
        ),
    ];
    let mut edits = Vec::new();
    let mut rendered_graph = String::new();
    graph.render(&mut rendered_graph);
    edits.push((original_graph.range(), rendered_graph));
    let children = structure
        .child("children")
        .ok_or_else(|| missing("components"))?;
    for source_uid in changed_sources {
        let original = original_children
            .children()
            .find(|node| key(node.attribute("uid")) == Some(source_uid))
            .ok_or_else(|| missing("original source component"))?;
        let changed = children
            .children
            .iter()
            .find(|component| key(component.attr("uid")) == Some(source_uid))
            .ok_or_else(|| missing("rewritten source component"))?;
        let mut rendered = String::new();
        changed.render(&mut rendered);
        edits.push((original.range(), rendered));
    }
    let mut additions = String::new();
    for component in children.children.iter().filter(|component| {
        key(component.attr("uid")).is_some_and(|uid| !original_uids.contains(&uid))
    }) {
        component.render(&mut additions);
        additions.push('\n');
    }
    for removed_uid in removed_components
        .into_iter()
        .filter(|uid| original_uids.contains(uid))
    {
        let original = original_children
            .children()
            .find(|node| key(node.attribute("uid")) == Some(removed_uid))
            .ok_or_else(|| missing("original cloned component"))?;
        edits.push((original.range(), String::new()));
    }
    let children_range = original_children.range();
    let closing = xml[children_range.clone()]
        .rfind("</children>")
        .ok_or_else(|| missing("component closing tag"))?
        + children_range.start;
    edits.push((closing..closing, additions));
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut result = xml.to_string();
    for (range, replacement) in edits {
        result.replace_range(range, &replacement);
    }

    Ok(result)
}
