//! Scope tree navigation and editing: pick which scope's bindings/filter/
//! source path you're looking at, then edit them with plain widgets (no
//! drag-and-drop onto the canvas yet -- see the module-level note in
//! `app.rs` for why that's out of scope for this first GUI pass).

use egui::Ui;
use mapping::{Binding, Graph, NodeId, Scope};

use crate::path_picker::SourcePathCatalog;

/// Path of child-indices from the project root to the scope being edited.
pub type ScopePath = Vec<usize>;

fn first_node_id(graph: &Graph) -> Option<NodeId> {
    graph.nodes.keys().next().copied()
}

pub fn scope_at_mut<'a>(root: &'a mut Scope, path: &[usize]) -> &'a mut Scope {
    let mut scope = root;
    for &i in path {
        scope = &mut scope.children[i];
    }
    scope
}

/// Renders the scope tree as clickable labels; returns the newly selected
/// path, if the user clicked one.
pub fn show_scope_tree(ui: &mut Ui, root: &Scope, selected: &ScopePath) -> Option<ScopePath> {
    let mut new_selection = None;
    show_scope_node(ui, root, "root", &mut vec![], selected, &mut new_selection);
    new_selection
}

fn show_scope_node(
    ui: &mut Ui,
    scope: &Scope,
    label: &str,
    path: &mut ScopePath,
    selected: &ScopePath,
    new_selection: &mut Option<ScopePath>,
) {
    let is_selected = path == selected;
    egui::CollapsingHeader::new(label)
        .id_salt(format!("{path:?}"))
        .default_open(true)
        .show(ui, |ui| {
            if ui
                .selectable_label(is_selected, "(edit this scope)")
                .clicked()
            {
                *new_selection = Some(path.clone());
            }
            for (i, child) in scope.children.iter().enumerate() {
                path.push(i);
                let child_label = if child.target_field.is_empty() {
                    format!("child {i}")
                } else {
                    child.target_field.clone()
                };
                show_scope_node(ui, child, &child_label, path, selected, new_selection);
                path.pop();
            }
        });
}

/// Edits `scope`'s `source` path, `filter` node, and `bindings`.
pub fn show_scope_editor(
    ui: &mut Ui,
    scope: &mut Scope,
    graph: &Graph,
    source_paths: &SourcePathCatalog,
    nested: bool,
) {
    let first_node = first_node_id(graph);
    ui.strong(if scope.target_field.is_empty() {
        "root scope".to_string()
    } else {
        format!("scope: {}", scope.target_field)
    });

    ui.horizontal(|ui| {
        ui.label("source path:");
        let mut has_source = scope.source.is_some();
        if ui.checkbox(&mut has_source, "iterates").changed() {
            scope.source = has_source.then(Vec::new);
        }
    });
    if let Some(source) = &mut scope.source {
        ui.horizontal(|ui| {
            ui.label("  path:");
            source_paths.show_scope_picker(ui, "scope_source_path", source, nested);
        });

        ui.horizontal(|ui| {
            ui.label("  filter node:");
            let mut has_filter = scope.filter.is_some();
            if ui
                .add_enabled(
                    scope.filter.is_some() || first_node.is_some(),
                    egui::Checkbox::new(&mut has_filter, "filtered"),
                )
                .on_disabled_hover_text("Add a graph node before enabling a filter")
                .changed()
            {
                scope.filter = if has_filter { first_node } else { None };
            }
            if let Some(filter) = &mut scope.filter {
                node_picker(ui, "filter_node", filter, graph);
            }
        });

        ui.horizontal(|ui| {
            ui.label("  group-by key:");
            let mut has_group = scope.group_by.is_some();
            if ui
                .add_enabled(
                    scope.group_by.is_some() || first_node.is_some(),
                    egui::Checkbox::new(&mut has_group, "grouped"),
                )
                .on_disabled_hover_text("Add a graph node before enabling grouping")
                .changed()
            {
                scope.group_by = if has_group { first_node } else { None };
            }
            if let Some(group_by) = &mut scope.group_by {
                node_picker(ui, "group_by_node", group_by, graph);
            }
        });
    }

    ui.separator();
    ui.label("bindings (target field -> graph node):");
    let mut remove_idx = None;
    for (i, binding) in scope.bindings.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut binding.target_field);
            ui.label("->");
            node_picker(ui, format!("binding_{i}"), &mut binding.node, graph);
            if ui.small_button("x").clicked() {
                remove_idx = Some(i);
            }
        });
    }
    if let Some(i) = remove_idx {
        scope.bindings.remove(i);
    }
    if ui
        .add_enabled(first_node.is_some(), egui::Button::new("+ binding").small())
        .on_disabled_hover_text("Add a graph node before creating a binding")
        .clicked()
    {
        scope.bindings.push(Binding {
            target_field: String::new(),
            node: first_node.expect("button is disabled without a graph node"),
        });
    }
}

fn node_picker(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    node_id: &mut NodeId,
    graph: &Graph,
) {
    let current_label = graph.nodes.get(node_id).map_or_else(
        || "<missing>".to_string(),
        |n| format!("{node_id}: {}", node_kind_label(n)),
    );
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(current_label)
        .show_ui(ui, |ui| {
            for (&id, node) in &graph.nodes {
                let label = format!("{id}: {}", node_kind_label(node));
                ui.selectable_value(node_id, id, label);
            }
        });
}

fn node_kind_label(node: &mapping::Node) -> &'static str {
    match node {
        mapping::Node::SourceField { .. } => "source_field",
        mapping::Node::Const { .. } => "const",
        mapping::Node::Call { .. } => "call",
        mapping::Node::If { .. } => "if",
        mapping::Node::ValueMap { .. } => "value_map",
        mapping::Node::Lookup { .. } => "lookup",
        mapping::Node::Aggregate { .. } => "aggregate",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::Value;

    #[test]
    fn scope_references_default_to_an_existing_node_only() {
        let mut graph = Graph::default();
        assert_eq!(first_node_id(&graph), None);
        graph
            .nodes
            .insert(7, mapping::Node::Const { value: Value::Null });
        assert_eq!(first_node_id(&graph), Some(7));
    }
}
