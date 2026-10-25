use engine::RuntimeParameters;
use ir::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct HostParameterEntry {
    pub(super) name: String,
    pub(super) value: String,
}

/// Run-only host values for one project or pipeline session.
#[derive(Clone, Debug, Default)]
pub(crate) struct HostParameterEditor {
    pub(super) entries: Vec<HostParameterEntry>,
    /// Validation messages wait until the user leaves a field, so a row
    /// that was just added is not reported as an error while it is filled.
    show_errors: bool,
}

impl HostParameterEditor {
    pub(crate) fn compile(&self) -> Result<RuntimeParameters, String> {
        let mut parameters = RuntimeParameters::new();
        for (index, entry) in self.entries.iter().enumerate() {
            parameters
                .insert(&entry.name, Value::String(entry.value.clone()))
                .map_err(|error| format!("Run value {}: {error}", index + 1))?;
        }
        Ok(parameters)
    }

    /// Shows the run values table; returns whether the values compile.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui) -> bool {
        use crate::ui_kit::{self, FieldWidth, ListHeight};

        ui.horizontal(|ui| {
            ui.label(ui_kit::TextRole::Section.text("Run values"));
            ui_kit::help_icon(
                ui,
                "Supplied values override saved preview values and connected defaults. \
                 Preview uses a saved preview value first when no run value is supplied.",
            );
        });
        let mut remove = None;
        let mut left_field = false;
        if !self.entries.is_empty() {
            ui_kit::list_area("run_values_scroll", ListHeight::Short).show(ui, |ui| {
                ui_kit::table_grid(ui, "run_values_grid", 3, |ui| {
                    ui.label(ui_kit::TextRole::Help.text("Name"));
                    ui.label(ui_kit::TextRole::Help.text("Value"));
                    ui.label("");
                    ui.end_row();
                    for (index, entry) in self.entries.iter_mut().enumerate() {
                        let name = ui.add(
                            ui_kit::text_field(&mut entry.name, FieldWidth::Medium)
                                .hint_text("Name")
                                .char_limit(mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES),
                        );
                        let value = ui.add(
                            ui_kit::text_field(&mut entry.value, FieldWidth::Medium)
                                .hint_text("Value")
                                .char_limit(engine::MAX_RUNTIME_PARAMETER_STRING_BYTES),
                        );
                        left_field |= name.lost_focus() || value.lost_focus();
                        if ui_kit::icon_button(
                            ui,
                            lucide_icons::Icon::Trash2,
                            "Remove run value",
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
        }
        if let Some(index) = remove {
            self.entries.remove(index);
        }
        if left_field {
            self.show_errors = true;
        }
        let full = self.entries.len() >= engine::MAX_RUNTIME_PARAMETERS;
        let add = ui
            .add_enabled(!full, egui::Button::new("Add value"))
            .on_disabled_hover_text("Maximum number of run values reached");
        if add.clicked() {
            self.entries.push(HostParameterEntry::default());
            self.show_errors = false;
        }
        match self.compile() {
            Ok(_) => true,
            Err(error) => {
                if self.show_errors {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                } else {
                    crate::ui_kit::help(ui, "Name each run value, or remove it, before running.");
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_values_reject_invalid_names_duplicates_and_oversized_values() {
        let mut editor = HostParameterEditor::default();
        editor.entries.push(HostParameterEntry {
            name: String::new(),
            value: "first".into(),
        });
        assert!(editor.compile().unwrap_err().contains("cannot be empty"));

        editor.entries[0].name = "mode".into();
        editor.entries.push(HostParameterEntry {
            name: "mode".into(),
            value: "second".into(),
        });
        assert!(editor.compile().unwrap_err().contains("duplicated"));

        editor.entries.pop();
        editor.entries[0].value = "x".repeat(engine::MAX_RUNTIME_PARAMETER_STRING_BYTES + 1);
        assert!(
            editor
                .compile()
                .unwrap_err()
                .contains("string value exceeds")
        );
    }

    #[test]
    fn empty_value_is_still_supplied() {
        let editor = HostParameterEditor {
            entries: vec![HostParameterEntry {
                name: "mode".into(),
                value: String::new(),
            }],
            ..Default::default()
        };
        assert!(editor.compile().is_ok());
    }
}
