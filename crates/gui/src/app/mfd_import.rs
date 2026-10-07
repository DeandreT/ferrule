//! Session-only exception-order choice shared by both MFD import workflows.

use std::path::Path;

use super::FerruleApp;

impl FerruleApp {
    pub(super) fn show_mfd_import_options(&mut self, ui: &mut egui::Ui) -> egui::Response {
        ui.checkbox(&mut self.mfd_item_ordered_exceptions, "Preserve row error order")
            .on_hover_text("Keep supported row errors in evaluation order. Unsupported exception shapes refuse import. Applies to both MFD import actions for this session.")
    }

    pub(super) fn mfd_import_options(&self) -> Result<mfd::ImportOptions, mfd::MfdError> {
        let mut options = mfd::ImportOptions::default();
        if self.mfd_item_ordered_exceptions {
            options = options.with_item_ordered_exceptions();
        }
        if let Some(manifest_path) = &self.mfd_package_manifest {
            options = options.with_package_manifest(Path::new(manifest_path))?;
        }
        Ok(options)
    }

    pub(super) fn import_mfd_with_resources(
        &self,
        path: &str,
    ) -> Result<mfd::Imported, mfd::MfdError> {
        let mapping_path = Path::new(path);
        let options = self.mfd_import_options()?;
        if self.mfd_item_ordered_exceptions {
            mfd::import_with_profile(mapping_path, &options, mfd::ImportProfile::Executable)
                .map(|outcome| outcome.imported)
        } else {
            mfd::import_with_options(mapping_path, &options)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Debug;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;

    use crate::app::{DestructiveAction, DialogKind, pipeline_editor_ui};
    use crate::diagnostics::DiagnosticLevel;
    use mapping::Node;

    const DESIGN: &str = include_str!("../../../mfd/tests/fixtures/exception.mfd");
    const SOURCE: &str = include_str!("../../../mfd/tests/fixtures/exception-source.xsd");
    const TARGET: &str = include_str!("../../../mfd/tests/fixtures/exception-target.xsd");

    struct Directory(PathBuf);

    impl Directory {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule_gui_item_import_{label}_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            if std::thread::panicking()
                || std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Ok("1")
            {
                eprintln!("retained GUI item-import originals: {}", self.0.display());
            } else {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn retain(directory: &Path, label: &str, value: &impl Debug) {
        std::fs::write(
            directory.join(format!("{label}.debug.txt")),
            format!("{value:#?}\n"),
        )
        .unwrap();
    }

    fn fixture(directory: &Path, text: &str) -> PathBuf {
        std::fs::write(directory.join("exception-source.xsd"), SOURCE).unwrap();
        std::fs::write(directory.join("exception-target.xsd"), TARGET).unwrap();
        std::fs::write(directory.join("expenses.xml"), "<Expenses><Expense><Allowed>true</Allowed><Description>fixture</Description></Expense></Expenses>").unwrap();
        let path = directory.join("design.mfd");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn two_exceptions() -> String {
        DESIGN.replace("        <component name=\"reject-expense\"", r#"        <component name="constant" library="core" kind="2" uid="6"><targets><datapoint pos="0" key="60"/></targets><data><constant value="first rule" datatype="string"/></data></component>
        <component name="first-exception" library="core" kind="18" uid="7"><sources><datapoint pos="0" key="61"/><datapoint pos="1" key="62"/></sources><data><exception/></data></component>
        <component name="reject-expense""#)
            .replace("      <edge from=\"23\" to=\"40\"/>", "      <edge from=\"10\" to=\"61\"/>\n      <edge from=\"60\" to=\"62\"/>\n      <edge from=\"23\" to=\"40\"/>")
    }

    fn exception_pipeline() -> String {
        DESIGN.replace("<properties XSLTDefaultOutput=\"1\"/>", "<properties PassThrough=\"1\"/>")
            .replace("<entry name=\"Expense\" inpkey=\"30\"><entry name=\"Description\" inpkey=\"31\"/>", "<entry name=\"Expense\" inpkey=\"30\" outkey=\"32\"><entry name=\"Description\" inpkey=\"31\" outkey=\"33\"/>")
            .replace("    </children>", r#"        <component name="Final" library="xml" kind="14" uid="6"><properties XSLTDefaultOutput="1"/><data><root><entry name="Accepted"><entry name="Expense" inpkey="50"><entry name="Description" inpkey="51"/></entry></entry></root><document schema="exception-target.xsd" outputinstance="final.xml" instanceroot="{}Accepted"/></data></component>
    </children>"#)
            .replace("    </connections>", "      <edge from=\"32\" to=\"50\"/>\n      <edge from=\"33\" to=\"51\"/>\n    </connections>")
    }

    fn state(
        app: &FerruleApp,
    ) -> (
        String,
        crate::app::CanvasLayout,
        crate::document::DocumentLocation,
        bool,
        usize,
        bool,
    ) {
        (
            crate::project_state::project_snapshot_key(&app.project),
            crate::app::CanvasLayout::capture(
                &app.project,
                &app.main_canvas.snarl,
                &app.mapping_workspace,
            ),
            app.document.clone(),
            app.is_dirty(),
            app.history.undo_len(),
            app.can_undo(),
        )
    }

    fn retain_import(directory: &Path, label: &str, result: &Result<mfd::Imported, mfd::MfdError>) {
        match result {
            Ok(imported) => {
                retain(directory, &format!("{label}-project"), &imported.project);
                retain(directory, &format!("{label}-warnings"), &imported.warnings);
                retain(
                    directory,
                    &format!("{label}-mapping-path"),
                    &imported.mapping_path,
                );
            }
            Err(error) => retain(directory, &format!("{label}-error"), error),
        }
    }

    fn poll_single(app: &mut FerruleApp, path: &Path) {
        let (sender, receiver) = mpsc::channel();
        sender.send(Some(path.display().to_string())).unwrap();
        app.pending_dialog = Some((DialogKind::ImportMfd, receiver));
        app.poll_dialog(&egui::Context::default());
    }

    fn checkbox_frame(
        app: &mut FerruleApp,
        context: &egui::Context,
        directory: &Path,
        events: Vec<egui::Event>,
    ) -> egui::Rect {
        let enabled = app.ui_project_editing_enabled();
        let mut rect = None;
        let mut response_id = None;
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 240.0),
                )),
                time: Some(context.cumulative_frame_nr() as f64 / 10.0),
                events,
                ..Default::default()
            },
            |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    let response = app.show_mfd_import_options(ui);
                    rect = Some(response.rect);
                    response_id = Some(response.id);
                });
            },
        );
        retain(
            directory,
            &format!("checkbox-frame-{}", context.cumulative_frame_nr()),
            &(
                enabled,
                rect,
                response_id,
                app.mfd_item_ordered_exceptions,
                state(app),
            ),
        );
        rect.unwrap()
    }

    fn click_checkbox(
        app: &mut FerruleApp,
        context: &egui::Context,
        directory: &Path,
        position: egui::Pos2,
    ) {
        checkbox_frame(
            app,
            context,
            directory,
            vec![egui::Event::PointerMoved(position)],
        );
        for pressed in [true, false] {
            checkbox_frame(
                app,
                context,
                directory,
                vec![egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
    }

    #[test]
    fn real_session_checkbox_defaults_off_and_respects_dirty_import_guard_without_history_edit() {
        let dir = Directory::new("checkbox");
        let mut app = FerruleApp::default();
        app.mark_clean();
        app.rebase_history();
        let context = egui::Context::default();
        let before = state(&app);
        assert!(!app.mfd_item_ordered_exceptions);
        checkbox_frame(&mut app, &context, &dir.0, Vec::new());
        let rect = checkbox_frame(&mut app, &context, &dir.0, Vec::new());
        click_checkbox(&mut app, &context, &dir.0, rect.center());
        assert!(app.mfd_item_ordered_exceptions);
        assert_eq!(state(&app), before);
        app.history.mark_unsaved();
        assert!(
            app.request_destructive_action(DestructiveAction::ImportMfd)
                .is_none()
        );
        let guarded = state(&app);
        click_checkbox(&mut app, &context, &dir.0, rect.center());
        assert!(app.mfd_item_ordered_exceptions);
        assert_eq!(state(&app), guarded);
        assert!(app.pending_dialog.is_none());
        assert_eq!(
            app.pending_destructive_action,
            Some(DestructiveAction::ImportMfd)
        );
        // Existing Keep editing transition; only the session choice changes.
        app.pending_destructive_action = None;
        click_checkbox(&mut app, &context, &dir.0, rect.center());
        assert!(!app.mfd_item_ordered_exceptions);
        assert_eq!(state(&app), guarded);
        assert!(!FerruleApp::default().mfd_item_ordered_exceptions);
    }

    #[test]
    fn single_gui_import_uses_same_opt_in_and_strict_refusal_keeps_current_document() {
        for throw_true in [false, true] {
            let dir = Directory::new(if throw_true { "true" } else { "false" });
            let text = if throw_true {
                DESIGN
                    .replace("from=\"22\" to=\"30\"", "from=\"23\" to=\"30\"")
                    .replace("from=\"23\" to=\"40\"", "from=\"22\" to=\"40\"")
            } else {
                DESIGN.into()
            };
            let design = fixture(&dir.0, &text);
            let manifest = dir.0.join("package.json");
            std::fs::write(
                &manifest,
                r#"{"schemaVersion":1,"kind":"ferrule.mapping-package"}"#,
            )
            .unwrap();
            let original = std::fs::read(&design).unwrap();
            for item_order in [false, true] {
                let mut app = FerruleApp {
                    mfd_item_ordered_exceptions: item_order,
                    mfd_package_manifest: Some(manifest.display().to_string()),
                    ..Default::default()
                };
                let label = if item_order { "item" } else { "global" };
                let result = app.import_mfd_with_resources(design.to_str().unwrap());
                retain_import(&dir.0, label, &result);
                let imported = result.unwrap();
                poll_single(&mut app, &design);
                retain(&dir.0, &format!("{label}-gui-state"), &state(&app));
                assert_eq!(
                    serde_json::to_value(&app.project).unwrap(),
                    serde_json::to_value(&imported.project).unwrap()
                );
                assert_eq!(app.project.failure_rules.len(), usize::from(!item_order));
                assert_eq!(
                    app.project
                        .graph
                        .nodes
                        .values()
                        .filter(|n| matches!(n, Node::Raise { .. }))
                        .count(),
                    usize::from(item_order)
                );
                assert!(app.is_dirty());
                assert!(app.pending_dialog.is_none());
                assert_eq!(app.document.suggested_path(), design.with_extension("json"));
                assert!(!design.with_extension("json").exists());
            }
            assert_eq!(std::fs::read(&design).unwrap(), original);
            assert!(!dir.0.join("accepted.xml").exists());
        }
        let dir = Directory::new("unsupported");
        let design = fixture(&dir.0, &two_exceptions());
        let mut app = FerruleApp::default();
        poll_single(&mut app, &design);
        assert_eq!(app.project.failure_rules.len(), 2);
        app.mark_clean();
        app.rebase_history();
        app.diagnostics.clear();
        let before = state(&app);
        app.mfd_item_ordered_exceptions = true;
        let result = app.import_mfd_with_resources(design.to_str().unwrap());
        retain_import(&dir.0, "unsupported-strict", &result);
        assert!(matches!(result, Err(mfd::MfdError::IncompatibleImport(_))));
        poll_single(&mut app, &design);
        retain(&dir.0, "unsupported-gui-state", &state(&app));
        retain(
            &dir.0,
            "unsupported-gui-diagnostics",
            &app.diagnostics.items(),
        );
        assert_eq!(state(&app), before);
        assert_eq!(app.diagnostics.items().len(), 1);
        assert_eq!(app.diagnostics.items()[0].level, DiagnosticLevel::Error);
        assert!(
            app.diagnostics.items()[0]
                .message
                .contains("requires one exception")
        );
        assert!(!design.with_extension("json").exists());
        assert!(!dir.0.join("accepted.xml").exists());
    }

    #[test]
    fn pipeline_choice_and_manifest_are_captured_through_dirty_guard_and_no_fallback() {
        let dir = Directory::new("pipeline-capture");
        let design = fixture(&dir.0, &exception_pipeline());
        let manifest = dir.0.join("package.json");
        std::fs::write(
            &manifest,
            r#"{"schemaVersion":1,"kind":"ferrule.mapping-package"}"#,
        )
        .unwrap();
        let default_result = mfd::import_pipeline(&design);
        match &default_result {
            Ok(imported) => {
                retain(&dir.0, "default-pipeline", &imported.pipeline);
                retain(&dir.0, "default-warnings", &imported.warnings);
            }
            Err(error) => retain(&dir.0, "default-error", error),
        }
        let (document, warnings) = crate::pipeline_edit::PipelineEditorDocument::from_imported_mfd(
            &dir.0.join("old.pipeline.json"),
            default_result.unwrap(),
        )
        .unwrap();
        retain(&dir.0, "old-document-warnings", &warnings);
        let mut app = FerruleApp {
            pipeline_editor: Some(pipeline_editor_ui::PipelineEditorUi::new(document)),
            mfd_item_ordered_exceptions: true,
            mfd_package_manifest: Some(manifest.display().to_string()),
            ..Default::default()
        };
        let old = crate::project_state::pipeline_snapshot_key(
            &app.pipeline_editor.as_ref().unwrap().document.pipeline,
        );
        let main = state(&app);
        let (sender, receiver) = mpsc::channel();
        app.pipeline_mfd_dialog_override = Some(receiver);
        app.begin_mfd_pipeline_import();
        let Some(pipeline_editor_ui::PipelineEditorAction::ImportMfd(options)) =
            &app.pending_pipeline_editor_action
        else {
            panic!("dirty pipeline import must capture its options before the shared guard");
        };
        retain(&dir.0, "captured-package-root", &options.package_root());
        assert_eq!(
            options.package_root(),
            Some(std::fs::canonicalize(&dir.0).unwrap().as_path())
        );
        let captured_probe = mfd::import_pipeline_with_options(&design, options);
        match &captured_probe {
            Ok(imported) => {
                retain(&dir.0, "captured-probe-pipeline", &imported.pipeline);
                retain(&dir.0, "captured-probe-warnings", &imported.warnings);
            }
            Err(error) => retain(&dir.0, "captured-probe-error", error),
        }
        assert!(
            matches!(&captured_probe, Err(mfd::MfdError::UnsupportedImport(message)) if message.contains("requires one exception"))
        );
        assert!(app.pending_dialog.is_none());
        // The queued action owns the original options, regardless of later settings.
        app.mfd_item_ordered_exceptions = false;
        app.mfd_package_manifest = None;
        app.discard_pending_pipeline_editor_action(&egui::Context::default());
        sender.send(Some(design.display().to_string())).unwrap();
        app.poll_dialog(&egui::Context::default());
        retain(
            &dir.0,
            "captured-strict-gui-diagnostics",
            &app.diagnostics.items(),
        );
        retain(&dir.0, "captured-strict-gui-main-state", &state(&app));
        assert!(
            app.diagnostics
                .items()
                .iter()
                .any(|d| d.level == DiagnosticLevel::Error
                    && d.message.contains("requires one exception"))
        );
        assert_eq!(
            crate::project_state::pipeline_snapshot_key(
                &app.pipeline_editor.as_ref().unwrap().document.pipeline
            ),
            old
        );
        assert_eq!(state(&app), main);
        assert!(!app.pipeline_mfd_busy());
        assert!(app.pending_dialog.is_none());
        assert!(!dir.0.join("old.pipeline.json").exists());
        assert!(!design.with_extension("json").exists());
        assert!(!dir.0.join("accepted.xml").exists());
        assert!(!dir.0.join("final.xml").exists());
        // The unchanged default choice still completes both existing choosers.
        let (source_sender, source_receiver) = mpsc::channel();
        app.pipeline_mfd_dialog_override = Some(source_receiver);
        app.begin_mfd_pipeline_import();
        app.discard_pending_pipeline_editor_action(&egui::Context::default());
        let (destination_sender, destination_receiver) = mpsc::channel();
        app.pipeline_mfd_dialog_override = Some(destination_receiver);
        source_sender
            .send(Some(design.display().to_string()))
            .unwrap();
        app.poll_dialog(&egui::Context::default());
        let destination = dir.0.join("new.pipeline.json");
        destination_sender
            .send(Some(destination.display().to_string()))
            .unwrap();
        app.poll_dialog(&egui::Context::default());
        let pipeline = &app.pipeline_editor.as_ref().unwrap().document.pipeline;
        retain(&dir.0, "default-gui-pipeline", pipeline);
        assert_eq!(pipeline.stages.len(), 2);
        assert_eq!(pipeline.stages[0].project.failure_rules.len(), 1);
        assert!(pipeline.stages[1].project.failure_rules.is_empty());
        assert_eq!(state(&app), main);
        assert!(!destination.exists());
        assert!(app.pending_dialog.is_none());
        assert!(!app.pipeline_mfd_busy());
    }
}
