use super::*;
use crate::new_mapping::SqliteBoundaryDraft;

impl FerruleApp {
    pub(super) fn begin_extra_source(&mut self) {
        self.extra_source_draft = Some(ExtraSourceDraft::default());
    }

    pub(super) fn stage_extra_source_schema(&mut self, path: PathBuf) {
        match crate::new_mapping::import_schema(&path) {
            Ok(schema) => {
                let Some(draft) = self.extra_source_draft.as_mut() else {
                    return;
                };
                if draft.name.trim().is_empty() {
                    draft.name.clone_from(&schema.name);
                }
                draft.set_schema(schema);
                self.status = format!("loaded extra source schema {}", path.display());
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "failed to load extra source schema".to_string();
                self.diagnostics
                    .error("Schema import failed", error.to_string());
            }
        }
    }

    pub(super) fn stage_extra_source_sqlite_table(&mut self) {
        let Some(draft) = self.extra_source_draft.as_mut() else {
            return;
        };
        draft.clear_schema();
        let result = (|| {
            let mut sqlite =
                SqliteBoundaryDraft::source(PathBuf::from(draft.instance_path.trim()))?;
            sqlite.set_table(draft.sqlite_table.clone());
            sqlite.load_schema()?;
            sqlite.schema(false)
        })();
        match result {
            Ok(schema) => {
                if draft.name.trim().is_empty() {
                    draft.name.clone_from(&schema.name);
                }
                draft.set_sqlite_schema(schema);
                self.status = "loaded extra source SQLite table".to_owned();
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "failed to load extra source SQLite table".to_owned();
                self.diagnostics
                    .error("SQLite table import failed", error.to_string());
            }
        }
    }

    pub(super) fn show_extra_source_setup(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.extra_source_draft.as_mut() else {
            return;
        };
        let dialog_idle = self.pending_dialog.is_none();
        let schema_label = draft
            .schema
            .as_ref()
            .map_or_else(|| "Not selected".to_owned(), |schema| schema.name.clone());
        let can_add = !draft.name.trim().is_empty()
            && !draft.instance_path.trim().is_empty()
            && draft.schema.is_some();
        let sqlite_instance = is_sqlite_instance_path(&draft.instance_path);
        let mut action = None;
        egui::Window::new("Add Extra Source")
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("extra_source_fields")
                    .num_columns(3)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong("Name");
                        ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(260.0));
                        ui.end_row();

                        ui.strong("Instance");
                        let mut instance_path = draft.instance_path.clone();
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut instance_path).desired_width(260.0),
                            )
                            .changed()
                        {
                            draft.set_instance_path(instance_path);
                        }
                        if ui
                            .add_enabled(dialog_idle, egui::Button::new("Choose..."))
                            .clicked()
                        {
                            action = Some(ExtraSourceAction::ChooseInstance);
                        }
                        ui.end_row();

                        if sqlite_instance {
                            ui.strong("SQLite table");
                            let mut table = draft.sqlite_table.clone();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut table)
                                        .desired_width(260.0)
                                        .char_limit(256)
                                        .hint_text("Existing table name"),
                                )
                                .changed()
                            {
                                draft.set_sqlite_table(table);
                            }
                            if ui
                                .add_enabled(
                                    dialog_idle && !draft.sqlite_table.trim().is_empty(),
                                    egui::Button::new("Load table"),
                                )
                                .clicked()
                            {
                                action = Some(ExtraSourceAction::LoadSqliteTable);
                            }
                            ui.end_row();
                        }

                        ui.strong("Schema");
                        ui.label(schema_label);
                        if ui
                            .add_enabled(dialog_idle, egui::Button::new("Choose..."))
                            .clicked()
                        {
                            action = Some(ExtraSourceAction::ChooseSchema);
                        }
                        ui.end_row();
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Cancel"))
                        .clicked()
                    {
                        action = Some(ExtraSourceAction::Cancel);
                    }
                    if ui
                        .add_enabled(dialog_idle && can_add, egui::Button::new("Add source"))
                        .clicked()
                    {
                        action = Some(ExtraSourceAction::Add);
                    }
                });
            });

        match action {
            Some(ExtraSourceAction::ChooseInstance) => {
                self.pending_dialog = Some((
                    DialogKind::BrowseExtraSourceInstance,
                    pick_file(
                        "input data",
                        &[
                            "csv", "xml", "json", "db", "sqlite", "sqlite3", "edi", "x12",
                            "edifact",
                        ],
                    ),
                ));
            }
            Some(ExtraSourceAction::ChooseSchema) => {
                self.pending_dialog = Some((
                    DialogKind::BrowseExtraSourceSchema,
                    pick_file("schema", &["xsd", "json"]),
                ));
            }
            Some(ExtraSourceAction::LoadSqliteTable) => {
                self.stage_extra_source_sqlite_table();
            }
            Some(ExtraSourceAction::Cancel) => {
                self.extra_source_draft = None;
                self.status = "extra source cancelled".to_string();
            }
            Some(ExtraSourceAction::Add) => self.finish_extra_source(),
            None => {}
        }
    }

    fn finish_extra_source(&mut self) {
        let Some(draft) = self.extra_source_draft.as_ref() else {
            return;
        };
        match draft.clone().build(&self.project.extra_sources) {
            Ok(source) => {
                self.project.extra_sources.push(source);
                self.extra_source_draft = None;
                self.status = "extra source added".to_string();
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "extra source is incomplete".to_string();
                self.diagnostics
                    .error("Extra source not added", error.to_string());
            }
        }
    }

    pub(super) fn show_extra_source_removal_confirmation(&mut self, ctx: &egui::Context) {
        let Some(index) = self.pending_extra_source_removal else {
            return;
        };
        let Some(source) = self.project.extra_sources.get(index) else {
            self.pending_extra_source_removal = None;
            return;
        };
        let name = source.name.clone();
        let mut remove = None;
        egui::Window::new("Remove Extra Source")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Remove {name} from this mapping?"));
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        remove = Some(false);
                    }
                    if ui.button("Remove").clicked() {
                        remove = Some(true);
                    }
                });
            });
        match remove {
            Some(true) => {
                let removed = remove_extra_source(&mut self.project.extra_sources, index);
                self.pending_extra_source_removal = None;
                if let Some(source) = removed {
                    self.status = format!("removed extra source {}", source.name);
                    let issues = cli::validate(&self.project);
                    if issues.is_empty() {
                        self.diagnostics.clear();
                    } else {
                        self.diagnostics.validation(&self.project, issues);
                    }
                }
            }
            Some(false) => self.pending_extra_source_removal = None,
            None => {}
        }
    }
}

