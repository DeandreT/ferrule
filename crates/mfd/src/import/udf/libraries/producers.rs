//! Prove raw upstream identities before accepting a call; never recover an
//! owner from normalized schema/function maps. Fan-out is not producer ownership.
use std::collections::{BTreeMap, BTreeSet};

use super::super::certificate::{ArgumentClosure, Boundary, Profile, RawOutput, RawOwner, RawPin};
use super::limits::ConstructionBudget;
use crate::import::schema::parse_u32;

#[derive(Clone, Copy)]
enum Direction {
    Input,
    Output,
}

struct PinAuthority<'a, 'input> {
    inputs: usize,
    outputs: usize,
    // This owner is usable only after proving exactly one output and no input
    // claims. A collided key never selects its first (or last) declaration.
    owner: roxmltree::Node<'a, 'input>,
    pin: roxmltree::Node<'a, 'input>,
}

pub(super) fn prove(
    call: roxmltree::Node<'_, '_>,
    children: roxmltree::Node<'_, '_>,
    feeds: &BTreeMap<u32, u32>,
    budget: &ConstructionBudget,
) -> Result<ArgumentClosure, String> {
    let mut pins = BTreeMap::<u32, PinAuthority<'_, '_>>::new();
    for owner in children.children().filter(|n| element(*n, "component")) {
        for node in owned_nodes(owner) {
            for (key, direction) in input_keys(node)
                .map(|key| (key, Direction::Input))
                .chain(output_keys(node).map(|key| (key, Direction::Output)))
            {
                budget.claim(1, 0, 0)?;
                let authority = pins.entry(key).or_insert(PinAuthority {
                    inputs: 0,
                    outputs: 0,
                    owner,
                    pin: node,
                });
                match direction {
                    Direction::Input => authority.inputs += 1,
                    Direction::Output => authority.outputs += 1,
                }
            }
        }
    }

    let mut pending = Vec::new();
    let mut closure = ArgumentClosure::default();
    for key in owned_nodes(call).flat_map(input_keys) {
        if let Some(feed) = feeds.get(&key) {
            budget.claim(1, 0, 0)?;
            pending.push((*feed, false));
        }
    }
    // Explicit receiving scopes are dependencies too. A promoted source must
    // not steal a row driver while the scalar argument remains unique.
    for output in owned_nodes(call).flat_map(output_keys) {
        for (&input, _) in feeds.iter().filter(|(_, feed)| **feed == output) {
            for owner in children.children().filter(|n| element(*n, "component")) {
                for entry in
                    owned_nodes(owner).filter(|node| input_keys(*node).any(|key| key == input))
                {
                    budget.claim(1, 0, 0)?;
                    let range = entry.range();
                    let mut boundary = Boundary {
                        pin: RawPin {
                            owner: RawOwner::of(owner),
                            range: (range.start, range.end),
                            output: false,
                        },
                        key: input,
                        output,
                        attribute: crate::import::schema::normalize_xml_entry_name(
                            entry.attribute("name").unwrap_or_default(),
                        )
                        .1 || entry.attribute("type") == Some("attribute"),
                        path: xml_path(owner, entry, budget)?,
                        scopes: Vec::new(),
                    };
                    for ancestor in entry.ancestors().skip(1).take_while(|node| *node != owner) {
                        for input in input_keys(ancestor) {
                            if let Some(feed) = feeds.get(&input) {
                                let authority = pins
                                    .get(&input)
                                    .ok_or("consumer scope has no raw input identity")?;
                                if authority.inputs != 1
                                    || authority.outputs != 0
                                    || authority.owner != owner
                                    || authority.pin != ancestor
                                {
                                    return Err(
                                        "consumer scope has ambiguous raw input ownership".into()
                                    );
                                }
                                budget.claim(1, 0, 0)?;
                                pending.push((*feed, true));
                                let path = xml_path(owner, ancestor, budget)?.unwrap_or_default();
                                budget.claim(1, 0, 0)?;
                                boundary.scopes.push((path, input, *feed));
                            }
                        }
                    }
                    closure.boundaries.push(boundary);
                }
            }
        }
    }
    let mut visited = BTreeSet::new();
    while let Some((feed, driver)) = pending.pop() {
        budget.claim(1, 0, 0)?;
        if !visited.insert((feed, driver)) {
            continue;
        }
        let Some(authority) = pins.get(&feed) else {
            return Err(format!(
                "imported argument feed `{feed}` has no raw producer declaration; schema/name fallback is not permitted"
            ));
        };
        // Source readers may retain input-direction entries in source.ports;
        // an output-only census does not prove which owner lowering selects.
        // Opposite claims must remain visible, even when disconnected.
        if authority.inputs != 0 {
            return Err(format!(
                "imported argument feed `{feed}` has conflicting raw pin authority ({} output, {} input declarations); no normalized-map winner is selected",
                authority.outputs, authority.inputs
            ));
        }
        if authority.outputs != 1 {
            return Err(format!(
                "imported argument feed `{feed}` has ambiguous raw producer ownership ({} declarations); no producer-order winner is selected",
                authority.outputs
            ));
        }
        let owner = authority.owner;
        budget.claim(1, 0, 0)?;
        let mut proof = output_proof(owner, authority.pin, budget)?;
        if matches!(&proof.profile, Profile::Function { inputs, .. } if inputs.iter().flatten().any(|pin| !feeds.contains_key(pin)))
        {
            proof.profile = Profile::Unproved;
        }
        if driver {
            closure.drivers.insert(feed, proof);
        } else {
            closure.arguments.insert(feed, proof);
        }
        // A unique intermediate output must not launder ambiguity in one of
        // its connected inputs. Include unused arguments and every connected
        // raw input; do not infer a pruned/lazy producer dependency profile.
        for input in owned_nodes(owner).flat_map(input_keys) {
            if let Some(feed) = feeds.get(&input) {
                budget.claim(1, 0, 0)?;
                pending.push((*feed, driver));
            }
        }
    }
    for key in owned_nodes(call).flat_map(output_keys) {
        let authority = pins
            .get(&key)
            .ok_or("declared output has no raw authority")?;
        if authority.inputs != 0 || authority.outputs != 1 || authority.owner != call {
            return Err("declared output does not have unique active call ownership".into());
        }
        budget.claim(1, 0, 0)?;
        let range = authority.pin.range();
        closure.outputs.insert(
            key,
            RawOutput {
                pin: RawPin {
                    owner: RawOwner::of(call),
                    range: (range.start, range.end),
                    output: true,
                },
                profile: Profile::DeclaredCall,
            },
        );
    }
    Ok(closure)
}

