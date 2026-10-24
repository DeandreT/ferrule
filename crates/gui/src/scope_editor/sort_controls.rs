use super::{first_node_id, node_picker};
use egui::Ui;
use mapping::{Graph, Scope, SortFilterOrder, SortKey};

#[derive(Clone, Copy)]
enum KeyAction {
    Remove(usize),
    MoveUp(usize),
    MoveDown(usize),
}

pub(super) fn show(ui: &mut Ui, scope: &mut Scope, graph: &Graph) {
    let first_node = first_node_id(graph);
    crate::ui_kit::section(ui, "Sort", |ui| {
        let mut sorted = scope.has_sort();
        if ui
            .add_enabled(
                sorted || first_node.is_some(),
                egui::Checkbox::new(&mut sorted, "Sort items"),
            )
            .on_disabled_hover_text("Add a graph node before enabling sorting")
            .changed()
        {
            if sorted {
                scope.sort_by = first_node;
            } else {
                scope.sort_by = None;
                scope.sort_then_by.clear();
            }
        }
        if let Some(sort_by) = &mut scope.sort_by {
            ui.horizontal(|ui| {
                ui.label("Primary key");
                ui.checkbox(&mut scope.sort_descending, "Descending");
            });
            node_picker(ui, "sort_by_node", sort_by, graph);
        } else if !scope.sort_then_by.is_empty() {
            ui.label("Primary key is missing");
            if ui
                .add_enabled(first_node.is_some(), egui::Button::new("Set primary key"))
                .clicked()
            {
                scope.sort_by = first_node;
            }
        }
    });

    if scope.has_sort() {
        let count = scope.sort_then_by.len();
        let mut action = None;
        for (index, key) in scope.sort_then_by.iter_mut().enumerate() {
            ui.push_id(("sort_key", index), |ui| {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("Then key {}", index + 1));
                        if crate::ui_kit::icon_button(
                            ui,
                            lucide_icons::Icon::ArrowUp,
                            "Move sort key earlier",
                            (index == 0).then_some("Already the first secondary key"),
                        )
                        .clicked()
                        {
                            action = Some(KeyAction::MoveUp(index));
                        }
                        if crate::ui_kit::icon_button(
                            ui,
                            lucide_icons::Icon::ArrowDown,
                            "Move sort key later",
                            (index + 1 == count).then_some("Already the last key"),
                        )
                        .clicked()
                        {
                            action = Some(KeyAction::MoveDown(index));
                        }
                        if crate::ui_kit::icon_button(
                            ui,
                            lucide_icons::Icon::Trash2,
                            "Remove sort key",
                            None,
                        )
                        .clicked()
                        {
                            action = Some(KeyAction::Remove(index));
                        }
                    });
                    node_picker(ui, ("sort_then_by_node", index), &mut key.node, graph);
                    ui.checkbox(&mut key.descending, "Descending");
                });
            });
        }
        match action {
            Some(KeyAction::Remove(index)) => {
                scope.sort_then_by.remove(index);
            }
            Some(KeyAction::MoveUp(index)) => scope.sort_then_by.swap(index, index - 1),
            Some(KeyAction::MoveDown(index)) => scope.sort_then_by.swap(index, index + 1),
            None => {}
        }
        if ui
            .add_enabled(scope.sort_by.is_some(), egui::Button::new("Add sort key"))
            .on_hover_text("Break equal primary values with another graph expression")
            .clicked()
            && let Some(node) = scope.sort_by
        {
            scope.sort_then_by.push(SortKey {
                node,
                descending: false,
            });
        }
        if scope.filter.is_some() {
            let mut filter_first = scope.sort_filter_order == SortFilterOrder::FilterThenSort;
            if ui
                .checkbox(&mut filter_first, "Filter before sorting")
                .on_hover_text(
                    "Before sorting, the filter sees original source positions; after sorting, it sees sorted positions",
                )
                .changed()
            {
                scope.sort_filter_order = if filter_first {
                    SortFilterOrder::FilterThenSort
                } else {
                    SortFilterOrder::SortThenFilter
                };
            }
        }
    }
}
