use super::*;

#[path = "functions/calls.rs"]
mod calls;

#[cfg(test)]
use calls::reserve_call_ids;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{FunctionParameter, FunctionParameterId, Node, UserFunction};

#[derive(Clone, Debug)]
pub(super) struct NewFunctionDraft {
    library: String,
    name: String,
    description: String,
    parameters: Vec<ParameterDraft>,
    output_name: String,
    output_type: ScalarType,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct ParameterDraft {
    name: String,
    ty: ScalarType,
}

impl Default for NewFunctionDraft {
    fn default() -> Self {
        Self {
            library: "local".to_string(),
            name: String::new(),
            description: String::new(),
            parameters: Vec::new(),
            output_name: "result".to_string(),
            output_type: ScalarType::String,
            error: None,
        }
    }
}

enum TabAction {
    Activate(MappingDocument),
    Close(MappingDocument),
    Split(FunctionId),
    Float(FunctionId),
}

impl FerruleApp {
    pub(super) fn function_names(&self) -> std::collections::BTreeMap<FunctionId, String> {
        self.project
            .user_functions
            .iter()
            .map(|(&id, function)| (id, function_label(function)))
            .collect()
    }

    pub(super) fn function_inputs(&self) -> std::collections::BTreeMap<FunctionId, Vec<String>> {
        self.project
            .user_functions
            .iter()
            .map(|(&id, function)| {
                (
                    id,
                    function
                        .parameters
                        .iter()
                        .map(|parameter| parameter.name.clone())
                        .collect(),
                )
            })
            .collect()
    }

    pub(super) fn open_function_tab(&mut self, function: FunctionId) {
        if !self.project.user_functions.contains_key(&function) {
            self.diagnostics.error(
                "Open function failed",
                format!("function {} does not exist", function.get()),
            );
            return;
        }
        self.mapping_workspace.floating.remove(&function);
        if self.mapping_workspace.split == Some(MappingDocument::Function(function)) {
            self.mapping_workspace.split = None;
        }
        let document = MappingDocument::Function(function);
        if !self.mapping_workspace.tabs.contains(&document) {
            self.mapping_workspace.tabs.push(document);
        }
        self.mapping_workspace.active = document;
        self.mapping_workspace.focused = document;
    }

    pub(super) fn open_target_tab(&mut self, target: usize) {
        if target >= self.project.extra_targets.len() {
            self.diagnostics.error(
                "Open target failed",
                format!("target position {} does not exist", target + 1),
            );
            return;
        }
        let document = MappingDocument::Target(target);
        if !self.mapping_workspace.tabs.contains(&document) {
            self.mapping_workspace.tabs.push(document);
        }
        self.mapping_workspace.active = document;
        self.mapping_workspace.focused = document;
        self.selected_scope.clear();
    }

    fn open_function_split(&mut self, function: FunctionId) {
        if !self.project.user_functions.contains_key(&function) {
            return;
        }
        self.mapping_workspace.floating.remove(&function);
        self.mapping_workspace
            .tabs
            .retain(|document| *document != MappingDocument::Function(function));
        if self.mapping_workspace.active == MappingDocument::Function(function) {
            self.mapping_workspace.active = MappingDocument::Main;
        }
        self.mapping_workspace.split = Some(MappingDocument::Function(function));
        self.mapping_workspace.focused = MappingDocument::Function(function);
    }

    fn float_function(&mut self, function: FunctionId) {
        if !self.project.user_functions.contains_key(&function) {
            return;
        }
        self.mapping_workspace
            .tabs
            .retain(|document| *document != MappingDocument::Function(function));
        if self.mapping_workspace.active == MappingDocument::Function(function) {
            self.mapping_workspace.active = MappingDocument::Main;
        }
        if self.mapping_workspace.split == Some(MappingDocument::Function(function)) {
            self.mapping_workspace.split = None;
        }
        self.mapping_workspace.floating.insert(function);
        self.mapping_workspace.focused = MappingDocument::Function(function);
    }

