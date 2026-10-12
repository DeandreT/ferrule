use ir::{SchemaKind, SchemaNode};

/// Variable selections describe a value below the schema document; structural
/// views in the saved path do not become property names.
pub(super) fn select(
    data: roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
) -> Result<Option<SchemaNode>, &'static str> {
    let parameters = data
        .children()
        .filter(|node| {
            node.has_tag_name("parameter") && node.attribute("usageKind") == Some("variable")
        })
        .collect::<Vec<_>>();
    let parameter = match parameters.as_slice() {
        [] => return Ok(None),
        [parameter] => *parameter,
        _ => return Err("variable must have one declared parameter"),
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
        return Err("compute-when and parent-context controls require a separate scope contract");
    }
    let roots = parameter
        .children()
        .filter(|node| node.has_tag_name("root"))
        .collect::<Vec<_>>();
    let selected = match roots.as_slice() {
        [] => schema,
        [root] => {
            let entries = root
                .children()
                .filter(|node| node.has_tag_name("entry"))
                .collect::<Vec<_>>();
            if entries.is_empty() || entries.len() > 64 {
                return Err("selection must contain between one and 64 path entries");
            }
            let mut selected = schema;
            for (index, entry) in entries.iter().enumerate() {
                if entry.children().any(|node| node.has_tag_name("entry")) {
                    return Err("selection must be one flat declared path");
                }
                match (entry.attribute("type"), entry.attribute("name")) {
                    (None, Some("root")) if index == 0 => {}
                    (None, Some("object"))
                        if !selected.repeating
                            && matches!(selected.kind, SchemaKind::Group { .. }) => {}
                    (Some("json-property"), Some(name)) => {
                        if selected.repeating {
                            return Err("selection crosses an array");
                        }
                        if !selected.alternatives().is_empty() {
                            return Err("selection crosses alternative object views");
                        }
                        selected = selected
                            .child(name)
                            .ok_or("selected property is absent from its schema")?;
                    }
                    _ => return Err("selection has an unsupported structural view"),
                }
            }
            selected
        }
        _ => return Err("selection contains multiple roots"),
    };
    if selected.repeating {
        return Err("selected root is an array");
    }
    Ok(Some(selected.clone()))
}
