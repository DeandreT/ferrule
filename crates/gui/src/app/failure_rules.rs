use super::*;
use mapping::{FailureIteration, FailureRule, FailureSelection, Node, SequenceExpr};

#[derive(Clone, Copy)]
enum RuleAction {
    Add,
    AddSequence(GeneratedRuleKind),
    EnableRegexFlags(usize),
    Remove(usize),
    Move { from: usize, to: usize },
}

#[derive(Clone, Copy)]
enum GeneratedRuleKind {
    IntegerRange,
    SplitText,
    SplitTextByLength,
    SplitTextByRegex,
}

fn editable_generated_rule(project: &Project, rule: &FailureRule) -> bool {
    let FailureIteration::Sequence { sequence } = &rule.iteration else {
        return false;
    };
    if !matches!(
        sequence,
        SequenceExpr::Generate { .. }
            | SequenceExpr::Tokenize { .. }
            | SequenceExpr::TokenizeByLength { .. }
            | SequenceExpr::TokenizeRegex { .. }
    ) {
        return false;
    }
    let item = sequence.item();
    matches!(
        project.graph.nodes.get(&item),
        Some(Node::SourceField { path, frame: None }) if path.is_empty()
    ) && crate::graph_viewer::failure_rule_is_only_item_owner(project, rule, item)
        && crate::graph_viewer::sequence_arguments_have_no_private_items(project, sequence)
        && sequence
            .inputs()
            .into_iter()
            .chain(rule.selection.predicate())
            .chain(rule.message)
            .all(|id| {
                project
                    .graph
                    .nodes
                    .get(&id)
                    .is_some_and(|node| !matches!(node, Node::Unconnected))
            })
}

