use super::*;
use crate::new_mapping::{
    CsvBoundaryDraft, CsvColumnDraft, CsvQuoteMode, MappingBoundary, SqliteBoundaryDraft,
};

#[path = "new_mapping/fixed_width.rs"]
mod fixed_width;
#[path = "new_mapping/flextext.rs"]
mod flextext;
#[path = "new_mapping/protobuf.rs"]
mod protobuf;
#[path = "new_mapping/xlsx.rs"]
mod xlsx;

#[cfg(test)]
#[path = "new_mapping/dialog_visual_tests.rs"]
mod dialog_visual_tests;

impl FerruleApp {
    pub(super) fn begin_new_mapping(&mut self) {
        self.new_mapping_setup = Some(NewMappingSetup::default());
    }

    pub(super) fn stage_mapping_schema(&mut self, side: SchemaSide, path: PathBuf) {
        if crate::new_mapping::is_flextext_configuration(&path) {
            self.stage_mapping_flextext(side, path);
            return;
        }
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("proto"))
        {
            self.stage_mapping_protobuf(side, path);
            return;
        }
        match crate::new_mapping::import_schema(&path) {
            Ok(schema) => {
                let imported = crate::new_mapping::MappingBoundary::Schema(Box::new(
                    crate::new_mapping::ImportedSchema { path, schema },
                ));
                let Some(setup) = self.new_mapping_setup.as_mut() else {
                    return;
                };
                match side {
                    SchemaSide::Source => setup.source = Some(imported),
                    SchemaSide::Target => setup.target = Some(imported),
                }
                self.status = format!("loaded {} schema", side.label().to_lowercase());
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = format!("failed to load {} schema", side.label().to_lowercase());
                self.diagnostics
                    .error("Schema import failed", format!("{error:#}"));
            }
        }
    }

    pub(super) fn stage_mapping_csv_source(&mut self, path: PathBuf) {
        match CsvBoundaryDraft::source(path) {
            Ok(draft) => {
                if let Some(setup) = self.new_mapping_setup.as_mut() {
                    let sample_error = draft.sample_error.clone();
                    setup.source = Some(MappingBoundary::Csv(draft));
                    if let Some(error) = sample_error {
                        self.status = "CSV sample needs format settings".to_owned();
                        self.diagnostics
                            .error("CSV sample needs format settings", error);
                    } else {
                        self.status = "loaded CSV source sample".to_owned();
                        self.diagnostics.clear();
                    }
                }
            }
            Err(error) => {
                self.status = "failed to sample CSV source".to_owned();
                self.diagnostics
                    .error("CSV source sample failed", format!("{error:#}"));
            }
        }
    }

    pub(super) fn stage_mapping_csv_target_output(&mut self, path: String) {
        if let Some(setup) = self.new_mapping_setup.as_mut()
            && let Some(MappingBoundary::Csv(draft)) = setup.target.as_mut()
        {
            if draft.delimiter == ',' && path.to_ascii_lowercase().ends_with(".tsv") {
                draft.delimiter = '\t';
            }
            draft.path = path;
        }
    }

    pub(super) fn stage_mapping_sqlite(&mut self, side: SchemaSide, path: PathBuf) {
        let result = match side {
            SchemaSide::Source => SqliteBoundaryDraft::source(path),
            SchemaSide::Target => SqliteBoundaryDraft::target(path),
        };
        match result {
            Ok(draft) => {
                if let Some(setup) = self.new_mapping_setup.as_mut() {
                    match side {
                        SchemaSide::Source => {
                            setup.source = Some(MappingBoundary::Sqlite(Box::new(draft)))
                        }
                        SchemaSide::Target => {
                            setup.target = Some(MappingBoundary::Sqlite(Box::new(draft)))
                        }
                    }
                    self.status =
                        format!("selected {} SQLite database", side.label().to_lowercase());
                    self.diagnostics.clear();
                }
            }
            Err(error) => {
                self.status = format!(
                    "failed to select {} SQLite database",
                    side.label().to_lowercase()
                );
                self.diagnostics
                    .error("SQLite database selection failed", format!("{error:#}"));
            }
        }
    }

    pub(super) fn stage_mapping_sqlite_table(&mut self, side: SchemaSide) {
        let Some(draft) = self
            .new_mapping_setup
            .as_mut()
            .and_then(|setup| match side {
                SchemaSide::Source => setup.source.as_mut(),
                SchemaSide::Target => setup.target.as_mut(),
            })
            .and_then(|boundary| match boundary {
                MappingBoundary::Sqlite(draft) => Some(draft),
                _ => None,
            })
        else {
            return;
        };
        match draft.load_schema() {
            Ok(()) => {
                self.status = format!("loaded {} SQLite table", side.label().to_lowercase());
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = format!(
                    "failed to load {} SQLite table",
                    side.label().to_lowercase()
                );
                self.diagnostics
                    .error("SQLite table import failed", format!("{error:#}"));
            }
        }
    }

    pub(super) fn stage_mapping_sqlite_target_output(&mut self, path: String) {
        if let Some(MappingBoundary::Sqlite(draft)) = self
            .new_mapping_setup
            .as_mut()
            .and_then(|setup| setup.target.as_mut())
        {
            draft.output_path = path;
        }
    }

    pub(super) fn show_new_mapping_setup(&mut self, ctx: &egui::Context) {
        let Some(setup) = self.new_mapping_setup.as_mut() else {
            return;
        };
        let can_create = setup.can_create();
        let dialog_idle = self.pending_dialog.is_none();
        let mut action = None;
        let mut refresh_source = false;

        let response = crate::ui_kit::dialog(ctx, crate::ui_kit::DialogKind::Modal, "New mapping")
            .width(crate::ui_kit::DialogWidth::Wide)
            .show(
                |ui| {
                    ui.columns(2, |columns| {
                        egui::Frame::group(columns[0].style()).show(&mut columns[0], |ui| {
                            if let Some(selected) = show_boundary_picker(
                                ui,
                                setup.source.as_ref(),
                                SchemaSide::Source,
                                dialog_idle,
                            ) {
                                action = Some(selected);
                            }
                            refresh_source = show_boundary_options(
                                ui,
                                setup.source.as_mut(),
                                SchemaSide::Source,
                                dialog_idle,
                                &mut action,
                            );
                        });
                        egui::Frame::group(columns[1].style()).show(&mut columns[1], |ui| {
                            if let Some(selected) = show_boundary_picker(
                                ui,
                                setup.target.as_ref(),
                                SchemaSide::Target,
                                dialog_idle,
                            ) {
                                action = Some(selected);
                            }
                            show_boundary_options(
                                ui,
                                setup.target.as_mut(),
                                SchemaSide::Target,
                                dialog_idle,
                                &mut action,
                            );
                        });
                    });
                },
                |ui| {
                    crate::ui_kit::dialog_buttons(
                        ui,
                        crate::ui_kit::DialogButton::new("Create mapping")
                            .disabled_if(!dialog_idle, "Finish choosing a file first")
                            .disabled_if(
                                !can_create,
                                "Select a source and target and complete their format settings",
                            ),
                        Some(
                            crate::ui_kit::DialogButton::new("Cancel")
                                .disabled_if(!dialog_idle, "Finish choosing a file first"),
                        ),
                        None,
                    )
                },
            );
        if response.close_requested && dialog_idle
            || response.footer == Some(crate::ui_kit::DialogAction::Secondary)
        {
            action = Some(NewMappingAction::Cancel);
        } else if response.footer == Some(crate::ui_kit::DialogAction::Primary) {
            action = Some(NewMappingAction::Create);
        }

        if refresh_source
            && let Some(MappingBoundary::Csv(draft)) = self
                .new_mapping_setup
                .as_mut()
                .and_then(|setup| setup.source.as_mut())
        {
            if let Err(error) = draft.refresh_sample() {
                self.status = "failed to refresh CSV source sample".to_owned();
                self.diagnostics
                    .error("CSV source sample failed", format!("{error:#}"));
            } else {
                self.diagnostics.clear();
            }
        }

        match action {
            Some(NewMappingAction::ChooseSchema(side)) => {
                let kind = match side {
                    SchemaSide::Source => DialogKind::BrowseSourceSchema,
                    SchemaSide::Target => DialogKind::BrowseTargetSchema,
                };
                self.pending_dialog = Some((
                    kind,
                    pick_file("schema or layout", &["xsd", "json", "proto", "mft"]),
                ));
            }
            Some(NewMappingAction::ChooseCsvSource) => {
                self.pending_dialog = Some((
                    DialogKind::BrowseSourceCsv,
                    pick_file("CSV source", &["csv", "txt", "tsv"]),
                ));
            }
            Some(NewMappingAction::ChooseSqlite(side)) => {
                let kind = match side {
                    SchemaSide::Source => DialogKind::BrowseSourceSqlite,
                    SchemaSide::Target => DialogKind::BrowseTargetSqlite,
                };
                self.pending_dialog = Some((
                    kind,
                    pick_file("SQLite database", &["db", "sqlite", "sqlite3"]),
                ));
            }
            Some(NewMappingAction::LoadSqliteTable(side)) => {
                self.stage_mapping_sqlite_table(side);
            }
            Some(NewMappingAction::ConfigureCsvTarget) => {
                if let Some(setup) = self.new_mapping_setup.as_mut()
                    && !matches!(setup.target.as_ref(), Some(MappingBoundary::Csv(_)))
                {
                    // Choosing the active format must not discard its edited draft.
                    setup.target = Some(MappingBoundary::Csv(CsvBoundaryDraft::target()));
                }
            }
            Some(NewMappingAction::ConfigureXlsx(side)) => {
                self.configure_mapping_xlsx(side);
            }
            Some(NewMappingAction::AbandonXlsx(side)) => {
                self.abandon_mapping_xlsx(side);
            }
            Some(NewMappingAction::ConfigureFixedWidth(side)) => {
                self.configure_mapping_fixed_width(side);
            }
            Some(NewMappingAction::AbandonFixedWidth(side)) => {
                self.abandon_mapping_fixed_width(side);
            }
            Some(NewMappingAction::ChooseCsvTargetOutput) => {
                let current = self
                    .new_mapping_setup
                    .as_ref()
                    .and_then(|setup| setup.target.as_ref())
                    .and_then(|boundary| match boundary {
                        MappingBoundary::Csv(draft) => Some(draft.path.as_str()),
                        MappingBoundary::Schema(_)
                        | MappingBoundary::FixedWidth(_)
                        | MappingBoundary::Xlsx(_)
                        | MappingBoundary::Sqlite(_)
                        | MappingBoundary::Protobuf(_)
                        | MappingBoundary::FlexText(_) => None,
                    })
                    .filter(|path| !path.is_empty())
                    .unwrap_or("output.csv");
                self.pending_dialog = Some((
                    DialogKind::BrowseTargetCsvOutput,
                    save_file("CSV output", &["csv", "txt", "tsv"], current),
                ));
            }
            Some(NewMappingAction::ChooseSqliteTargetOutput) => {
                let current = self
                    .new_mapping_setup
                    .as_ref()
                    .and_then(|setup| setup.target.as_ref())
                    .and_then(|boundary| match boundary {
                        MappingBoundary::Sqlite(draft) => Some(draft.output_path.as_str()),
                        _ => None,
                    })
                    .filter(|path| !path.is_empty())
                    .unwrap_or("output.db");
                self.pending_dialog = Some((
                    DialogKind::BrowseTargetSqliteOutput,
                    save_file("SQLite output", &["db", "sqlite", "sqlite3"], current),
                ));
            }
            Some(NewMappingAction::Cancel) => {
                self.new_mapping_setup = None;
                self.status = "new mapping cancelled".to_string();
            }
            Some(NewMappingAction::Create) => self.finish_new_mapping(),
            None => {}
        }
    }

    pub(super) fn finish_new_mapping(&mut self) {
        let Some(setup) = self.new_mapping_setup.as_ref() else {
            return;
        };
        let project = match setup.build_project() {
            Ok(project) => project,
            Err(error) => {
                self.status = "new mapping is incomplete".to_string();
                self.diagnostics
                    .error("New mapping is incomplete", format!("{error:#}"));
                return;
            }
        };
        self.new_mapping_setup = None;
        self.clear_run_report();
        self.project = project;
        self.document_ui_id = new_document_ui_id();
        self.primary_document = None;
        self.host_parameters = super::host_parameters::HostParameterEditor::default();
        self.input_path.clear();
        self.output_path.clear();
        self.mapping_workspace.reset();
        self.main_canvas = CanvasDocumentState::main(&self.project);
        self.reset_canvas_view();
        self.document = DocumentLocation::untitled("mapping.json");
        self.mark_unsaved();
        self.selected_scope.clear();
        self.rebase_history();
        self.diagnostics.clear();
        self.status = "new mapping ready".to_string();
    }
}

