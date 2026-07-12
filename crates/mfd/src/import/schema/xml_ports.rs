use std::collections::BTreeMap;

use ir::{SchemaKind, SchemaNode};

use super::schema_node_at;

/// MapForce puts a simple-content value on its parent element's port. Ferrule
/// stores that value under `#text`; mixed-content parent ports stay structural.
pub(super) fn normalize_xml_text_ports(
    schema: &SchemaNode,
    ports: &mut BTreeMap<u32, Vec<String>>,
) {
    for path in ports.values_mut() {
        let node = schema_node_at(schema, path);
        if let Some(text) = node
            .and_then(SchemaNode::text_child)
            .filter(|_| node.is_some_and(has_only_text_and_attributes))
        {
            path.push(text.name.clone());
        }
    }
}

fn has_only_text_and_attributes(node: &SchemaNode) -> bool {
    matches!(&node.kind, SchemaKind::Group { children, .. } if children
        .iter()
        .all(|child| child.attribute || child.text))
}
