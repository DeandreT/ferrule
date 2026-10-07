//! Physical terminal classification for legacy global rules in serial XML stages.

use std::collections::{BTreeMap, BTreeSet};

use super::{StageSelection, entry_keys, graph, schema, trace_xml_sinks};
use crate::MfdError;

#[derive(Default)]
pub(super) struct Terminals {
    pub(super) inputs: BTreeSet<u32>,
    owner_inputs: BTreeMap<u32, BTreeSet<u32>>,
}

impl Terminals {
    pub(super) fn applies_to(
        &self,
        component: &roxmltree::Node<'_, '_>,
        selection: StageSelection<'_>,
    ) -> bool {
        let Some(owner_inputs) = pins(component, "sources", 2)
            .ok()
            .and_then(|pins| pins.first().copied())
            .and_then(|throw| self.owner_inputs.get(&throw))
        else {
            return false;
        };
        match selection {
            StageSelection::Ordinary => true,
            StageSelection::Into {
                intermediate_key, ..
            } => owner_inputs.contains(&intermediate_key),
            StageSelection::Between { target_key, .. } => owner_inputs.contains(&target_key),
            StageSelection::OutOf { final_inputs, .. } => owner_inputs
                .iter()
                .any(|input| final_inputs.connected.contains(input)),
        }
    }
}

fn refusal(reason: &str) -> MfdError {
    MfdError::UnsupportedImport(format!("serial pipeline exception terminal: {reason}"))
}

fn pins(
    component: &roxmltree::Node<'_, '_>,
    tag: &str,
    maximum: usize,
) -> Result<Vec<u32>, MfdError> {
    let sections = component
        .children()
        .filter(|node| node.has_tag_name(tag))
        .collect::<Vec<_>>();
    if sections.len() > 1 {
        return Err(refusal("duplicate pin sections"));
    }
    let Some(section) = sections.first() else {
        return Ok(Vec::new());
    };
    let mut pins = BTreeMap::new();
    for node in section.children().filter(|node| node.is_element()) {
        if !node.has_tag_name("datapoint") || node.children().any(|child| child.is_element()) {
            return Err(refusal("unsupported pin declaration"));
        }
        let position = schema::parse_u32(node.attribute("pos"))
            .and_then(|pos| usize::try_from(pos).ok())
            .filter(|pos| *pos < maximum)
            .ok_or_else(|| refusal("unsupported pin position"))?;
        let key = schema::parse_u32(node.attribute("key"))
            .ok_or_else(|| refusal("missing or invalid pin identity"))?;
        if pins.insert(position, key).is_some() {
            return Err(refusal("duplicate pin positions"));
        }
    }
    if pins.keys().copied().ne(0..pins.len()) {
        return Err(refusal("noncontiguous pin positions"));
    }
    let result = pins.into_values().collect::<Vec<_>>();
    if result.iter().copied().collect::<BTreeSet<_>>().len() != result.len() {
        return Err(refusal("duplicate pin identities"));
    }
    Ok(result)
}

