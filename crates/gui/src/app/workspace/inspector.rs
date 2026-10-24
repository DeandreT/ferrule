use super::*;

impl FerruleApp {
    pub(in crate::app) fn show_source_explorer(
        &mut self,
        ui: &mut egui::Ui,
        editing_enabled: bool,
    ) {
        let editing_enabled = editing_enabled && self.embedded_stage_namespace.is_none();
        let source_x12 = crate::x12_tooltips::boundary_has_x12(
            &self.project.source,
            self.project.source_path.as_deref(),
            &self.project.source_options,
        );
        ui.horizontal(|ui| {
            ui.strong("Source schema");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        editing_enabled,
                        egui::Button::new(crate::icons::text(
                            lucide_icons::Icon::CirclePlus,
                            crate::theme::METRICS.icon_size,
                        )),
                    )
                    .on_hover_text("Add source")
                    .clicked()
                {
                    self.begin_extra_source();
                }
            });
        });
        let options_before = crate::graph_viewer::frame_has_edit_input(ui.ctx())
            .then(|| self.project.source_options.clone());
        super::primary_xml_input::show(
            ui,
            &self.project.source,
            self.project.source_path.as_deref(),
            &mut self.project.source_options,
            editing_enabled,
        );
        if let Some(options) = options_before
            && options != self.project.source_options
        {
            let mut before = self.project.clone();
            before.source_options = options;
            self.queue_project_edit(&before, editor_ui::GraphAction::EditSourceOptions);
        }
        show_schema_search_input(ui, "source_schema_search", &mut self.source_schema_explorer);
        let source_matches = self
            .source_schema_explorer
            .match_count(&self.project.source)
            + self
                .project
                .extra_sources
                .iter()
                .map(|source| self.source_schema_explorer.match_count(&source.schema))
                .sum::<usize>();
        let source_fields = schema_field_count(&self.project.source)
            + self
                .project
                .extra_sources
                .iter()
                .map(|source| schema_field_count(&source.schema))
                .sum::<usize>();
        show_schema_result_count(
            ui,
            &self.source_schema_explorer,
            source_matches,
            source_fields,
        );
        let mut remove = None;
        let focused_schema = self.focused_schema.clone();
        egui::ScrollArea::both()
            .id_salt("source_schema_scroll")
            .show(ui, |ui| {
                if source_matches == 0 && self.source_schema_explorer.is_filtering() {
                    ui.weak("No matching fields or groups");
                } else {
                    let mut section_shown = show_schema_tree(
                        ui,
                        &self.project.source,
                        &self.source_schema_explorer,
                        "primary_source_schema",
                        source_x12,
                        focused_schema
                            .as_ref()
                            .filter(|focus| matches!(focus.endpoint, engine::ValidationEndpoint::Source))
                            .map(|focus| (focus.visible_path.as_slice(), focus.pending_scroll)),
                    );
                    for (index, extra) in self.project.extra_sources.iter().enumerate() {
                        if self.source_schema_explorer.is_filtering()
                            && self.source_schema_explorer.match_count(&extra.schema) == 0
                        {
                            continue;
                        }
                        if section_shown {
                            ui.separator();
                        }
                        ui.horizontal(|ui| {
                            ui.strong(format!("Extra: {}", extra.name));
                            if ui
                                .add_enabled(
                                    editing_enabled,
                                    egui::Button::new(crate::icons::text(
                                        lucide_icons::Icon::Trash2,
                                        crate::theme::METRICS.icon_size,
                                    ))
                                    .small(),
                                )
                                .on_hover_text("Remove source")
                                .clicked()
                            {
                                remove = Some(index);
                            }
                        });
                        show_schema_tree(
                            ui,
                            &extra.schema,
                            &self.source_schema_explorer,
                            ("extra_source_schema", index),
                            crate::x12_tooltips::boundary_has_x12(
                                &extra.schema,
                                Some(&extra.path),
                                &extra.options,
                            ),
                            focused_schema.as_ref().filter(|focus| {
                                matches!(&focus.endpoint, engine::ValidationEndpoint::NamedSource { index: wanted, name }
                                    if *wanted == index && name == &extra.name)
                            }).map(|focus| (focus.visible_path.as_slice(), focus.pending_scroll)),
                        );
                        section_shown = true;
                    }
                }
            });
        if let Some(focus) = self.focused_schema.as_mut()
            && matches!(
                focus.endpoint,
                engine::ValidationEndpoint::Source | engine::ValidationEndpoint::NamedSource { .. }
            )
        {
            focus.pending_scroll = false;
        }
        if let Some(index) = remove {
            self.pending_extra_source_removal = Some(index);
        }
    }

    pub(in crate::app) fn show_inspector(&mut self, ui: &mut egui::Ui, editing_enabled: bool) {
        let boundary_editing = editing_enabled && self.embedded_stage_namespace.is_none();
        let mut activate = None;
        let active_target = match self.mapping_workspace.active {
            MappingDocument::Target(index) => Some(index),
            MappingDocument::Main | MappingDocument::Function(_) => None,
        };
        ui.horizontal(|ui| {
            ui.strong("Target");
            egui::ComboBox::from_id_salt("active_target")
                .selected_text(active_target.map_or("Primary", |index| {
                    self.project
                        .extra_targets
                        .get(index)
                        .map(|target| target.name.as_str())
                        .unwrap_or("Missing target")
                }))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(active_target.is_none(), "Primary")
                        .clicked()
                    {
                        activate = Some(MappingDocument::Main);
                    }
                    for (index, target) in self.project.extra_targets.iter().enumerate() {
                        if ui
                            .selectable_label(active_target == Some(index), &target.name)
                            .clicked()
                        {
                            activate = Some(MappingDocument::Target(index));
                        }
                    }
                });
            if crate::icons::button(
                ui,
                boundary_editing,
                lucide_icons::Icon::CirclePlus,
                "Add target",
            )
            .clicked()
            {
                self.begin_extra_target();
            }
            if let Some(index) = active_target {
                if crate::icons::button(
                    ui,
                    boundary_editing,
                    lucide_icons::Icon::Pencil,
                    "Edit target",
                )
                .clicked()
                {
                    self.edit_extra_target(index);
                }
                if crate::icons::button(
                    ui,
                    boundary_editing,
                    lucide_icons::Icon::Trash2,
                    "Remove target",
                )
                .clicked()
                {
                    self.pending_extra_target_removal = Some(index);
                }
            }
        });
        if let Some(document) = activate {
            match document {
                MappingDocument::Main => {
                    self.mapping_workspace.active = MappingDocument::Main;
                    self.mapping_workspace.focused = MappingDocument::Main;
                    self.selected_scope.clear();
                }
                MappingDocument::Target(index) => self.open_target_tab(index),
                MappingDocument::Function(_) => {}
            }
        }
        self.show_failure_rules(ui, editing_enabled);
        let active_target = match self.mapping_workspace.active {
            MappingDocument::Target(index) => Some(index),
            MappingDocument::Main | MappingDocument::Function(_) => None,
        };
        ui.add_enabled_ui(editing_enabled, |ui| {
            let (schema, root, path, options) = active_target
                .and_then(|index| self.project.extra_targets.get(index))
                .map_or(
                    (
                        &self.project.target,
                        &self.project.root,
                        self.project.target_path.as_deref(),
                        &self.project.target_options,
                    ),
                    |target| {
                        (
                            &target.schema,
                            &target.root,
                            target.path.as_deref(),
                            &target.options,
                        )
                    },
                );
            let target_x12 = crate::x12_tooltips::boundary_has_x12(schema, path, options);
            let focused_schema = self.focused_schema.clone();
            ui.strong("Target schema");
            show_schema_search_input(ui, "target_schema_search", &mut self.target_schema_explorer);
            let target_matches = self.target_schema_explorer.match_count(schema);
            show_schema_result_count(
                ui,
                &self.target_schema_explorer,
                target_matches,
                schema_field_count(schema),
            );
            egui::ScrollArea::both()
                .id_salt(("target_schema_scroll", active_target))
                .max_height(200.0)
                .show(ui, |ui| {
                    if target_matches == 0 && self.target_schema_explorer.is_filtering() {
                        ui.weak("No matching fields or groups");
                    } else {
                        show_schema_tree(
                            ui,
                            schema,
                            &self.target_schema_explorer,
                            ("target_schema", active_target),
                            target_x12,
                            focused_schema
                                .as_ref()
                                .filter(|focus| match (&focus.endpoint, active_target) {
                                    (engine::ValidationEndpoint::Target, None) => true,
                                    (
                                        engine::ValidationEndpoint::NamedTarget {
                                            index: wanted,
                                            name,
                                        },
                                        Some(index),
                                    ) => {
                                        *wanted == index
                                            && self
                                                .project
                                                .extra_targets
                                                .get(index)
                                                .is_some_and(|target| &target.name == name)
                                    }
                                    _ => false,
                                })
                                .map(|focus| (focus.visible_path.as_slice(), focus.pending_scroll)),
                        );
                    }
                });

            ui.separator();
            ui.strong("Scopes");
            egui::ScrollArea::both()
                .id_salt(("scope_tree_scroll", active_target))
                .max_height(200.0)
                .show(ui, |ui| {
                    if let Some(new_selection) =
                        show_scope_tree(ui, root, &self.selected_scope, self.pending_scope_scroll)
                    {
                        self.selected_scope = new_selection;
                    }
                });
        });
        if let Some(focus) = self.focused_schema.as_mut()
            && matches!(
                focus.endpoint,
                engine::ValidationEndpoint::Target | engine::ValidationEndpoint::NamedTarget { .. }
            )
        {
            focus.pending_scroll = false;
        }
        self.pending_scope_scroll = false;
        ui.add_enabled_ui(editing_enabled, |ui| self.show_scope_controls(ui));
        self.show_join_authoring(ui, editing_enabled);

        ui.separator();
        let source_paths =
            SourcePathCatalog::new(&self.project.source, &self.project.extra_sources)
                .with_project_ownership(&self.project);
        let primary_root_bindings = crate::primary_root_authoring::available(&self.project);
        let computed_property_owner = self.embedded_stage_namespace.is_none()
            && matches!(
                self.mapping_workspace.active,
                MappingDocument::Main | MappingDocument::Target(_)
            );
        let mut type_action = None;
        // The edited scope's fields before and after this pass, retained only
        // on frames with input, and the document that owns the scope.
        let mut scope_edit = None;
        let watch_scope = crate::graph_viewer::frame_has_edit_input(ui.ctx());
        egui::ScrollArea::both()
            .id_salt(("scope_editor_scroll", active_target))
            .show(ui, |ui| {
                self.show_filter_map_scope_editor(ui, editing_enabled);
                ui.add_enabled_ui(editing_enabled, |ui| {
                    let nested = !self.selected_scope.is_empty();
                    match active_target {
                        Some(index) => {
                            let Some(target) = self.project.extra_targets.get_mut(index) else {
                                return;
                            };
                            let target_chain =
                                scope_target_chain(&target.root, &self.selected_scope);
                            let target_fields =
                                binding_target_fields(&target.schema, &target_chain);
                            let target_xml_types =
                                crate::target_xml_type::schema_at(&target.schema, &target_chain)
                                    .map(crate::target_xml_type::choices)
                                    .unwrap_or_default();
                            let target_xml_default =
                                crate::target_xml_type::schema_at(&target.schema, &target_chain)
                                    .and_then(|schema| schema.xml_default_type.as_deref())
                                    .filter(|identity| {
                                        target_xml_types
                                            .iter()
                                            .any(|choice| choice.as_str() == *identity)
                                    });
                            let target_type_editable =
                                crate::scope_editor::copied_ancestor_at_path(
                                    &target.root,
                                    &self.selected_scope,
                                )
                                .is_none();
                            let output_profile = crate::scope_editor::output_profile(
                                &target.root,
                                &target.schema,
                                &self.selected_scope,
                                &target.options,
                                target.path.as_deref(),
                            );
                            let computed_property_profile =
                                crate::scope_editor::computed_properties::profile(
                                    &target.root,
                                    &target.schema,
                                    &self.selected_scope,
                                    &target.options,
                                    computed_property_owner,
                                );
                            let scope = scope_at_mut(&mut target.root, &self.selected_scope);
                            let before = watch_scope.then(|| ScopeFields::capture(scope));
                            type_action = show_scope_editor(
                                ui,
                                scope,
                                &self.project.graph,
                                &source_paths,
                                &target_fields,
                                crate::scope_editor::ScopeEditorOwner {
                                    nested,
                                    primary_root_bindings: false,
                                    target_xml_types: &target_xml_types,
                                    target_xml_default,
                                    target_type_editable,
                                },
                                output_profile,
                            );
                            crate::scope_editor::computed_properties::show(
                                ui,
                                scope,
                                &self.project.graph,
                                computed_property_profile,
                            );
                            scope_edit = before.map(|before| {
                                let document =
                                    GraphDocument::Target(TargetId::Named(target.name.clone()));
                                (document, before, ScopeFields::capture(scope))
                            });
                        }
                        None => {
                            let target_chain =
                                scope_target_chain(&self.project.root, &self.selected_scope);
                            let target_fields =
                                binding_target_fields(&self.project.target, &target_chain);
                            let target_xml_types = crate::target_xml_type::schema_at(
                                &self.project.target,
                                &target_chain,
                            )
                            .map(crate::target_xml_type::choices)
                            .unwrap_or_default();
                            let target_xml_default = crate::target_xml_type::schema_at(
                                &self.project.target,
                                &target_chain,
                            )
                            .and_then(|schema| schema.xml_default_type.as_deref())
                            .filter(|identity| {
                                target_xml_types
                                    .iter()
                                    .any(|choice| choice.as_str() == *identity)
                            });
                            let target_type_editable =
                                crate::scope_editor::copied_ancestor_at_path(
                                    &self.project.root,
                                    &self.selected_scope,
                                )
                                .is_none();
                            let output_profile = crate::scope_editor::output_profile(
                                &self.project.root,
                                &self.project.target,
                                &self.selected_scope,
                                &self.project.target_options,
                                self.project.target_path.as_deref(),
                            );
                            let computed_property_profile =
                                crate::scope_editor::computed_properties::profile(
                                    &self.project.root,
                                    &self.project.target,
                                    &self.selected_scope,
                                    &self.project.target_options,
                                    computed_property_owner,
                                );
                            let scope = scope_at_mut(&mut self.project.root, &self.selected_scope);
                            let before = watch_scope.then(|| ScopeFields::capture(scope));
                            type_action = show_scope_editor(
                                ui,
                                scope,
                                &self.project.graph,
                                &source_paths,
                                &target_fields,
                                crate::scope_editor::ScopeEditorOwner {
                                    nested,
                                    primary_root_bindings,
                                    target_xml_types: &target_xml_types,
                                    target_xml_default,
                                    target_type_editable,
                                },
                                output_profile,
                            );
                            crate::scope_editor::computed_properties::show(
                                ui,
                                scope,
                                &self.project.graph,
                                computed_property_profile,
                            );
                            scope_edit = before.map(|before| {
                                let document = GraphDocument::Target(TargetId::Primary);
                                (document, before, ScopeFields::capture(scope))
                            });
                        }
                    }
                });
            });
        if let Some((document, before, after)) = scope_edit {
            let location = editor_ui::ScopeLocation::of_children(&self.selected_scope);
            self.frame_commands
                .extend(graph_commands::scope_edit_commands(
                    &document, &location, &before, &after,
                ));
        }
        if let Some(action) = type_action {
            self.apply_selected_target_xml_type(&action);
        }
    }
}

fn show_schema_search_input(
    ui: &mut egui::Ui,
    id_salt: impl egui::AsIdSalt,
    state: &mut SchemaExplorerState,
) {
    ui.horizontal(|ui| {
        let search_width = (ui.available_width()
            - crate::theme::METRICS.icon_button_size
            - ui.spacing().item_spacing.x)
            .max(64.0);
        ui.add(
            egui::TextEdit::singleline(state.query_mut())
                .id_source(id_salt)
                .hint_text("Search fields and groups")
                .desired_width(search_width),
        )
        .on_hover_text("Filter by field, group, path, or scalar type");
        if crate::icons::button(
            ui,
            state.is_filtering(),
            lucide_icons::Icon::SearchX,
            "Clear schema search",
        )
        .clicked()
        {
            state.clear();
        }
    });
}

fn show_schema_result_count(
    ui: &mut egui::Ui,
    state: &SchemaExplorerState,
    matches: usize,
    fields: usize,
) {
    let summary = if state.is_filtering() {
        match matches {
            1 => "1 match".to_string(),
            count => format!("{count} matches"),
        }
    } else {
        match fields {
            1 => "1 field".to_string(),
            count => format!("{count} fields"),
        }
    };
    ui.weak(summary);
}
