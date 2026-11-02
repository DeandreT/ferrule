//! Prove raw upstream identities before accepting a call; never recover an
//! owner from normalized schema/function maps. Fan-out is not producer ownership.
use std::collections::{BTreeMap, BTreeSet};

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
}

pub(super) fn validate(
    call: roxmltree::Node<'_, '_>,
    children: roxmltree::Node<'_, '_>,
    feeds: &BTreeMap<u32, u32>,
    budget: &ConstructionBudget,
) -> Result<(), String> {
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
                });
                match direction {
                    Direction::Input => authority.inputs += 1,
                    Direction::Output => authority.outputs += 1,
                }
            }
        }
    }

    let mut pending = Vec::new();
    for key in owned_nodes(call).flat_map(input_keys) {
        if let Some(feed) = feeds.get(&key) {
            budget.claim(1, 0, 0)?;
            pending.push(*feed);
        }
    }
    let mut visited = BTreeSet::new();
    while let Some(feed) = pending.pop() {
        budget.claim(1, 0, 0)?;
        if !visited.insert(feed) {
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
        // A unique intermediate output must not launder ambiguity in one of
        // its connected inputs. Include unused arguments and every connected
        // raw input; do not infer a pruned/lazy producer dependency profile.
        for input in owned_nodes(owner).flat_map(input_keys) {
            if let Some(feed) = feeds.get(&input) {
                budget.claim(1, 0, 0)?;
                pending.push(*feed);
            }
        }
    }
    Ok(())
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
