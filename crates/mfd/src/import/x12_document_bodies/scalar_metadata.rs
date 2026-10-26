//! Exact metadata admission local to the explicit X12-body importer. Do not
//! broaden the ordinary importer or its separate total String-map proof.
use super::*;
use roxmltree::Node as XmlNode;

pub(super) fn is_explicit_scalar(component: XmlNode<'_, '_>) -> bool {
    (component.attribute("library") == Some("core")
        && component
            .attribute("kind")
            .and_then(|s| s.parse::<u32>().ok())
            == Some(23))
        || matches!(
            (component.attribute("library"), component.attribute("name")),
            (Some("lang"), Some("format-guid-string")) | (Some("db"), Some("is-not-null"))
        )
}

fn attributes(node: XmlNode<'_, '_>, allowed: &[&str]) -> bool {
    node.tag_name().namespace().is_none()
        && node
            .attributes()
            .all(|a| a.namespace().is_none() && allowed.contains(&a.name()))
        && !node
            .children()
            .any(|n| n.is_text() && n.text().is_some_and(|s| !s.trim().is_empty()))
}

fn leaf(node: XmlNode<'_, '_>, allowed: &[&str]) -> bool {
    attributes(node, allowed) && !node.children().any(|n| n.is_element())
}

fn single_child<'a, 'input>(node: XmlNode<'a, 'input>, name: &str) -> Option<XmlNode<'a, 'input>> {
    let mut children = node.children().filter(|n| n.has_tag_name(name));
    let child = children.next()?;
    children.next().is_none().then_some(child)
}

pub(super) fn valid(component: XmlNode<'_, '_>, feeds: &BTreeMap<u32, u32>) -> bool {
    if !matches!(
        (
            component.attribute("library"),
            component.attribute("kind"),
            component.attribute("name")
        ),
        (Some("core"), Some("23"), Some("value-map"))
            | (Some("lang"), Some("5"), Some("format-guid-string"))
            | (Some("db"), Some("5"), Some("is-not-null"))
    ) {
        return false;
    }
    // Bound all metadata before collecting table rows. Text/attribute size is
    // bounded too, including discarded presentation metadata.
    let mut bytes = 0usize;
    for (index, node) in component.descendants().enumerate() {
        if index >= 1024 || node.attributes().len() > 8 {
            return false;
        }
        bytes = bytes.saturating_add(node.text().map_or(0, str::len));
        for attribute in node.attributes() {
            bytes = bytes
                .saturating_add(attribute.name().len())
                .saturating_add(attribute.value().len());
        }
        if bytes > 131_072 {
            return false;
        }
    }
    if !attributes(component, &["name", "library", "kind", "uid"])
        || component.children().filter(|n| n.is_element()).any(|n| {
            !matches!(
                n.tag_name().name(),
                "sources" | "targets" | "data" | "properties" | "view"
            ) || n.tag_name().namespace().is_some()
        })
    {
        return false;
    }
    for name in ["properties", "view"] {
        let children = component
            .children()
            .filter(|n| n.has_tag_name(name))
            .collect::<Vec<_>>();
        if children.len() > 1
            || children.iter().any(|n| {
                if name == "properties" {
                    !leaf(*n, &[])
                } else {
                    !leaf(*n, &["ltx", "lty", "rbx", "rby"])
                        || n.attributes().any(|a| a.value().parse::<i32>().is_err())
                }
            })
        {
            return false;
        }
    }
    let mut keys = Vec::new();
    for name in ["sources", "targets"] {
        let Some(wrapper) = single_child(component, name) else {
            return false;
        };
        let points = wrapper
            .children()
            .filter(|n| n.is_element())
            .collect::<Vec<_>>();
        let [point] = points.as_slice() else {
            return false;
        };
        if !attributes(wrapper, &[])
            || !point.has_tag_name("datapoint")
            || !leaf(*point, &["pos", "key"])
            || point.attribute("pos") != Some("0")
        {
            return false;
        }
        let Ok(key) = number(point.attribute("key")) else {
            return false;
        };
        keys.push(key);
    }
    if keys[0] == keys[1]
        || !feeds.contains_key(&keys[0])
        || !feeds.values().any(|key| *key == keys[1])
    {
        return false;
    }
    let data = component
        .children()
        .filter(|n| n.has_tag_name("data"))
        .collect::<Vec<_>>();
    if component.attribute("kind") != Some("23") {
        return data.len() <= 1 && data.iter().all(|n| leaf(*n, &[]));
    }
    let [data] = data.as_slice() else {
        return false;
    };
    let Some(map) = single_child(*data, "valuemap") else {
        return false;
    };
    if !attributes(*data, &[])
        || data.children().filter(|n| n.is_element()).count() != 1
        || !attributes(map, &["defaultValueMode", "enableDefaultValue"])
        || !matches!(map.attribute("defaultValueMode"), None | Some("none"))
        || !matches!(map.attribute("enableDefaultValue"), None | Some("0"))
        || map.children().filter(|n| n.is_element()).count() != 3
    {
        return false;
    }
    let (Some(input), Some(result), Some(table)) = (
        single_child(map, "input"),
        single_child(map, "result"),
        single_child(map, "valuemapTable"),
    ) else {
        return false;
    };
    // The input/result elements and their single position-zero ports define
    // the column roles. Their names are display labels, not role identifiers.
    // The String declarations above the table remain the scalar authority.
    for node in [input, result] {
        if !leaf(node, &["name", "type"])
            || node
                .attribute("name")
                .is_none_or(|name| name.trim().is_empty())
            || !matches!(node.attribute("type"), Some("string" | "String"))
        {
            return false;
        }
    }
    if !attributes(table, &[]) {
        return false;
    }
    let mut seen = BTreeSet::new();
    let mut string_bytes = 0usize;
    for (index, entry) in table.children().filter(|n| n.is_element()).enumerate() {
        if index >= 256 || !entry.has_tag_name("entry") || !leaf(entry, &["from", "to"]) {
            return false;
        }
        let (Some(from), Some(to)) = (entry.attribute("from"), entry.attribute("to")) else {
            return false;
        };
        string_bytes = string_bytes
            .saturating_add(from.len())
            .saturating_add(to.len());
        if string_bytes > 65_536 || !seen.insert(from) {
            return false;
        }
    }
    true
}
