use crate::new_mapping::{CsvDialectDraft, CsvQuoteMode};

/// Returns whether a control changed. Callers keep a local copy until that
/// happens, so drawing an untouched editor never changes saved format options.
pub(super) fn show_fields(
    ui: &mut egui::Ui,
    draft: &mut CsvDialectDraft,
    path: &str,
    source: bool,
    salt: &str,
) -> bool {
    let mut changed = false;
    ui.strong(if source { "CSV input" } else { "CSV output" });
    changed |= ui.checkbox(&mut draft.headers, "Header row").changed();
    ui.horizontal(|ui| {
        ui.label("Delimiter");
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut draft.delimiter)
                    .char_limit(8)
                    .desired_width(44.0),
            )
            .changed();
        if ui.small_button("Tab").clicked() {
            draft.delimiter = "\t".to_owned();
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Quoting");
        egui::ComboBox::from_id_salt(("named_csv_quote", salt))
            .selected_text(match draft.quote_mode {
                CsvQuoteMode::Standard => "Double quote",
                CsvQuoteMode::Custom => "Custom character",
                CsvQuoteMode::Disabled => "Disabled",
            })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(
                        &mut draft.quote_mode,
                        CsvQuoteMode::Standard,
                        "Double quote",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut draft.quote_mode,
                        CsvQuoteMode::Custom,
                        "Custom character",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(&mut draft.quote_mode, CsvQuoteMode::Disabled, "Disabled")
                    .changed();
            });
        if draft.quote_mode == CsvQuoteMode::Custom {
            ui.label("Quote");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut draft.custom_quote)
                        .char_limit(8)
                        .desired_width(44.0),
                )
                .changed();
        }
    });
    if source {
        changed |= ui
            .checkbox(
                &mut draft.preserve_empty_strings,
                "Keep present empty text fields",
            )
            .changed();
    } else {
        changed |= ui
            .checkbox(&mut draft.utf8_bom, "UTF-8 byte order mark")
            .changed();
    }
    if let Err(error) = draft.validated_options(path, source) {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    changed
}