fn show_boundary_options(
    ui: &mut egui::Ui,
    boundary: Option<&mut MappingBoundary>,
    side: SchemaSide,
    dialog_idle: bool,
    action: &mut Option<NewMappingAction>,
) -> bool {
    let target = matches!(side, SchemaSide::Target);
    let salt = if target { "target" } else { "source" };
    match boundary {
        Some(MappingBoundary::Csv(draft)) => {
            if target {
                ui.label("Output path (required)");
                ui.horizontal(|ui| {
                    let was_tsv = draft.path.to_ascii_lowercase().ends_with(".tsv");
                    if ui
                        .add(
                            crate::ui_kit::text_field(
                                &mut draft.path,
                                crate::ui_kit::FieldWidth::Medium,
                            )
                            .hint_text("Enter a CSV output path"),
                        )
                        .changed()
                        && !was_tsv
                        && draft.path.to_ascii_lowercase().ends_with(".tsv")
                        && draft.delimiter == ','
                    {
                        draft.delimiter = '\t';
                    }
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Browse…"))
                        .clicked()
                    {
                        *action = Some(NewMappingAction::ChooseCsvTargetOutput);
                    }
                });
            }
            let refresh = show_csv_options(ui, draft, salt);
            if !target {
                crate::ui_kit::help(
                    ui,
                    "Changing CSV options refreshes column names; selected types stay.",
                );
            }
            show_csv_columns(ui, draft, !target);
            if target {
                if draft.columns.len() < 256 && ui.button("Add column").clicked() {
                    draft.columns.push(CsvColumnDraft {
                        name: String::new(),
                        ty: ir::ScalarType::String,
                    });
                }
            } else {
                show_csv_preview(ui, draft);
            }
            show_csv_validation(ui, draft);
            return refresh && !target;
        }
        Some(MappingBoundary::FixedWidth(draft)) => {
            if fixed_width::show_options(ui, draft, target) {
                *action = Some(NewMappingAction::AbandonFixedWidth(side));
            }
        }
        Some(MappingBoundary::Xlsx(draft)) => {
            if ui
                .add_enabled_ui(dialog_idle, |ui| xlsx::show_options(ui, draft, target))
                .inner
            {
                *action = Some(NewMappingAction::AbandonXlsx(side));
            }
        }
        Some(MappingBoundary::Protobuf(draft)) => protobuf::show_options(ui, draft, salt, target),
        Some(MappingBoundary::FlexText(draft)) => flextext::show_options(ui, draft, target),
        Some(MappingBoundary::Sqlite(draft)) => {
            if show_sqlite_table(ui, draft, salt, target) {
                *action = Some(NewMappingAction::LoadSqliteTable(side));
            }
            if target {
                ui.label("Output path (required)");
                ui.horizontal(|ui| {
                    ui.add(
                        crate::ui_kit::text_field(
                            &mut draft.output_path,
                            crate::ui_kit::FieldWidth::Medium,
                        )
                        .hint_text("Enter a database output path"),
                    );
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Browse…"))
                        .clicked()
                    {
                        *action = Some(NewMappingAction::ChooseSqliteTargetOutput);
                    }
                });
                crate::ui_kit::help(
                    ui,
                    "Running the mapping replaces this table's rows in the output database.",
                );
                show_sqlite_validation(ui, draft, true);
            }
        }
        Some(MappingBoundary::Schema(_)) | None => (),
    }
    false
}

