use ir::{
    SchemaKind, SchemaNode, XML_ELEMENTS_FIELD, XML_LOCAL_NAME_FIELD, XML_NODE_NAME_FIELD,
    XML_TEXT_FIELD,
};

use super::{entry_tree_schema, normalize_xml_entry_name};

pub(super) fn merge_entries(entry: &roxmltree::Node, schema: &mut SchemaNode) {
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        let (name, _) = normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
        if name == XML_ELEMENTS_FIELD {
            if let SchemaKind::Group { children, .. } = &mut schema.kind
                && !children
                    .iter()
                    .any(|child| child.name == XML_ELEMENTS_FIELD)
            {
                children.push(generic_entry_schema(&child));
            }
        } else if child.attribute("type") == Some("xml-type") {
            merge_entries(&child, schema);
        } else if let SchemaKind::Group { children, .. } = &mut schema.kind
            && let Some(schema_child) = children.iter_mut().find(|node| node.name == name)
        {
            merge_entries(&child, schema_child);
        }
    }
}

pub(super) fn generic_entry_schema(entry: &roxmltree::Node) -> SchemaNode {
    SchemaNode::group(XML_ELEMENTS_FIELD, entry_children(entry)).repeating()
}

fn entry_children(entry: &roxmltree::Node) -> Vec<SchemaNode> {
    let mut children = Vec::new();
    collect_entry_children(entry, &mut children);
    if !children.iter().any(|child| child.name == XML_TEXT_FIELD) {
        children.push(text_schema());
    }
    children
}

fn collect_entry_children(entry: &roxmltree::Node, children: &mut Vec<SchemaNode>) {
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        let raw_name = child.attribute("name").unwrap_or_default();
        let (name, legacy_attribute) = normalize_xml_entry_name(raw_name);
        if name == XML_ELEMENTS_FIELD {
            children.push(generic_entry_schema(&child));
        } else if name == XML_TEXT_FIELD || name == "text()" {
            if !children.iter().any(|child| child.name == XML_TEXT_FIELD) {
                children.push(text_schema());
            }
        } else if child.attribute("type") == Some("xml-type") {
            collect_entry_children(&child, children);
        } else if matches!(name, XML_LOCAL_NAME_FIELD | XML_NODE_NAME_FIELD) {
            children.push(SchemaNode::scalar(name, ir::ScalarType::String));
        } else if legacy_attribute || child.attribute("type") == Some("attribute") {
            children.push(SchemaNode::scalar(name, ir::ScalarType::String).attribute());
        } else {
            children.push(entry_tree_schema(&child));
        }
    }
}

fn text_schema() -> SchemaNode {
    SchemaNode::scalar(XML_TEXT_FIELD, ir::ScalarType::String).text()
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, SchemaKind};

    use super::*;

    fn parse_entry(xml: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(xml).unwrap()
    }

    fn group_children(schema: &SchemaNode) -> &[SchemaNode] {
        let SchemaKind::Group { children, .. } = &schema.kind else {
            panic!("expected a group schema");
        };
        children
    }

    fn assert_one_text_child(schema: &SchemaNode) {
        let text_children: Vec<_> = group_children(schema)
            .iter()
            .filter(|child| child.name == XML_TEXT_FIELD)
            .collect();
        assert_eq!(text_children.len(), 1);
        assert!(text_children[0].text);
        assert!(matches!(
            text_children[0].kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        ));
    }

    #[test]
    fn generic_elements_gain_implicit_text_after_declared_children() {
        let document = parse_entry(
            r#"<entry name="element()">
                <entry name="LocalName"/>
                <entry name="Record" type="xml-type">
                    <entry name="Label"/>
                </entry>
            </entry>"#,
        );

        let schema = generic_entry_schema(&document.root_element());

        assert!(schema.repeating);
        assert_eq!(
            group_children(&schema)
                .iter()
                .map(|child| child.name.as_str())
                .collect::<Vec<_>>(),
            [XML_LOCAL_NAME_FIELD, "Label", XML_TEXT_FIELD]
        );
        assert_one_text_child(&schema);
    }

    #[test]
    fn explicit_text_aliases_collapse_at_their_first_position() {
        let document = parse_entry(
            r##"<entry name="element()">
                <entry name="LocalName"/>
                <entry name="text()" type="xml-type"/>
                <entry name="Label"/>
                <entry name="#text"/>
                <entry name="Record" type="xml-type">
                    <entry name="text()" type="xml-type"/>
                </entry>
            </entry>"##,
        );

        let schema = generic_entry_schema(&document.root_element());

        assert_eq!(
            group_children(&schema)
                .iter()
                .map(|child| child.name.as_str())
                .collect::<Vec<_>>(),
            [XML_LOCAL_NAME_FIELD, XML_TEXT_FIELD, "Label"]
        );
        assert_one_text_child(&schema);
    }

    #[test]
    fn nested_generic_elements_each_gain_one_text_child() {
        let document = parse_entry(
            r##"<entry name="element()">
                <entry name="NodeName"/>
                <entry name="element()">
                    <entry name="#text" type="xml-type"/>
                    <entry name="LocalName"/>
                    <entry name="text()" type="xml-type"/>
                </entry>
            </entry>"##,
        );

        let schema = generic_entry_schema(&document.root_element());
        let nested = group_children(&schema)
            .iter()
            .find(|child| child.name == XML_ELEMENTS_FIELD)
            .unwrap();

        assert_one_text_child(&schema);
        assert_one_text_child(nested);
        assert!(nested.repeating);
    }
}
