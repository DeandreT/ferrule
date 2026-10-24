use super::*;

#[path = "workspace/command_bar.rs"]
mod command_bar;
#[path = "workspace/inspector.rs"]
mod inspector;

impl FerruleApp {
    fn arrange_focused_canvas(&mut self, mode: ArrangeMode) {
        let document = self.mapping_workspace.focused;
        if mode == ArrangeMode::Compact {
            let canvas = match document {
                MappingDocument::Main => Some(&mut self.main_canvas),
                MappingDocument::Target(target) => {
                    self.ensure_target_canvas(target);
                    self.mapping_workspace.target_canvases.get_mut(&target)
                }
                MappingDocument::Function(function) => {
                    self.ensure_function_canvas(function);
                    self.mapping_workspace.function_canvases.get_mut(&function)
                }
            };
            if let Some(canvas) = canvas {
                canvas.pending_compact = true;
            }
            return;
        }
        if self.arrange_document(document, mode) {
            self.status = match mode {
                ArrangeMode::LeftToRight => "canvas arranged left to right",
                ArrangeMode::Compact => "canvas arranged in compact lanes",
            }
            .to_string();
        }
    }

    pub(super) fn arrange_document(
        &mut self,
        document: MappingDocument,
        mode: ArrangeMode,
    ) -> bool {
        let wire = *self.appearance.wire();
        let canvas = match document {
            MappingDocument::Main => &mut self.main_canvas,
            MappingDocument::Target(target) => {
                if !self.ensure_target_canvas(target) {
                    return false;
                }
                let Some(canvas) = self.mapping_workspace.target_canvases.get_mut(&target) else {
                    return false;
                };
                canvas
            }
            MappingDocument::Function(function) => {
                if !self.ensure_function_canvas(function) {
                    return false;
                }
                let Some(canvas) = self.mapping_workspace.function_canvases.get_mut(&function)
                else {
                    return false;
                };
                canvas
            }
        };
        arrange_snarl_with_mode(
            &mut canvas.snarl,
            &canvas.node_sizes,
            wire,
            mode,
            canvas.viewport_width,
        );
        canvas.reset_view();
        true
    }

    fn fit_focused_canvas(&mut self) {
        let document = self.mapping_workspace.focused;
        if self.fit_document(document) {
            self.status = "canvas fitted".to_string();
        }
    }

    pub(super) fn fit_document(&mut self, document: MappingDocument) -> bool {
        let canvas = match document {
            MappingDocument::Main => &mut self.main_canvas,
            MappingDocument::Target(target) => {
                if !self.ensure_target_canvas(target) {
                    return false;
                }
                let Some(canvas) = self.mapping_workspace.target_canvases.get_mut(&target) else {
                    return false;
                };
                canvas
            }
            MappingDocument::Function(function) => {
                if !self.ensure_function_canvas(function) {
                    return false;
                }
                let Some(canvas) = self.mapping_workspace.function_canvases.get_mut(&function)
                else {
                    return false;
                };
                canvas
            }
        };
        canvas.reset_view();
        true
    }

    pub(super) fn reset_canvas_view(&mut self) {
        self.main_canvas.reset_view();
        for canvas in self.mapping_workspace.target_canvases.values_mut() {
            canvas.reset_view();
        }
    }

    pub(super) fn validate_now(&mut self) {
        let issues = cli::validate(&self.project);
        if issues.is_empty() {
            self.status = "project is valid".to_string();
            self.diagnostics.clear();
        } else {
            self.status = format!("{} validation issue(s)", issues.len());
            self.diagnostics.validation(&self.project, issues);
        }
    }