    fn close_function_view(&mut self, function: FunctionId) {
        let document = MappingDocument::Function(function);
        self.mapping_workspace
            .tabs
            .retain(|candidate| *candidate != document);
        if self.mapping_workspace.active == document {
            self.mapping_workspace.active = MappingDocument::Main;
        }
        if self.mapping_workspace.focused == document {
            self.mapping_workspace.focused = self.mapping_workspace.active;
        }
        if self.mapping_workspace.split == Some(document) {
            self.mapping_workspace.split = None;
        }
        self.mapping_workspace.floating.remove(&function);
    }

    fn close_mapping_view(&mut self, document: MappingDocument) {
        match document {
            MappingDocument::Function(function) => self.close_function_view(function),
            MappingDocument::Target(_) => {
                self.mapping_workspace
                    .tabs
                    .retain(|candidate| *candidate != document);
                if self.mapping_workspace.active == document {
                    self.mapping_workspace.active = MappingDocument::Main;
                }
                if self.mapping_workspace.focused == document {
                    self.mapping_workspace.focused = self.mapping_workspace.active;
                }
                if self.mapping_workspace.split == Some(document) {
                    self.mapping_workspace.split = None;
                }
                self.selected_scope.clear();
            }
            MappingDocument::Main => {}
        }
    }

    pub(super) fn show_mapping_tabs(&mut self, ui: &mut egui::Ui, editing_enabled: bool) {
        self.mapping_workspace.reconcile(
            &self.project.user_functions,
            self.project.extra_targets.len(),
        );
        let labels = self.function_names();
        let tabs = self.mapping_workspace.tabs.clone();
        let mut action = None;
        crate::theme::top_bar_row(ui, 28.0, |ui| {
            ui.horizontal_wrapped(|ui| {
                for document in tabs {
                    let label = match document {
                        MappingDocument::Main => "Main Mapping",
                        MappingDocument::Target(index) => self
                            .project
                            .extra_targets
                            .get(index)
                            .map(|target| target.name.as_str())
                            .unwrap_or("Missing target"),
                        MappingDocument::Function(id) => labels
                            .get(&id)
                            .map(String::as_str)
                            .unwrap_or("Missing function"),
                    };
                    let response =
                        ui.selectable_label(self.mapping_workspace.active == document, label);
                    if response.clicked() {
                        action = Some(TabAction::Activate(document));
                    }
                    if let MappingDocument::Function(id) = document {
                        response.context_menu(|ui| {
                            if ui.button("Open to side").clicked() {
                                action = Some(TabAction::Split(id));
                                ui.close();
                            }
                            if self.embedded_stage_namespace.is_none()
                                && ui.button("Float").clicked()
                            {
                                action = Some(TabAction::Float(id));
                                ui.close();
                            }
                            if ui.button("Close view").clicked() {
                                action = Some(TabAction::Close(document));
                                ui.close();
                            }
                        });
                    }
                    if !matches!(document, MappingDocument::Main)
                        && ui
                            .add(egui::Button::new(crate::icons::text(
                                lucide_icons::Icon::X,
                                11.0,
                            )))
                            .on_hover_text("Close mapping view")
                            .clicked()
                    {
                        action = Some(TabAction::Close(document));
                    }
                }
                ui.separator();
                ui.add_enabled_ui(
                    editing_enabled && self.embedded_stage_namespace.is_none(),
                    |ui| {
                        if crate::icons::button(
                            ui,
                            true,
                            lucide_icons::Icon::Library,
                            "Function navigator",
                        )
                        .clicked()
                        {
                            self.show_function_navigator = true;
                        }
                        if crate::icons::button(ui, true, lucide_icons::Icon::Plus, "New function")
                            .clicked()
                        {
                            self.new_function_draft = Some(NewFunctionDraft::default());
                        }
                    },
                );
            });
        });
        match action {
            Some(TabAction::Activate(document)) => {
                self.mapping_workspace.active = document;
                self.mapping_workspace.focused = document;
            }
            Some(TabAction::Close(document)) => self.close_mapping_view(document),
            Some(TabAction::Split(function)) => self.open_function_split(function),
            Some(TabAction::Float(function)) => self.float_function(function),
            None => {}
        }
    }

