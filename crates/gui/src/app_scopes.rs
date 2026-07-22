use super::*;

impl FerruleApp {
    pub(super) fn show_scope_controls(&mut self, ui: &mut egui::Ui) {
        let candidates = available_static_child_scopes(
            &self.project.root,
            &self.project.target,
            &self.selected_scope,
        )
        .unwrap_or_default();
        let mut action = None;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!candidates.is_empty(), |ui| {
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
            .on_disabled_hover_text("No unrepresented target groups");
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

        let result = match action {
            Some(ScopeAction::Add(target_field)) => create_static_child_scope(
                &mut self.project.root,
                &self.project.target,
                &self.selected_scope,
                &target_field,
            ),
            Some(ScopeAction::Remove) => {
                remove_child_scope(&mut self.project.root, &self.selected_scope)
            }
            None => return,
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

    fn rebuild_snarl_preserving_positions(&mut self) {
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
    Remove,
}
