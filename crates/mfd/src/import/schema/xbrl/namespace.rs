use std::collections::BTreeMap;

use mapping::XbrlNamespaceBinding;

use super::super::normalize_xml_entry_name;

pub(super) fn bindings(
    root: &roxmltree::Node<'_, '_>,
    payload: &roxmltree::Node<'_, '_>,
) -> Result<Vec<XbrlNamespaceBinding>, String> {
    let namespaces = root
        .children()
        .find(|node| node.has_tag_name("header"))
        .and_then(|header| {
            header
                .children()
                .find(|node| node.has_tag_name("namespaces"))
        })
        .map(|container| {
            container
                .children()
                .filter(|node| node.has_tag_name("namespace"))
                .map(|node| node.attribute("uid").unwrap_or_default())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut by_path = BTreeMap::new();
    collect(payload, &namespaces, &mut Vec::new(), &mut by_path)?;
    by_path
        .into_iter()
        .map(|(path, namespace)| {
            XbrlNamespaceBinding::new(path, namespace)
                .map_err(|error| format!("invalid XBRL namespace binding ({error})"))
        })
        .collect()
}

fn collect<'a>(
    entry: &roxmltree::Node<'a, '_>,
    namespaces: &[&'a str],
    path: &mut Vec<String>,
    bindings: &mut BTreeMap<Vec<String>, String>,
) -> Result<(), String> {
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        if !has_connected_port(&child) {
            continue;
        }
        let (name, _) = normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
        if name.is_empty() {
            return Err("connected XBRL namespace entry has no name".to_string());
        }
        path.push(name.to_string());
        if let Some(raw_index) = child.attribute("ns") {
            let index = raw_index.parse::<usize>().map_err(|_| {
                format!("XBRL namespace slot `{raw_index}` is not a non-negative integer")
            })?;
            let namespace = namespaces.get(index).copied().ok_or_else(|| {
                format!("XBRL namespace slot `{index}` is not declared in the entry header")
            })?;
            if !namespace.is_empty()
                && let Some(existing) = bindings.insert(path.clone(), namespace.to_string())
                && existing != namespace
            {
                return Err(format!(
                    "XBRL path `{}` has conflicting namespace bindings `{existing}` and `{namespace}`",
                    path.join("/")
                ));
            }
        }
        collect(&child, namespaces, path, bindings)?;
        path.pop();
    }
    Ok(())
}

fn has_connected_port(entry: &roxmltree::Node<'_, '_>) -> bool {
    entry.descendants().any(|node| {
        node.has_tag_name("entry")
            && (node.attribute("inpkey").is_some() || node.attribute("outkey").is_some())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_connected_namespace_paths_and_rejects_unknown_slots() {
        let xml = r#"<root><header><namespaces><namespace/><namespace uid="urn:xbrli"/><namespace uid="urn:concept"/></namespaces></header>
          <entry name="xbrl"><entry name="Table"><entry name="period" ns="1" inpkey="1"/><entry name="Amount" ns="2" inpkey="2"/><entry name="Ignored" ns="9"/></entry></entry>
        </root>"#;
        let document = roxmltree::Document::parse(xml).unwrap();
        let root = document.root_element();
        let payload = root
            .children()
            .find(|node| node.has_tag_name("entry"))
            .unwrap();
        let retained = bindings(&root, &payload).unwrap();
        assert_eq!(retained.len(), 2);
        assert!(retained.iter().any(|binding| {
            binding.path() == ["Table", "Amount"] && binding.namespace() == "urn:concept"
        }));

        let malformed = xml.replace("ns=\"2\" inpkey=\"2\"", "ns=\"8\" inpkey=\"2\"");
        let document = roxmltree::Document::parse(&malformed).unwrap();
        let root = document.root_element();
        let payload = root
            .children()
            .find(|node| node.has_tag_name("entry"))
            .unwrap();
        assert!(matches!(bindings(&root, &payload), Err(error) if error.contains("slot `8`")));
    }
}
