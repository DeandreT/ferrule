use ir::SchemaNode;

/// Resolve the declared schema path without adding document-wrapper roles or
/// guessing a suffix. Configuration compilation remains authoritative.
pub(super) fn select(
    data: roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
) -> Result<SchemaNode, &'static str> {
    let parameters = data
        .children()
        .filter(|node| {
            node.has_tag_name("parameter") && node.attribute("usageKind") == Some("variable")
        })
        .collect::<Vec<_>>();
    let [parameter] = parameters.as_slice() else {
        return Err("variable must have one declared parameter");
    };
    if data
        .children()
        .filter(|node| node.has_tag_name("root"))
        .any(|root| {
            root.children().any(|entry| {
                entry.has_tag_name("entry")
                    && matches!(
                        entry.attribute("name"),
                        Some("compute-when" | "parent-context")
                    )
            })
        })
    {
        return Err("variable scope controls require a separate contract");
    }
    let roots = parameter
        .children()
        .filter(|node| node.has_tag_name("root"))
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err("variable must declare one selection root");
    };
    let entries = root
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    if entries.is_empty() || entries.len() > 64 {
        return Err("selection must contain between one and 64 entries");
    }
    let mut selected = schema;
    for (index, entry) in entries.iter().enumerate() {
        if entry
            .attributes()
            .any(|attribute| attribute.name() != "name")
            || entry.children().any(|child| child.is_element())
        {
            return Err("selection must be one flat path of named entries");
        }
        let name = entry
            .attribute("name")
            .filter(|name| !name.is_empty())
            .ok_or("selection entry has no name")?;
        if index == 0 {
            if name != schema.name {
                return Err("selection root does not match its compiled configuration");
            }
        } else {
            selected = selected
                .child(name)
                .ok_or("selected child is absent from its compiled configuration")?;
        }
    }
    Ok(selected.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_selected_segment_has_an_independent_complete_schema() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/edi-structure-variables");
        let compiled =
            format_edi::config::import_config(&directory.join("Envelope.Config"), &["850".into()])
                .unwrap();
        let text = std::fs::read_to_string(directory.join("mapping.mfd")).unwrap();
        let document = roxmltree::Document::parse(&text).unwrap();
        let data = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component") && node.attribute("name") == Some("segment")
            })
            .unwrap()
            .children()
            .find(|node| node.has_tag_name("data"))
            .unwrap();
        let selected = select(data, &compiled.schema).unwrap();
        let expected = SchemaNode::group(
            "N1",
            vec![
                SchemaNode::scalar("Text", ir::ScalarType::String),
                SchemaNode::scalar("Count", ir::ScalarType::Int),
            ],
        );
        assert_eq!(selected, expected);

        let repeated = text.replace(
            "<entry name=\"N1\"/></root></parameter>",
            "</root></parameter>",
        );
        let document = roxmltree::Document::parse(&repeated).unwrap();
        let data = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component") && node.attribute("name") == Some("segment")
            })
            .unwrap()
            .children()
            .find(|node| node.has_tag_name("data"))
            .unwrap();
        let expected = SchemaNode::group("Loop0100", vec![expected]).repeating();
        assert_eq!(select(data, &compiled.schema).unwrap(), expected);
    }
}
