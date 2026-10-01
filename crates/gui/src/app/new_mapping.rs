use super::*;
use crate::new_mapping::{CsvBoundaryDraft, CsvColumnDraft, MappingBoundary, SqliteBoundaryDraft};

impl FerruleApp {
    pub(super) fn begin_new_mapping(&mut self) {
        self.new_mapping_setup = Some(NewMappingSetup::default());
    }

    pub(super) fn stage_mapping_schema(&mut self, side: SchemaSide, path: PathBuf) {
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
                    .error("Schema import failed", error.to_string());
            }
        }
    }

    pub(super) fn stage_mapping_csv_source(&mut self, path: PathBuf) {
        match CsvBoundaryDraft::source(path) {
            Ok(draft) => {
                if let Some(setup) = self.new_mapping_setup.as_mut() {
                    setup.source = Some(MappingBoundary::Csv(draft));
                    self.status = "loaded CSV source sample".to_owned();
                    self.diagnostics.clear();
                }
            }
            Err(error) => {
                self.status = "failed to sample CSV source".to_owned();
                self.diagnostics
                    .error("CSV source sample failed", error.to_string());
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
                    .error("SQLite database selection failed", error.to_string());
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
                    .error("SQLite table import failed", error.to_string());
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
        let can_create = setup.build_project().is_ok();
        let dialog_idle = self.pending_dialog.is_none();
        let mut action = None;
        let mut refresh_source = false;

        egui::Window::new("New Mapping")
            .collapsible(false)
            .resizable(true)
            .default_width(680.0)
            .min_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.strong("Source");
                ui.horizontal_wrapped(|ui| {
                    ui.label(boundary_label(setup.source.as_ref()));
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Choose schema..."))
                        .clicked()
                    {
                        action = Some(NewMappingAction::ChooseSchema(SchemaSide::Source));
                    }
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Choose CSV..."))
                        .clicked()
                    {
                        action = Some(NewMappingAction::ChooseCsvSource);
                    }
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Choose SQLite..."))
                        .clicked()
                    {
                        action = Some(NewMappingAction::ChooseSqlite(SchemaSide::Source));
                    }
                });
                if let Some(MappingBoundary::Csv(draft)) = setup.source.as_mut() {
                    refresh_source = show_csv_options(ui, draft, "source");
                    ui.weak("Changing CSV options refreshes column names; selected types stay.");
                    show_csv_columns(ui, draft, true);
                    show_csv_preview(ui, draft);
                    show_csv_validation(ui, draft);
                }
                if let Some(MappingBoundary::Sqlite(draft)) = setup.source.as_mut()
                    && show_sqlite_table(ui, draft, "source", false)
                {
                    action = Some(NewMappingAction::LoadSqliteTable(SchemaSide::Source));
                }

                ui.separator();
                ui.strong("Target");
                ui.horizontal_wrapped(|ui| {
                    ui.label(boundary_label(setup.target.as_ref()));
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Choose schema..."))
                        .clicked()
                    {
                        action = Some(NewMappingAction::ChooseSchema(SchemaSide::Target));
                    }
                    if !matches!(setup.target.as_ref(), Some(MappingBoundary::Csv(_)))
                        && ui
                            .add_enabled(dialog_idle, egui::Button::new("Configure CSV"))
                            .clicked()
                    {
                        action = Some(NewMappingAction::ConfigureCsvTarget);
                    }
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Choose SQLite..."))
                        .clicked()
                    {
                        action = Some(NewMappingAction::ChooseSqlite(SchemaSide::Target));
                    }
                });
                if let Some(MappingBoundary::Csv(draft)) = setup.target.as_mut() {
                    ui.horizontal(|ui| {
                        ui.label("Output path");
                        let was_tsv = draft.path.to_ascii_lowercase().ends_with(".tsv");
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut draft.path)
                                    .desired_width(380.0)
                                    .hint_text("output.csv"),
                            )
                            .changed()
                            && !was_tsv
                            && draft.path.to_ascii_lowercase().ends_with(".tsv")
                            && draft.delimiter == ','
                        {
                            draft.delimiter = '\t';
                        }
                        if ui
                            .add_enabled(dialog_idle, egui::Button::new("Choose..."))
                            .clicked()
                        {
                            action = Some(NewMappingAction::ChooseCsvTargetOutput);
                        }
                    });
                    show_csv_options(ui, draft, "target");
                    show_csv_columns(ui, draft, false);
                    if draft.columns.len() < 256 && ui.button("Add column").clicked() {
                        draft.columns.push(CsvColumnDraft {
                            name: String::new(),
                            ty: ir::ScalarType::String,
                        });
                    }
                    show_csv_validation(ui, draft);
                }
                if let Some(MappingBoundary::Sqlite(draft)) = setup.target.as_mut() {
                    if show_sqlite_table(ui, draft, "target", true) {
                        action = Some(NewMappingAction::LoadSqliteTable(SchemaSide::Target));
                    }
                    ui.horizontal(|ui| {
                        ui.label("Output path");
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.output_path)
                                .desired_width(380.0)
                                .hint_text("output.db"),
                        );
                        if ui
                            .add_enabled(dialog_idle, egui::Button::new("Choose..."))
                            .clicked()
                        {
                            action = Some(NewMappingAction::ChooseSqliteTargetOutput);
                        }
                    });
                    ui.weak(
                        "Running the mapping replaces this table's rows in the output database.",
                    );
                    show_sqlite_validation(ui, draft, true);
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Cancel"))
                        .clicked()
                    {
                        action = Some(NewMappingAction::Cancel);
                    }
                    if ui
                        .add_enabled(
                            dialog_idle && can_create,
                            egui::Button::new("Create mapping"),
                        )
                        .clicked()
                    {
                        action = Some(NewMappingAction::Create);
                    }
                });
            });

        if refresh_source
            && let Some(MappingBoundary::Csv(draft)) = self
                .new_mapping_setup
                .as_mut()
                .and_then(|setup| setup.source.as_mut())
        {
            if let Err(error) = draft.refresh_sample() {
                self.status = "failed to refresh CSV source sample".to_owned();
                self.diagnostics
                    .error("CSV source sample failed", error.to_string());
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
                self.pending_dialog = Some((kind, pick_file("schema", &["xsd", "json"])));
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
                if let Some(setup) = self.new_mapping_setup.as_mut() {
                    setup.target = Some(MappingBoundary::Csv(CsvBoundaryDraft::target()));
                }
            }
            Some(NewMappingAction::ChooseCsvTargetOutput) => {
                let current = self
                    .new_mapping_setup
                    .as_ref()
                    .and_then(|setup| setup.target.as_ref())
                    .and_then(|boundary| match boundary {
                        MappingBoundary::Csv(draft) => Some(draft.path.as_str()),
                        MappingBoundary::Schema(_) | MappingBoundary::Sqlite(_) => None,
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
                    .error("New mapping is incomplete", error.to_string());
                return;
            }
        };
        self.new_mapping_setup = None;
        self.clear_run_report();
        self.project = project;
        self.host_parameters = super::host_parameters::HostParameterEditor::default();
        self.input_path.clear();
        self.output_path.clear();
        self.mapping_workspace.reset();
        self.main_canvas = CanvasDocumentState::main(&self.project);
        self.reset_canvas_view();
        self.document = DocumentLocation::untitled("mapping.json");
        self.history.mark_unsaved();
        self.selected_scope.clear();
        self.rebase_history();
        self.diagnostics.clear();
        self.status = "new mapping ready".to_string();
    }
}

