use std::collections::{BTreeMap, BTreeSet};

use super::schema::parse_u32;

/// These readers retain data ports, but do not lower the enclosing
/// filename inputs or outputs into acquisition and document-output scopes.
pub(super) fn warn(
    component: roxmltree::Node<'_, '_>,
    feeds: &BTreeMap<u32, u32>,
    format: &str,
    warnings: &mut Vec<String>,
) {
    warn_with_admitted_outputs(component, feeds, format, &BTreeSet::new(), warnings);
}

pub(super) fn warn_with_admitted_outputs(
    component: roxmltree::Node<'_, '_>,
    feeds: &BTreeMap<u32, u32>,
    format: &str,
    admitted_outputs: &BTreeSet<u32>,
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
                        && (if direction == "input" {
                            feeds.contains_key(&key)
                        } else {
                            outputs.contains(&key)
                        })
                        && (direction != "input" || !admitted_outputs.contains(&key))
                    {
                        connected.insert((direction, key));
                    }
                }
            }
        }
    }
    for (direction, key) in connected {
        warnings.push(format!(
            "{format} component `{name}` has connected FileInstance {direction} port `{key}`; filename-driven documents are unsupported"
        ));
    }
}
