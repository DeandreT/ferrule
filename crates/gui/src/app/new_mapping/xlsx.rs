use super::*;
use crate::new_mapping::XlsxBoundaryDraft;

impl FerruleApp {
    pub(super) fn configure_mapping_xlsx(&mut self, side: SchemaSide) {
        if self.pending_dialog.is_some() {
            return;
        }
        let result = (|| -> anyhow::Result<XlsxBoundaryDraft> {
            let setup = self
                .new_mapping_setup
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("start a new mapping first"))?;
            let boundary = match side {
                SchemaSide::Source => setup.source.as_ref(),
                SchemaSide::Target => setup.target.as_ref(),
            };
            let Some(MappingBoundary::Schema(imported)) = boundary else {
                anyhow::bail!("choose a flat XSD or JSON Schema before configuring a workbook");
            };
            XlsxBoundaryDraft::from_imported(imported)
        })();
        match result {
            Ok(draft) => {
                let Some(setup) = self.new_mapping_setup.as_mut() else {
                    return;
                };
                let slot = match side {
                    SchemaSide::Source => &mut setup.source,
                    SchemaSide::Target => &mut setup.target,
                };
                *slot = Some(MappingBoundary::Xlsx(Box::new(draft)));
                self.status = format!("configuring {} workbook", side.label().to_lowercase());
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = format!("cannot configure {} workbook", side.label().to_lowercase());
                self.diagnostics
                    .error("Workbook setup failed", format!("{error:#}"));
            }
        }
    }

    pub(super) fn abandon_mapping_xlsx(&mut self, side: SchemaSide) {
        if self.pending_dialog.is_some() {
            return;
        }
        let Some(setup) = self.new_mapping_setup.as_mut() else {
            return;
        };
        let slot = match side {
            SchemaSide::Source => &mut setup.source,
            SchemaSide::Target => &mut setup.target,
        };
        *slot = slot.take().map(|boundary| match boundary {
            MappingBoundary::Xlsx(draft) => {
                MappingBoundary::Schema(Box::new((*draft).into_imported()))
            }
            other => other,
        });
        self.status = format!(
            "kept {} schema without workbook settings",
            side.label().to_lowercase()
        );
        self.diagnostics.clear();
    }
}

pub(super) fn show_options(ui: &mut egui::Ui, draft: &mut XlsxBoundaryDraft, target: bool) -> bool {
    ui.strong("Workbook table");
    ui.vertical(|ui| {
        ui.label(if target {
            "Output file (optional)"
        } else {
            "Input file (optional)"
        });
        let response = ui.add(
            crate::ui_kit::text_field(&mut draft.data_path, crate::ui_kit::FieldWidth::Fill)
                .char_limit(4096)
                .hint_text(if target {
                    "Optional: output.xlsx"
                } else {
                    "Optional: input.xlsx"
                }),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::TextInput,
                ui.is_enabled(),
                if target {
                    "Target workbook path"
                } else {
                    "Source workbook path"
                },
            )
        });
    });
    ui.vertical(|ui| {
        ui.label("Sheet name (optional)");
        ui.add(
            crate::ui_kit::text_field(&mut draft.sheet, crate::ui_kit::FieldWidth::Fill)
                .char_limit(31),
        );
    });
    ui.weak(if target {
        "Leave the sheet name empty to use the default sheet."
    } else {
        "Leave the sheet name empty to read the first sheet."
    });
    if !target {
        let eligible = draft.supports_transposed();
        ui.vertical(|ui| {
            let label = ui.label("Read layout");
            egui::ComboBox::from_id_salt("new_mapping_workbook_source_layout")
                .selected_text(if draft.transposed {
                    "Columns as records (transposed)"
                } else {
                    "Rows as records"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut draft.transposed, false, "Rows as records");
                    ui.add_enabled_ui(eligible, |ui| {
                        ui.selectable_value(
                            &mut draft.transposed,
                            true,
                            "Columns as records (transposed)",
                        );
                    });
                })
                .response
                .labelled_by(label.id);
        });
        if !eligible {
            ui.weak("Transposed columns need a data field; `n`, if present, must be an integer.");
        }
    }
    if draft.transposed && !target {
        ui.weak(
            "Selected rows become fields in schema order. The first selected row drives records.",
        );
        crate::ui_kit::list_area(
            "new_mapping_transposed_fields",
            crate::ui_kit::ListHeight::Medium,
        )
        .show(ui, |ui| {
            if let Ok(fields) = crate::extra_targets::flat_scalar_fields(&draft.schema) {
                egui::Grid::new("new_mapping_transposed_rows")
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Field");
                        ui.strong("Worksheet row");
                        ui.end_row();
                        let mut rows = draft.rows.iter_mut();
                        for (index, field) in fields.iter().enumerate() {
                            let label = ui.label(&field.name);
                            if field.name == "n" {
                                ui.weak("Physical column number (gaps kept)");
                            } else if let Some(row) = rows.next() {
                                let response = ui
                                    .add(
                                        crate::ui_kit::text_field(
                                            &mut *row,
                                            crate::ui_kit::FieldWidth::Small,
                                        )
                                        .id_salt(("new_mapping_transposed_row", index))
                                        .char_limit(7),
                                    )
                                    .labelled_by(label.id);
                                response.widget_info(|| egui::WidgetInfo {
                                    current_text_value: Some(row.clone()),
                                    ..egui::WidgetInfo::labeled(
                                        egui::Role::TextInput,
                                        ui.is_enabled(),
                                        format!("Worksheet row for {}", field.name),
                                    )
                                });
                            }
                            ui.end_row();
                        }
                    });
            }
        });
        ui.weak("Rows are numbered from 1. No header row is skipped in this layout.");
    } else {
        ui.checkbox(
            &mut draft.has_header_row,
            if target {
                "Write header row"
            } else {
                "Skip header row"
            },
        );
        ui.horizontal(|ui| {
            ui.label(if draft.has_header_row {
                "Header row"
            } else {
                "First data row"
            });
            ui.add(
                crate::ui_kit::text_field(&mut draft.start_row, crate::ui_kit::FieldWidth::Small)
                    .char_limit(10),
            );
        });
        ui.weak(
            "Worksheet columns are numbered from 1 (A). Fields keep their schema names and types.",
        );
        crate::ui_kit::list_area(
            ("new_mapping_workbook_fields", target),
            crate::ui_kit::ListHeight::Medium,
        )
        .show(ui, |ui| {
            if let Ok(fields) = crate::extra_targets::flat_scalar_fields(&draft.schema) {
                egui::Grid::new(("new_mapping_workbook_columns", target))
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Field");
                        ui.strong("Worksheet column");
                        if target && draft.has_header_row {
                            ui.strong("Header text");
                        }
                        ui.end_row();
                        for (index, (field, column)) in
                            fields.iter().zip(&mut draft.columns).enumerate()
                        {
                            ui.label(&field.name);
                            ui.add(
                                crate::ui_kit::text_field(column, crate::ui_kit::FieldWidth::Small)
                                    .char_limit(5),
                            );
                            if target
                                && draft.has_header_row
                                && let Some(header) = draft.headers.get_mut(index)
                            {
                                ui.add(crate::ui_kit::text_field(
                                    header,
                                    crate::ui_kit::FieldWidth::Small,
                                ));
                            }
                            ui.end_row();
                        }
                    });
            }
        });
    }
    if let Err(error) = draft.validate_for(target) {
        ui.colored_label(ui.visuals().error_fg_color, format!("{error:#}"));
    }
    ui.button("Abandon workbook setup").clicked()
}

#[cfg(test)]
#[path = "xlsx_tests.rs"]
mod tests;
