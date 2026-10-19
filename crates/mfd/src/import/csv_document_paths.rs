use std::collections::{BTreeMap, BTreeSet};

use super::schema::parse_u32;

/// CSV lowering retains row/field ports, but does not lower the enclosing
/// filename inputs or outputs into acquisition and document-output scopes.
pub(super) fn warn(
    component: roxmltree::Node<'_, '_>,
    feeds: &BTreeMap<u32, u32>,
    warnings: &mut Vec<String>,
) {
    let name = component.attribute("name").unwrap_or_default();
    let outputs = feeds.values().copied().collect::<BTreeSet<_>>();
    let mut connected = BTreeSet::new();
    for data in component
        .children()
        .filter(|node| node.has_tag_name("data"))
    {
        for root in data.children().filter(|node| node.has_tag_name("root")) {
            for wrapper in root.children().filter(|node| {
                node.has_tag_name("entry") && node.attribute("name") == Some("FileInstance")
            }) {
                for (attribute, direction) in [("inpkey", "input"), ("outkey", "output")] {
                    if let Some(key) = parse_u32(wrapper.attribute(attribute))
                        && if direction == "input" {
                            feeds.contains_key(&key)
                        } else {
                            outputs.contains(&key)
                        }
                    {
                        connected.insert((direction, key));
                    }
                }
            }
        }
    }
    for (direction, key) in connected {
        warnings.push(format!(
            "CSV component `{name}` has connected FileInstance {direction} port `{key}`; filename-driven documents are unsupported"
        ));
    }
}
