//! Scope tree navigation and editing: pick which scope's bindings/filter/
//! source path you're looking at, then edit them with plain widgets (no
//! drag-and-drop onto the canvas yet -- see the module-level note in
//! `app.rs` for why that's out of scope for this first GUI pass).

use egui::Ui;
use mapping::{Binding, Graph, NodeId, Scope, ScopeIteration};

use crate::path_picker::SourcePathCatalog;

/// Path of child-indices from the project root to the scope being edited.
pub type ScopePath = Vec<usize>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum GroupingMode {
    None,
    ByKey,
    StartingWith,
    IntoBlocks,
}

impl GroupingMode {
    fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ByKey => "key",
            Self::StartingWith => "starting predicate",
            Self::IntoBlocks => "fixed blocks",
        }
    }
}

fn first_node_id(graph: &Graph) -> Option<NodeId> {
    graph.nodes.keys().next().copied()
}

fn generated_sequence_label(sequence: &mapping::SequenceExpr) -> &'static str {
    match sequence {
        mapping::SequenceExpr::Tokenize { .. } => "tokenize",
        mapping::SequenceExpr::TokenizeByLength { .. } => "tokenize-by-length",
        mapping::SequenceExpr::Generate { .. } => "generate-sequence",
    }
}

fn join_summary(id: mapping::JoinId, plan: &mapping::JoinPlan) -> String {
    format!(
        "inner join #{} ({} sources, {} conditions)",
        id.get(),
        plan.sources().count(),
        plan.stages()
            .map(|(_, conditions)| conditions.iter().count())
            .sum::<usize>()
    )
}

fn join_condition_label(source: &mapping::JoinSource, key: &mapping::JoinKey) -> String {
    let mut left = key.left_collection().to_vec();
    left.extend(key.left_path().iter().cloned());
    let mut right = source.collection().to_vec();
    right.extend(key.right_path().iter().cloned());
    format!("{} = {}", left.join("/"), right.join("/"))
}

