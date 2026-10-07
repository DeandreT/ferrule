use super::*;

impl FerruleApp {
    pub(super) fn apply_selected_target_xml_type(
        &mut self,
        action: &crate::target_xml_type::Action,
    ) {
        let target = match self.mapping_workspace.active {
            MappingDocument::Main => None,
            MappingDocument::Target(index) => Some(index),
            MappingDocument::Function(_) => return,
        };
        match crate::target_xml_type::apply(&mut self.project, target, &self.selected_scope, action)
        {
            Ok(true) => {
                if let Some(index) = target {
                    let nodes = self
                        .mapping_workspace
                        .target_canvases
                        .get(&index)
                        .map(|canvas| CanvasLayout::capture_nodes(&canvas.snarl))
                        .unwrap_or_default();
                    let mut snarl = canvas_build::build_named_target_snarl(&self.project, index);
                    CanvasLayout::apply_nodes(&nodes, &mut snarl);
                    if let Some(canvas) = self.mapping_workspace.target_canvases.get_mut(&index) {
                        // Keep this named document's search, viewport, and
                        // endpoint scroll state while updating its bindings.
                        canvas.snarl = snarl;
                    } else {
                        self.mapping_workspace
                            .target_canvases
                            .insert(index, CanvasDocumentState::with_snarl(snarl));
                    }
                } else {
                    self.rebuild_snarl_preserving_positions();
                }
                if matches!(action, crate::target_xml_type::Action::Declared(_)) {
                    let root = target
                        .and_then(|index| self.project.extra_targets.get(index))
                        .map_or(&self.project.root, |target| &target.root);
                    let expression = crate::auto_connect::scope_at(root, &self.selected_scope)
                        .and_then(|scope| {
                            scope
                                .bindings
                                .iter()
                                .find(|binding| binding.target_field == ir::XML_TYPE_FIELD)
                        })
                        .map(|binding| binding.node);
                    let canvas = match target {
                        Some(index) => self.mapping_workspace.target_canvases.get_mut(&index),
                        None => Some(&mut self.main_canvas),
                    };
                    if let (Some(expression), Some(canvas)) = (expression, canvas) {
                        crate::canvas_layout::place_graph_node_without_overlap(
                            &mut canvas.snarl,
                            &canvas.node_sizes,
                            expression,
                        );
                    }
                }
                self.status = "target XML type updated".to_string();
                let issues = cli::validate(&self.project);
                if issues.is_empty() {
                    self.diagnostics.clear();
                } else {
                    self.diagnostics.validation(&self.project, issues);
                }
            }
            Ok(false) => {}
            Err(error) => {
                self.status = "target XML type edit failed".to_string();
                self.diagnostics.error("Target XML type edit failed", error);
            }
        }
    }

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
        let joined_subtree = selected.is_some_and(join_authoring::subtree_has_join);
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
                    !self.selected_scope.is_empty() && !joined_subtree,
                    egui::Button::new("Remove scope"),
                )
                .on_disabled_hover_text(join_authoring::JOIN_REMOVAL_REASON)
                .clicked()
            {
                action = Some(ScopeAction::Remove);
            }
        });
        if joined_subtree {
            ui.weak(join_authoring::JOIN_REMOVAL_REASON);
        }

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
