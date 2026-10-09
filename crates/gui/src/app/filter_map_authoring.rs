//! Scalar sequence authoring in the selected ordinary target scope.

use super::*;
use ir::SchemaKind;
use mapping::{ScopeConstruction, ScopeIteration, SequenceExpr};

fn selected(app: &FerruleApp) -> Option<&Scope> {
    let root = match app.mapping_workspace.active {
        MappingDocument::Main => &app.project.root,
        MappingDocument::Target(index) => &app.project.extra_targets.get(index)?.root,
        _ => return None,
    };
    crate::auto_connect::scope_at(root, &app.selected_scope)
}

fn compatible_scope(app: &FerruleApp) -> bool {
    let (mut scope, mut schema) = match app.mapping_workspace.active {
        MappingDocument::Main => (&app.project.root, &app.project.target),
        MappingDocument::Target(index) => {
            let Some(target) = app.project.extra_targets.get(index) else {
                return false;
            };
            (&target.root, &target.schema)
        }
        _ => return false,
    };
    for &index in &app.selected_scope {
        if scope.construction != ScopeConstruction::Constructed
            || matches!(
                scope.iteration,
                ScopeIteration::InnerJoin { .. } | ScopeIteration::Concatenate(_)
            )
        {
            return false;
        }
        let Some(child) = scope.children.get(index) else {
            return false;
        };
        let Some(child_schema) = schema.child(&child.target_field) else {
            return false;
        };
        scope = child;
        schema = child_schema;
    }
    schema.repeating
        && matches!(schema.kind, SchemaKind::Group { .. })
        && scope.construction == ScopeConstruction::Constructed
        && (matches!(
            scope.iteration,
            ScopeIteration::None | ScopeIteration::Source(_)
        ) || matches!(scope.sequence(), Some(SequenceExpr::FilterMapV1(_))))
}

impl FerruleApp {
    fn set_selected_filter_map(&mut self, sequence: SequenceExpr) {
        let root = match self.mapping_workspace.active {
            MappingDocument::Main => &mut self.project.root,
            MappingDocument::Target(index) => {
                let Some(target) = self.project.extra_targets.get_mut(index) else {
                    return;
                };
                &mut target.root
            }
            _ => return,
        };
        scope_at_mut(root, &self.selected_scope).set_sequence(Some(sequence));
        self.rebuild_mapping_canvases_after_retirement();
    }

    pub(super) fn create_selected_filter_map(&mut self) -> Result<(), String> {
        if !self.ui_project_editing_enabled() {
            return Err("Finish the current operation before editing the mapping.".into());
        }
        if !compatible_scope(self) || selected(self).is_some_and(|scope| scope.sequence().is_some())
        {
            return Err(
                "Choose an ordinary repeating target group without a generated sequence.".into(),
            );
        }
        let ids = crate::graph_viewer::reserve_project_node_ids(&self.project, 4)?;
        let (sequence, nodes) =
            crate::filter_map_editor::create(&ids, &self.project.user_functions)?;
        self.project.graph.nodes.extend(nodes);
        self.set_selected_filter_map(sequence);
        Ok(())
    }

    pub(super) fn show_filter_map_scope_editor(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let enabled = enabled && self.ui_project_editing_enabled();
        let Some(scope) = selected(self) else {
            return;
        };
        let current = scope.sequence().cloned();
        let stored_filter_map = matches!(current, Some(SequenceExpr::FilterMapV1(_)));
        let compatible = compatible_scope(self);
        if !stored_filter_map && !compatible {
            return;
        }
        let functions_available =
            crate::filter_map_editor::default_stages(&self.project.user_functions).is_some();
        if !stored_filter_map {
            let create = ui.add_enabled(
                enabled && compatible && current.is_none() && functions_available,
                egui::Button::new("Use filter/map"),
            );
            if !functions_available {
                create.on_disabled_hover_text("Add filter and map functions with input value and input position parameters first.");
            } else if create.clicked()
                && let Err(error) = self.create_selected_filter_map()
            {
                self.status = "sequence edit failed".into();
                self.diagnostics.error("Sequence edit failed", error);
            }
            return;
        }
        let Some(mut staged) = current.clone() else {
            return;
        };
        let owned = crate::graph_viewer::project_sequence_item_ids(&self.project);
        let private_inputs = crate::graph_viewer::filter_map_private_inputs(&self.project);
        ui.add_enabled_ui(enabled && compatible, |ui| {
            crate::filter_map_editor::show(
                ui,
                &mut staged,
                &self.project.graph,
                &self.project.user_functions,
                crate::filter_map_editor::Items {
                    all: &owned,
                    inputs: &private_inputs,
                },
                None,
                false,
            );
        });
        if !compatible {
            ui.weak("These stored settings are retained. Select an ordinary repeating target group to edit them.");
        }
        if current.as_ref() != Some(&staged) {
            self.set_selected_filter_map(staged);
        }
    }
}

#[cfg(test)]
#[path = "filter_map_authoring_tests.rs"]
mod tests;
