//! Read-only recursive rendering of a `SchemaNode` tree (used for both the
//! source and target schema panes).

use egui::Ui;
use ir::{SchemaKind, SchemaNode};

pub fn show_schema_tree(ui: &mut Ui, schema: &SchemaNode) {
    show_node(ui, schema, 0);
}

fn show_node(ui: &mut Ui, node: &SchemaNode, depth: usize) {
    let suffix = if node.repeating { " []" } else { "" };
    // XML attributes render with the conventional @ prefix.
    let prefix = if node.attribute { "@" } else { "" };
    match &node.kind {
        SchemaKind::Scalar { ty } => {
            ui.label(format!("{prefix}{}{suffix}: {ty:?}", node.name));
        }
        SchemaKind::Group { children, .. } => {
            let leaves = scalar_leaf_count(node);
            let response = egui::CollapsingHeader::new(format!("{}{suffix}", node.name))
                .default_open(depth == 0 || leaves <= 12)
                .show(ui, |ui| {
                    for child in children {
                        show_node(ui, child, depth + 1);
                    }
                });
            response
                .header_response
                .on_hover_text(format!("{leaves} scalar field(s)"));
        }
    }
}

fn scalar_leaf_count(node: &SchemaNode) -> usize {
    match &node.kind {
        SchemaKind::Scalar { .. } => 1,
        SchemaKind::Group { children, .. } => children.iter().map(scalar_leaf_count).sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

    #[test]
    fn counts_nested_scalar_fields_for_expansion_policy() {
        let schema = SchemaNode::group(
            "root",
            vec![
                SchemaNode::scalar("id", ScalarType::Int),
                SchemaNode::group(
                    "details",
                    vec![
                        SchemaNode::scalar("name", ScalarType::String),
                        SchemaNode::scalar("active", ScalarType::Bool),
                    ],
                ),
            ],
        );
        assert_eq!(scalar_leaf_count(&schema), 3);
    }
}
