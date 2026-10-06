use super::*;
use mapping::{FailureIteration, FailureRule, FailureSelection};

#[derive(Clone, Copy)]
enum RuleAction {
    Add,
    Remove(usize),
    Move { from: usize, to: usize },
}

impl FerruleApp {
    pub(super) fn show_failure_rules(&mut self, ui: &mut egui::Ui, editing_enabled: bool) {
        let editing_enabled = editing_enabled && self.project_editing_enabled();
        if self
            .selected_failure_rule
            .is_some_and(|index| index >= self.project.failure_rules.len())
        {
            self.selected_failure_rule = None;
            self.pending_failure_rule_scroll = false;
        }
        let mut selection = None;
        let mut action = None;
        egui::CollapsingHeader::new(format!("Failure rules ({})", self.project.failure_rules.len()))
            .id_salt("project_failure_rules")
            .default_open(true)
            .open(self.pending_failure_rule_scroll.then_some(true))
            .show(ui, |ui| {
                ui.weak("Stop before producing any target when a source item matches a rule. Rules run from top to bottom.");
                if ui.add_enabled(editing_enabled, egui::Button::new("Add source rule")).clicked() {
                    action = Some(RuleAction::Add);
                }
                if self.project.failure_rules.is_empty() {
                    ui.weak("No failure rules.");
                }
                egui::ScrollArea::vertical()
                    .id_salt("failure_rules_scroll")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for (index, rule) in self.project.failure_rules.iter().enumerate() {
                            let source = match &rule.iteration {
                                FailureIteration::Source { collection } if collection.is_empty() => {
                                    "current source".to_string()
                                }
                                FailureIteration::Source { collection } => collection.join("/"),
                                FailureIteration::Sequence { .. } => "generated sequence".to_string(),
                            };
                            let response = ui.selectable_label(
                                self.selected_failure_rule == Some(index),
                                format!("Rule {}: {source}", index + 1),
                            );
                            if response.clicked() {
                                selection = Some(index);
                            }
                            if self.pending_failure_rule_scroll && self.selected_failure_rule == Some(index) {
                                response.scroll_to_me(None);
                            }
                        }
                    });
                if let Some(index) = self.selected_failure_rule {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(editing_enabled && index > 0, egui::Button::new("Move up")).clicked() {
                            action = Some(RuleAction::Move { from: index, to: index - 1 });
                        }
                        if ui.add_enabled(editing_enabled && index + 1 < self.project.failure_rules.len(), egui::Button::new("Move down")).clicked() {
                            action = Some(RuleAction::Move { from: index, to: index + 1 });
                        }
                        let source_rule = matches!(&self.project.failure_rules[index].iteration, FailureIteration::Source { .. });
                        if ui.add_enabled(editing_enabled && source_rule, egui::Button::new("Remove rule")).on_disabled_hover_text("Generated-sequence rules keep their owned item nodes in this editor.").clicked() {
                            action = Some(RuleAction::Remove(index));
                        }
                    });
                    self.show_selected_failure_rule(ui, index, editing_enabled);
                }
            });
        if let Some(index) = selection {
            self.selected_failure_rule = Some(index);
        }
        self.pending_failure_rule_scroll = false;
        if let Some(action) = action {
            self.apply_failure_rule_action(action, editing_enabled);
        }
    }

    fn show_selected_failure_rule(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        editing_enabled: bool,
    ) {
        let static_sources = self
            .project
            .extra_sources
            .iter()
            .filter(|source| source.dynamic_path.is_none())
            .cloned()
            .collect::<Vec<_>>();
        let paths = SourcePathCatalog::new(&self.project.source, &static_sources);
        let first_node = self
            .project
            .graph
            .nodes
            .iter()
            .find(|(_, node)| !matches!(node, mapping::Node::Unconnected))
            .map(|(&id, _)| id);
        let graph = &self.project.graph;
        let rule = &mut self.project.failure_rules[index];
        let before = rule.clone();
        match &mut rule.iteration {
            FailureIteration::Source { collection } => {
                ui.add_enabled_ui(editing_enabled, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Source collection:");
                        paths.show_scope_picker(ui, ("failure_source", index), collection, false);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Stop when:");
                        let mut mode = selection_mode(rule.selection);
                        egui::ComboBox::from_id_salt(("failure_selection", index))
                            .selected_text(selection_label(rule.selection))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut mode, 0, "All items");
                                ui.add_enabled_ui(
                                    first_node.is_some() || rule.selection.predicate().is_some(),
                                    |ui| {
                                        ui.selectable_value(&mut mode, 1, "Expression is true");
                                        ui.selectable_value(&mut mode, 2, "Expression is false");
                                    },
                                );
                            });
                        if mode != selection_mode(rule.selection) {
                            let predicate = rule.selection.predicate().or(first_node);
                            rule.selection = match (mode, predicate) {
                                (1, Some(predicate)) => FailureSelection::WhenTrue { predicate },
                                (2, Some(predicate)) => FailureSelection::WhenFalse { predicate },
                                _ => FailureSelection::All,
                            };
                        }
                    });
                    match &mut rule.selection {
                        FailureSelection::All => {
                            ui.weak("Every item in the chosen collection stops the mapping.");
                        }
                        FailureSelection::WhenTrue { predicate }
                        | FailureSelection::WhenFalse { predicate } => {
                            ui.horizontal(|ui| {
                                ui.label("Selection expression:");
                                crate::scope_editor::node_picker(
                                    ui,
                                    ("failure_predicate", index),
                                    predicate,
                                    graph,
                                );
                            });
                        }
                    }
                    let mut has_message = rule.message.is_some();
                    if ui
                        .add_enabled(
                            first_node.is_some() || has_message,
                            egui::Checkbox::new(&mut has_message, "Custom message expression"),
                        )
                        .changed()
                    {
                        rule.message = if has_message { first_node } else { None };
                    }
                    if let Some(message) = &mut rule.message {
                        ui.horizontal(|ui| {
                            ui.label("Message:");
                            crate::scope_editor::node_picker(
                                ui,
                                ("failure_message", index),
                                message,
                                graph,
                            );
                        });
                    } else {
                        ui.weak("Use the default failure message.");
                    }
                });
                ui.weak("Expressions read the selected source item. Use Validate to check their type and context. The message is evaluated only for the first matching item.");
            }
            FailureIteration::Sequence { .. } => {
                ui.weak("Generated-sequence rule: its expressions and owned item are retained. You can change its order, but source-rule editing does not replace or remove it.");
                ui.weak(selection_label(rule.selection));
                if let Some(predicate) = rule.selection.predicate() {
                    ui.monospace(format!("Selection expression: node {predicate}"));
                }
                if let Some(message) = rule.message {
                    ui.monospace(format!("Message expression: node {message}"));
                } else {
                    ui.weak("Use the default failure message.");
                }
            }
        }
        if *rule != before {
            self.clear_diagnostic_navigation();
            self.selected_failure_rule = Some(index);
        }
    }

    fn apply_failure_rule_action(&mut self, action: RuleAction, editing_enabled: bool) {
        if !editing_enabled || !self.project_editing_enabled() {
            return;
        }
        let selected =
            match action {
                RuleAction::Add => {
                    self.project.failure_rules.push(FailureRule {
                        iteration: FailureIteration::Source {
                            collection: Vec::new(),
                        },
                        selection: FailureSelection::All,
                        message: None,
                    });
                    Some(self.project.failure_rules.len() - 1)
                }
                RuleAction::Remove(index) => {
                    if !self.project.failure_rules.get(index).is_some_and(|rule| {
                        matches!(&rule.iteration, FailureIteration::Source { .. })
                    }) {
                        return;
                    }
                    self.project.failure_rules.remove(index);
                    (!self.project.failure_rules.is_empty())
                        .then(|| index.min(self.project.failure_rules.len() - 1))
                }
                RuleAction::Move { from, to } => {
                    if from >= self.project.failure_rules.len()
                        || to >= self.project.failure_rules.len()
                        || from.abs_diff(to) != 1
                    {
                        return;
                    }
                    self.project.failure_rules.swap(from, to);
                    Some(to)
                }
            };
        self.clear_diagnostic_navigation();
        self.selected_failure_rule = selected;
        self.pending_failure_rule_scroll = selected.is_some();
    }
}

fn selection_mode(selection: FailureSelection) -> u8 {
    match selection {
        FailureSelection::All => 0,
        FailureSelection::WhenTrue { .. } => 1,
        FailureSelection::WhenFalse { .. } => 2,
    }
}

fn selection_label(selection: FailureSelection) -> &'static str {
    match selection {
        FailureSelection::All => "All items",
        FailureSelection::WhenTrue { .. } => "Expression is true",
        FailureSelection::WhenFalse { .. } => "Expression is false",
    }
}

#[cfg(test)]
#[path = "failure_rules_tests.rs"]
mod tests;
