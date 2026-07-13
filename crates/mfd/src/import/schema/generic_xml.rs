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
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        let raw_name = child.attribute("name").unwrap_or_default();
        let (name, legacy_attribute) = normalize_xml_entry_name(raw_name);
        if name == XML_ELEMENTS_FIELD {
            children.push(generic_entry_schema(&child));
        } else if child.attribute("type") == Some("xml-type") {
            if name == XML_TEXT_FIELD || name == "text()" {
                children.push(SchemaNode::scalar(XML_TEXT_FIELD, ir::ScalarType::String).text());
            } else {
                children.extend(entry_children(&child));
            }
        } else if matches!(name, XML_LOCAL_NAME_FIELD | XML_NODE_NAME_FIELD) {
            children.push(SchemaNode::scalar(name, ir::ScalarType::String));
        } else if legacy_attribute || child.attribute("type") == Some("attribute") {
            children.push(SchemaNode::scalar(name, ir::ScalarType::String).attribute());
        } else {
            children.push(entry_tree_schema(&child));
        }
    }
    children
}
