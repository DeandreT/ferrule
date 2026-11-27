//! Repair one binding identified by a current validator issue. Save evidence
//! belongs to the opened document; undo history and clean untitled projects
//! are not sources of saved node definitions.

use std::collections::BTreeSet;
use std::time::Instant;

use crate::diagnostics::broken_binding::{BrokenBinding, child, target};
use crate::graph_viewer::readable_node_label;
use crate::project_state::{model_key, project_snapshot_key};
use crate::ui_kit::{self, DialogAction, DialogButton, DialogKind, DialogWidth};
use engine::ValidationIssue;
use mapping::{Node, NodeId, Project, ScopeRootSlot};

use super::FerruleApp;

pub(super) struct SavedMapping {
    document: egui::Id,
    revision: String,
    project: Project,
}

pub(super) struct RepairDraft {
    document: egui::Id,
    draft_key: String,
    binding: BrokenBinding,
    target_label: String,
    saved: Option<SavedProposal>,
    choices: Vec<NodeId>,
    replacement: Option<NodeId>,
    error: Option<String>,
}

struct SavedProposal {
    revision: String,
    label: String,
    definition: String,
    inputs: Vec<String>,
}

impl SavedMapping {
    pub(super) fn belongs_to(&self, document: egui::Id) -> bool {
        self.document == document
    }
}

/// Check the whole dependency closure, not just whether the same IDs exist.
/// A source-field node also depends on the source schema and enclosing scopes.
fn saved_definition<'a>(
    current: &Project,
    saved: &'a Project,
    binding: &BrokenBinding,
) -> Option<&'a Node> {
    let (schema, mut scope) = target(current, &binding.owner.target)?;
    let (saved_schema, saved_scope) = target(saved, &binding.owner.target)?;
    if scope != saved_scope
        || schema != saved_schema
        || current.source != saved.source
        || current.source_options != saved.source_options
        || current.source_path != saved.source_path
        || current.extra_sources != saved.extra_sources
        || current.execution_contract != saved.execution_contract
        || model_key(&current.user_functions) != model_key(&saved.user_functions)
        || current.graph.nodes.contains_key(&binding.missing)
    {
        return None;
    }
    let node = saved.graph.nodes.get(&binding.missing)?;
    let mut pending = node.dependencies();
    // Scope controls can be expressions as well. Their IDs being unchanged
    // does not mean the iteration and selection still have the same meaning.
    for step in binding.owner.path.iter().map(Some).chain([None]) {
        scope.visit_roots(&mut |root| {
            if !matches!(
                root.slot,
                ScopeRootSlot::Binding(_)
                    | ScopeRootSlot::DynamicBindingKey(_)
                    | ScopeRootSlot::DynamicBindingValue(_)
            ) {
                pending.push(root.node);
            }
        });
        if let Some(step) = step {
            scope = child(scope, step)?;
        }
    }
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if visited.insert(id) {
            let expected = saved.graph.nodes.get(&id)?;
            let actual = current.graph.nodes.get(&id)?;
            if model_key(expected) != model_key(actual) {
                return None;
            }
            pending.extend(expected.dependencies());
        }
    }
    Some(node)
}

/// A repair must remove the selected issue without introducing a different
/// validation failure. Unrelated pre-existing issues can still be repaired
/// separately. Preserve multiplicity when validation reports duplicates.
fn validate_candidate(
    before: &[ValidationIssue],
    candidate: &Project,
    binding: &BrokenBinding,
) -> bool {
    let mut remaining: Vec<_> = before.iter().collect();
    for issue in cli::validate(candidate) {
        if BrokenBinding::from_issue(candidate, &issue).as_ref() == Some(binding) {
            return false;
        }
        let Some(index) = remaining.iter().position(|existing| **existing == issue) else {
            return false;
        };
        remaining.swap_remove(index);
    }
    true
}

impl FerruleApp {
    /// Only call after reading a mapping from its file or publishing its bytes.
    pub(super) fn remember_saved_mapping(&mut self, revision: &str) {
        self.saved_mapping = Some(SavedMapping {
            project: self.project.clone(),
            document: self.document_ui_id,
            revision: revision.to_owned(),
        });
        self.connection_repair = None;
    }

    pub(super) fn inspect_broken_connection(
        &mut self,
        binding: BrokenBinding,
        issues: &[ValidationIssue],
    ) {
        let Some(target_label) = binding.target_label(&self.project) else {
            return;
        };
        let Some((_, mut scope)) = target(&self.project, &binding.owner.target) else {
            return;
        };
        let mut ancestors = vec![scope];
        for step in &binding.owner.path {
            let Some(next) = child(scope, step) else {
                return;
            };
            scope = next;
            ancestors.push(scope);
        }
        let primary_root = binding.owner.target == engine::ValidationEndpoint::Target
            && binding.owner.path.is_empty();
        let choices = if binding.field == ir::XML_TYPE_FIELD {
            // Virtual XML types have a specialized inspector, not a scalar node picker.
            Vec::new()
        } else {
            crate::scope_editor::binding_replacement_choices(
                &self.project,
                &ancestors,
                primary_root,
            )
        };
        let saved = self
            .saved_mapping
            .as_ref()
            .filter(|saved| saved.document == self.document_ui_id)
            .and_then(|saved| {
                let node = saved_definition(&self.project, &saved.project, &binding)?;
                let mut candidate = self.project.clone();
                candidate.graph.nodes.insert(binding.missing, node.clone());
                if !validate_candidate(issues, &candidate, &binding) {
                    return None;
                }
                Some(SavedProposal {
                    revision: saved.revision.clone(),
                    label: readable_node_label(node),
                    definition: serde_json::to_string(node).ok()?,
                    inputs: node
                        .dependencies()
                        .into_iter()
                        .map(|id| {
                            format!(
                                "Node {id}: {}",
                                readable_node_label(&self.project.graph.nodes[&id])
                            )
                        })
                        .collect(),
                })
            });
        self.connection_repair = Some(RepairDraft {
            document: self.document_ui_id,
            draft_key: project_snapshot_key(&self.project),
            binding,
            target_label,
            saved,
            choices,
            replacement: None,
            error: None,
        });
    }