impl FerruleApp {
    pub(super) fn show_failure_rules(&mut self, ui: &mut egui::Ui, editing_enabled: bool) {
        let editing_enabled = editing_enabled && self.ui_project_editing_enabled();
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
                ui.weak("Stop before producing any target when an item matches a rule. Rules run from top to bottom.");
                if ui.add_enabled(editing_enabled, egui::Button::new("Add source rule")).clicked() {
                    action = Some(RuleAction::Add);
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(editing_enabled, egui::Button::new("Add integer range rule")).clicked() {
                        action = Some(RuleAction::AddSequence(GeneratedRuleKind::IntegerRange));
                    }
                    if ui.add_enabled(editing_enabled, egui::Button::new("Add split text rule")).clicked() {
                        action = Some(RuleAction::AddSequence(GeneratedRuleKind::SplitText));
                    }
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(editing_enabled, egui::Button::new("Add fixed-length text rule")).clicked() {
                        action = Some(RuleAction::AddSequence(GeneratedRuleKind::SplitTextByLength));
                    }
                    if ui.add_enabled(editing_enabled, egui::Button::new("Add regex text rule")).clicked() {
                        action = Some(RuleAction::AddSequence(GeneratedRuleKind::SplitTextByRegex));
                    }
                });
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
                        let rule = &self.project.failure_rules[index];
                        let removable = matches!(&rule.iteration, FailureIteration::Source { .. })
                            || editable_generated_rule(&self.project, rule);
                        if ui.add_enabled(editing_enabled && removable, egui::Button::new("Remove rule")).on_disabled_hover_text("This imported generated rule is retained. Its sequence kind or item ownership needs repair before editing.").clicked() {
                            action = Some(RuleAction::Remove(index));
                        }
                    });
                    if let Some(flags_action) = self.show_selected_failure_rule(ui, index, editing_enabled) {
                        action = Some(flags_action);
                    }
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
    ) -> Option<RuleAction> {
        let mut flags_action = None;
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
        let generated_editable =
            editable_generated_rule(&self.project, &self.project.failure_rules[index]);
        let graph = &self.project.graph;
        let rule = &mut self.project.failure_rules[index];
        let before = rule.clone();
        let FailureRule {
            iteration,
            selection,
            message,
        } = &mut *rule;
        match iteration {
            FailureIteration::Source { collection } => {
                ui.add_enabled_ui(editing_enabled, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Source collection:");
                        paths.show_scope_picker(ui, ("failure_source", index), collection, false);
                    });
                    show_rule_expressions(ui, index, selection, message, graph, first_node);
                });
                ui.weak("Expressions read the selected source item. Use Validate to check their type and context. The message is evaluated only for the first matching item.");
            }
            FailureIteration::Sequence { sequence } if generated_editable => {
                let item = sequence.item();
                ui.add_enabled_ui(editing_enabled, |ui| {
                    match sequence {
                        SequenceExpr::Generate { from, to, .. } => {
                            ui.label("Integer range (inclusive)");
                            let mut custom_start = from.is_some();
                            if ui.checkbox(&mut custom_start, "Custom start expression").changed()
                            {
                                *from = custom_start.then_some(*to);
                            }
                            if let Some(from) = from {
                                ui.horizontal(|ui| {
                                    ui.label("From:");
                                    crate::scope_editor::node_picker(ui, ("failure_from", index), from, graph);
                                });
                            } else {
                                ui.weak("Start at 1.");
                            }
                            ui.horizontal(|ui| {
                                ui.label("To:");
                                crate::scope_editor::node_picker(ui, ("failure_to", index), to, graph);
                            });
                        }
                        SequenceExpr::Tokenize { input, delimiter, .. } => {
                            ui.label("Split text");
                            ui.horizontal(|ui| {
                                ui.label("Text:");
                                crate::scope_editor::node_picker(ui, ("failure_text", index), input, graph);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Delimiter:");
                                crate::scope_editor::node_picker(ui, ("failure_delimiter", index), delimiter, graph);
                            });
                        }
                        SequenceExpr::TokenizeByLength { input, length, .. } => {
                            ui.label("Split text by character count");
                            ui.horizontal(|ui| {
                                ui.label("Text:");
                                crate::scope_editor::node_picker(ui, ("failure_fixed_text", index), input, graph);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Characters per item:");
                                crate::scope_editor::node_picker(ui, ("failure_fixed_length", index), length, graph);
                            });
                            ui.weak("Counts Unicode characters. Combining marks count separately; the last item may be shorter.");
                        }
                        SequenceExpr::TokenizeRegex { input, pattern, flags, .. } => {
                            ui.label("Split text using a regular expression");
                            ui.horizontal(|ui| {
                                ui.label("Text:");
                                crate::scope_editor::node_picker(ui, ("failure_regex_text", index), input, graph);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Pattern:");
                                crate::scope_editor::node_picker(ui, ("failure_regex_pattern", index), pattern, graph);
                            });
                            let mut custom_flags = flags.is_some();
                            if ui.checkbox(&mut custom_flags, "Custom flags expression").changed() {
                                if custom_flags {
                                    flags_action = Some(RuleAction::EnableRegexFlags(index));
                                } else {
                                    *flags = None;
                                }
                            }
                            if let Some(flags) = flags {
                                ui.horizontal(|ui| {
                                    ui.label("Flags:");
                                    crate::scope_editor::node_picker(ui, ("failure_regex_flags", index), flags, graph);
                                });
                            }
                            ui.weak("Flags: i ignores case, m enables line anchors, s includes line breaks, x ignores pattern spacing.");
                            ui.weak("Patterns must consume at least one character.");
                        }
                        _ => unreachable!("editable generated rule kind"),
                    }
                    ui.label(format!("Generated item: node {item}"))
                        .on_hover_text("Use this generated item node on the canvas in the rule's selection or message expression. Its identity and item ownership stay fixed.");
                    show_rule_expressions(ui, index, selection, message, graph, first_node);
                });
                ui.weak("Arguments read the main source. Selection and message expressions read each generated item. The message is evaluated only for the first matching item. Use Validate to check their type and context.");
            }
            FailureIteration::Sequence { .. } => {
                ui.weak("Imported generated rule: its sequence, expressions and item ownership are retained. You can change its order. Other sequence kinds and invalid or shared items are read-only here.");
                ui.weak(selection_label(*selection));
                if let Some(predicate) = selection.predicate() {
                    ui.monospace(format!("Selection expression: node {predicate}"));
                }
                if let Some(message) = *message {
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
        flags_action
    }

    fn apply_failure_rule_action(&mut self, action: RuleAction, editing_enabled: bool) {
        if !editing_enabled || !self.ui_project_editing_enabled() {
            return;
        }
        let selected = match action {
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
            RuleAction::AddSequence(kind) => {
                let ids = match crate::graph_viewer::reserve_project_node_ids(&self.project, 3) {
                    Ok(ids) => ids,
                    Err(message) => {
                        self.failure_rule_edit_failed(message);
                        return;
                    }
                };
                let first = ids[0];
                let second = ids[1];
                let item = ids[2];
                let (first_value, second_value, sequence) = match kind {
                    GeneratedRuleKind::IntegerRange => (
                        ir::Value::Int(1),
                        ir::Value::Int(3),
                        SequenceExpr::Generate {
                            from: Some(first),
                            to: second,
                            item,
                        },
                    ),
                    GeneratedRuleKind::SplitText => (
                        ir::Value::String("first,second".into()),
                        ir::Value::String(",".into()),
                        SequenceExpr::Tokenize {
                            input: first,
                            delimiter: second,
                            item,
                        },
                    ),
                    GeneratedRuleKind::SplitTextByLength => (
                        ir::Value::String("aé🙂z".into()),
                        ir::Value::Int(2),
                        SequenceExpr::TokenizeByLength {
                            input: first,
                            length: second,
                            item,
                        },
                    ),
                    GeneratedRuleKind::SplitTextByRegex => (
                        ir::Value::String("first,second".into()),
                        ir::Value::String("[,;]+".into()),
                        SequenceExpr::TokenizeRegex {
                            input: first,
                            pattern: second,
                            flags: None,
                            item,
                        },
                    ),
                };
                self.project.graph.nodes.extend([
                    (first, Node::Const { value: first_value }),
                    (
                        second,
                        Node::Const {
                            value: second_value,
                        },
                    ),
                    (
                        item,
                        Node::SourceField {
                            path: Vec::new(),
                            frame: None,
                        },
                    ),
                ]);
                self.project.failure_rules.push(FailureRule {
                    iteration: FailureIteration::Sequence { sequence },
                    selection: FailureSelection::All,
                    message: None,
                });
                self.rebuild_mapping_canvases_after_retirement();
                Some(self.project.failure_rules.len() - 1)
            }
            RuleAction::EnableRegexFlags(index) => {
                let Some(rule) = self.project.failure_rules.get(index) else {
                    return;
                };
                if !editable_generated_rule(&self.project, rule)
                    || !matches!(
                        &rule.iteration,
                        FailureIteration::Sequence {
                            sequence: SequenceExpr::TokenizeRegex { flags: None, .. },
                        }
                    )
                {
                    return;
                }
                let existing = self.project.graph.nodes.iter().find_map(|(&id, node)| {
                    matches!(node, Node::Const { value: ir::Value::String(value) } if value.is_empty())
                        .then_some(id)
                });
                let (flags_node, created) = match existing {
                    Some(id) => (id, false),
                    None => {
                        let ids =
                            match crate::graph_viewer::reserve_project_node_ids(&self.project, 1) {
                                Ok(ids) => ids,
                                Err(message) => {
                                    self.failure_rule_edit_failed(message);
                                    return;
                                }
                            };
                        let id = ids[0];
                        self.project.graph.nodes.insert(
                            id,
                            Node::Const {
                                value: ir::Value::String(String::new()),
                            },
                        );
                        (id, true)
                    }
                };
                let FailureIteration::Sequence {
                    sequence: SequenceExpr::TokenizeRegex { flags, .. },
                } = &mut self.project.failure_rules[index].iteration
                else {
                    unreachable!("validated regex flags action");
                };
                *flags = Some(flags_node);
                if created {
                    self.rebuild_mapping_canvases_after_retirement();
                }
                Some(index)
            }
            RuleAction::Remove(index) => {
                let Some(rule) = self.project.failure_rules.get(index) else {
                    return;
                };
                let item = match &rule.iteration {
                    FailureIteration::Source { .. } => None,
                    FailureIteration::Sequence { sequence } => {
                        if !editable_generated_rule(&self.project, rule) {
                            return;
                        }
                        let item = sequence.item();
                        let references = crate::graph_viewer::references_outside_failure_rule(
                            &self.project,
                            rule,
                            item,
                        );
                        if !references.is_empty() {
                            self.failure_rule_edit_failed(format!(
                                    "Generated item #{item} is still used by {}; disconnect those references before removing its rule",
                                    references.join(", ")
                                ));
                            return;
                        }
                        Some(item)
                    }
                };
                self.project.failure_rules.remove(index);
                if let Some(item) = item {
                    self.project.graph.nodes.remove(&item);
                    self.rebuild_mapping_canvases_after_retirement();
                }
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

    fn failure_rule_edit_failed(&mut self, message: String) {
        self.status = "failure rule edit failed".into();
        self.diagnostics.error("Failure rule edit failed", message);
    }
}

fn show_rule_expressions(
    ui: &mut egui::Ui,
    index: usize,
    selection: &mut FailureSelection,
    message: &mut Option<NodeId>,
    graph: &mapping::Graph,
    first_node: Option<NodeId>,
) {
    ui.horizontal(|ui| {
        ui.label("Stop when:");
        let mut mode = selection_mode(*selection);
        egui::ComboBox::from_id_salt(("failure_selection", index))
            .selected_text(selection_label(*selection))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut mode, 0, "All items");
                ui.add_enabled_ui(
                    first_node.is_some() || selection.predicate().is_some(),
                    |ui| {
                        ui.selectable_value(&mut mode, 1, "Expression is true");
                        ui.selectable_value(&mut mode, 2, "Expression is false");
                    },
                );
            });
        if mode != selection_mode(*selection) {
            let predicate = selection.predicate().or(first_node);
            *selection = match (mode, predicate) {
                (1, Some(predicate)) => FailureSelection::WhenTrue { predicate },
                (2, Some(predicate)) => FailureSelection::WhenFalse { predicate },
                _ => FailureSelection::All,
            };
        }
    });
    match selection {
        FailureSelection::All => {
            ui.weak("Every chosen item stops the mapping.");
        }
        FailureSelection::WhenTrue { predicate } | FailureSelection::WhenFalse { predicate } => {
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
    let mut has_message = message.is_some();
    if ui
        .add_enabled(
            first_node.is_some() || has_message,
            egui::Checkbox::new(&mut has_message, "Custom message expression"),
        )
        .changed()
    {
        *message = if has_message { first_node } else { None };
    }
    if let Some(message) = message {
        ui.horizontal(|ui| {
            ui.label("Message:");
            crate::scope_editor::node_picker(ui, ("failure_message", index), message, graph);
        });
    } else {
        ui.weak("Use the default failure message.");
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

#[cfg(test)]
#[path = "generated_failure_rules_tests.rs"]
mod generated_tests;