fn show_boundary_picker(
    ui: &mut egui::Ui,
    boundary: Option<&MappingBoundary>,
    side: SchemaSide,
    enabled: bool,
) -> Option<NewMappingAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.strong(side.label());
        ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button("Choose format…", |ui| {
                if ui
                    .button("Schema or layout…")
                    .on_hover_text("Import XSD, JSON Schema, Protocol Buffers or FlexText")
                    .clicked()
                {
                    action = Some(NewMappingAction::ChooseSchema(side));
                    ui.close();
                }
                if ui.button("CSV / delimited text…").clicked() {
                    action = Some(match side {
                        SchemaSide::Source => NewMappingAction::ChooseCsvSource,
                        SchemaSide::Target => NewMappingAction::ConfigureCsvTarget,
                    });
                    ui.close();
                }
                if ui.button("SQLite database…").clicked() {
                    action = Some(NewMappingAction::ChooseSqlite(side));
                    ui.close();
                }
            });
        });
    });
    let label = boundary_label(boundary);
    ui.add(
        egui::Label::new(if boundary.is_none() {
            egui::RichText::new(&label).weak().italics()
        } else {
            egui::RichText::new(&label)
        })
        .truncate(),
    )
    .on_hover_text(&label);
    if let Some(MappingBoundary::Schema(imported)) = boundary {
        ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button("Use schema with…", |ui| {
                if ui
                    .add_enabled(
                        crate::extra_targets::flat_scalar_fields(&imported.schema).is_ok(),
                        egui::Button::new("Fixed-width text"),
                    )
                    .clicked()
                {
                    action = Some(NewMappingAction::ConfigureFixedWidth(side));
                    ui.close();
                }
                if ui
                    .add_enabled(
                        crate::new_mapping::XlsxBoundaryDraft::supports_schema(&imported.schema),
                        egui::Button::new("Excel workbook"),
                    )
                    .clicked()
                {
                    action = Some(NewMappingAction::ConfigureXlsx(side));
                    ui.close();
                }
            });
        });
    }
    action
}

