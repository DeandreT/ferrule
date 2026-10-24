use super::*;
use crate::new_mapping::FixedWidthBoundaryDraft;

impl FerruleApp {
    pub(super) fn configure_mapping_fixed_width(&mut self, side: SchemaSide) {
        let result = (|| -> anyhow::Result<FixedWidthBoundaryDraft> {
            let setup = self
                .new_mapping_setup
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("start a new mapping first"))?;
            let boundary = match side {
                SchemaSide::Source => setup.source.as_ref(),
                SchemaSide::Target => setup.target.as_ref(),
            };
            let Some(MappingBoundary::Schema(imported)) = boundary else {
                anyhow::bail!("import a flat XSD or JSON Schema before configuring fixed-width");
            };
            FixedWidthBoundaryDraft::from_imported(imported)
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
                *slot = Some(MappingBoundary::FixedWidth(Box::new(draft)));
                self.status = format!(
                    "configuring {} fixed-width layout",
                    side.label().to_lowercase()
                );
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = format!(
                    "cannot configure {} fixed-width layout",
                    side.label().to_lowercase()
                );
                self.diagnostics
                    .error("Fixed-width setup failed", format!("{error:#}"));
            }
        }
    }

    pub(super) fn abandon_mapping_fixed_width(&mut self, side: SchemaSide) {
        let Some(setup) = self.new_mapping_setup.as_mut() else {
            return;
        };
        let slot = match side {
            SchemaSide::Source => &mut setup.source,
            SchemaSide::Target => &mut setup.target,
        };
        *slot = slot.take().map(|boundary| match boundary {
            MappingBoundary::FixedWidth(draft) => {
                MappingBoundary::Schema(Box::new((*draft).into_imported()))
            }
            other => other,
        });
        self.status = format!(
            "kept {} schema without fixed-width settings",
            side.label().to_lowercase()
        );
        self.diagnostics.clear();
    }
}

pub(super) fn show_options(
    ui: &mut egui::Ui,
    draft: &mut FixedWidthBoundaryDraft,
    target: bool,
) -> bool {
    ui.strong("Fixed-width layout");
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
                    "Optional: output.dat"
                } else {
                    "Optional: input.dat"
                }),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::TextInput,
                ui.is_enabled(),
                if target {
                    "Target fixed-width path"
                } else {
                    "Source fixed-width path"
                },
            )
        });
    });
    ui.weak("Widths count Unicode characters. The file path does not determine this format.");
    if let Ok(fields) = crate::extra_targets::flat_scalar_fields(&draft.schema) {
        crate::ui_kit::form_grid(ui, ("new_mapping_widths", target), |form| {
            for (field, width) in fields.into_iter().zip(&mut draft.layout.widths) {
                form.row(format!("{} width", field.name), |ui| {
                    let response = ui.add(
                        crate::ui_kit::text_field(width, crate::ui_kit::FieldWidth::Small)
                            .char_limit(10),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::Role::TextInput,
                            ui.is_enabled(),
                            format!(
                                "{} {} width",
                                if target { "Target" } else { "Source" },
                                field.name
                            ),
                        )
                    });
                });
            }
        });
    }
    ui.horizontal(|ui| {
        ui.label("Fill character");
        ui.add(
            crate::ui_kit::text_field(&mut draft.layout.fill, crate::ui_kit::FieldWidth::Small)
                .char_limit(1),
        );
    });
    ui.radio_value(
        &mut draft.layout.record_delimiters,
        true,
        "Delimited records (LF output; LF or CRLF input)",
    );
    ui.radio_value(
        &mut draft.layout.record_delimiters,
        false,
        "Contiguous records (no line endings)",
    );
    ui.checkbox(
        &mut draft.layout.treat_empty_as_absent,
        "Treat fill-only fields as absent on input",
    );
    if let Err(error) = draft.validate() {
        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
    }
    ui.button("Abandon fixed-width setup").clicked()
}
