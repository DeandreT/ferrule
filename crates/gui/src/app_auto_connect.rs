use super::*;

use crate::auto_connect::{
    apply_auto_connect, plan_auto_connect, scope_at, scope_at_mut, source_frame_hint,
};

impl FerruleApp {
    pub(super) fn can_auto_connect(&self) -> bool {
        matches!(
            self.mapping_workspace.active,
            MappingDocument::Main | MappingDocument::Target(_)
        )
    }

    pub(super) fn begin_auto_connect(&mut self) {
        let target = match self.mapping_workspace.active {
            MappingDocument::Main => AutoConnectTarget::Primary,
            MappingDocument::Target(index) if index < self.project.extra_targets.len() => {
                self.ensure_target_canvas(index);
                AutoConnectTarget::Named(index)
            }
            MappingDocument::Target(_) | MappingDocument::Function(_) => {
                self.status = "auto-connect requires an active target mapping".to_owned();
                return;
            }
        };
        let (schema, root, boundary_name) = match target {
            AutoConnectTarget::Primary => (&self.project.target, &self.project.root, "Primary"),
            AutoConnectTarget::Named(index) => {
                let Some(named) = self.project.extra_targets.get(index) else {
                    return;
                };
                (&named.schema, &named.root, named.name.as_str())
            }
        };
        let Some(selected_scope) = scope_at(root, &self.selected_scope) else {
            self.selected_scope.clear();
            self.status = "auto-connect selection no longer exists".to_owned();
            return;
        };
        let target_chain = scope_target_chain(root, &self.selected_scope);
        let hint = source_frame_hint(root, &self.selected_scope);
        let plan = plan_auto_connect(
            &self.project.source,
            schema,
            selected_scope,
            &target_chain,
            hint.as_deref(),
        );
        let scope_label = if target_chain.is_empty() {
            format!("{boundary_name} / root")
        } else {
            format!("{boundary_name} / {}", target_chain.join(" / "))
        };
        self.pending_auto_connect = Some(PendingAutoConnect {
            target,
            scope_path: self.selected_scope.clone(),
            scope_label,
            plan,
        });
    }

    pub(super) fn show_auto_connect_confirmation(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_auto_connect.as_ref() else {
            return;
        };
        let mut action = None;
        egui::Window::new("Auto-connect Fields")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(&pending.scope_label);
                egui::Grid::new("auto_connect_summary")
                    .num_columns(2)
                    .spacing([18.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Connect");
                        ui.strong(pending.plan.connections.len().to_string());
                        ui.end_row();
                        ui.label("Skipped: ambiguous");
                        ui.label(pending.plan.skipped_ambiguous.to_string());
                        ui.end_row();
                        ui.label("Skipped: incompatible or unmatched");
                        ui.label(pending.plan.skipped_incompatible.to_string());
                        ui.end_row();
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        action = Some(false);
                    }
                    if ui
                        .add_enabled(
                            !pending.plan.is_empty(),
                            egui::Button::new("Apply connections"),
                        )
                        .clicked()
                    {
                        action = Some(true);
                    }
                });
            });

        match action {
            Some(true) => self.apply_pending_auto_connect(),
            Some(false) => {
                self.pending_auto_connect = None;
                self.status = "auto-connect cancelled".to_owned();
            }
            None => {}
        }
    }

    pub(super) fn apply_pending_auto_connect(&mut self) {
        let Some(pending) = self.pending_auto_connect.take() else {
            return;
        };
        let result = match pending.target {
            AutoConnectTarget::Primary => {
                let Some(scope) = scope_at_mut(&mut self.project.root, &pending.scope_path) else {
                    self.status = "auto-connect selection no longer exists".to_owned();
                    return;
                };
                apply_auto_connect(&mut self.project.graph, scope, &pending.plan)
            }
            AutoConnectTarget::Named(index) => {
                let Some(target) = self.project.extra_targets.get_mut(index) else {
                    self.status = "auto-connect target no longer exists".to_owned();
                    return;
                };
                let Some(scope) = scope_at_mut(&mut target.root, &pending.scope_path) else {
                    self.status = "auto-connect selection no longer exists".to_owned();
                    return;
                };
                apply_auto_connect(&mut self.project.graph, scope, &pending.plan)
            }
        };
        let applied = match result {
            Ok(applied) => applied,
            Err(error) => {
                self.status = "auto-connect failed".to_owned();
                self.diagnostics.error("Auto-connect failed", error);
                return;
            }
        };
        if applied > 0 {
            self.rebuild_auto_connected_canvas(pending.target);
        }
        self.status = format!(
            "auto-connect: {applied} connected, {} ambiguous, {} incompatible or unmatched",
            pending.plan.skipped_ambiguous, pending.plan.skipped_incompatible
        );
        let issues = cli::validate(&self.project);
        if issues.is_empty() {
            self.diagnostics.clear();
        } else {
            self.diagnostics.validation(issues);
        }
    }

    fn rebuild_auto_connected_canvas(&mut self, target: AutoConnectTarget) {
        match target {
            AutoConnectTarget::Primary => {
                let positions = CanvasLayout::capture_nodes(&self.main_canvas.snarl);
                self.main_canvas.snarl = build_snarl(&self.project);
                CanvasLayout::apply_nodes(&positions, &mut self.main_canvas.snarl);
            }
            AutoConnectTarget::Named(index) => {
                let Some(canvas) = self.mapping_workspace.target_canvases.get_mut(&index) else {
                    return;
                };
                let positions = CanvasLayout::capture_nodes(&canvas.snarl);
                canvas.snarl = canvas_build::build_named_target_snarl(&self.project, index);
                CanvasLayout::apply_nodes(&positions, &mut canvas.snarl);
            }
        }
    }
}