fn boundary_label(boundary: Option<&MappingBoundary>) -> String {
    match boundary {
        None => "Choose a format to get started".to_owned(),
        Some(MappingBoundary::Schema(imported)) => imported.path.display().to_string(),
        Some(MappingBoundary::Xlsx(draft)) => {
            format!("Workbook: {}", draft.schema_path.display())
        }
        Some(MappingBoundary::FixedWidth(draft)) => {
            format!("Fixed-width: {}", draft.schema_path.display())
        }
        Some(MappingBoundary::Csv(draft)) if draft.path.is_empty() => "CSV".to_owned(),
        Some(MappingBoundary::Csv(draft)) => format!("CSV: {}", draft.path),
        Some(MappingBoundary::Sqlite(draft)) if draft.table.is_empty() => {
            format!("SQLite: {}", draft.schema_path)
        }
        Some(MappingBoundary::Sqlite(draft)) => {
            format!("SQLite: {} / {}", draft.schema_path, draft.table)
        }
        Some(MappingBoundary::Protobuf(draft)) => {
            format!("Protocol Buffers: {}", draft.schema_path.display())
        }
        Some(MappingBoundary::FlexText(draft)) => {
            format!("FlexText: {}", draft.configuration_path.display())
        }
    }
}

fn show_sqlite_table(
    ui: &mut egui::Ui,
    draft: &mut SqliteBoundaryDraft,
    side: &str,
    target: bool,
) -> bool {
    let mut load = false;
    ui.label("Table");
    ui.horizontal(|ui| {
        let mut table = draft.table.clone();
        if ui
            .add(
                crate::ui_kit::text_field(&mut table, crate::ui_kit::FieldWidth::Medium)
                    .char_limit(256)
                    .hint_text("Existing table name"),
            )
            .changed()
        {
            draft.set_table(table);
        }
        if ui
            .add_enabled(
                !draft.table.trim().is_empty(),
                egui::Button::new("Load table"),
            )
            .clicked()
        {
            load = true;
        }
    });
    if let Some(schema) = &draft.schema
        && let ir::SchemaKind::Group { children, .. } = &schema.kind
    {
        ui.label(format!("{} columns from `{}`", children.len(), schema.name));
        crate::ui_kit::list_area(
            ("new_mapping_sqlite_columns", side),
            crate::ui_kit::ListHeight::Medium,
        )
        .show(ui, |ui| {
            egui::Grid::new(("new_mapping_sqlite_columns_grid", side))
                .num_columns(3)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    for column in children {
                        ui.monospace(short_preview_text(&column.name));
                        let ty = match &column.kind {
                            ir::SchemaKind::Scalar { ty } => scalar_type_label(*ty),
                            _ => "Other",
                        };
                        ui.label(ty);
                        if column.value_generation.is_some() {
                            ui.weak("Generated");
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
        });
    }
    if !target {
        show_sqlite_validation(ui, draft, false);
    }
    load
}

fn show_sqlite_validation(ui: &mut egui::Ui, draft: &SqliteBoundaryDraft, target: bool) {
    if let Err(error) = draft.schema(target) {
        ui.colored_label(ui.visuals().error_fg_color, format!("{error:#}"));
    }
}

fn show_csv_options(ui: &mut egui::Ui, draft: &mut CsvBoundaryDraft, side: &str) -> bool {
    let previous = (draft.delimiter, draft.has_header_row, draft.quote_mode);
    let mut quote_character_changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut draft.has_header_row, "Header row");
        ui.label("Delimiter");
        egui::ComboBox::from_id_salt(("new_mapping_delimiter", side))
            .width(crate::ui_kit::FieldWidth::Small.min())
            .selected_text(delimiter_label(draft.delimiter))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut draft.delimiter, ',', "Comma");
                ui.selectable_value(&mut draft.delimiter, '\t', "Tab");
                ui.selectable_value(&mut draft.delimiter, ';', "Semicolon");
                ui.selectable_value(&mut draft.delimiter, '|', "Pipe");
                ui.selectable_value(&mut draft.delimiter, '~', "Custom");
            });
        if !matches!(draft.delimiter, ',' | '\t' | ';' | '|') {
            let mut custom = draft.delimiter.to_string();
            if ui
                .add(
                    crate::ui_kit::text_field(&mut custom, crate::ui_kit::FieldWidth::Small)
                        .char_limit(1),
                )
                .changed()
                && let Some(character) = custom.chars().next()
            {
                draft.delimiter = character;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Quoting");
        egui::ComboBox::from_id_salt(("new_mapping_quote", side))
            .width(crate::ui_kit::FieldWidth::Medium.min())
            .selected_text(quote_mode_label(draft.quote_mode))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut draft.quote_mode,
                    CsvQuoteMode::Standard,
                    "Double quote",
                );
                ui.selectable_value(
                    &mut draft.quote_mode,
                    CsvQuoteMode::Custom,
                    "Custom character",
                );
                ui.selectable_value(&mut draft.quote_mode, CsvQuoteMode::Disabled, "Disabled");
            });
        if draft.quote_mode == CsvQuoteMode::Custom {
            ui.label("Quote");
            quote_character_changed = ui
                .add(
                    crate::ui_kit::text_field(
                        &mut draft.custom_quote,
                        crate::ui_kit::FieldWidth::Small,
                    )
                    .char_limit(1),
                )
                .changed();
        }
    });
    if side == "source" {
        ui.checkbox(&mut draft.preserve_empty_strings, "Keep empty text fields");
    } else {
        ui.checkbox(&mut draft.utf8_bom, "UTF-8 byte order mark");
    }
    quote_character_changed || previous != (draft.delimiter, draft.has_header_row, draft.quote_mode)
}

