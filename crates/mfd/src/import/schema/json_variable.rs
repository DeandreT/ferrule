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
        [] if schema.repeating => return Err("selected root is an array"),
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
            let mut expects_item = false;
            let mut item_view = false;
            let mut terminal_array = false;
            for (index, entry) in entries.iter().enumerate() {
                if entry.children().any(|node| node.has_tag_name("entry")) {
                    return Err("selection must be one flat declared path");
                }
                if expects_item {
                    if entry.attribute("type") != Some("json-item")
                        || entry.attribute("name") != Some("item")
                    {
                        return Err("array view must be followed by its canonical item role");
                    }
                    expects_item = false;
                    item_view = true;
                    continue;
                }
                match (entry.attribute("type"), entry.attribute("name")) {
                    (None, Some("root")) if index == 0 => {}
                    (None, Some("object"))
                        if (!selected.repeating || item_view)
                            && matches!(selected.kind, SchemaKind::Group { .. }) => {}
                    (None, Some("array"))
                        if selected.repeating
                            && !item_view
                            && !selected.json_array_wrapper
                            && selected.alternatives().is_empty()
                            && matches!(selected.kind, SchemaKind::Group { .. }) =>
                    {
                        terminal_array = index + 1 == entries.len();
                        expects_item = !terminal_array;
                    }
                    (Some("json-property"), Some(name)) => {
                        if selected.repeating && !item_view {
                            return Err("selection crosses an array");
                        }
                        if !selected.alternatives().is_empty() {
                            return Err("selection crosses alternative object views");
                        }
                        selected = selected
                            .child(name)
                            .ok_or("selected property is absent from its schema")?;
                        item_view = false;
                    }
                    _ => return Err("selection has an unsupported structural view"),
                }
            }
            if expects_item {
                return Err("array view must be followed by its canonical item role");
            }
            if selected.repeating && !terminal_array {
                return Err("selected root is an array");
            }
            selected
        }
        _ => return Err("selection contains multiple roots"),
    };
    if selected.repeating
        && (selected.json_array_wrapper
            || !selected.alternatives().is_empty()
            || !matches!(selected.kind, SchemaKind::Group { .. }))
    {
        return Err("selected root is an array");
    }
    Ok(Some(selected.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(path: &str, schema: &SchemaNode) -> Result<Option<SchemaNode>, &'static str> {
        let text = format!(
            "<data><parameter usageKind=\"variable\"><root>{path}</root></parameter></data>"
        );
        let document = roxmltree::Document::parse(&text).unwrap();
        select(document.root_element(), schema)
    }

    fn schema() -> SchemaNode {
        let value = SchemaNode::scalar("Value", ir::ScalarType::Int)
            .with_numeric_range(ir::NumericRange::Integer(
                ir::IntegerRange::new(Some(-5), Some(10)).unwrap(),
            ))
            .unwrap();
        SchemaNode::group(
            "Dictionary",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::group("Header", vec![value])])
                    .repeating()
                    .with_item_count_range(ir::ItemCountRange::new(2, Some(4)).unwrap())
                    .unwrap(),
            ],
        )
    }

    const PREFIX: &str = "<entry name=\"root\"/><entry name=\"object\"/><entry name=\"Rows\" type=\"json-property\"/>";
    const ITEM: &str =
        "<entry name=\"array\"/><entry name=\"item\" type=\"json-item\"/><entry name=\"object\"/>";
    const CHILD: &str = "<entry name=\"Header\" type=\"json-property\"/><entry name=\"object\"/>";

    #[test]
    fn array_descendant_selection_retains_the_complete_independent_schema() {
        let expected = SchemaNode::group(
            "Header",
            vec![
                SchemaNode::scalar("Value", ir::ScalarType::Int)
                    .with_numeric_range(ir::NumericRange::Integer(
                        ir::IntegerRange::new(Some(-5), Some(10)).unwrap(),
                    ))
                    .unwrap(),
            ],
        );
        assert_eq!(
            resolve(&format!("{PREFIX}{ITEM}{CHILD}"), &schema()).unwrap(),
            Some(expected)
        );
    }

    #[test]
    fn terminal_array_selection_retains_repetition_and_complete_facets() {
        let mut schema = schema();
        if let SchemaKind::Group { children, .. } = &mut schema.kind {
            children[0].json_unique_items = true;
        }
        let value = SchemaNode::scalar("Value", ir::ScalarType::Int)
            .with_numeric_range(ir::NumericRange::Integer(
                ir::IntegerRange::new(Some(-5), Some(10)).unwrap(),
            ))
            .unwrap();
        let mut expected =
            SchemaNode::group("Rows", vec![SchemaNode::group("Header", vec![value])])
                .repeating()
                .with_item_count_range(ir::ItemCountRange::new(2, Some(4)).unwrap())
                .unwrap();
        expected.json_unique_items = true;
        assert_eq!(
            resolve(&format!("{PREFIX}<entry name=\"array\"/>"), &schema).unwrap(),
            Some(expected)
        );
        assert!(resolve(&format!("{PREFIX}{ITEM}"), &schema).is_err());
        assert!(resolve(PREFIX, &schema).is_err());
    }

    #[test]
    fn array_descendant_selection_refuses_unpaired_items_and_repeating_roots() {
        let suffixes = [
            format!("<entry name=\"array\"/>{CHILD}"),
            format!("<entry name=\"array\"/><entry name=\"wrong\" type=\"json-item\"/>{CHILD}"),
            format!("<entry name=\"array\"/><entry name=\"item\"/>{CHILD}"),
            format!("<entry name=\"item\" type=\"json-item\"/>{CHILD}"),
            format!("{ITEM}<entry name=\"item\" type=\"json-item\"/>{CHILD}"),
            ITEM.into(),
            CHILD.into(),
        ];
        for suffix in suffixes {
            assert!(
                resolve(&format!("{PREFIX}{suffix}"), &schema()).is_err(),
                "{suffix}"
            );
        }
        let inner =
            SchemaNode::group("item", vec![SchemaNode::group("Header", vec![])]).repeating();
        let mut outer = SchemaNode::group("Rows", vec![inner]).repeating();
        outer.json_array_wrapper = true;
        let wrapped = SchemaNode::group("Dictionary", vec![outer]);
        assert!(resolve(&format!("{PREFIX}{ITEM}{CHILD}"), &wrapped).is_err());
        let row = SchemaNode::group(
            "Rows",
            vec![
                SchemaNode::group("Header", vec![]),
                SchemaNode::group("Other", vec![]),
            ],
        )
        .with_alternatives(vec![
            ir::GroupAlternative {
                name: "a".into(),
                members: vec!["Header".into()],
                required: vec![],
                constraints: vec![],
            },
            ir::GroupAlternative {
                name: "b".into(),
                members: vec!["Other".into()],
                required: vec![],
                constraints: vec![],
            },
        ])
        .unwrap()
        .repeating();
        let alternative = SchemaNode::group("Dictionary", vec![row]);
        assert!(resolve(&format!("{PREFIX}{ITEM}{CHILD}"), &alternative).is_err());
    }
}