    fn apply_connection_repair(&mut self) {
        let Some(draft) = self.connection_repair.take() else {
            return;
        };
        let stale = "Connection changed. Validate again to choose a repair.";
        if !self.ui_project_editing_enabled()
            || draft.document != self.document_ui_id
            || draft.draft_key != project_snapshot_key(&self.project)
        {
            self.status = stale.into();
            return;
        }
        let before = cli::validate(&self.project);
        if !before.iter().any(|issue| {
            BrokenBinding::from_issue(&self.project, issue).as_ref() == Some(&draft.binding)
        }) {
            self.status = stale.into();
            return;
        }
        let mut candidate = self.project.clone();
        if let Some(replacement) = draft.replacement {
            if !draft.choices.contains(&replacement) {
                self.status = stale.into();
                return;
            }
            let Some(binding) = draft.binding.binding_mut(&mut candidate) else {
                self.status = stale.into();
                return;
            };
            binding.node = replacement;
        } else {
            let restore = self
                .saved_mapping
                .as_ref()
                .filter(|saved| {
                    saved.document == draft.document
                        && draft
                            .saved
                            .as_ref()
                            .is_some_and(|p| p.revision == saved.revision)
                })
                .and_then(|saved| saved_definition(&candidate, &saved.project, &draft.binding));
            let Some(node) = restore else {
                self.status = stale.into();
                return;
            };
            candidate
                .graph
                .nodes
                .insert(draft.binding.missing, node.clone());
        }
        if !validate_candidate(&before, &candidate, &draft.binding) {
            self.connection_repair = Some(RepairDraft {
                error: Some(
                    "That connection introduces a validation issue. Choose another node.".into(),
                ),
                ..draft
            });
            return;
        }
        let now = Instant::now();
        self.observe_editor_history(now, false);
        self.commit_pending_history();
        self.project = candidate;
        self.rebuild_mapping_canvases_after_retirement();
        let restored = self.live_snapshot();
        let previous = self.adopt_observed(restored);
        self.history.record(
            editor_ui::HistoryRecord::Snapshot(previous),
            "Repair connection",
        );
        self.command_coalescing = None;
        self.validate_now();
        self.status = format!("Repaired {}. {}", draft.target_label, self.status);
    }

    pub(super) fn show_connection_repair(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.connection_repair.as_mut() else {
            return;
        };
        let graph = &self.project.graph;
        let can_apply = draft.saved.is_some() || draft.replacement.is_some();
        let response = ui_kit::dialog(ctx, DialogKind::Modal, "Repair a broken connection")
            .width(DialogWidth::Medium)
            .show(
                |ui| {
                    ui.strong(&draft.target_label);
                    ui.label(format!(
                        "References missing node {}.",
                        draft.binding.missing
                    ));
                    if let Some(error) = &draft.error {
                        ui.label(error);
                    }
                    if let Some(saved) = &draft.saved {
                        ui.heading(format!("Restore {}", saved.label));
                        ui.label(format!("Last saved revision: {}", saved.revision));
                        ui.monospace(&saved.definition);
                        for input in &saved.inputs {
                            ui.label(input);
                        }
                    } else {
                        ui.label("No matching saved definition.");
                    }
                    if draft.choices.is_empty() {
                        ui.label("No eligible replacement node. Cancel to edit the mapping.");
                        return;
                    }
                    let selected = draft.replacement.map_or_else(
                        || {
                            if draft.saved.is_some() {
                                "Use saved definition"
                            } else {
                                "Choose a node"
                            }
                            .to_owned()
                        },
                        |id| format!("Use node {id}"),
                    );
                    egui::ComboBox::from_label("Connection")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            if draft.saved.is_some() {
                                ui.selectable_value(
                                    &mut draft.replacement,
                                    None,
                                    "Use saved definition",
                                );
                            }
                            for &id in &draft.choices {
                                if let Some(node) = graph.nodes.get(&id) {
                                    ui.selectable_value(
                                        &mut draft.replacement,
                                        Some(id),
                                        format!("Node {id}: {}", readable_node_label(node)),
                                    );
                                }
                            }
                        });
                },
                |ui| {
                    ui_kit::dialog_buttons(
                        ui,
                        DialogButton::new("Apply repair")
                            .disabled_if(!can_apply, "Choose a node first"),
                        Some("Cancel".into()),
                        None,
                    )
                },
            );
        if response.close_requested || response.footer == Some(DialogAction::Secondary) {
            self.connection_repair = None;
        } else if response.footer == Some(DialogAction::Primary) {
            self.apply_connection_repair();
        }
    }
}

#[cfg(test)]
mod tests;