fn quote_mode_label(mode: CsvQuoteMode) -> &'static str {
    match mode {
        CsvQuoteMode::Standard => "Double quote",
        CsvQuoteMode::Custom => "Custom character",
        CsvQuoteMode::Disabled => "Disabled",
    }
}

fn delimiter_label(delimiter: char) -> String {
    match delimiter {
        ',' => "Comma".to_owned(),
        '\t' => "Tab".to_owned(),
        ';' => "Semicolon".to_owned(),
        '|' => "Pipe".to_owned(),
        other => format!("Custom ({other})"),
    }
}

fn show_csv_columns(ui: &mut egui::Ui, draft: &mut CsvBoundaryDraft, source: bool) {
    ui.label(if source {
        "Source columns (editable names and types)"
    } else {
        "Target columns (output order)"
    });
    let mut remove = None;
    crate::ui_kit::list_area(
        if source {
            "new_mapping_source_columns"
        } else {
            "new_mapping_target_columns"
        },
        crate::ui_kit::ListHeight::Medium,
    )
    .show(ui, |ui| {
        egui::Grid::new(if source {
            "new_mapping_source_column_grid"
        } else {
            "new_mapping_target_column_grid"
        })
        .num_columns(if source { 3 } else { 4 })
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for (index, column) in draft.columns.iter_mut().enumerate() {
                ui.label(format!("{}", index + 1));
                ui.add(
                    crate::ui_kit::text_field(&mut column.name, crate::ui_kit::FieldWidth::Medium)
                        .char_limit(256),
                );
                egui::ComboBox::from_id_salt(("new_mapping_column_type", source, index))
                    .width(crate::ui_kit::FieldWidth::Small.min())
                    .selected_text(scalar_type_label(column.ty))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut column.ty, ir::ScalarType::String, "String");
                        ui.selectable_value(&mut column.ty, ir::ScalarType::Int, "Integer");
                        ui.selectable_value(&mut column.ty, ir::ScalarType::Float, "Number");
                        ui.selectable_value(&mut column.ty, ir::ScalarType::Bool, "Boolean");
                    });
                if !source
                    && crate::ui_kit::icon_button(
                        ui,
                        lucide_icons::Icon::Trash2,
                        "Remove column",
                        None,
                    )
                    .clicked()
                {
                    remove = Some(index);
                }
                ui.end_row();
            }
        });
    });
    if let Some(index) = remove {
        draft.columns.remove(index);
    }
}