pub(super) fn read_for_stages(
    structure: &roxmltree::Node<'_, '_>,
    wrapper: &roxmltree::Node<'_, '_>,
) -> Result<Terminals, MfdError> {
    let components = structure
        .children()
        .find(|node| node.has_tag_name("children"))
        .into_iter()
        .flat_map(|children| {
            children
                .children()
                .filter(|node| node.has_tag_name("component"))
        })
        .collect::<Vec<_>>();
    // Keep legacy no-exception stage imports on their original path.
    if !components.iter().any(is_exception) {
        return Ok(Terminals::default());
    }
    if structure.descendants().take(65_537).count() > 65_536 || components.len() > 4096 {
        return Err(refusal(
            "physical ownership proof exceeds its design budget",
        ));
    }
    let edge_from = graph::read_edges(structure, Some(wrapper));
    let connected = edge_from.values().copied().collect::<BTreeSet<_>>();
    let mut consumers = BTreeMap::<u32, Vec<u32>>::new();
    for (&input, &output) in &edge_from {
        consumers.entry(output).or_default().push(input);
    }
    let mut functions = BTreeMap::new();
    for component in &components {
        if matches!(
            component.attribute("library"),
            Some("xml" | "text" | "json" | "binary" | "xbrl" | "xlsx")
        ) {
            continue;
        }
        let outputs = component
            .children()
            .find(|node| node.has_tag_name("targets"))
            .into_iter()
            .flat_map(|node| {
                node.descendants()
                    .filter(|node| node.has_tag_name("datapoint"))
            })
            .filter_map(|node| schema::parse_u32(node.attribute("key")))
            .collect::<Vec<_>>();
        for input in component
            .children()
            .find(|node| node.has_tag_name("sources"))
            .into_iter()
            .flat_map(|node| {
                node.descendants()
                    .filter(|node| node.has_tag_name("datapoint"))
            })
            .filter_map(|node| schema::parse_u32(node.attribute("key")))
        {
            functions.insert(input, outputs.clone());
        }
    }
    classify(
        structure,
        wrapper,
        &components,
        &edge_from,
        &consumers,
        &functions,
        &connected,
    )
}

fn is_exception(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("core") && component.attribute("kind") == Some("18")
}

pub(super) fn classify(
    structure: &roxmltree::Node<'_, '_>,
    wrapper: &roxmltree::Node<'_, '_>,
    components: &[roxmltree::Node<'_, '_>],
    edge_from: &BTreeMap<u32, u32>,
    consumers: &BTreeMap<u32, Vec<u32>>,
    functions: &BTreeMap<u32, Vec<u32>>,
    connected: &BTreeSet<u32>,
) -> Result<Terminals, MfdError> {
    if !components.iter().any(is_exception) {
        return Ok(Terminals::default());
    }
    // Bound the new physical proof before sets or dependency walks are built.
    if structure.descendants().take(65_537).count() > 65_536 || components.len() > 4096 {
        return Err(refusal(
            "physical ownership proof exceeds its design budget",
        ));
    }
    let mut terminals = Terminals::default();
    let mut throws = Vec::new();
    for component in components
        .iter()
        .filter(|component| is_exception(component))
    {
        let inputs = pins(component, "sources", 2)?;
        if inputs.is_empty()
            || !pins(component, "targets", 0)?.is_empty()
            || component
                .children()
                .filter(|node| node.has_tag_name("data"))
                .flat_map(|node| {
                    node.children()
                        .filter(|node| node.has_tag_name("exception"))
                })
                .count()
                != 1
        {
            return Err(refusal(
                "core kind18 needs a throw pin, at most one message pin, one exception marker and no outputs",
            ));
        }
        for &input in &inputs {
            if !terminals.inputs.insert(input) {
                return Err(refusal("shared terminal input identity"));
            }
        }
        if throws.len() == 128 {
            return Err(refusal("physical ownership proof exceeds 128 terminals"));
        }
        throws.push(inputs[0]);
    }
    let mut identities = BTreeMap::<u32, usize>::new();
    for component in components {
        for node in component
            .descendants()
            .filter(|node| node.has_tag_name("entry") || node.has_tag_name("datapoint"))
        {
            for attribute in if node.has_tag_name("entry") {
                &["inpkey", "outkey"][..]
            } else {
                &["key"][..]
            } {
                if let Some(key) = schema::parse_u32(node.attribute(*attribute)) {
                    *identities.entry(key).or_default() += 1;
                }
            }
        }
    }
    if identities.values().any(|count| *count != 1) {
        return Err(refusal(
            "terminal design has aliased physical port identities",
        ));
    }
    let incoming = incoming_counts(structure, wrapper)?;
    if incoming.values().any(|count| *count > 1)
        || throws.iter().any(|input| incoming.get(input) != Some(&1))
    {
        return Err(refusal(
            "throw needs one exact feed and message permits at most one feed",
        ));
    }
    let mut target_owners = BTreeMap::new();
    for (index, component) in components
        .iter()
        .enumerate()
        .filter(|(_, component)| component.attribute("library") == Some("xml"))
    {
        for input in entry_keys(component, "inpkey") {
            if target_owners.insert(input, index).is_some() {
                return Err(refusal("duplicate XML target input identity"));
            }
        }
    }
    for throw in throws {
        let feed = edge_from
            .get(&throw)
            .copied()
            .ok_or_else(|| refusal("throw input is disconnected"))?;
        let filters = components
            .iter()
            .filter(|component| {
                component.attribute("library") == Some("core")
                    && component.attribute("kind") == Some("3")
            })
            .filter_map(|component| {
                pins(component, "targets", 2)
                    .ok()
                    .filter(|outputs| outputs.contains(&feed))
                    .map(|outputs| (component, outputs))
            })
            .collect::<Vec<_>>();
        let [(filter, outputs)] = filters.as_slice() else {
            return Err(refusal("throw needs one ordinary two-output filter owner"));
        };
        let inputs = pins(filter, "sources", 2)?;
        if outputs.len() != 2
            || inputs.len() != 2
            || inputs.iter().any(|input| !edge_from.contains_key(input))
            || consumers.get(&feed).map(Vec::as_slice) != Some(&[throw][..])
        {
            return Err(refusal(
                "filter inputs, distinct branches or exclusive throw ownership are unsupported",
            ));
        }
        let opposite = outputs
            .iter()
            .copied()
            .find(|output| *output != feed)
            .ok_or_else(|| refusal("filter has no opposite branch"))?;
        let sinks = trace_xml_sinks(
            vec![opposite],
            connected,
            consumers,
            functions,
            &target_owners,
            &terminals.inputs,
        )?;
        let owners = sinks.into_iter().collect::<Vec<_>>();
        let [owner] = owners.as_slice() else {
            return Err(refusal("opposite branch needs one XML stage target owner"));
        };
        let owner_inputs = entry_keys(&components[*owner], "inpkey");
        if owner_inputs.is_empty() {
            return Err(refusal("opposite branch has no XML target input identity"));
        }
        terminals.owner_inputs.insert(throw, owner_inputs);
    }
    Ok(terminals)
}