    pub(super) fn show_main_canvas(&mut self, ui: &mut egui::Ui, editing_enabled: bool) {
        let function_names = self.function_names();
        let function_inputs = self.function_inputs();
        let parameter_names = std::collections::BTreeMap::new();
        let mut requested_function = None;
        let source_paths =
            SourcePathCatalog::new(&self.project.source, &self.project.extra_sources)
                .with_primary_source_options(&self.project.source_options)
                .with_project_ownership(&self.project);
        let source_x12 = crate::x12_tooltips::boundary_has_x12(
            &self.project.source,
            self.project.source_path.as_deref(),
            &self.project.source_options,
        );
        let target_x12 = crate::x12_tooltips::boundary_has_x12(
            &self.project.target,
            self.project.target_path.as_deref(),
            &self.project.target_options,
        );
        let primary_root_authoring = crate::primary_root_authoring::available(&self.project);
        let canvas_id = self.embedded_canvas_id(egui::Id::unique("main_mapping_canvas"));
        sequence_consumer_duplication::begin(ui.ctx());
        let mut planned_edits = Vec::new();
        ui.add_enabled_ui(editing_enabled, |ui| {
            let source_blocks = project_source_blocks(&self.project);
            let target_blocks = target_blocks(&self.project.target);
            let owned_items = crate::graph_viewer::project_sequence_item_ids(&self.project);
            let unavailable_blocks =
                source_conflicts(&self.project.source, &self.project.extra_sources);
            crate::app::sync_endpoint_wires_with_policy(
                &self.project.graph,
                &self.project.root,
                &source_blocks,
                &target_blocks,
                &self.main_canvas.endpoint_scroll,
                &mut self.main_canvas.snarl,
                SourcePinPolicy {
                    owned_items: &owned_items,
                    unavailable_blocks: &unavailable_blocks,
                },
            );
            let mut viewer = GraphViewer {
                graph: &mut self.project.graph,
                root_scope: &mut self.project.root,
                primary_root_authoring,
                extra_targets: &self.project.extra_targets,
                inactive_target_scopes: &[],
                project_references: crate::graph_viewer::ProjectGraphReferences::new(
                    &self.project.failure_rules,
                    &self.project.extra_sources,
                )
                .with_user_functions(&self.project.user_functions)
                .with_primary_source(&self.project.source),
                source_blocks: &source_blocks,
                target_blocks: &target_blocks,
                source_x12,
                target_x12,
                source_paths: &source_paths,
                function_names: function_names.clone(),
                function_inputs,
                parameter_names,
                protected_output: None,
                function_output: None,
                requested_function_open: None,
                colors: self.appearance.resolved_colors(self.palette),
                wire_color_mode: self.appearance.wire().color_mode(),
                endpoint_scroll: &mut self.main_canvas.endpoint_scroll,
                value_map_wheel: None,
                endpoint_search_match: None,
                node_sizes: Some(&mut self.main_canvas.node_sizes),
                hovered_node: None,
                hovered_node_this_frame: None,
                camera_pan: egui::Vec2::ZERO,
                camera_focus: None,
                canvas_transform: None,
                pin_interaction_ids: Vec::new(),
                planned_edits: Vec::new(),
                error: None,
            };
            let interaction = crate::canvas_keyboard::show(
                &mut self.main_canvas.snarl,
                &mut viewer,
                &mut self.main_canvas.search,
                crate::canvas_keyboard::CanvasOptions {
                    tools_id: self.main_canvas.tools_id,
                    density: self.appearance.density(),
                    wire: *self.appearance.wire(),
                    request_compact: std::mem::take(&mut self.main_canvas.pending_compact),
                    id_salt: canvas_id,
                    show_minimap: self.show_minimap,
                    view_generation: self.main_canvas.view_generation,
                    style: self.appearance.to_snarl_style_with_palette(self.palette),
                    focus: self.main_canvas.pending_focus.take(),
                },
                ui,
            );
            self.main_canvas.viewport_width = interaction.viewport_width;
            if interaction.focused {
                self.mapping_workspace.focused = MappingDocument::Main;
            }
            if let Some(error) = viewer.error {
                self.status = "graph edit failed".to_string();
                self.diagnostics.error("Graph edit failed", error);
            }
            #[cfg(test)]
            if self.embedded_stage_namespace.is_some() {
                self.embedded_stage_pin_ids = crate::canvas_keyboard::current_pin_interaction_ids(
                    ui.ctx(),
                    canvas_id,
                    self.main_canvas.view_generation,
                );
            }
            requested_function = viewer.requested_function_open;
            planned_edits = viewer.planned_edits;
        });
        self.queue_canvas_edits(MappingDocument::Main, planned_edits);
        if let Some(node) = sequence_consumer_duplication::take(ui.ctx()) {
            self.apply_sequence_duplication_request(MappingDocument::Main, node);
        }
        if let Some(function) = requested_function {
            self.open_function_tab(function);
        }
    }
}

#[cfg(test)]
#[path = "workspace/inspector_lock_tests.rs"]
mod inspector_lock_tests;

#[cfg(test)]
#[path = "fit_tests.rs"]
mod fit_tests;
