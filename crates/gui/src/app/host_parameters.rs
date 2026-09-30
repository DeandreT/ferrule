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

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) -> bool {
        ui.strong("Run values");
        ui.weak("Named values supplied to this run. Remove a value to use its connected default, when available.");
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("run_values_scroll")
            .max_height(170.0)
            .show(ui, |ui| {
                egui::Grid::new("run_values_grid")
                    .num_columns(3)
                    .spacing(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        for (index, entry) in self.entries.iter_mut().enumerate() {
                            ui.add(
                                egui::TextEdit::singleline(&mut entry.name)
                                    .hint_text("Name")
                                    .char_limit(mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES)
                                    .desired_width(150.0),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut entry.value)
                                    .hint_text("Value")
                                    .char_limit(engine::MAX_RUNTIME_PARAMETER_STRING_BYTES)
                                    .desired_width(190.0),
                            );
                            if ui.button("Remove").clicked() {
                                remove = Some(index);
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(index) = remove {
            self.entries.remove(index);
        }
        ui.add_enabled_ui(self.entries.len() < engine::MAX_RUNTIME_PARAMETERS, |ui| {
            if ui.button("Add value").clicked() {
                self.entries.push(HostParameterEntry::default());
            }
        });
        if self.entries.len() == engine::MAX_RUNTIME_PARAMETERS {
            ui.weak("Maximum number of run values reached.");
        }
        match self.compile() {
            Ok(_) => true,
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
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
        };
        assert!(editor.compile().is_ok());
    }
}
