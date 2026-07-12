//! Read-only recursive rendering of a `SchemaNode` tree (used for both the
//! source and target schema panes).

use egui::Ui;
use ir::{SchemaKind, SchemaNode};

pub fn show_schema_tree(ui: &mut Ui, schema: &SchemaNode) {
    show_node(ui, schema);
}

fn show_node(ui: &mut Ui, node: &SchemaNode) {
    let suffix = if node.repeating { " []" } else { "" };
    // XML attributes render with the conventional @ prefix.
    let prefix = if node.attribute { "@" } else { "" };
    match &node.kind {
        SchemaKind::Scalar { ty } => {
            ui.label(format!("{prefix}{}{suffix}: {ty:?}", node.name));
        }
        SchemaKind::Group { children, .. } => {
            egui::CollapsingHeader::new(format!("{}{suffix}", node.name))
                .default_open(true)
                .show(ui, |ui| {
                    for child in children {
                        show_node(ui, child);
                    }
                });
        }
    }
}