fn scalar_type_label(ty: ir::ScalarType) -> &'static str {
    match ty {
        ir::ScalarType::String => "String",
        ir::ScalarType::Int => "Integer",
        ir::ScalarType::Float => "Number",
        ir::ScalarType::Bool => "Boolean",
    }
}

fn show_csv_preview(ui: &mut egui::Ui, draft: &CsvBoundaryDraft) {
    ui.label(format!(
        "Preview (first {} data rows)",
        draft.preview_rows.len()
    ));
    egui::ScrollArea::both()
        .id_salt("new_mapping_csv_preview")
        .max_height(crate::ui_kit::ListHeight::Medium.px())
        .show(ui, |ui| {
            egui::Grid::new("new_mapping_csv_preview_grid")
                .striped(true)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    for column in &draft.columns {
                        ui.strong(short_preview_text(&column.name));
                    }
                    ui.end_row();
                    for row in &draft.preview_rows {
                        for index in 0..draft.columns.len() {
                            ui.monospace(short_preview_text(
                                row.get(index).map_or("", String::as_str),
                            ));
                        }
                        ui.end_row();
                    }
                });
        });
}

fn short_preview_text(value: &str) -> String {
    let mut characters = value.chars();
    let shortened = characters.by_ref().take(64).collect::<String>();
    if characters.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

fn show_csv_validation(ui: &mut egui::Ui, draft: &CsvBoundaryDraft) {
    if let Err(error) = draft.validate() {
        ui.colored_label(ui.visuals().error_fg_color, format!("{error:#}"));
    }
}

#[derive(Clone, Copy)]
enum NewMappingAction {
    ChooseSchema(SchemaSide),
    ChooseCsvSource,
    ChooseSqlite(SchemaSide),
    LoadSqliteTable(SchemaSide),
    ConfigureCsvTarget,
    ConfigureFixedWidth(SchemaSide),
    ConfigureXlsx(SchemaSide),
    AbandonXlsx(SchemaSide),
    AbandonFixedWidth(SchemaSide),
    ChooseCsvTargetOutput,
    ChooseSqliteTargetOutput,
    Cancel,
    Create,
}

#[cfg(test)]
#[path = "new_mapping/csv_quote_tests.rs"]
mod csv_quote_tests;

#[cfg(test)]
#[path = "new_mapping/fixed_width_tests.rs"]
mod fixed_width_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn bind_flat_columns(project: &mut mapping::Project, columns: &[&str]) {
        project.root.iteration = mapping::ScopeIteration::Source(vec![]);
        for (index, column) in columns.iter().enumerate() {
            let id = index as mapping::NodeId + 1;
            project.graph.nodes.insert(
                id,
                mapping::Node::SourceField {
                    path: vec![(*column).to_owned()],
                    frame: project.source.repeating.then(Vec::new),
                },
            );
            project.root.bindings.push(mapping::Binding {
                target_field: (*column).to_owned(),
                node: id,
            });
        }
    }

    #[test]
    fn csv_mapping_paths_survive_first_save_reopen_and_run() -> anyhow::Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "ferrule-gui-csv-new-mapping-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory)?;
        let source_path = directory.join("source.csv");
        let target_path = directory.join("target.csv");
        let project_path = directory.join("mapping.json");
        std::fs::write(&source_path, "Name,Quantity\nJane,2\n")?;

        let mut app = FerruleApp::default();
        app.begin_new_mapping();
        app.stage_mapping_csv_source(source_path.clone());
        let setup = app.new_mapping_setup.as_mut().unwrap();
        let mut target = CsvBoundaryDraft::target();
        target.path = target_path.to_str().unwrap().to_owned();
        target.columns[0].name = "Name".into();
        setup.target = Some(MappingBoundary::Csv(target));
        app.finish_new_mapping();

        assert!(app.new_mapping_setup.is_none());
        assert_eq!(app.project.source_path.as_deref(), source_path.to_str());
        assert_eq!(app.project.target_path.as_deref(), target_path.to_str());
        assert_eq!(app.project.source_options.has_header_row, Some(true));
        assert_eq!(app.project.target_options.delimiter, Some(','));
        app.project.graph.nodes.insert(
            1,
            mapping::Node::SourceField {
                path: vec!["Name".into()],
                frame: None,
            },
        );
        app.project.root.iteration = mapping::ScopeIteration::Source(vec![]);
        app.project.root.bindings.push(mapping::Binding {
            target_field: "Name".into(),
            node: 1,
        });
        app.save_document_to(&project_path)?;
        app.load_project_from(&project_path);
        assert_eq!(app.project.source_path.as_deref(), source_path.to_str());
        assert_eq!(app.project.target_path.as_deref(), target_path.to_str());
        assert!(cli::validate(&app.project).is_empty());

        let outcome = cli::run_project_with_paths(&project_path, None, None)?;
        assert_eq!(outcome.output_path, target_path);
        assert_eq!(std::fs::read_to_string(&target_path)?, "Name\nJane\n");
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn sqlite_boundary_paths_survive_save_reopen_and_run() -> anyhow::Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "ferrule-gui-sqlite-new-mapping-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory)?;
        let csv_path = directory.join("source.csv");
        let schema_db_path = directory.join("schema.sqlite");
        let output_db_path = directory.join("output.sqlite3");
        let readback_path = directory.join("readback.csv");
        std::fs::write(&csv_path, "Name,Quantity\nJane,2\n")?;

        // Build an existing table for introspection using the public CLI path.
        let columns = vec![
            ir::SchemaNode::scalar("Name", ir::ScalarType::String),
            ir::SchemaNode::scalar("Quantity", ir::ScalarType::Int),
        ];
        let mut seed = crate::new_mapping::blank_project();
        seed.source = ir::SchemaNode::group("row", columns.clone());
        seed.target = ir::SchemaNode::group("orders", columns).repeating();
        seed.source_path = Some(csv_path.to_str().unwrap().to_owned());
        seed.target_path = Some(schema_db_path.to_str().unwrap().to_owned());
        bind_flat_columns(&mut seed, &["Name", "Quantity"]);
        let seed_project_path = directory.join("seed.json");
        std::fs::write(&seed_project_path, serde_json::to_vec_pretty(&seed)?)?;
        cli::run_project_with_paths(&seed_project_path, None, None)?;

        let mut app = FerruleApp::default();
        app.begin_new_mapping();
        app.stage_mapping_csv_source(csv_path.clone());
        app.stage_mapping_sqlite(SchemaSide::Target, schema_db_path.clone());
        let target = app
            .new_mapping_setup
            .as_mut()
            .and_then(|setup| setup.target.as_mut());
        let Some(MappingBoundary::Sqlite(target)) = target else {
            panic!("SQLite target was not staged");
        };
        target.set_table("orders".to_owned());
        app.stage_mapping_sqlite_table(SchemaSide::Target);
        app.stage_mapping_sqlite_target_output(output_db_path.to_str().unwrap().to_owned());
        app.finish_new_mapping();
        assert!(app.project.target.repeating);
        assert_eq!(app.project.target.name, "orders");
        assert_eq!(app.project.target_path.as_deref(), output_db_path.to_str());
        bind_flat_columns(&mut app.project, &["Name", "Quantity"]);
        let project_path = directory.join("csv-to-db.json");
        app.save_document_to(&project_path)?;
        app.load_project_from(&project_path);
        assert!(cli::validate(&app.project).is_empty());
        cli::run_project_with_paths(&project_path, None, None)?;
        assert!(output_db_path.is_file());

        let mut readback = FerruleApp::default();
        readback.begin_new_mapping();
        readback.stage_mapping_sqlite(SchemaSide::Source, output_db_path.clone());
        let source = readback
            .new_mapping_setup
            .as_mut()
            .and_then(|setup| setup.source.as_mut());
        let Some(MappingBoundary::Sqlite(source)) = source else {
            panic!("SQLite source was not staged");
        };
        source.set_table("orders".to_owned());
        readback.stage_mapping_sqlite_table(SchemaSide::Source);
        let target = readback.new_mapping_setup.as_mut().unwrap();
        let mut csv_target = CsvBoundaryDraft::target();
        csv_target.path = readback_path.to_str().unwrap().to_owned();
        csv_target.columns = vec![
            CsvColumnDraft {
                name: "Name".to_owned(),
                ty: ir::ScalarType::String,
            },
            CsvColumnDraft {
                name: "Quantity".to_owned(),
                ty: ir::ScalarType::Int,
            },
        ];
        target.target = Some(MappingBoundary::Csv(csv_target));
        readback.finish_new_mapping();
        assert!(readback.project.source.repeating);
        assert_eq!(
            readback.project.source_path.as_deref(),
            output_db_path.to_str()
        );
        bind_flat_columns(&mut readback.project, &["Name", "Quantity"]);
        let readback_project_path = directory.join("db-to-csv.json");
        readback.save_document_to(&readback_project_path)?;
        readback.load_project_from(&readback_project_path);
        assert!(cli::validate(&readback.project).is_empty());
        cli::run_project_with_paths(&readback_project_path, None, None)?;
        assert_eq!(
            std::fs::read_to_string(&readback_path)?,
            "Name,Quantity\nJane,2\n"
        );
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "new_mapping/csv_dispatch_tests.rs"]
mod csv_dispatch_tests;
