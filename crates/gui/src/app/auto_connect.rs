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
        let hint = match source_frame_hint(root, &self.selected_scope) {
            Ok(hint) => hint,
            Err(reason) => {
                self.status = reason.to_owned();
                return;
            }
        };
        let plan = match plan_auto_connect(
            &self.project.source,
            schema,
            selected_scope,
            &target_chain,
            hint.as_deref(),
        ) {
            Ok(plan) => plan,
            Err(reason) => {
                self.status = reason.to_owned();
                return;
            }
        };
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
        let response = crate::ui_kit::dialog(ctx, crate::ui_kit::DialogKind::Modal, "Auto-connect fields")
            .width(crate::ui_kit::DialogWidth::Medium)
            .show(|ui| {
                ui.label(&pending.scope_label);
                crate::ui_kit::form_grid(ui, "auto_connect_summary", |form| {
                    form.row("Connections to add", |ui| { ui.strong(pending.plan.connections.len().to_string()); });
                    form.row("Ambiguous fields", |ui| { ui.label(pending.plan.skipped_ambiguous.to_string()); });
                    form.row("Incompatible / unmatched", |ui| { ui.label(pending.plan.skipped_incompatible.to_string()); });
                });
                if !pending.plan.skipped_fields.is_empty() {
                    crate::ui_kit::section(ui, "Skipped target fields", |ui| {
                        for field in &pending.plan.skipped_fields {
                            ui.add(egui::Label::new(field.target_path.join(" / ")).wrap());
                            crate::ui_kit::help(ui, field.reason.label());
                        }
                    });
                }
                if pending.plan.is_empty() {
                    crate::ui_kit::help(ui, "No compatible, unambiguous connections are available. Existing bindings are kept.");
                }
            }, |ui| {
                crate::ui_kit::dialog_buttons(ui,
                    crate::ui_kit::DialogButton::new("Apply connections").disabled_if(pending.plan.is_empty(), "No compatible, unambiguous connections are available"),
                    Some("Cancel".into()), None)
            });

        match response.footer {
            Some(crate::ui_kit::DialogAction::Primary) => self.apply_pending_auto_connect(),
            Some(_) => {
                self.pending_auto_connect = None;
                self.status = "auto-connect cancelled".to_owned();
            }
            None if response.close_requested => {
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
        // Recheck ancestors after confirmation before any graph or scope mutation.
        let root = match pending.target {
            AutoConnectTarget::Primary => Some(&self.project.root),
            AutoConnectTarget::Named(index) => self
                .project
                .extra_targets
                .get(index)
                .map(|target| &target.root),
        };
        if let Some(root) = root
            && let Err(reason) = source_frame_hint(root, &pending.scope_path)
        {
            self.status = reason.to_owned();
            return;
        }
        let owned_items = crate::graph_viewer::project_sequence_item_ids(&self.project);
        let before = self.project.clone();
        let result = match pending.target {
            AutoConnectTarget::Primary => {
                let Some(scope) = scope_at_mut(&mut self.project.root, &pending.scope_path) else {
                    self.status = "auto-connect selection no longer exists".to_owned();
                    return;
                };
                apply_auto_connect(&mut self.project.graph, scope, &pending.plan, &owned_items)
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
                apply_auto_connect(&mut self.project.graph, scope, &pending.plan, &owned_items)
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
            let document = match pending.target {
                AutoConnectTarget::Primary => MappingDocument::Main,
                AutoConnectTarget::Named(index) => MappingDocument::Target(index),
            };
            let action = editor_ui::GraphAction::AutoConnect {
                scope: editor_ui::ScopeLocation::of_children(&pending.scope_path),
                connected: applied,
            };
            self.queue_document_edit(document, &before, action, None);
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
            self.diagnostics.validation(&self.project, issues);
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

#[cfg(test)]
mod dialog_tests {
    use super::*;
    use crate::auto_connect::SkipReason;
    use ir::{ScalarType, SchemaNode};

    #[test]
    fn skipped_fields_explain_each_counter_without_changing_the_plan() {
        let source = SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group("One", vec![SchemaNode::scalar("Name", ScalarType::String)]),
                SchemaNode::group("Two", vec![SchemaNode::scalar("Name", ScalarType::String)]),
                SchemaNode::scalar("Count", ScalarType::Bool),
                SchemaNode::scalar("Exact", ScalarType::String),
            ],
        );
        let target = SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("Count", ScalarType::Int),
                SchemaNode::scalar("Missing", ScalarType::String),
                SchemaNode::scalar("Exact", ScalarType::String),
            ],
        );
        let plan = plan_auto_connect(&source, &target, &Scope::default(), &[], None).unwrap();
        assert_eq!(plan.connections.len(), 1);
        assert_eq!(plan.connections[0].target_field, "Exact");
        assert_eq!(plan.skipped_ambiguous, 1);
        assert_eq!(plan.skipped_incompatible, 2);
        let details = plan
            .skipped_fields
            .iter()
            .map(|field| (field.target_path.join("/"), field.reason))
            .collect::<Vec<_>>();
        assert_eq!(
            details,
            vec![
                ("Name".into(), SkipReason::Ambiguous),
                ("Count".into(), SkipReason::Incompatible),
                ("Missing".into(), SkipReason::Unmatched)
            ]
        );
        let scope = Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..Default::default()
        };
        let plan = plan_auto_connect(&source, &target, &scope, &[], None).unwrap();
        assert!(plan.is_empty());
        assert_eq!(plan.skipped_fields.len(), 4);
        assert!(
            plan.skipped_fields
                .iter()
                .all(|field| field.reason == SkipReason::UnavailableScope)
        );
    }
}
