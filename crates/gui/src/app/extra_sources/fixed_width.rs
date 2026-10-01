use super::*;

pub(super) fn show_options(ui: &mut egui::Ui, draft: &mut ExtraSourceDraft) {
    ui.strong("Fixed-width input");
    ui.weak("Widths count Unicode characters. The instance path does not determine this format.");
    let mut abandon = false;
    let mut use_path = false;
    if let Some(pending) = draft.fixed_width_draft.as_mut() {
        if let Some(schema) = draft.schema.as_ref() {
            match crate::extra_targets::flat_scalar_fields(schema) {
                Ok(fields) => {
                    for (field, width) in fields.into_iter().zip(&mut pending.widths) {
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
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            }
            ui.horizontal(|ui| {
                ui.label("Fill character");
                ui.add(
                    egui::TextEdit::singleline(&mut pending.fill)
                        .char_limit(1)
                        .desired_width(36.0),
                );
            });
            ui.radio_value(
                &mut pending.record_delimiters,
                true,
                "Delimited records (LF output; LF or CRLF input)",
            );
            ui.radio_value(
                &mut pending.record_delimiters,
                false,
                "Contiguous records (no line endings)",
            );
            ui.checkbox(
                &mut pending.treat_empty_as_absent,
                "Treat fill-only fields as absent on input",
            );
            if let Err(error) = pending.layout_for_schema(schema) {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Choose a source schema first");
        }
        ui.horizontal(|ui| {
            abandon = ui.button("Abandon fixed-width changes").clicked();
            use_path = ui.button("Use path format").clicked();
        });
    }
    if abandon {
        draft.abandon_fixed_width();
    }
    if use_path {
        draft.use_path_format();
    }
}