#[cfg(test)]
fn validate(
    call: roxmltree::Node<'_, '_>,
    children: roxmltree::Node<'_, '_>,
    feeds: &BTreeMap<u32, u32>,
    budget: &ConstructionBudget,
) -> Result<(), String> {
    prove(call, children, feeds, budget).map(|_| ())
}

fn output_proof(
    owner: roxmltree::Node<'_, '_>,
    pin: roxmltree::Node<'_, '_>,
    budget: &ConstructionBudget,
) -> Result<RawOutput, String> {
    let range = pin.range();
    let identity = RawPin {
        owner: RawOwner::of(owner),
        range: (range.start, range.end),
        output: true,
    };
    let profile = if owner.attribute("library") == Some("xml")
        && parse_u32(owner.attribute("kind")) == Some(14)
    {
        xml_path(owner, pin, budget)?.map_or(Profile::Unproved, |path| Profile::XmlField {
            path,
            attribute: crate::import::schema::normalize_xml_entry_name(
                pin.attribute("name").unwrap_or_default(),
            )
            .1 || pin.attribute("type") == Some("attribute"),
        })
    } else if matches!(owner.attribute("library"), Some("core" | "lang"))
        && matches!(parse_u32(owner.attribute("kind")), Some(2 | 5))
    {
        match (
            function_pins(owner, "sources", budget)?,
            function_pins(owner, "targets", budget)?,
        ) {
            (Some(inputs), Some(outputs)) if outputs.len() == 1 && outputs[0].is_some() => {
                budget.claim(outputs.len(), 0, 0)?;
                Profile::Function {
                    inputs,
                    outputs: outputs.into_iter().flatten().collect(),
                }
            }
            _ => Profile::Unproved,
        }
    } else {
        Profile::Unproved
    };
    Ok(RawOutput {
        pin: identity,
        profile,
    })
}

fn function_pins(
    owner: roxmltree::Node<'_, '_>,
    tag: &str,
    budget: &ConstructionBudget,
) -> Result<Option<Vec<Option<u32>>>, String> {
    let mut containers = owner.children().filter(|n| element(*n, tag));
    let Some(container) = containers.next() else {
        return Ok(Some(Vec::new()));
    };
    if containers.next().is_some() || container.attributes().len() != 0 {
        return Ok(None);
    }
    let mut pins = Vec::new();
    for (index, point) in container.children().filter(|n| n.is_element()).enumerate() {
        if !element(point, "datapoint")
            || point.children().any(|n| n.is_element())
            || point
                .attributes()
                .any(|a| !matches!(a.name(), "key" | "pos"))
            || point
                .attribute("pos")
                .is_some_and(|value| parse_u32(Some(value)) != u32::try_from(index).ok())
            || index >= 64
        {
            return Ok(None);
        }
        budget.claim(1, 0, 0)?;
        let key = parse_u32(point.attribute("key"));
        if key.is_none() {
            return Ok(None);
        }
        pins.push(key);
    }
    Ok(Some(pins))
}