fn incoming_counts(
    structure: &roxmltree::Node<'_, '_>,
    wrapper: &roxmltree::Node<'_, '_>,
) -> Result<BTreeMap<u32, usize>, MfdError> {
    let mut counts = BTreeMap::new();
    let mut edge_count = 0usize;
    let mut observe = |to: Option<u32>| -> Result<(), MfdError> {
        edge_count += 1;
        if edge_count > 65_536 {
            return Err(refusal("physical feed proof exceeds 65536 edges"));
        }
        if let Some(to) = to {
            *counts.entry(to).or_default() += 1;
        }
        Ok(())
    };
    for graph in structure
        .children()
        .filter(|node| node.has_tag_name("graph"))
    {
        for vertex in graph
            .descendants()
            .filter(|node| node.has_tag_name("vertex"))
        {
            for edge in vertex
                .children()
                .filter(|node| node.has_tag_name("edges"))
                .flat_map(|edges| edges.children().filter(|node| node.has_tag_name("edge")))
            {
                observe(schema::parse_u32(edge.attribute("vertexkey")))?;
            }
        }
    }
    for connections in structure
        .children()
        .filter(|node| node.has_tag_name("connections"))
        .chain(
            wrapper
                .children()
                .filter(|node| node.has_tag_name("connections")),
        )
    {
        for edge in connections
            .children()
            .filter(|node| node.has_tag_name("edge"))
        {
            observe(schema::parse_u32(edge.attribute("to")))?;
        }
    }
    Ok(counts)
}
