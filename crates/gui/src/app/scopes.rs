use super::*;

impl FerruleApp {
    pub(super) fn show_scope_controls(&mut self, ui: &mut egui::Ui) {
        let target_index = match self.mapping_workspace.active {
            MappingDocument::Target(index) => Some(index),
            MappingDocument::Main | MappingDocument::Function(_) => None,
        };
        let candidates = match target_index {
            Some(index) => self
                .project
                .extra_targets
                .get(index)
                .map(|target| {
                    available_static_child_scopes(
                        &target.root,
                        &target.schema,
                        &self.selected_scope,
                    )
                    .unwrap_or_default()
                })
                .unwrap_or_default(),
            None => available_static_child_scopes(
                &self.project.root,
                &self.project.target,
                &self.selected_scope,
            )
            .unwrap_or_default(),
        };
        let selected = match target_index {
            Some(index) => self.project.extra_targets.get(index).and_then(|target| {
                crate::auto_connect::scope_at(&target.root, &self.selected_scope)
            }),
            None => crate::auto_connect::scope_at(&self.project.root, &self.selected_scope),
        };
        let whole_group_copy = match target_index {
            Some(index) => self.project.extra_targets.get(index).is_some_and(|target| {
                crate::scope_editor::copied_ancestor_at_path(&target.root, &self.selected_scope)
                    .is_some()
            }),
            None => crate::scope_editor::copied_ancestor_at_path(
                &self.project.root,
                &self.selected_scope,
            )
            .is_some(),
        };
        let can_expand = !whole_group_copy
            && selected.is_some_and(|scope| {
                matches!(scope.construction, mapping::ScopeConstruction::Constructed)
                    && scope.concatenated().is_none()
            });
        let mut action = None;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!whole_group_copy && !candidates.is_empty(), |ui| {
                ui.menu_button("Add child", |ui| {
                    for candidate in &candidates {
                        let label = if candidate.repeating {
                            format!("{} (repeating)", candidate.target_field)
                        } else {
                            candidate.target_field.clone()
                        };
                        if ui.button(label).clicked() {
                            action = Some(ScopeAction::Add(candidate.target_field.clone()));
                            ui.close();
                        }
                    }
                });
            })
            .response
            .on_disabled_hover_text(if whole_group_copy {
                "Whole source group copies retain their complete children; individual child scopes cannot be added"
            } else {
                "No unrepresented target groups"
            });
            if ui
                .add_enabled(can_expand, egui::Button::new("Expand subtree"))
                .on_hover_text("Add missing target group scopes below this scope. Configure source iteration separately for repeating groups.")
                .clicked()
            {
                action = Some(ScopeAction::Expand);
            }
            if ui
                .add_enabled(
                    !self.selected_scope.is_empty(),
                    egui::Button::new("Remove scope"),
                )
                .clicked()
            {
                action = Some(ScopeAction::Remove);
            }
        });

        if matches!(action, Some(ScopeAction::Expand)) {
            self.expand_selected_target_subtree();
            return;
        }

        if matches!(action, Some(ScopeAction::Remove)) {
            self.remove_selected_target_scope();
            return;
        }

        let result = match (target_index, action) {
            (Some(index), Some(ScopeAction::Add(target_field))) => {
                let Some(target) = self.project.extra_targets.get_mut(index) else {
                    return;
                };
                create_static_child_scope(
                    &mut target.root,
                    &target.schema,
                    &self.selected_scope,
                    &target_field,
                )
            }
            (None, Some(ScopeAction::Add(target_field))) => create_static_child_scope(
                &mut self.project.root,
                &self.project.target,
                &self.selected_scope,
                &target_field,
            ),
            (_, Some(ScopeAction::Expand | ScopeAction::Remove)) => {
                unreachable!("structural actions handled above")
            }
            (_, None) => return,
        };
        match result {
            Ok(selection) => {
                self.selected_scope = selection;
                self.rebuild_snarl_preserving_positions();
                self.status = "scope tree updated".to_string();
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "scope edit failed".to_string();
                self.diagnostics
                    .error("Scope edit failed", error.to_string());
            }
        }
    }

    pub(super) fn expand_selected_target_subtree(&mut self) {
        let result = match self.mapping_workspace.active {
            MappingDocument::Target(index) => {
                let Some(target) = self.project.extra_targets.get_mut(index) else {
                    return;
                };
                expand_static_target_subtree(&mut target.root, &target.schema, &self.selected_scope)
            }
            MappingDocument::Main => expand_static_target_subtree(
                &mut self.project.root,
                &self.project.target,
                &self.selected_scope,
            ),
            MappingDocument::Function(_) => return,
        };
        match result {
            Ok(expansion) => {
                if expansion.created > 0 {
                    self.rebuild_snarl_preserving_positions();
                    self.diagnostics.clear();
                }
                self.status = format!("created {} target scopes", expansion.created);
                if expansion.repeating_unconfigured > 0 {
                    self.status.push_str(&format!(
                        "; {} repeating scopes need source paths",
                        expansion.repeating_unconfigured
                    ));
                }
                if expansion.skipped_incompatible > 0 {
                    self.status.push_str(&format!(
                        "; skipped {} incompatible branches",
                        expansion.skipped_incompatible
                    ));
                }
            }
            Err(error) => {
                self.status = "scope expansion failed".to_string();
                self.diagnostics
                    .error("Scope expansion failed", error.to_string());
            }
        }
    }

    fn rebuild_snarl_preserving_positions(&mut self) {
        if let MappingDocument::Target(index) = self.mapping_workspace.active {
            let nodes = self
                .mapping_workspace
                .target_canvases
                .get(&index)
                .map(|canvas| CanvasLayout::capture_nodes(&canvas.snarl))
                .unwrap_or_default();
            let mut snarl = canvas_build::build_named_target_snarl(&self.project, index);
            CanvasLayout::apply_nodes(&nodes, &mut snarl);
            self.mapping_workspace
                .target_canvases
                .insert(index, CanvasDocumentState::with_snarl(snarl));
            return;
        }
        let layout = CanvasLayout::capture(
            &self.project,
            &self.main_canvas.snarl,
            &self.mapping_workspace,
        );
        self.main_canvas.snarl = build_snarl_with_layout(&self.project, Some(&layout));
    }
}

enum ScopeAction {
    Add(String),
    Expand,
    Remove,
}