    pub(super) fn show_mapping_workspace_canvas(
        &mut self,
        ui: &mut egui::Ui,
        editing_enabled: bool,
    ) {
        self.mapping_workspace.reconcile(
            &self.project.user_functions,
            self.project.extra_targets.len(),
        );
        let active = self.mapping_workspace.active;
        let split = self.mapping_workspace.split;
        if let Some(split) = split {
            ui.horizontal(|ui| {
                ui.weak("Split");
                ui.selectable_value(
                    &mut self.mapping_workspace.split_orientation,
                    SplitOrientation::Horizontal,
                    "Side by side",
                );
                ui.selectable_value(
                    &mut self.mapping_workspace.split_orientation,
                    SplitOrientation::Vertical,
                    "Stacked",
                );
                if ui.button("Close split").clicked() {
                    self.mapping_workspace.split = None;
                }
            });
            match self.mapping_workspace.split_orientation {
                SplitOrientation::Horizontal => {
                    ui.columns(2, |columns| {
                        self.show_mapping_document(active, &mut columns[0], editing_enabled);
                        self.show_mapping_document(split, &mut columns[1], editing_enabled);
                    });
                }
                SplitOrientation::Vertical => {
                    let height = (ui.available_height() - ui.spacing().item_spacing.y).max(0.0);
                    let half = height / 2.0;
                    ui.allocate_ui(egui::vec2(ui.available_width(), half), |ui| {
                        self.show_mapping_document(active, ui, editing_enabled);
                    });
                    ui.separator();
                    ui.allocate_ui(egui::vec2(ui.available_width(), half), |ui| {
                        self.show_mapping_document(split, ui, editing_enabled);
                    });
                }
            }
        } else {
            self.show_mapping_document(active, ui, editing_enabled);
        }
    }

    fn show_mapping_document(
        &mut self,
        document: MappingDocument,
        ui: &mut egui::Ui,
        editing_enabled: bool,
    ) {
        match document {
            MappingDocument::Main => self.show_main_canvas(ui, editing_enabled),
            MappingDocument::Target(target) => {
                self.show_named_target_canvas(target, ui, editing_enabled)
            }
            MappingDocument::Function(function) => {
                self.show_function_canvas(function, ui, editing_enabled)
            }
        }
    }

    pub(super) fn ensure_target_canvas(&mut self, target: usize) -> bool {
        if self.mapping_workspace.target_canvases.contains_key(&target) {
            return true;
        }
        if target >= self.project.extra_targets.len() {
            return false;
        }
        let canvas = CanvasDocumentState::with_snarl(canvas_build::build_named_target_snarl(
            &self.project,
            target,
        ));
        self.mapping_workspace
            .target_canvases
            .insert(target, canvas);
        true
    }

