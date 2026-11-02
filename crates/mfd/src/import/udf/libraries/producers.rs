//! Prove raw upstream identities before accepting a call; never recover an
//! owner from normalized schema/function maps. Fan-out is not producer ownership.
use std::collections::{BTreeMap, BTreeSet};

use super::limits::ConstructionBudget;
use crate::import::schema::parse_u32;

pub(super) fn validate(
    call: roxmltree::Node<'_, '_>,
    children: roxmltree::Node<'_, '_>,
    feeds: &BTreeMap<u32, u32>,
    budget: &ConstructionBudget,
) -> Result<(), String> {
    let mut producers = BTreeMap::<u32, (usize, roxmltree::Node<'_, '_>)>::new();
    for owner in children.children().filter(|n| element(*n, "component")) {
        for node in owned_nodes(owner) {
            for key in output_keys(node) {
                budget.claim(1, 0, 0)?;
                producers
                    .entry(key)
                    .and_modify(|(count, _)| *count += 1)
                    .or_insert((1, owner));
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
        let Some(&(count, owner)) = producers.get(&feed) else {
            return Err(format!(
                "imported argument feed `{feed}` has no raw producer declaration; schema/name fallback is not permitted"
            ));
        };
        if count != 1 {
            return Err(format!(
                "imported argument feed `{feed}` has ambiguous raw producer ownership ({count} declarations); no producer-order winner is selected"
            ));
        }
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
    owner
        .descendants()
        .filter(move |node| node.ancestors().find(|n| element(*n, "component")) == Some(owner))
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
