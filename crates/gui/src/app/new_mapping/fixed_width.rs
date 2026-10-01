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
    ui.weak(format!("Schema: {}", draft.schema_path.display()));
    ui.horizontal(|ui| {
        ui.label(if target {
            "Output file (optional)"
        } else {
            "Input file (optional)"
        });
        ui.add(
            egui::TextEdit::singleline(&mut draft.data_path)
                .char_limit(4096)
                .desired_width(380.0)
                .hint_text(if target { "output.dat" } else { "input.dat" }),
        );
    });
    ui.weak("Widths count Unicode characters. The file path does not determine this format.");
    if let Ok(fields) = crate::extra_targets::flat_scalar_fields(&draft.schema) {
        for (field, width) in fields.into_iter().zip(&mut draft.layout.widths) {
            ui.horizontal(|ui| {
                ui.label(format!("{} width", field.name));
                ui.add(
                    egui::TextEdit::singleline(width)
                        .char_limit(10)
                        .desired_width(72.0),
                );
            });
        }
    }
    ui.horizontal(|ui| {
        ui.label("Fill character");
        ui.add(
            egui::TextEdit::singleline(&mut draft.layout.fill)
                .char_limit(1)
                .desired_width(36.0),
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