fn scope_iterates(scope: &Scope) -> bool {
    scope.iterates()
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

/// Edits `scope`'s sequence controls and bindings.
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

    match &scope.iteration {
        ScopeIteration::Sequence(sequence) => {
            ui.horizontal(|ui| {
                ui.label("iteration:");
                ui.label(format!(
                    "generated ({})",
                    generated_sequence_label(sequence)
                ));
            });
        }
        ScopeIteration::InnerJoin { id, plan } => {
            ui.horizontal(|ui| {
                ui.label("iteration:");
                ui.label(join_summary(*id, plan));
            });
            for (index, source) in plan.sources().enumerate() {
                let collection = if source.collection().is_empty() {
                    "<rows>".to_string()
                } else {
                    source.collection().join("/")
                };
                ui.horizontal(|ui| {
                    ui.label(format!("  input {}:", index + 1));
                    ui.monospace(collection);
                });
            }
            for (source, conditions) in plan.stages() {
                for condition in conditions.iter() {
                    ui.horizontal(|ui| {
                        ui.label("  on:");
                        ui.monospace(join_condition_label(source, condition));
                    });
                }
            }
        }
        ScopeIteration::None | ScopeIteration::Source(_) => {
            ui.horizontal(|ui| {
                ui.label("source path:");
                let mut has_source = scope.source().is_some();
                if ui.checkbox(&mut has_source, "iterates").changed() {
                    scope.set_source(has_source.then(Vec::new));
                }
            });
            if let Some(source) = scope.source_mut() {
                ui.horizontal(|ui| {
                    ui.label("  path:");
                    source_paths.show_scope_picker(ui, "scope_source_path", source, nested);
                });
            }
        }
    }

    if scope_iterates(scope) {
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

        if scope.join().is_none() {
            ui.horizontal(|ui| {
                ui.label("  grouping:");
                let mut mode = if scope.group_by.is_some() {
                    GroupingMode::ByKey
                } else if scope.group_starting_with.is_some() {
                    GroupingMode::StartingWith
                } else if scope.group_into_blocks.is_some() {
                    GroupingMode::IntoBlocks
                } else {
                    GroupingMode::None
                };
                let previous = mode;
                ui.add_enabled_ui(mode != GroupingMode::None || first_node.is_some(), |ui| {
                    egui::ComboBox::from_id_salt("scope_grouping_mode")
                        .selected_text(mode.label())
                        .show_ui(ui, |ui| {
                            for choice in [
                                GroupingMode::None,
                                GroupingMode::ByKey,
                                GroupingMode::StartingWith,
                                GroupingMode::IntoBlocks,
                            ] {
                                ui.selectable_value(&mut mode, choice, choice.label());
                            }
                        });
                })
                .response
                .on_disabled_hover_text("Add a graph node before enabling grouping");
                if mode != previous {
                    scope.group_by = if mode == GroupingMode::ByKey {
                        first_node
                    } else {
                        None
                    };
                    scope.group_starting_with = if mode == GroupingMode::StartingWith {
                        first_node
                    } else {
                        None
                    };
                    scope.group_into_blocks = if mode == GroupingMode::IntoBlocks {
                        first_node
                    } else {
                        None
                    };
                }
                match mode {
                    GroupingMode::None => {}
                    GroupingMode::ByKey => {
                        if let Some(group_by) = &mut scope.group_by {
                            node_picker(ui, "group_by_node", group_by, graph);
                        }
                    }
                    GroupingMode::StartingWith => {
                        if let Some(predicate) = &mut scope.group_starting_with {
                            node_picker(ui, "group_starting_node", predicate, graph);
                        }
                    }
                    GroupingMode::IntoBlocks => {
                        if let Some(block_size) = &mut scope.group_into_blocks {
                            node_picker(ui, "group_block_size_node", block_size, graph);
                        }
                    }
                }
            });
        }

        ui.horizontal(|ui| {
            ui.label("  sort key:");
            let mut has_sort = scope.sort_by.is_some();
            if ui
                .add_enabled(
                    scope.sort_by.is_some() || first_node.is_some(),
                    egui::Checkbox::new(&mut has_sort, "sorted"),
                )
                .on_disabled_hover_text("Add a graph node before enabling sorting")
                .changed()
            {
                scope.sort_by = if has_sort { first_node } else { None };
            }
            if let Some(sort_by) = &mut scope.sort_by {
                node_picker(ui, "sort_by_node", sort_by, graph);
                ui.checkbox(&mut scope.sort_descending, "descending");
            }
        });

        ui.horizontal(|ui| {
            ui.label("  item limit:");
            let mut has_take = scope.take.is_some();
            if ui
                .add_enabled(
                    scope.take.is_some() || first_node.is_some(),
                    egui::Checkbox::new(&mut has_take, "limited"),
                )
                .on_disabled_hover_text("Add a graph node before enabling an item limit")
                .changed()
            {
                scope.take = if has_take { first_node } else { None };
            }
            if let Some(take) = &mut scope.take {
                node_picker(ui, "take_node", take, graph);
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
        mapping::Node::Position { .. } => "position",
        mapping::Node::JoinField { .. } => "join_field",
        mapping::Node::JoinPosition { .. } => "join_position",
        mapping::Node::Const { .. } => "const",
        mapping::Node::RuntimeValue { .. } => "runtime_value",
        mapping::Node::Call { .. } => "call",
        mapping::Node::If { .. } => "if",
        mapping::Node::ValueMap { .. } => "value_map",
        mapping::Node::Lookup { .. } => "lookup",
        mapping::Node::SequenceExists { .. } => "sequence_exists",
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

    #[test]
    fn generated_sequences_are_named_and_count_as_iteration() {
        let mut scope = Scope {
            iteration: ScopeIteration::Sequence(mapping::SequenceExpr::Tokenize {
                input: 1,
                delimiter: 2,
                item: 3,
            }),
            ..Default::default()
        };
        assert!(scope_iterates(&scope));
        assert_eq!(
            generated_sequence_label(scope.sequence().expect("sequence exists")),
            "tokenize"
        );

        scope.set_sequence(Some(mapping::SequenceExpr::TokenizeByLength {
            input: 1,
            length: 2,
            item: 3,
        }));
        assert_eq!(
            generated_sequence_label(scope.sequence().expect("sequence exists")),
            "tokenize-by-length"
        );

        scope.set_sequence(Some(mapping::SequenceExpr::Generate {
            from: Some(1),
            to: 2,
            item: 3,
        }));
        assert_eq!(
            generated_sequence_label(scope.sequence().expect("sequence exists")),
            "generate-sequence"
        );

        scope.set_sequence(None);
        assert!(!scope_iterates(&scope));
        scope.set_source(Some(Vec::new()));
        assert!(scope_iterates(&scope));
    }

    #[test]
    fn joins_are_named_and_count_as_iteration() {
        let orders = mapping::JoinSource::new(vec!["orders".into()]);
        let products = mapping::JoinSource::new(vec!["products".into()]);
        let condition = mapping::JoinConditions::new(mapping::JoinKey::new(
            vec!["orders".into()],
            vec!["sku".into()],
            vec!["sku".into()],
        ));
        let plan = mapping::JoinPlan::new(orders, products, condition).unwrap();
        let scope = Scope {
            iteration: ScopeIteration::InnerJoin {
                id: mapping::JoinId::new(12),
                plan,
            },
            ..Scope::default()
        };

        assert!(scope_iterates(&scope));
        let Some((id, plan)) = scope.join() else {
            panic!("expected join iteration");
        };
        assert_eq!(
            join_summary(id, plan),
            "inner join #12 (2 sources, 1 conditions)"
        );
        let (source, conditions) = plan.stages().next().expect("join has one stage");
        assert_eq!(
            join_condition_label(source, conditions.iter().next().expect("stage has a key")),
            "orders/sku = products/sku"
        );
    }
}