    fn show_named_target_canvas(
        &mut self,
        target_index: usize,
        ui: &mut egui::Ui,
        editing_enabled: bool,
    ) {
        let canvas_id =
            self.embedded_canvas_id(egui::Id::unique(("named_target_canvas", target_index)));
        if !self.ensure_target_canvas(target_index) {
            ui.colored_label(self.palette.error, "Target definition is missing");
            return;
        }
        let function_names = self.function_names();
        let function_inputs = self.function_inputs();
        let source_paths =
            SourcePathCatalog::new(&self.project.source, &self.project.extra_sources)
                .with_primary_source_options(&self.project.source_options)
                .with_project_ownership(&self.project);
        let source_x12 = crate::x12_tooltips::boundary_has_x12(
            &self.project.source,
            self.project.source_path.as_deref(),
            &self.project.source_options,
        );
        let source_blocks = project_source_blocks(&self.project);
        let owned_items = crate::graph_viewer::project_sequence_item_ids(&self.project);
        let unavailable_blocks =
            source_conflicts(&self.project.source, &self.project.extra_sources);
        let mut requested_function = None;
        let mut error = None;
        sequence_consumer_duplication::begin(ui.ctx());
        let mut planned_edits = Vec::new();
        ui.add_enabled_ui(editing_enabled, |ui| {
            let (targets, canvases) = (
                &mut self.project.extra_targets,
                &mut self.mapping_workspace.target_canvases,
            );
            if target_index >= targets.len() {
                return;
            }
            let (before, active_and_after) = targets.split_at_mut(target_index);
            let Some((target, after)) = active_and_after.split_first_mut() else {
                return;
            };
            let inactive_target_scopes =
                crate::graph_viewer::inactive_target_scopes(&self.project.root, before, after);
            let Some(canvas) = canvases.get_mut(&target_index) else {
                return;
            };
            let target_blocks = target_blocks(&target.schema);
            let target_x12 = crate::x12_tooltips::boundary_has_x12(
                &target.schema,
                target.path.as_deref(),
                &target.options,
            );
            crate::app::sync_endpoint_wires_with_policy(
                &self.project.graph,
                &target.root,
                &source_blocks,
                &target_blocks,
                &canvas.endpoint_scroll,
                &mut canvas.snarl,
                SourcePinPolicy {
                    owned_items: &owned_items,
                    unavailable_blocks: &unavailable_blocks,
                },
            );
            let mut viewer = GraphViewer {
                graph: &mut self.project.graph,
                root_scope: &mut target.root,
                primary_root_authoring: false,
                extra_targets: &[],
                inactive_target_scopes: &inactive_target_scopes,
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
                function_names,
                function_inputs,
                parameter_names: std::collections::BTreeMap::new(),
                protected_output: None,
                function_output: None,
                requested_function_open: None,
                colors: self.appearance.resolved_colors(self.palette),
                wire_color_mode: self.appearance.wire().color_mode(),
                endpoint_scroll: &mut canvas.endpoint_scroll,
                value_map_wheel: None,
                endpoint_search_match: None,
                node_sizes: Some(&mut canvas.node_sizes),
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
                &mut canvas.snarl,
                &mut viewer,
                &mut canvas.search,
                crate::canvas_keyboard::CanvasOptions {
                    tools_id: canvas.tools_id,
                    density: self.appearance.density(),
                    wire: *self.appearance.wire(),
                    request_compact: std::mem::take(&mut canvas.pending_compact),
                    id_salt: canvas_id,
                    show_minimap: self.show_minimap,
                    view_generation: canvas.view_generation,
                    style: self.appearance.to_snarl_style_with_palette(self.palette),
                    focus: canvas.pending_focus.take(),
                },
                ui,
            );
            canvas.viewport_width = interaction.viewport_width;
            if interaction.focused {
                self.mapping_workspace.focused = MappingDocument::Target(target_index);
            }
            requested_function = viewer.requested_function_open;
            error = viewer.error;
            planned_edits = viewer.planned_edits;
        });
        self.queue_canvas_edits(MappingDocument::Target(target_index), planned_edits);
        if let Some(node) = sequence_consumer_duplication::take(ui.ctx()) {
            self.apply_sequence_duplication_request(MappingDocument::Target(target_index), node);
        }
        if let Some(error) = error {
            self.status = "target graph edit failed".to_string();
            self.diagnostics.error("Target edit failed", error);
        }
        if let Some(function) = requested_function {
            self.open_function_tab(function);
        }
    }

    pub(super) fn ensure_function_canvas(&mut self, function: FunctionId) -> bool {
        if self
            .mapping_workspace
            .function_canvases
            .contains_key(&function)
        {
            return true;
        }
        let Some(definition) = self.project.user_functions.get(&function) else {
            return false;
        };
        let canvas = CanvasDocumentState::with_snarl(build_function_snarl(definition));
        self.mapping_workspace
            .function_canvases
            .insert(function, canvas);
        true
    }

