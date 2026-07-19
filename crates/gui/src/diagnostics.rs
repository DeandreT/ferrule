#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
}

#[derive(Default)]
pub struct Diagnostics {
    title: String,
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn clear(&mut self) {
        self.title.clear();
        self.items.clear();
    }

    pub fn replace(
        &mut self,
        title: impl Into<String>,
        items: impl IntoIterator<Item = Diagnostic>,
    ) {
        self.title = title.into();
        self.items = items.into_iter().collect();
    }

    pub fn error(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.replace(
            title,
            [Diagnostic {
                level: DiagnosticLevel::Error,
                message: message.into(),
            }],
        );
    }

    pub fn warnings(
        &mut self,
        title: impl Into<String>,
        warnings: impl IntoIterator<Item = String>,
    ) {
        self.replace(
            title,
            warnings.into_iter().map(|message| Diagnostic {
                level: DiagnosticLevel::Warning,
                message,
            }),
        );
    }

    pub fn validation<T: ToString>(&mut self, issues: impl IntoIterator<Item = T>) {
        self.replace(
            "Validation",
            issues.into_iter().map(|issue| Diagnostic {
                level: DiagnosticLevel::Error,
                message: issue.to_string(),
            }),
        );
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[cfg(test)]
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        let errors = self
            .items
            .iter()
            .filter(|item| item.level == DiagnosticLevel::Error)
            .count();
        let warnings = self.items.len() - errors;
        ui.horizontal(|ui| {
            ui.strong(&self.title);
            let summary = match (errors, warnings) {
                (0, warnings) => format!("{warnings} warning(s)"),
                (errors, 0) => format!("{errors} error(s)"),
                (errors, warnings) => format!("{errors} error(s), {warnings} warning(s)"),
            };
            ui.label(summary);
            if ui.button("Clear").clicked() {
                self.clear();
            }
        });
        egui::ScrollArea::vertical()
            .max_height(140.0)
            .show(ui, |ui| {
                for item in &self.items {
                    let prefix = match item.level {
                        DiagnosticLevel::Warning => "Warning:",
                        DiagnosticLevel::Error => "Error:",
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(prefix);
                        ui.label(&item.message);
                    });
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warnings_remain_individual_diagnostics() {
        let mut diagnostics = Diagnostics::default();
        diagnostics.warnings("Import", ["first".to_string(), "second".to_string()]);
        assert_eq!(diagnostics.items().len(), 2);
        assert_eq!(diagnostics.items()[0].message, "first");
        assert_eq!(diagnostics.items()[1].message, "second");
    }
}
