use super::*;
use crate::new_mapping::FlexTextBoundaryDraft;

impl FerruleApp {
    pub(in crate::app) fn stage_mapping_flextext(&mut self, side: SchemaSide, path: PathBuf) {
        match FlexTextBoundaryDraft::from_configuration(path) {
            Ok(draft) => {
                if let Some(setup) = self.new_mapping_setup.as_mut() {
                    let boundary = MappingBoundary::FlexText(Box::new(draft));
                    match side {
                        SchemaSide::Source => setup.source = Some(boundary),
                        SchemaSide::Target => setup.target = Some(boundary),
                    }
                    self.status = format!("loaded {} FlexText layout", side.label().to_lowercase());
                    self.diagnostics.clear();
                }
            }
            Err(error) => {
                self.status = format!(
                    "failed to load {} FlexText layout",
                    side.label().to_lowercase()
                );
                self.diagnostics
                    .error("FlexText layout import failed", format!("{error:#}"));
            }
        }
    }
}

pub(super) fn show_options(ui: &mut egui::Ui, draft: &mut FlexTextBoundaryDraft, target: bool) {
    ui.label(format!("Root: {}", draft.layout().root_name()));
    let line_ending = match draft.layout().output_line_ending() {
        mapping::FlexLineEnding::Lf => "LF",
        mapping::FlexLineEnding::Crlf => "CRLF",
    };
    ui.label(format!("Output line endings: {line_ending}"));
    ui.label(format!(
        "UTF-8 byte order mark on output: {}",
        if draft.layout().write_bom() {
            "On"
        } else {
            "Off"
        }
    ));
    ui.horizontal(|ui| {
        ui.label(if target {
            "Output file (optional)"
        } else {
            "Input file (optional)"
        });
        ui.add(
            egui::TextEdit::singleline(&mut draft.instance_path)
                .desired_width(380.0)
                .hint_text(if target { "output.txt" } else { "input.txt" }),
        );
    });
    ui.weak("The project keeps this layout. You can choose a data file when running.");
    if let Err(error) = draft.validate() {
        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
    }
}

#[cfg(test)]
#[path = "flextext_tests.rs"]
mod tests;