fn xml_path(
    owner: roxmltree::Node<'_, '_>,
    pin: roxmltree::Node<'_, '_>,
    budget: &ConstructionBudget,
) -> Result<Option<Vec<String>>, String> {
    use crate::import::schema::{normalize_xml_entry_name, xml_content_entry};
    if owner
        .children()
        .any(|node| element(node, "properties") && node.attribute("PassThrough") == Some("1"))
    {
        return Ok(None);
    }
    let Some(root) = owner
        .children()
        .find(|n| element(*n, "data"))
        .and_then(|data| data.children().find(|n| element(*n, "root")))
        .and_then(xml_content_entry)
    else {
        return Ok(None);
    };
    let data = root.ancestors().find(|node| element(*node, "data"));
    let document_root = data
        .and_then(|data| data.children().find(|node| element(*node, "document")))
        .and_then(|document| document.attribute("instanceroot"))
        .and_then(|name| name.strip_prefix("{}"));
    let root_name = normalize_xml_entry_name(root.attribute("name").unwrap_or_default()).0;
    if root_name.is_empty()
        || root_name.contains(':')
        || document_root != Some(root_name)
        || root.attribute("ns").is_some_and(|value| value != "0")
    {
        return Ok(None);
    }
    if let Some(header) = data
        .and_then(|data| data.children().find(|node| element(*node, "root")))
        .and_then(|root| root.children().find(|node| element(*node, "header")))
    {
        let namespace = header
            .children()
            .find(|node| element(*node, "namespaces"))
            .and_then(|namespaces| {
                namespaces
                    .children()
                    .find(|node| element(*node, "namespace"))
            });
        if namespace.is_none_or(|node| node.attribute("uid").is_some_and(|uri| !uri.is_empty())) {
            return Ok(None);
        }
    }
    if pin == root || !pin.ancestors().any(|n| n == root) || !element(pin, "entry") {
        return Ok(None);
    }
    let mut path = Vec::new();
    for entry in pin.ancestors().take_while(|n| *n != root) {
        if !element(entry, "entry")
            || entry.attribute("ns").is_some_and(|value| value != "0")
            || entry.children().any(|n| element(n, "condition"))
            || entry
                .attribute("type")
                .is_some_and(|value| !matches!(value, "element" | "attribute"))
        {
            return Ok(None);
        }
        let name = normalize_xml_entry_name(entry.attribute("name").unwrap_or_default()).0;
        if name.is_empty() || name.contains(':') || name == "FileInstance" {
            return Ok(None);
        }
        budget.claim(1, name.len(), path.len())?;
        path.push(name.to_owned());
    }
    path.reverse();
    Ok(Some(path))
}

fn element(node: roxmltree::Node<'_, '_>, local_name: &str) -> bool {
    node.is_element() && node.tag_name().name() == local_name
}

fn owned_nodes<'a, 'input>(
    owner: roxmltree::Node<'a, 'input>,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    // Nested definition/component namespaces are not global graph port scopes.
    // Local tag matching includes aliases consumed by the existing raw readers.
    let graph = owner.ancestors().find(|n| element(*n, "structure"));
    owner.descendants().filter(move |node| {
        node.ancestors().find(|n| element(*n, "component")) == Some(owner)
            && node.ancestors().find(|n| element(*n, "structure")) == graph
    })
}

fn output_keys(node: roxmltree::Node<'_, '_>) -> impl Iterator<Item = u32> {
    parse_u32(node.attribute("outkey")).into_iter().chain(
        (element(node, "datapoint") && node.parent().is_some_and(|n| element(n, "targets")))
            .then(|| parse_u32(node.attribute("key")))
            .flatten(),
    )
}

fn input_keys(node: roxmltree::Node<'_, '_>) -> impl Iterator<Item = u32> {
    parse_u32(node.attribute("inpkey")).into_iter().chain(
        (element(node, "datapoint") && node.parent().is_some_and(|n| element(n, "sources")))
            .then(|| parse_u32(node.attribute("key")))
            .flatten(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_pin_claims_are_confined_to_the_owning_graph_and_component() {
        let text = r#"<mapping><component><structure><children>
<component name="source"><data><root><entry outkey="10"/></root></data><structure><sources><datapoint key="10"/></sources><children><component><targets><datapoint key="10"/></targets></component></children></structure></component>
<component name="call"><data><root><entry inpkey="20"/></root></data></component>
</children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component>
<component library="local"><structure><children><component><sources><datapoint key="10"/></sources></component></children></structure></component></mapping>"#;
        let document = roxmltree::Document::parse(text).unwrap();
        let call = document
            .descendants()
            .find(|n| n.attribute("name") == Some("call"))
            .unwrap();
        let children = call.parent().unwrap();
        let structure = children.parent().unwrap();
        let feeds = crate::import::graph::read_edges(&structure, structure.parent().as_ref());
        validate(call, children, &feeds, &ConstructionBudget::default()).unwrap();
    }
}