enum ExtraSourceAction {
    ChooseInstance,
    ChooseSchema,
    LoadSqliteTable,
    Cancel,
    Add,
}

fn is_sqlite_instance_path(path: &str) -> bool {
    let extension = std::path::Path::new(path.trim())
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "db" | "sqlite" | "sqlite3"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_mapping::{CsvBoundaryDraft, CsvColumnDraft, MappingBoundary};

    #[test]
    fn csv_lookup_uses_introspected_named_sqlite_source_after_reopen() -> anyhow::Result<()> {
        let directory =
            std::env::temp_dir().join(format!("ferrule-gui-sqlite-lookup-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let customers_csv = directory.join("customers.csv");
        let customers_db = directory.join("customers.db");
        let orders_csv = directory.join("orders.csv");
        let output_csv = directory.join("output.csv");
        std::fs::write(&customers_csv, "id,name\n1,Jane\n2,John\n")?;
        std::fs::write(&orders_csv, "customer_id\n2\n1\n")?;

        let columns = vec![
            ir::SchemaNode::scalar("id", ir::ScalarType::Int),
            ir::SchemaNode::scalar("name", ir::ScalarType::String),
        ];
        let mut seed = crate::new_mapping::blank_project();
        seed.source = ir::SchemaNode::group("row", columns.clone());
        seed.target = ir::SchemaNode::group("customers", columns).repeating();
        seed.source_path = Some(customers_csv.to_str().unwrap().to_owned());
        seed.target_path = Some(customers_db.to_str().unwrap().to_owned());
        seed.root.iteration = mapping::ScopeIteration::Source(vec![]);
        for (id, column) in [(1, "id"), (2, "name")] {
            seed.graph.nodes.insert(
                id,
                mapping::Node::SourceField {
                    path: vec![column.to_owned()],
                    frame: None,
                },
            );
            seed.root.bindings.push(mapping::Binding {
                target_field: column.to_owned(),
                node: id,
            });
        }
        let seed_project_path = directory.join("seed.json");
        std::fs::write(&seed_project_path, serde_json::to_vec_pretty(&seed)?)?;
        cli::run_project_with_paths(&seed_project_path, None, None)?;

        let mut app = FerruleApp::default();
        app.begin_new_mapping();
        app.stage_mapping_csv_source(orders_csv.clone());
        let setup = app.new_mapping_setup.as_mut().unwrap();
        let Some(MappingBoundary::Csv(source)) = setup.source.as_mut() else {
            panic!("CSV source was not staged");
        };
        source.columns[0].ty = ir::ScalarType::Int;
        let mut target = CsvBoundaryDraft::target();
        target.path = output_csv.to_str().unwrap().to_owned();
        target.columns = vec![CsvColumnDraft {
            name: "customer_name".to_owned(),
            ty: ir::ScalarType::String,
        }];
        setup.target = Some(MappingBoundary::Csv(target));
        app.finish_new_mapping();

        app.begin_extra_source();
        let draft = app.extra_source_draft.as_mut().unwrap();
        draft.set_instance_path(customers_db.to_str().unwrap().to_owned());
        draft.set_sqlite_table("customers".to_owned());
        app.stage_extra_source_sqlite_table();
        assert_eq!(app.extra_source_draft.as_ref().unwrap().name, "customers");
        app.finish_extra_source();
        assert_eq!(app.project.extra_sources.len(), 1);
        assert!(app.project.extra_sources[0].schema.repeating);
        assert_eq!(
            app.project.extra_sources[0].path,
            customers_db.to_str().unwrap()
        );

        app.project.graph.nodes.insert(
            1,
            mapping::Node::SourceField {
                path: vec!["customer_id".to_owned()],
                frame: None,
            },
        );
        app.project.graph.nodes.insert(
            2,
            mapping::Node::Lookup {
                collection: vec!["customers".to_owned()],
                key: vec!["id".to_owned()],
                matches: 1,
                value: vec!["name".to_owned()],
            },
        );
        app.project.root.iteration = mapping::ScopeIteration::Source(vec![]);
        app.project.root.bindings.push(mapping::Binding {
            target_field: "customer_name".to_owned(),
            node: 2,
        });
        let project_path = directory.join("lookup.json");
        app.save_document_to(&project_path)?;
        app.load_project_from(&project_path);
        assert!(cli::validate(&app.project).is_empty());
        cli::run_project_with_paths(&project_path, None, None)?;
        assert_eq!(
            std::fs::read_to_string(&output_csv)?,
            "customer_name\nJohn\nJane\n"
        );
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }
}