fn boundary_label(boundary: Option<&MappingBoundary>) -> String {
    match boundary {
        None => "Not selected".to_owned(),
        Some(MappingBoundary::Schema(imported)) => imported.path.display().to_string(),
        Some(MappingBoundary::Csv(draft)) if draft.path.is_empty() => "CSV".to_owned(),
        Some(MappingBoundary::Csv(draft)) => format!("CSV: {}", draft.path),
        Some(MappingBoundary::Sqlite(draft)) if draft.table.is_empty() => {
            format!("SQLite: {}", draft.schema_path)
        }
        Some(MappingBoundary::Sqlite(draft)) => {
            format!("SQLite: {} / {}", draft.schema_path, draft.table)
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
    ui.horizontal(|ui| {
        ui.label("Table");
        let mut table = draft.table.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut table)
                    .desired_width(260.0)
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
        egui::ScrollArea::vertical()
            .id_salt(("new_mapping_sqlite_columns", side))
            .max_height(180.0)
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
        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
    }
}

fn show_csv_options(ui: &mut egui::Ui, draft: &mut CsvBoundaryDraft, side: &str) -> bool {
    let previous = (draft.delimiter, draft.has_header_row);
    ui.horizontal(|ui| {
        ui.checkbox(&mut draft.has_header_row, "Header row");
        ui.label("Delimiter");
        egui::ComboBox::from_id_salt(("new_mapping_delimiter", side))
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
                    egui::TextEdit::singleline(&mut custom)
                        .char_limit(1)
                        .desired_width(28.0),
                )
                .changed()
                && let Some(character) = custom.chars().next()
            {
                draft.delimiter = character;
            }
        }
    });
    if side == "source" {
        ui.checkbox(&mut draft.preserve_empty_strings, "Keep empty text fields");
    } else {
        ui.checkbox(&mut draft.utf8_bom, "UTF-8 byte order mark");
    }
    previous != (draft.delimiter, draft.has_header_row)
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
    egui::ScrollArea::vertical()
        .id_salt(if source {
            "new_mapping_source_columns"
        } else {
            "new_mapping_target_columns"
        })
        .max_height(210.0)
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
                        egui::TextEdit::singleline(&mut column.name)
                            .desired_width(250.0)
                            .char_limit(256),
                    );
                    egui::ComboBox::from_id_salt(("new_mapping_column_type", source, index))
                        .selected_text(scalar_type_label(column.ty))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut column.ty, ir::ScalarType::String, "String");
                            ui.selectable_value(&mut column.ty, ir::ScalarType::Int, "Integer");
                            ui.selectable_value(&mut column.ty, ir::ScalarType::Float, "Number");
                            ui.selectable_value(&mut column.ty, ir::ScalarType::Bool, "Boolean");
                        });
                    if !source && ui.small_button("Remove").clicked() {
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
        .max_height(170.0)
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
        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
    }
}

#[derive(Clone, Copy)]
enum NewMappingAction {
    ChooseSchema(SchemaSide),
    ChooseCsvSource,
    ChooseSqlite(SchemaSide),
    LoadSqliteTable(SchemaSide),
    ConfigureCsvTarget,
    ChooseCsvTargetOutput,
    ChooseSqliteTargetOutput,
    Cancel,
    Create,
}

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