    fn show_function_canvas(
        &mut self,
        function_id: FunctionId,
        ui: &mut egui::Ui,
        editing_enabled: bool,
    ) {
        let canvas_id = self.embedded_canvas_id(egui::Id::unique((
            "function_mapping_canvas",
            function_id.get(),
        )));
        if !self.ensure_function_canvas(function_id) {
            ui.colored_label(self.palette.error, "Function definition is missing");
            return;
        }
        let function_names = self.function_names();
        let function_inputs = self.function_inputs();
        let (parameter_names, output) = self
            .project
            .user_functions
            .get(&function_id)
            .map(|function| {
                (
                    function
                        .parameters
                        .iter()
                        .map(|parameter| (parameter.id, parameter.name.clone()))
                        .collect::<std::collections::BTreeMap<_, _>>(),
                    function.output,
                )
            })
            .unwrap_or_default();
        let source = SchemaNode::group("function", Vec::new());
        let source_paths = SourcePathCatalog::new(&source, &[]);
        let source_blocks = Vec::new();
        let target_blocks = Vec::new();
        let mut root = Scope::default();
        let mut requested = None;
        let mut error = None;
        let mut planned_edits = Vec::new();
        ui.add_enabled_ui(editing_enabled, |ui| {
            let (functions, canvases) = (
                &mut self.project.user_functions,
                &mut self.mapping_workspace.function_canvases,
            );
            let Some(function) = functions.get_mut(&function_id) else {
                return;
            };
            let Some(canvas) = canvases.get_mut(&function_id) else {
                return;
            };
            let mut viewer = GraphViewer {
                graph: &mut function.body,
                root_scope: &mut root,
                primary_root_authoring: false,
                extra_targets: &[],
                inactive_target_scopes: &[],
                project_references: Default::default(),
                source_blocks: &source_blocks,
                target_blocks: &target_blocks,
                source_x12: false,
                target_x12: false,
                source_paths: &source_paths,
                function_names: function_names.clone(),
                function_inputs: function_inputs.clone(),
                parameter_names: parameter_names.clone(),
                protected_output: Some(output),
                function_output: Some(&mut function.output),
                requested_function_open: None,
                colors: self.appearance.resolved_colors(self.palette),
                wire_color_mode: self.appearance.wire().color_mode(),
                endpoint_scroll: &mut canvas.endpoint_scroll,
                value_map_wheel: None,
                endpoint_search_match: None,
                node_sizes: Some(&mut canvas.node_sizes),
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
                &mut canvas.snarl,
                &mut viewer,
                &mut canvas.search,
                crate::canvas_keyboard::CanvasOptions {
                    tools_id: canvas.tools_id,
                    density: self.appearance.density(),
                    wire: *self.appearance.wire(),
                    request_compact: std::mem::take(&mut canvas.pending_compact),
                    id_salt: canvas_id,
                    show_minimap: self.show_minimap,
                    view_generation: canvas.view_generation,
                    style: self.appearance.to_snarl_style_with_palette(self.palette),
                    focus: canvas.pending_focus.take(),
                },
                ui,
            );
            canvas.viewport_width = interaction.viewport_width;
            if interaction.focused {
                self.mapping_workspace.focused = MappingDocument::Function(function_id);
            }
            requested = viewer.requested_function_open;
            error = viewer.error;
            planned_edits = viewer.planned_edits;
        });
        self.queue_canvas_edits(MappingDocument::Function(function_id), planned_edits);
        if let Some(error) = error {
            self.status = "function graph edit failed".to_string();
            self.diagnostics.error("Function edit failed", error);
        }
        if let Some(function) = requested {
            self.open_function_tab(function);
        }
    }

    pub(super) fn show_floating_function_windows(
        &mut self,
        ctx: &egui::Context,
        editing_enabled: bool,
    ) {
        let floating = self
            .mapping_workspace
            .floating
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for function in floating {
            let title = self
                .project
                .user_functions
                .get(&function)
                .map(function_label)
                .unwrap_or_else(|| "Missing function".to_string());
            let viewport = egui::ViewportId::from_hash_of(("ferrule-function", function.get()));
            let mut dock = false;
            let mut close = false;
            ctx.show_viewport_immediate(
                viewport,
                egui::ViewportBuilder::default()
                    .with_title(format!("{title} - ferrule"))
                    .with_inner_size([900.0, 650.0]),
                |ui, _class| {
                    close = ui.ctx().input(|input| input.viewport().close_requested());
                    ui.horizontal(|ui| {
                        ui.strong(&title);
                        if ui.button("Arrange").clicked() {
                            self.mapping_workspace.focused = MappingDocument::Function(function);
                            self.arrange_document(
                                MappingDocument::Function(function),
                                ArrangeMode::LeftToRight,
                            );
                        }
                        if ui.button("Arrange compact").clicked() {
                            self.mapping_workspace.focused = MappingDocument::Function(function);
                            self.arrange_document(
                                MappingDocument::Function(function),
                                ArrangeMode::Compact,
                            );
                        }
                        if ui.button("Fit").clicked() {
                            self.mapping_workspace.focused = MappingDocument::Function(function);
                            self.fit_document(MappingDocument::Function(function));
                        }
                        if ui.button("Dock").clicked() {
                            dock = true;
                        }
                    });
                    ui.separator();
                    self.show_function_canvas(function, ui, editing_enabled);
                },
            );
            if dock {
                self.open_function_tab(function);
            } else if close {
                self.close_function_view(function);
            }
        }
    }

