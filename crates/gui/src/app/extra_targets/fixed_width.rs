use super::*;

pub(super) fn show_options(ui: &mut egui::Ui, draft: &mut ExtraTargetDraft) {
    ui.strong("Fixed-width output");
    ui.weak("Widths count Unicode characters. The saved layout applies to any output filename.");
    let mut abandon = false;
    if let Some(pending) = draft.fixed_width_draft.as_mut() {
        if let Some(schema) = draft.schema.as_ref() {
            match crate::extra_targets::flat_scalar_fields(schema) {
                Ok(fields) => {
                    crate::ui_kit::form_grid(ui, "target_fixed_widths", |form| {
                        for (field, width) in fields.into_iter().zip(&mut pending.widths) {
                            form.row(format!("{} width", field.name), |ui| {
                                ui.add(
                                    crate::ui_kit::text_field(
                                        width,
                                        crate::ui_kit::FieldWidth::Small,
                                    )
                                    .char_limit(10),
                                );
                            });
                        }
                    });
                }
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            }
            ui.horizontal(|ui| {
                ui.label("Fill character");
                ui.add(
                    crate::ui_kit::text_field(&mut pending.fill, crate::ui_kit::FieldWidth::Small)
                        .char_limit(1),
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
            ui.colored_label(ui.visuals().error_fg_color, "Choose a target schema first");
        }
        abandon = ui.button("Abandon fixed-width changes").clicked();
    }
    if abandon {
        draft.abandon_fixed_width();
    }
}
