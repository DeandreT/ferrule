use std::path::PathBuf;

/// Ownership belongs to the project snapshot that produced these diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagnosticLocation {
    Validation(engine::ValidationOwner),
    /// Import warnings currently carry file provenance, not component identity.
    ImportFile(PathBuf),
    /// Native export findings retain the exact emitted component identity.
    ExportComponent {
        name: String,
        uid: Option<u32>,
        feature: mfd::ExportCompatibilityFeature,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    pub location: Option<DiagnosticLocation>,
    /// Fingerprint of the project that produced a validation owner.
    pub project_fingerprint: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            message: message.into(),
            location: None,
            project_fingerprint: None,
        }
    }

    pub fn export_compatibility_issue(issue: mfd::ExportCompatibilityIssue) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            message: format!("{}: {}", issue.component, issue.message),
            location: Some(DiagnosticLocation::ExportComponent {
                name: issue.component,
                uid: issue.component_uid,
                feature: issue.feature,
            }),
            project_fingerprint: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Warning,
            message: message.into(),
            location: None,
            project_fingerprint: None,
        }
    }

    pub fn import_warning(message: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            location: Some(DiagnosticLocation::ImportFile(path.into())),
            ..Self::warning(message)
        }
    }

    #[cfg(test)]
    pub fn validation(project: &mapping::Project, issue: engine::ValidationIssue) -> Self {
        Self::validation_with_fingerprint(issue, &crate::layout_store::project_fingerprint(project))
    }

    pub(crate) fn validation_with_fingerprint(
        issue: engine::ValidationIssue,
        fingerprint: &str,
    ) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            message: issue.to_string(),
            location: issue.owner.map(DiagnosticLocation::Validation),
            project_fingerprint: Some(fingerprint.to_string()),
        }
    }
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
        self.replace(title, [Diagnostic::error(message)]);
    }

    pub fn warnings(
        &mut self,
        title: impl Into<String>,
        warnings: impl IntoIterator<Item = String>,
    ) {
        self.replace(title, warnings.into_iter().map(Diagnostic::warning));
    }

    pub fn validation(
        &mut self,
        project: &mapping::Project,
        issues: impl IntoIterator<Item = engine::ValidationIssue>,
    ) {
        let fingerprint = crate::layout_store::project_fingerprint(project);
        self.replace(
            "Validation",
            issues
                .into_iter()
                .map(|issue| Diagnostic::validation_with_fingerprint(issue, &fingerprint)),
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

    /// Returns a clicked diagnostic so the app can resolve its owner against
    /// the current project after this panel finishes borrowing the list.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<Diagnostic> {
        let mut navigate = None;
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
                        let label = ui.label(&item.message);
                        match &item.location {
                            Some(DiagnosticLocation::ImportFile(path)) => {
                                label.on_hover_text(format!("Imported from {}", path.display()));
                            }
                            Some(DiagnosticLocation::ExportComponent { name, uid, .. }) => {
                                let identity = uid.map_or_else(
                                    || format!("MFD component {name}"),
                                    |uid| format!("MFD component {name} (UID {uid})"),
                                );
                                label.on_hover_text(identity);
                            }
                            _ => {}
                        }
                        if matches!(item.location, Some(DiagnosticLocation::Validation(_)))
                            && ui.button("Go to").clicked()
                        {
                            navigate = Some(item.clone());
                        }
                    });
                }
            });
        navigate
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

    #[test]
    fn validation_preserves_typed_ownership_and_existing_display_text() {
        let owner = engine::ValidationOwner::GraphNode {
            function: Some(mapping::FunctionId::new(3)),
            node: 12,
        };
        let mut diagnostics = Diagnostics::default();
        let project = crate::new_mapping::blank_project();
        diagnostics.validation(
            &project,
            [engine::ValidationIssue {
                location: "the existing location text".into(),
                message: "the existing message".into(),
                owner: Some(owner.clone()),
            }],
        );

        let diagnostic = &diagnostics.items()[0];
        assert_eq!(
            diagnostic.message,
            "the existing location text: the existing message"
        );
        assert_eq!(
            diagnostic.location,
            Some(DiagnosticLocation::Validation(owner))
        );
    }

    #[test]
    fn import_warning_preserves_file_provenance_without_inventing_a_node() {
        let diagnostic = Diagnostic::import_warning("component is unsupported", "/package/map.mfd");
        assert_eq!(diagnostic.message, "component is unsupported");
        assert_eq!(
            diagnostic.location,
            Some(DiagnosticLocation::ImportFile(PathBuf::from(
                "/package/map.mfd"
            )))
        );
        assert_eq!(Diagnostic::warning("ordinary warning").location, None);
    }
}