    pub(super) fn show_function_navigator(&mut self, ctx: &egui::Context, editing_enabled: bool) {
        if !self.show_function_navigator {
            return;
        }
        let functions = self
            .project
            .user_functions
            .iter()
            .map(|(&id, function)| (id, function_label(function)))
            .collect::<Vec<_>>();
        let mut action = None;
        let mut navigator_open = self.show_function_navigator;
        egui::Window::new("Functions")
            .open(&mut navigator_open)
            .default_size([360.0, 420.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.function_search)
                        .hint_text("Search functions")
                        .desired_width(f32::INFINITY),
                );
                if ui
                    .add_enabled(editing_enabled, egui::Button::new("New function"))
                    .clicked()
                {
                    self.new_function_draft = Some(NewFunctionDraft::default());
                }
                ui.separator();
                let query = self.function_search.trim().to_ascii_lowercase();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (id, label) in &functions {
                        if !query.is_empty() && !label.to_ascii_lowercase().contains(&query) {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            if ui.selectable_label(false, label).clicked() {
                                action = Some(TabAction::Activate(MappingDocument::Function(*id)));
                            }
                            if ui.small_button("Side").clicked() {
                                action = Some(TabAction::Split(*id));
                            }
                            if ui.small_button("Float").clicked() {
                                action = Some(TabAction::Float(*id));
                            }
                            if ui
                                .add_enabled(editing_enabled, egui::Button::new("Add call").small())
                                .clicked()
                            {
                                self.insert_function_call(*id);
                            }
                        });
                    }
                    if functions.is_empty() {
                        ui.weak("No user-defined functions");
                    }
                });
            });
        self.show_function_navigator = navigator_open;
        match action {
            Some(TabAction::Activate(MappingDocument::Function(function))) => {
                self.open_function_tab(function)
            }
            Some(TabAction::Split(function)) => self.open_function_split(function),
            Some(TabAction::Float(function)) => self.float_function(function),
            _ => {}
        }
    }

    pub(super) fn show_new_function_dialog(&mut self, ctx: &egui::Context, editing_enabled: bool) {
        let Some(mut draft) = self.new_function_draft.take() else {
            return;
        };
        let mut keep_open = true;
        let mut create = false;
        egui::Window::new("New user-defined function")
            .collapsible(false)
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.set_min_width(400.0);
                egui::Grid::new("new_function_identity").show(ui, |ui| {
                    ui.label("Library");
                    ui.add_sized(
                        [280.0, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut draft.library),
                    );
                    ui.end_row();
                    ui.label("Name");
                    ui.add_sized(
                        [280.0, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut draft.name),
                    );
                    ui.end_row();
                    ui.label("Description");
                    ui.add_sized(
                        [280.0, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut draft.description),
                    );
                    ui.end_row();
                });
                ui.separator();
                ui.strong("Inputs");
                let mut remove = None;
                for (index, parameter) in draft.parameters.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut parameter.name);
                        scalar_type_picker(
                            ui,
                            ui.scope_id().with(("parameter", index)),
                            &mut parameter.ty,
                        );
                        if ui.small_button("Remove").clicked() {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    draft.parameters.remove(index);
                }
                if ui.button("Add input").clicked() {
                    draft.parameters.push(ParameterDraft {
                        name: format!("input{}", draft.parameters.len() + 1),
                        ty: ScalarType::String,
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Output");
                    ui.text_edit_singleline(&mut draft.output_name);
                    scalar_type_picker(ui, ui.scope_id().with("output"), &mut draft.output_type);
                });
                if let Some(error) = &draft.error {
                    ui.colored_label(self.palette.error, error);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(editing_enabled, egui::Button::new("Create"))
                        .clicked()
                    {
                        create = true;
                    }
                    if ui.button("Cancel").clicked() {
                        keep_open = false;
                    }
                });
            });
        if create {
            match self.create_function(&draft) {
                Ok(function) => {
                    keep_open = false;
                    self.open_function_tab(function);
                }
                Err(error) => draft.error = Some(error),
            }
        }
        if keep_open {
            self.new_function_draft = Some(draft);
        }
    }

    fn create_function(&mut self, draft: &NewFunctionDraft) -> Result<FunctionId, String> {
        let name = draft.name.trim().to_string();
        if name.is_empty() {
            return Err("Function name is required".to_string());
        }
        let library = draft.library.trim().to_string();
        if self
            .project
            .user_functions
            .values()
            .any(|function| function.name == name && function.library == library)
        {
            return Err("Library and function name must be unique".to_string());
        }
        if draft
            .parameters
            .iter()
            .any(|parameter| parameter.name.trim().is_empty())
        {
            return Err("Every input needs a name".to_string());
        }
        let unique = draft
            .parameters
            .iter()
            .map(|parameter| parameter.name.trim())
            .collect::<std::collections::BTreeSet<_>>();
        if unique.len() != draft.parameters.len() {
            return Err("Input names must be unique".to_string());
        }
        let next = self
            .project
            .user_functions
            .keys()
            .next_back()
            .map_or(Some(1), |id| id.get().checked_add(1));
        let Some(next) = next else {
            return Err("No function identifiers remain".to_string());
        };
        let function_id = FunctionId::new(next);
        let mut nodes = std::collections::BTreeMap::new();
        let parameters = draft
            .parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                let id = FunctionParameterId::new(index as u64 + 1);
                nodes.insert(index as NodeId, Node::FunctionParameter { parameter: id });
                FunctionParameter {
                    id,
                    name: parameter.name.trim().to_string(),
                    ty: parameter.ty,
                }
            })
            .collect::<Vec<_>>();
        let output = parameters.len() as NodeId;
        nodes.insert(output, Node::Const { value: Value::Null });
        let before = self.project.clone();
        self.project.user_functions.insert(
            function_id,
            UserFunction {
                library,
                name,
                description: (!draft.description.trim().is_empty())
                    .then(|| draft.description.trim().to_string()),
                parameters,
                output_name: if draft.output_name.trim().is_empty() {
                    "result".to_string()
                } else {
                    draft.output_name.trim().to_string()
                },
                output_type: draft.output_type,
                body: Graph { nodes },
                output,
            },
        );
        self.queue_project_edit(
            &before,
            editor_ui::GraphAction::CreateFunction {
                function: function_id,
            },
        );
        Ok(function_id)
    }

    fn insert_function_call(&mut self, function: FunctionId) {
        if let Err(error) = self.try_insert_function_call(function) {
            self.status = "function call edit failed".to_string();
            self.diagnostics.error("Add function call failed", error);
        }
    }
}

fn function_label(function: &UserFunction) -> String {
    if function.library.trim().is_empty() {
        function.name.clone()
    } else {
        format!("{}:{}", function.library, function.name)
    }
}

fn scalar_type_picker(ui: &mut egui::Ui, id: egui::Id, ty: &mut ScalarType) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(scalar_type_label(*ty))
        .show_ui(ui, |ui| {
            for candidate in [
                ScalarType::String,
                ScalarType::Int,
                ScalarType::Float,
                ScalarType::Bool,
            ] {
                ui.selectable_value(ty, candidate, scalar_type_label(candidate));
            }
        });
}

fn scalar_type_label(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
    }
}

#[cfg(test)]
#[path = "functions/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "functions/output_tests.rs"]
mod output_tests;

#[cfg(test)]
#[path = "functions/lock_tests.rs"]
mod lock_tests;

#[cfg(test)]
#[path = "functions/allocation_tests.rs"]
mod allocation_tests;
