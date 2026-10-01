//! MFD export profile selection and GUI diagnostics.

use std::path::Path;

use mfd::{ExportProfile, MfdError};

use super::FerruleApp;
use crate::diagnostics::Diagnostic;

impl FerruleApp {
    pub(super) fn mfd_export_suggestion(&self) -> String {
        self.document
            .suggested_path()
            .with_extension("mfd")
            .display()
            .to_string()
    }

    pub(super) fn finish_mfd_export(&mut self, path: &Path, profile: ExportProfile) {
        let native = profile == ExportProfile::NativeMfd;
        let qualifier = if native { "native MFD " } else { "" };
        match mfd::export_with_profile(&self.project, path, profile) {
            Ok(report) if report.warnings.is_empty() => {
                self.status = format!("exported {qualifier}{}", path.display());
                self.diagnostics.clear();
            }
            Ok(report) => {
                self.status = format!(
                    "exported {qualifier}{} with {} warning(s)",
                    path.display(),
                    report.warnings.len()
                );
                self.diagnostics.warnings("MFD export", report.warnings);
            }
            Err(MfdError::IncompatibleExport(report)) => {
                let report = *report;
                self.status = format!("native MFD export rejected for {}", path.display());
                let issues = report
                    .issues
                    .into_iter()
                    .map(Diagnostic::export_compatibility_issue);
                let warnings = report
                    .warnings
                    .into_iter()
                    .map(|warning| Diagnostic::error(format!("Export warning: {warning}")));
                self.diagnostics
                    .replace("Native MFD export rejected", issues.chain(warnings));
            }
            Err(error) => {
                self.status = format!("failed to export {qualifier}{}", path.display());
                self.diagnostics
                    .error("MFD export failed", error.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;

    use ir::{ScalarType, SchemaNode};
    use mapping::{Binding, Node, Scope};
    use mfd::ExportCompatibilityFeature;

    use super::super::DialogKind;
    use super::*;
    use crate::diagnostics::{DiagnosticLevel, DiagnosticLocation};
    use crate::document::DocumentLocation;

    struct TestDir(std::path::PathBuf);

    impl TestDir {
        fn new() -> std::io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule-gui-native-mfd-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path)?;
            Ok(Self(path))
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn mapped_xml_app() -> FerruleApp {
        let mut app = FerruleApp::default();
        app.project.source = SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        );
        app.project.target = SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        );
        app.project.source_path = Some("source.xml".into());
        app.project.target_path = Some("target.xml".into());
        app.project.graph.nodes.insert(
            0,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: None,
            },
        );
        app.project.root = Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        };
        app
    }

    fn complete_export_dialog(app: &mut FerruleApp, kind: DialogKind, path: &Path) {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Some(path.to_string_lossy().into_owned()))
            .expect("dialog path is sent");
        app.pending_dialog = Some((kind, receiver));
        app.poll_dialog(&egui::Context::default());
        assert!(app.pending_dialog.is_none());
    }

    #[test]
    fn export_suggestion_changes_the_extension_without_changing_the_project_name() {
        let mut app = FerruleApp::default();
        let saved = std::env::temp_dir()
            .join("Map Work")
            .join("Részumé project.v2.json");
        app.document = DocumentLocation::saved(saved.clone());
        let suggested = app.mfd_export_suggestion();
        let expected = saved.with_extension("mfd");
        assert_eq!(std::path::Path::new(&suggested), expected.as_path());
        assert_ne!(std::path::Path::new(&suggested), saved.as_path());
        assert!(suggested.ends_with("Részumé project.v2.mfd"));

        app.document = DocumentLocation::untitled("project.json");
        assert_eq!(app.mfd_export_suggestion(), "project.mfd");
    }

    #[test]
    fn native_export_dialog_publishes_a_strictly_reimportable_design()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TestDir::new()?;
        let path = temp.0.join("mapping.mfd");
        let mut app = mapped_xml_app();
        complete_export_dialog(&mut app, DialogKind::ExportNativeMfd, &path);
        assert_eq!(
            app.status,
            format!("exported native MFD {}", path.display())
        );
        assert!(app.diagnostics.is_empty());
        let outcome = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )?;
        let imported = outcome.imported;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(imported.project.source, app.project.source);
        assert_eq!(imported.project.target, app.project.target);
        Ok(())
    }

    #[test]
    fn native_rejection_keeps_existing_artifacts_and_component_findings()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TestDir::new()?;
        let path = temp.0.join("mapping.mfd");
        let siblings = [
            temp.0.join("mapping-source.xsd"),
            temp.0.join("mapping-target.xsd"),
        ];
        for artifact in std::iter::once(&path).chain(siblings.iter()) {
            std::fs::write(artifact, b"preserve existing content")?;
        }
        let mut app = mapped_xml_app();
        app.project.graph.nodes.insert(
            1,
            Node::Call {
                function: "isbn10_to_isbn13".into(),
                args: vec![0],
            },
        );
        app.project.root.bindings[0].node = 1;
        complete_export_dialog(&mut app, DialogKind::ExportNativeMfd, &path);
        assert_eq!(
            app.status,
            format!("native MFD export rejected for {}", path.display())
        );
        let diagnostics = app.diagnostics.items();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].level, DiagnosticLevel::Error);
        assert!(diagnostics[0].message.contains("isbn10_to_isbn13"));
        assert!(matches!(
            &diagnostics[0].location,
            Some(DiagnosticLocation::ExportComponent {
                name,
                uid: Some(_),
                feature: ExportCompatibilityFeature::FerruleComponent,
            }) if name == "isbn10_to_isbn13"
        ));
        for artifact in std::iter::once(&path).chain(siblings.iter()) {
            assert_eq!(
                std::fs::read(artifact)?.as_slice(),
                b"preserve existing content"
            );
        }
        assert_eq!(std::fs::read_dir(&temp.0)?.count(), 3);

        // The ordinary action still chooses the Ferrule-extension profile.
        complete_export_dialog(&mut app, DialogKind::ExportMfd, &path);
        assert_eq!(app.status, format!("exported {}", path.display()));
        assert!(std::fs::read_to_string(&path)?.contains("library=\"ferrule\""));
        Ok(())
    }
}
