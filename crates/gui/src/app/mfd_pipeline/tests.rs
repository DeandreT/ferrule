use super::*;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};

use ir::{Instance, Value};
use mapping::{Node, PipelineInput};

use crate::diagnostics::{DiagnosticLevel, DiagnosticLocation};
use crate::pipeline_edit::PipelineEditorDocument;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-mfd-pipeline-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).expect("unique test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if !std::thread::panicking()
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                != Some(std::ffi::OsStr::new("1"))
        {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn dialog_override(app: &mut FerruleApp) -> Sender<Option<String>> {
    let (sender, receiver) = mpsc::channel();
    app.pipeline_mfd_dialog_override = Some(receiver);
    sender
}

fn complete_dialog(app: &mut FerruleApp, sender: Sender<Option<String>>, path: Option<&Path>) {
    sender
        .send(path.map(|path| path.display().to_string()))
        .unwrap();
    app.poll_dialog(&egui::Context::default());
}

fn main_state(
    app: &FerruleApp,
) -> (
    String,
    super::super::CanvasLayout,
    crate::document::DocumentLocation,
    bool,
    usize,
    bool,
) {
    (
        crate::project_state::project_snapshot_key(&app.project),
        super::super::CanvasLayout::capture(
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

fn imported_editor(mapping: &Path, destination: &Path) -> pipeline_editor_ui::PipelineEditorUi {
    let imported = mfd::import_pipeline(mapping).unwrap();
    let (document, warnings) =
        PipelineEditorDocument::from_imported_mfd(destination, imported).unwrap();
    assert!(warnings.is_empty());
    pipeline_editor_ui::PipelineEditorUi::new(document)
}

fn begin_source_dialog(app: &mut FerruleApp) -> Sender<Option<String>> {
    let sender = dialog_override(app);
    app.begin_mfd_pipeline_import();
    assert!(matches!(
        app.pending_dialog.as_ref(),
        Some((DialogKind::ImportMfdPipeline, _))
    ));
    sender
}

fn pending_pipeline_dialog(
    app: &mut FerruleApp,
    kind: DialogKind,
    mapping: &Path,
) -> Sender<Option<String>> {
    let sender = dialog_override(app);
    match kind {
        DialogKind::ImportMfdPipeline => {
            app.start_mfd_pipeline_import(mfd::ImportOptions::default());
            sender
        }
        DialogKind::ChooseImportedPipelineDestination => {
            app.start_mfd_pipeline_import(mfd::ImportOptions::default());
            let destination = dialog_override(app);
            complete_dialog(app, sender, Some(mapping));
            destination
        }
        DialogKind::ExportPipelineMfd => {
            app.begin_pipeline_mfd_export(ExportProfile::NativeMfd);
            sender
        }
        _ => unreachable!("only new pipeline MFD dialogs"),
    }
}

fn assert_pipeline_dialog_cleared(app: &FerruleApp) {
    assert!(app.pending_dialog.is_none());
    assert!(app.pending_mfd_pipeline_import.is_none());
    assert!(app.pending_pipeline_mfd_export.is_none());
    assert!(!app.pipeline_mfd_busy());
}

fn append_final_suffix(pipeline: &mut mapping::Pipeline, value: &str) {
    let stage = pipeline.stages.last_mut().unwrap();
    let binding = stage
        .project
        .root
        .bindings
        .iter_mut()
        .find(|binding| binding.target_field == "Result")
        .unwrap();
    let input = binding.node;
    let constant = stage.project.graph.nodes.keys().max().copied().unwrap_or(0) + 1;
    let output = constant + 1;
    stage.project.graph.nodes.insert(
        constant,
        Node::Const {
            value: Value::String(value.into()),
        },
    );
    stage.project.graph.nodes.insert(
        output,
        Node::Call {
            function: "concat".into(),
            args: vec![input, constant],
        },
    );
    binding.node = output;
}

fn final_value(pipeline: &mapping::Pipeline) -> Value {
    let PipelineInput::Host { name } = &pipeline.stages[0].source else {
        panic!("serial fixture's first stage must read its declared host source");
    };
    // Later imported stages retain the original source as a named alias.
    // Every fixture binding must reuse this one host, supplied exactly once.
    let declared_hosts = pipeline
        .stages
        .iter()
        .flat_map(|stage| {
            std::iter::once(&stage.source)
                .chain(stage.extra_sources.iter().map(|binding| &binding.from))
        })
        .filter_map(|input| match input {
            PipelineInput::Host { name } => Some(name.as_str()),
            PipelineInput::StageTarget { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(declared_hosts, BTreeSet::from([name.as_str()]));
    let input = Instance::Group(
        vec![(
            "Start".into(),
            Instance::Scalar(Value::String("source".into())),
        )]
        .into(),
    );
    let outputs = engine::run_pipeline(pipeline, &BTreeMap::from([(name.clone(), input)])).unwrap();
    outputs
        .stage(&pipeline.stages.last().unwrap().id)
        .unwrap()
        .primary
        .field("Result")
        .and_then(Instance::as_scalar)
        .unwrap()
        .clone()
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

fn write_design(directory: &Path, intermediate_count: usize) -> PathBuf {
    let schema = |root: &str, field: &str| {
        format!(
            "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element name=\"{root}\"><xs:complexType><xs:sequence><xs:element name=\"{field}\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:element></xs:schema>"
        )
    };
    write(&directory.join("source.xsd"), &schema("Source", "Start"));
    write(&directory.join("target.xsd"), &schema("Target", "Result"));
    let mut children = String::from(
        r#"<component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Start" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>"#,
    );
    // Declare the pass-through components in reverse order. Stage order must
    // come from graph feeds, never from component position.
    for index in (1..=intermediate_count).rev() {
        let root = format!("Buffer{index}");
        let field = format!("Step{index}");
        write(
            &directory.join(format!("buffer{index}.xsd")),
            &schema(&root, &field),
        );
        children.push_str(&format!(
            "<component name=\"buffer-{index}\" library=\"xml\" kind=\"14\"><properties PassThrough=\"1\"/><data><root><entry name=\"{root}\"><entry name=\"{field}\" inpkey=\"{}\" outkey=\"{}\"/></entry></root><document schema=\"buffer{index}.xsd\" outputinstance=\"buffer{index}-out.xml\" inputinstance=\"buffer{index}-preview.xml\" instanceroot=\"{{}}{root}\"/></data></component>",
            index * 20,
            index * 20 + 10,
        ));
    }
    children.push_str(&format!(
        "<component name=\"target\" library=\"xml\" kind=\"14\"><properties XSLTDefaultOutput=\"1\"/><data><root><entry name=\"Target\"><entry name=\"Result\" inpkey=\"{}\"/></entry></root><document schema=\"target.xsd\" outputinstance=\"target.xml\" instanceroot=\"{{}}Target\"/></data></component>",
        (intermediate_count + 1) * 20,
    ));
    let mut vertices = String::new();
    for index in 0..=intermediate_count {
        vertices.push_str(&format!(
            "<vertex vertexkey=\"{}\"><edges><edge vertexkey=\"{}\"/></edges></vertex>",
            index * 20 + 10,
            (index + 1) * 20,
        ));
    }
    let mapping = directory.join("design.mfd");
    write(
        &mapping,
        &format!(
            "<mapping version=\"26\"><component name=\"map\"><structure><children>{children}</children><graph><vertices>{vertices}</vertices></graph></structure></component></mapping>"
        ),
    );
    mapping
}

#[test]
fn import_chooses_a_new_unsaved_location_and_preserves_main_canvas() {
    for intermediates in [1, 3] {
        let directory = TestDirectory::new();
        let design_dir = directory.0.join("designs");
        let pipeline_dir = directory.0.join("pipelines");
        std::fs::create_dir(&design_dir).unwrap();
        std::fs::create_dir(&pipeline_dir).unwrap();
        let mapping = write_design(&design_dir, intermediates);
        let destination = pipeline_dir.join("flow.json");
        let mut app = FerruleApp::default();
        let before = main_state(&app);
        let source = begin_source_dialog(&mut app);
        let output = dialog_override(&mut app);
        complete_dialog(&mut app, source, Some(&mapping));
        assert!(matches!(
            app.pending_dialog.as_ref(),
            Some((DialogKind::ChooseImportedPipelineDestination, _))
        ));
        assert!(!destination.exists());
        assert_eq!(main_state(&app), before);
        complete_dialog(&mut app, output, Some(&destination));
        let editor = app.pipeline_editor.as_mut().unwrap();
        assert_eq!(editor.selected_stage, Some(0));
        assert_eq!(editor.document.pipeline.stages.len(), intermediates + 1);
        assert!(editor.document.issues().is_empty());
        assert!(editor.document.is_dirty());
        assert!(!editor.document.is_saved());
        assert!(!destination.exists());
        assert_eq!(
            editor.document.pipeline.main_mapping_path.as_deref(),
            Some("../designs/design.mfd")
        );
        for (index, stage) in editor.document.pipeline.stages.iter().enumerate() {
            assert_eq!(stage.id, format!("mfd-stage-{}", index + 1));
            assert_eq!(stage.mapping_path.as_deref(), Some("../designs/design.mfd"));
            if index == 0 {
                assert_eq!(
                    stage.source,
                    PipelineInput::Host {
                        name: "source".into()
                    }
                );
                assert_eq!(
                    stage.project.source_path.as_deref(),
                    Some("../designs/source.xml")
                );
            } else {
                assert_eq!(
                    stage.source,
                    PipelineInput::StageTarget {
                        stage: format!("mfd-stage-{index}"),
                        target: None
                    }
                );
                assert_eq!(
                    stage.project.source_path,
                    Some(format!("../designs/buffer{index}-preview.xml"))
                );
            }
            if index < intermediates {
                assert_eq!(
                    stage.project.target_path,
                    Some(format!("../designs/buffer{}-out.xml", index + 1))
                );
            } else {
                assert_eq!(
                    stage.project.target_path.as_deref(),
                    Some("../designs/target.xml")
                );
            }
        }
        editor.document.save().unwrap();
        assert!(editor.document.is_saved());
        assert!(!editor.document.is_dirty());
        let reloaded = PipelineEditorDocument::load(&destination).unwrap();
        assert_eq!(
            final_value(&reloaded.pipeline),
            Value::String("source".into())
        );
        assert_eq!(main_state(&app), before);
        assert!(!crate::layout_store::layout_path(&destination).exists());
    }
}

#[test]
fn single_mapping_keeps_ordinary_import_and_pipeline_route_refuses_without_fallback() {
    let directory = TestDirectory::new();
    let mapping = write_design(&directory.0, 0);
    let mut app = FerruleApp::default();
    let before = main_state(&app);
    app.pending_mfd_pipeline_import = Some(PendingMfdPipelineImport {
        options: mfd::ImportOptions::default(),
        imported: None,
    });
    let error = app
        .stage_mfd_pipeline_source(mapping.clone())
        .err()
        .unwrap();
    assert!(matches!(error, MfdError::UnsupportedImport(_)), "{error}");
    assert!(app.pending_mfd_pipeline_import.is_none());
    assert!(app.pending_dialog.is_none());
    assert!(app.pipeline_editor.is_none());
    assert_eq!(main_state(&app), before);

    let (sender, receiver) = mpsc::channel();
    app.pending_dialog = Some((DialogKind::ImportMfd, receiver));
    complete_dialog(&mut app, sender, Some(&mapping));
    assert_eq!(app.project.source.name, "Source");
    assert_eq!(app.project.target.name, "Target");
    assert!(app.pipeline_editor.is_none());
    assert!(app.is_dirty());
    assert_eq!(
        app.document.suggested_path(),
        mapping.with_extension("json")
    );
}

#[test]
fn dirty_pipeline_guard_and_both_cancelled_choosers_keep_the_previous_document() {
    let directory = TestDirectory::new();
    let mapping = write_design(&directory.0, 1);
    let original_path = directory.0.join("old-flow.json");
    let mut app = FerruleApp {
        pipeline_editor: Some(imported_editor(&mapping, &original_path)),
        ..Default::default()
    };
    let before = main_state(&app);
    let original = crate::project_state::pipeline_snapshot_key(
        &app.pipeline_editor.as_ref().unwrap().document.pipeline,
    );

    let source = dialog_override(&mut app);
    app.begin_mfd_pipeline_import();
    assert!(app.pending_dialog.is_none());
    assert!(matches!(
        app.pending_pipeline_editor_action.as_ref(),
        Some(pipeline_editor_ui::PipelineEditorAction::ImportMfd(_))
    ));
    assert!(app.pipeline_mfd_busy());
    app.request_pipeline_editor_action(pipeline_editor_ui::PipelineEditorAction::Close);
    assert!(matches!(
        app.pending_pipeline_editor_action.as_ref(),
        Some(pipeline_editor_ui::PipelineEditorAction::ImportMfd(_))
    ));
    // This is the existing Keep editing transition.
    app.pending_pipeline_editor_action = None;
    assert!(!app.pipeline_mfd_busy());
    assert_eq!(
        crate::project_state::pipeline_snapshot_key(
            &app.pipeline_editor.as_ref().unwrap().document.pipeline
        ),
        original
    );

    app.begin_mfd_pipeline_import();
    app.discard_pending_pipeline_editor_action(&egui::Context::default());
    assert!(matches!(
        app.pending_dialog.as_ref(),
        Some((DialogKind::ImportMfdPipeline, _))
    ));
    complete_dialog(&mut app, source, None);
    assert!(!app.pipeline_mfd_busy());
    assert_eq!(
        app.pipeline_editor.as_ref().unwrap().document.path,
        original_path
    );

    let source = dialog_override(&mut app);
    app.begin_mfd_pipeline_import();
    app.discard_pending_pipeline_editor_action(&egui::Context::default());
    let destination = dialog_override(&mut app);
    complete_dialog(&mut app, source, Some(&mapping));
    complete_dialog(&mut app, destination, None);
    assert!(!app.pipeline_mfd_busy());
    assert!(app.pending_dialog.is_none());
    assert_eq!(
        crate::project_state::pipeline_snapshot_key(
            &app.pipeline_editor.as_ref().unwrap().document.pipeline
        ),
        original
    );
    assert!(!original_path.exists());
    assert_eq!(main_state(&app), before);
}

#[test]
fn parsed_import_is_retained_across_destination_dialog_and_save_conflict_is_preserved() {
    let directory = TestDirectory::new();
    let mapping = write_design(&directory.0, 1);
    let destination = directory.0.join("flow.json");
    let mut app = FerruleApp::default();
    let before = main_state(&app);
    let source = begin_source_dialog(&mut app);
    let output = dialog_override(&mut app);
    complete_dialog(&mut app, source, Some(&mapping));
    write(&mapping, "changed after parsing");
    complete_dialog(&mut app, output, Some(&destination));
    let editor = app.pipeline_editor.as_mut().unwrap();
    assert_eq!(
        final_value(&editor.document.pipeline),
        Value::String("source".into())
    );
    write(&destination, "other document appeared");
    let error = editor.document.save().unwrap_err();
    assert!(error.to_string().contains("appeared"), "{error:#}");
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "other document appeared"
    );
    assert!(editor.document.is_dirty());
    assert!(!editor.document.is_saved());
    assert_eq!(main_state(&app), before);
}

#[test]
fn failed_destination_keeps_old_editor_and_warning_text_keeps_file_provenance() {
    let directory = TestDirectory::new();
    let mapping = write_design(&directory.0, 1);
    let mut app = FerruleApp::default();
    let old_path = directory.0.join("old-flow.json");
    app.pipeline_editor = Some(imported_editor(&mapping, &old_path));
    let collision = directory.0.join("exists.json");
    write(&collision, "untouched");
    app.pending_mfd_pipeline_import = Some(PendingMfdPipelineImport {
        options: mfd::ImportOptions::default(),
        imported: Some(mfd::import_pipeline(&mapping).unwrap()),
    });
    assert!(app.finish_mfd_pipeline_import(&collision).is_err());
    assert_eq!(
        app.pipeline_editor.as_ref().unwrap().document.path,
        old_path
    );
    assert_eq!(std::fs::read_to_string(&collision).unwrap(), "untouched");
    assert!(!app.pipeline_mfd_busy());

    let mut imported = mfd::import_pipeline(&mapping).unwrap();
    let canonical = imported.mapping_path.clone();
    // Native import currently rejects real stage warnings. This injected
    // warning binds forward-compatible GUI propagation, without inventing owners.
    let warning = "mfd-stage-2: retained warning text";
    imported.warnings.push(warning.into());
    app.pending_mfd_pipeline_import = Some(PendingMfdPipelineImport {
        options: mfd::ImportOptions::default(),
        imported: Some(imported),
    });
    app.finish_mfd_pipeline_import(&directory.0.join("new-flow.json"))
        .unwrap();
    assert_eq!(app.diagnostics.items().len(), 1);
    let diagnostic = &app.diagnostics.items()[0];
    assert_eq!(diagnostic.level, DiagnosticLevel::Warning);
    assert_eq!(diagnostic.message, warning);
    assert_eq!(
        diagnostic.location,
        Some(DiagnosticLocation::ImportFile(canonical))
    );
}

#[test]
fn selected_manifest_identity_is_captured_before_the_dirty_guard_and_invalid_selection_refuses() {
    let directory = TestDirectory::new();
    let package = directory.0.join("package");
    let other = directory.0.join("other");
    std::fs::create_dir(&package).unwrap();
    std::fs::create_dir(&other).unwrap();
    let manifest = package.join("package.json");
    let other_manifest = other.join("package.json");
    let valid = r#"{"schemaVersion":1,"kind":"ferrule.mapping-package"}"#;
    write(&manifest, valid);
    write(&other_manifest, valid);
    let mapping = write_design(&package, 1);
    let mut app = FerruleApp {
        pipeline_editor: Some(imported_editor(&mapping, &directory.0.join("old.json"))),
        ..Default::default()
    };
    app.mfd_package_manifest = Some(manifest.display().to_string());
    let source = dialog_override(&mut app);
    app.begin_mfd_pipeline_import();
    let Some(pipeline_editor_ui::PipelineEditorAction::ImportMfd(options)) =
        &app.pending_pipeline_editor_action
    else {
        panic!("dirty pipeline import is guarded");
    };
    assert_eq!(
        options.package_root(),
        Some(std::fs::canonicalize(&package).unwrap().as_path())
    );
    app.mfd_package_manifest = Some(other_manifest.display().to_string());
    app.discard_pending_pipeline_editor_action(&egui::Context::default());
    let destination = dialog_override(&mut app);
    complete_dialog(&mut app, source, Some(&mapping));
    assert!(matches!(
        app.pending_dialog.as_ref(),
        Some((DialogKind::ChooseImportedPipelineDestination, _))
    ));
    complete_dialog(&mut app, destination, None);
    assert_eq!(
        app.pipeline_editor.as_ref().unwrap().document.path,
        directory.0.join("old.json")
    );

    let mut app = FerruleApp::default();
    let before = main_state(&app);
    app.mfd_package_manifest = Some(directory.0.join("missing.json").display().to_string());
    app.begin_mfd_pipeline_import();
    assert!(app.pending_dialog.is_none());
    assert!(app.pending_pipeline_editor_action.is_none());
    assert!(app.pending_mfd_pipeline_import.is_none());
    assert!(!app.diagnostics.is_empty());
    assert_eq!(main_state(&app), before);
}

#[test]
fn export_uses_current_unsaved_applied_snapshot_for_both_profiles_and_rebases_paths() {
    for profile in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd] {
        let directory = TestDirectory::new();
        let design_dir = directory.0.join("designs");
        let pipeline_dir = directory.0.join("pipelines");
        let output_dir = directory.0.join("exports");
        for path in [&design_dir, &pipeline_dir, &output_dir] {
            std::fs::create_dir(path).unwrap();
        }
        let mapping = write_design(&design_dir, 1);
        let document_path = pipeline_dir.join("unsaved-flow.json");
        let output_path = output_dir.join("current.mfd");
        let mut app = FerruleApp {
            pipeline_editor: Some(imported_editor(&mapping, &document_path)),
            ..Default::default()
        };
        assert_eq!(
            app.pipeline_editor
                .as_ref()
                .unwrap()
                .document
                .pipeline
                .stages[0]
                .source,
            PipelineInput::Host {
                name: "source".into()
            }
        );
        append_final_suffix(
            &mut app.pipeline_editor.as_mut().unwrap().document.pipeline,
            " applied edit",
        );
        let before = main_state(&app);
        let chooser = dialog_override(&mut app);
        app.begin_pipeline_mfd_export(profile);
        assert!(matches!(
            app.pending_dialog.as_ref(),
            Some((DialogKind::ExportPipelineMfd, _))
        ));
        assert!(app.pipeline_mfd_busy());
        app.request_pipeline_editor_action(pipeline_editor_ui::PipelineEditorAction::Close);
        assert!(app.pipeline_editor.is_some());
        assert!(app.pending_pipeline_editor_action.is_none());
        append_final_suffix(
            &mut app.pipeline_editor.as_mut().unwrap().document.pipeline,
            " later edit",
        );
        complete_dialog(&mut app, chooser, Some(&output_path));
        assert!(!app.pipeline_mfd_busy());
        assert!(app.diagnostics.is_empty(), "{:?}", app.diagnostics.items());
        let exported = mfd::import_pipeline(&output_path).unwrap();
        // Native export names the primary source component from the schema
        // root; pipeline import then uses that component name as host identity.
        assert_eq!(
            exported.pipeline.stages[0].source,
            PipelineInput::Host {
                name: "Source".into()
            }
        );
        assert_eq!(
            exported.pipeline.stages[0].project.source,
            app.pipeline_editor
                .as_ref()
                .unwrap()
                .document
                .pipeline
                .stages[0]
                .project
                .source
        );
        let wrong_case_input = Instance::Group(
            vec![(
                "Start".into(),
                Instance::Scalar(Value::String("source".into())),
            )]
            .into(),
        );
        assert!(matches!(engine::run_pipeline(&exported.pipeline,
            &BTreeMap::from([("source".into(), wrong_case_input)])),
            Err(engine::PipelineError::MissingHostInput { name }) if name == "Source"));
        assert_eq!(
            final_value(&exported.pipeline),
            Value::String("source applied edit".into())
        );
        let first = &exported.pipeline.stages[0].project;
        assert_eq!(first.source_path.as_deref(), Some("../designs/source.xml"));
        assert_eq!(
            first.target_path.as_deref(),
            Some("../designs/buffer1-out.xml")
        );
        assert_eq!(
            exported.pipeline.stages[1].project.source_path.as_deref(),
            Some("../designs/buffer1-preview.xml")
        );
        assert!(!document_path.exists());
        assert!(app.pipeline_editor.as_ref().unwrap().document.is_dirty());
        assert!(!app.pipeline_editor.as_ref().unwrap().document.is_saved());
        assert_eq!(
            final_value(&app.pipeline_editor.as_ref().unwrap().document.pipeline),
            Value::String("source applied edit later edit".into())
        );
        assert_eq!(main_state(&app), before);
    }
}

#[test]
fn export_cancellation_and_unapplied_text_never_write_or_reload_the_document() {
    let directory = TestDirectory::new();
    let mapping = write_design(&directory.0, 1);
    let document_path = directory.0.join("flow.json");
    let mut app = FerruleApp {
        pipeline_editor: Some(imported_editor(&mapping, &document_path)),
        ..Default::default()
    };
    let before = main_state(&app);
    let chooser = dialog_override(&mut app);
    app.begin_pipeline_mfd_export(ExportProfile::NativeMfd);
    complete_dialog(&mut app, chooser, None);
    assert!(!app.pipeline_mfd_busy());
    assert!(!document_path.exists());
    assert!(app.pipeline_editor.as_ref().unwrap().document.is_dirty());

    // A stage-ID or host-name draft that differs from its applied value must
    // be applied before the snapshot can be captured.
    app.pipeline_editor
        .as_mut()
        .unwrap()
        .document
        .pipeline
        .stages[0]
        .id = "changed applied name".into();
    app.begin_pipeline_mfd_export(ExportProfile::FerruleExtensions);
    assert!(app.pending_pipeline_mfd_export.is_none());
    assert!(app.pending_dialog.is_none());
    assert!(
        app.diagnostics.items()[0]
            .message
            .contains("Apply staged text edits")
    );
    app.pipeline_editor = Some(imported_editor(&mapping, &document_path));
    app.pipeline_editor
        .as_mut()
        .unwrap()
        .document
        .pipeline
        .stages[0]
        .source = PipelineInput::Host {
        name: "changed applied host".into(),
    };
    let error = PipelineMfdExport::capture(
        app.pipeline_editor.as_ref().unwrap(),
        ExportProfile::NativeMfd,
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("Apply staged text edits"));
    let mut editor = imported_editor(&mapping, &document_path);
    let final_id = editor.document.pipeline.stages[1].id.clone();
    editor.document.pipeline.stages[1].source = PipelineInput::StageTarget {
        stage: final_id,
        target: None,
    };
    app.pipeline_editor = Some(pipeline_editor_ui::PipelineEditorUi::new(editor.document));
    app.begin_pipeline_mfd_export(ExportProfile::NativeMfd);
    assert!(app.pending_pipeline_mfd_export.is_none());
    assert!(app.pending_dialog.is_none());
    assert!(
        app.diagnostics.items()[0]
            .message
            .contains("Pipeline validation failed")
    );
    assert!(!document_path.exists());
    assert_eq!(main_state(&app), before);
}

#[test]
fn unsupported_branching_retains_typed_error_before_any_design_or_schema_publication() {
    let directory = TestDirectory::new();
    let mapping = write_design(&directory.0, 2);
    let document_path = directory.0.join("flow.json");
    let mut editor = imported_editor(&mapping, &document_path);
    // The engine admits this independent host branch, but the serial MFD
    // profile must reject it without relaxing the native boundary.
    editor.document.pipeline.stages[2].source = PipelineInput::Host {
        name: "other".into(),
    };
    assert!(editor.document.issues().is_empty());
    let snapshot = PipelineMfdExport {
        pipeline: editor.document.pipeline.clone(),
        document_path,
        profile: ExportProfile::FerruleExtensions,
    };
    let output_dir = directory.0.join("outputs");
    std::fs::create_dir(&output_dir).unwrap();
    let sentinel = output_dir.join("existing.txt");
    write(&sentinel, "keep");
    let output_path = output_dir.join("refused.mfd");
    let error = snapshot.publish(&output_path).unwrap_err();
    assert!(
        matches!(error.downcast_ref::<MfdError>(), Some(MfdError::Unsupported(message)) if message.contains("serial primary-target chain")),
        "{error:#}"
    );
    assert_eq!(std::fs::read_dir(&output_dir).unwrap().count(), 1);
    assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "keep");
}

#[test]
fn toolbar_cancel_clears_all_pipeline_dialog_snapshots_and_resumes_actions() {
    for kind in [
        DialogKind::ImportMfdPipeline,
        DialogKind::ChooseImportedPipelineDestination,
        DialogKind::ExportPipelineMfd,
    ] {
        let directory = TestDirectory::new();
        let mapping = write_design(&directory.0, 1);
        let original_path = directory.0.join("old-flow.json");
        let late_path = directory.0.join("late-result.mfd");
        let mut app = FerruleApp {
            pipeline_editor: Some(imported_editor(&mapping, &original_path)),
            ..Default::default()
        };
        let main = main_state(&app);
        let original = crate::project_state::pipeline_snapshot_key(
            &app.pipeline_editor.as_ref().unwrap().document.pipeline,
        );
        let late = pending_pipeline_dialog(&mut app, kind, &mapping);
        assert!(app.pipeline_mfd_busy());
        assert!(!app.project_editing_enabled());
        assert!(app.pending_dialog.as_ref().map(|(kind, _)| *kind) == Some(kind));

        // This is the handler invoked by the Waiting for file dialog toolbar.
        app.cancel_pending_file_dialog();
        assert_pipeline_dialog_cleared(&app);
        assert!(app.project_editing_enabled());
        assert_eq!(main_state(&app), main);
        assert_eq!(
            app.pipeline_editor.as_ref().unwrap().document.path,
            original_path
        );
        assert_eq!(
            crate::project_state::pipeline_snapshot_key(
                &app.pipeline_editor.as_ref().unwrap().document.pipeline
            ),
            original
        );
        assert!(app.pipeline_editor.as_ref().unwrap().document.is_dirty());
        assert!(!app.pipeline_editor.as_ref().unwrap().document.is_saved());
        assert!(late.send(Some(late_path.display().to_string())).is_err());
        app.poll_dialog(&egui::Context::default());
        assert!(!late_path.exists());
        assert!(!original_path.exists());

        // A fresh export must open normally after each cancellation.
        let next = dialog_override(&mut app);
        app.begin_pipeline_mfd_export(ExportProfile::NativeMfd);
        assert!(app.pending_pipeline_mfd_export.is_some());
        assert!(
            app.pending_dialog.as_ref().map(|(kind, _)| *kind)
                == Some(DialogKind::ExportPipelineMfd)
        );
        app.cancel_pending_file_dialog();
        assert_pipeline_dialog_cleared(&app);
        assert!(next.send(None).is_err());
        assert_eq!(main_state(&app), main);
    }
}

#[test]
fn app_close_cancels_all_active_pipeline_choosers_before_existing_dirty_pipeline_guard() {
    for kind in [
        DialogKind::ImportMfdPipeline,
        DialogKind::ChooseImportedPipelineDestination,
        DialogKind::ExportPipelineMfd,
    ] {
        let directory = TestDirectory::new();
        let mapping = write_design(&directory.0, 1);
        let document_path = directory.0.join("old-flow.json");
        let late_path = directory.0.join("late-result.mfd");
        let mut app = FerruleApp {
            pipeline_editor: Some(imported_editor(&mapping, &document_path)),
            ..Default::default()
        };
        let main = main_state(&app);
        let late = pending_pipeline_dialog(&mut app, kind, &mapping);
        let context = egui::Context::default();
        app.guard_app_close_requested(&context, false);
        assert!(app.pipeline_mfd_busy());
        assert!(app.pending_pipeline_editor_action.is_none());

        app.guard_app_close_requested(&context, true);
        assert_pipeline_dialog_cleared(&app);
        assert!(matches!(
            app.pending_pipeline_editor_action.as_ref(),
            Some(pipeline_editor_ui::PipelineEditorAction::CloseApp)
        ));
        assert!(app.pipeline_editor.is_some());
        assert!(!app.allow_close);
        assert_eq!(main_state(&app), main);
        assert!(late.send(Some(late_path.display().to_string())).is_err());
        app.poll_dialog(&context);
        assert!(!late_path.exists());
        assert!(!document_path.exists());

        // Keep editing only dismisses the existing close guard, and controls resume.
        app.pending_pipeline_editor_action = None;
        assert!(app.project_editing_enabled());
        let next = dialog_override(&mut app);
        app.begin_pipeline_mfd_export(ExportProfile::NativeMfd);
        assert!(app.pending_pipeline_mfd_export.is_some());
        app.cancel_pending_file_dialog();
        assert!(next.send(None).is_err());
        assert_eq!(main_state(&app), main);
    }
}

#[test]
fn closing_dirty_main_cancels_import_and_clean_pipeline_export_before_late_publication() {
    for kind in [
        DialogKind::ImportMfdPipeline,
        DialogKind::ChooseImportedPipelineDestination,
        DialogKind::ExportPipelineMfd,
    ] {
        let directory = TestDirectory::new();
        let mapping = write_design(&directory.0, 1);
        let document_path = directory.0.join("saved-flow.json");
        let late_path = directory.0.join("late-result.mfd");
        let mut app = FerruleApp::default();
        if kind == DialogKind::ExportPipelineMfd {
            let mut editor = imported_editor(&mapping, &document_path);
            editor.document.save().unwrap();
            app.pipeline_editor = Some(editor);
        }
        app.history.mark_unsaved();
        assert!(app.is_dirty());
        let main = main_state(&app);
        let late = pending_pipeline_dialog(&mut app, kind, &mapping);
        let context = egui::Context::default();
        app.guard_app_close_requested(&context, true);
        assert_pipeline_dialog_cleared(&app);
        assert!(app.pending_pipeline_editor_action.is_none());
        assert!(matches!(
            app.pending_destructive_action,
            Some(super::super::DestructiveAction::Close)
        ));
        assert!(!app.allow_close);
        assert_eq!(main_state(&app), main);
        assert!(late.send(Some(late_path.display().to_string())).is_err());
        app.poll_dialog(&context);
        assert!(!late_path.exists());
        assert_eq!(
            app.pipeline_editor.is_some(),
            kind == DialogKind::ExportPipelineMfd
        );
        // Continuing the existing main-document Discard response cannot admit
        // an unsaved pipeline that arrived after the close decision.
        let action = app.pending_destructive_action.take().unwrap();
        app.perform_destructive_action(action, &context);
        assert!(app.allow_close);
        assert_pipeline_dialog_cleared(&app);
        assert_eq!(main_state(&app), main);
    }
}

#[test]
fn app_close_replaces_queued_import_guard_with_close_continuation() {
    let directory = TestDirectory::new();
    let mapping = write_design(&directory.0, 1);
    let document_path = directory.0.join("old-flow.json");
    let mut app = FerruleApp {
        pipeline_editor: Some(imported_editor(&mapping, &document_path)),
        ..Default::default()
    };
    let main = main_state(&app);
    let _unused_source = dialog_override(&mut app);
    app.begin_mfd_pipeline_import();
    assert!(matches!(
        app.pending_pipeline_editor_action.as_ref(),
        Some(pipeline_editor_ui::PipelineEditorAction::ImportMfd(_))
    ));
    assert!(app.pending_dialog.is_none());
    let context = egui::Context::default();
    app.guard_app_close_requested(&context, true);
    assert!(matches!(
        app.pending_pipeline_editor_action.as_ref(),
        Some(pipeline_editor_ui::PipelineEditorAction::CloseApp)
    ));
    assert_pipeline_dialog_cleared(&app);
    assert!(app.pipeline_editor.is_some());
    app.discard_pending_pipeline_editor_action(&context);
    assert!(app.allow_close);
    assert!(app.pipeline_editor.is_none());
    assert_pipeline_dialog_cleared(&app);
    assert!(!document_path.exists());
    assert_eq!(main_state(&app), main);
}

#[test]
fn ready_pipeline_dialog_results_are_cancelled_before_polling_a_real_close_request() {
    for kind in [
        DialogKind::ImportMfdPipeline,
        DialogKind::ChooseImportedPipelineDestination,
        DialogKind::ExportPipelineMfd,
    ] {
        let directory = TestDirectory::new();
        let mapping = write_design(&directory.0, 1);
        let old_path = directory.0.join("old-flow.json");
        let destination = directory.0.join("new-flow.json");
        let output = directory.0.join("ready-export.mfd");
        let mut app = FerruleApp {
            pipeline_editor: Some(imported_editor(&mapping, &old_path)),
            ..Default::default()
        };
        let main = main_state(&app);
        let original = crate::project_state::pipeline_snapshot_key(
            &app.pipeline_editor.as_ref().unwrap().document.pipeline,
        );
        let sender = pending_pipeline_dialog(&mut app, kind, &mapping);
        let ready = match kind {
            DialogKind::ImportMfdPipeline => &mapping,
            DialogKind::ChooseImportedPipelineDestination => &destination,
            DialogKind::ExportPipelineMfd => &output,
            _ => unreachable!(),
        };
        sender.send(Some(ready.display().to_string())).unwrap();
        let context = egui::Context::default();
        // ui() calls this before polling a ready result, then runs the normal
        // pipeline/main close guards with the same viewport close request.
        app.poll_dialog_with_close_guard(&context, true);
        assert_pipeline_dialog_cleared(&app);
        assert_eq!(
            app.pipeline_editor.as_ref().unwrap().document.path,
            old_path
        );
        assert_eq!(
            crate::project_state::pipeline_snapshot_key(
                &app.pipeline_editor.as_ref().unwrap().document.pipeline
            ),
            original
        );
        assert!(!destination.exists());
        assert!(!output.exists());
        assert_eq!(main_state(&app), main);
        app.guard_app_close_requested(&context, true);
        assert!(matches!(
            app.pending_pipeline_editor_action.as_ref(),
            Some(pipeline_editor_ui::PipelineEditorAction::CloseApp)
        ));
        assert!(!app.allow_close);
    }
}

#[test]
fn queued_import_guard_locks_mapping_controls_and_shortcuts_until_keep_editing() {
    let directory = TestDirectory::new();
    let mapping = write_design(&directory.0, 1);
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        100,
        Node::Const {
            value: Value::String("main edit".into()),
        },
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    assert!(app.can_undo());
    app.pipeline_editor = Some(imported_editor(
        &mapping,
        &directory.0.join("old-flow.json"),
    ));
    let main = main_state(&app);
    let original = crate::project_state::pipeline_snapshot_key(
        &app.pipeline_editor.as_ref().unwrap().document.pipeline,
    );
    let _source = dialog_override(&mut app);
    app.begin_mfd_pipeline_import();
    assert!(matches!(
        app.pending_pipeline_editor_action.as_ref(),
        Some(pipeline_editor_ui::PipelineEditorAction::ImportMfd(_))
    ));
    assert!(app.pending_dialog.is_none());
    // The existing non-UI admission helper has no receiver yet; the exact
    // helper used by ui() must nevertheless lock every mapping control route.
    assert!(app.project_editing_enabled());
    assert!(!app.ui_project_editing_enabled());
    let context = egui::Context::default();
    crate::icons::install(&context);
    for (key, shift) in [
        (egui::Key::O, false),
        (egui::Key::S, false),
        (egui::Key::R, false),
        (egui::Key::V, true),
        (egui::Key::Z, false),
        (egui::Key::Z, true),
        (egui::Key::Y, false),
    ] {
        let modifiers = egui::Modifiers {
            ctrl: true,
            command: true,
            shift,
            ..Default::default()
        };
        let mut retained = false;
        let _ = context.run_ui(egui::RawInput {
            modifiers,
            events: [true, false].into_iter().map(|pressed| egui::Event::Key {
                key, physical_key: Some(key), pressed, repeat: false, modifiers,
            }).collect(),
            ..Default::default()
        }, |ui| {
            let enabled = app.ui_project_editing_enabled();
            app.handle_history_shortcuts(ui.ctx(), enabled);
            let [undo, redo, _] = super::super::history_shortcuts();
            app.show_command_bar(ui, enabled, crate::workspace_layout::LayoutClass::Wide, &undo, &redo);
            retained = ui.ctx().input(|input| input.events.iter().any(|event|
                matches!(event, egui::Event::Key { key: observed, pressed: true, .. } if *observed == key)));
        });
        assert!(retained, "locked mapping shortcut was consumed");
        assert!(app.pending_dialog.is_none());
        assert!(app.pending_destructive_action.is_none());
        assert!(app.library_generation_draft.is_none());
        assert!(app.pending_file_run.is_none());
        assert!(app.pending_pipeline_run.is_none());
        assert_eq!(main_state(&app), main);
        assert_eq!(
            crate::project_state::pipeline_snapshot_key(
                &app.pipeline_editor.as_ref().unwrap().document.pipeline
            ),
            original
        );
    }
    // Existing Keep editing transition releases the queued import lock.
    app.pending_pipeline_editor_action = None;
    assert!(app.ui_project_editing_enabled());
    assert!(!app.pipeline_mfd_busy());
    assert_eq!(main_state(&app), main);
    app.begin_library_generation();
    assert!(app.library_generation_draft.is_some());
    assert_eq!(main_state(&app), main);
}

#[test]
fn discarded_import_guard_stays_locked_through_source_dialog_and_cancel_resumes_editing() {
    let directory = TestDirectory::new();
    let mapping = write_design(&directory.0, 1);
    let document_path = directory.0.join("old-flow.json");
    let mut app = FerruleApp {
        pipeline_editor: Some(imported_editor(&mapping, &document_path)),
        ..Default::default()
    };
    let main = main_state(&app);
    let source = dialog_override(&mut app);
    app.begin_mfd_pipeline_import();
    assert!(!app.ui_project_editing_enabled());
    app.discard_pending_pipeline_editor_action(&egui::Context::default());
    assert!(app.pending_pipeline_editor_action.is_none());
    assert!(matches!(
        app.pending_dialog.as_ref(),
        Some((DialogKind::ImportMfdPipeline, _))
    ));
    assert!(!app.ui_project_editing_enabled());
    app.cancel_pending_file_dialog();
    assert_pipeline_dialog_cleared(&app);
    assert!(app.ui_project_editing_enabled());
    assert!(source.send(None).is_err());
    assert_eq!(
        app.pipeline_editor.as_ref().unwrap().document.path,
        document_path
    );
    assert!(!document_path.exists());
    assert_eq!(main_state(&app), main);
}

#[test]
fn existing_pipeline_run_setup_refuses_mfd_import_and_export_without_displacing_state() {
    let directory = TestDirectory::new();
    let mapping = write_design(&directory.0, 1);
    let document_path = directory.0.join("saved-flow.json");
    let mut editor = imported_editor(&mapping, &document_path);
    editor.document.save().unwrap();
    let mut app = FerruleApp {
        pipeline_editor: Some(editor),
        ..Default::default()
    };
    app.load_pipeline_for_run(&document_path);
    let runner = app.pipeline_run_draft.as_ref().unwrap();
    let runner_path = runner.path.clone();
    let runner_model = crate::project_state::pipeline_snapshot_key(&runner.pipeline);
    let input_paths = runner
        .inputs
        .iter()
        .map(|input| (input.name.clone(), input.path.clone()))
        .collect::<Vec<_>>();
    let output_paths = runner
        .outputs
        .iter()
        .map(|output| {
            (
                output.stage.clone(),
                output.target.clone(),
                output.path.clone(),
            )
        })
        .collect::<Vec<_>>();
    let main = main_state(&app);
    let original = crate::project_state::pipeline_snapshot_key(
        &app.pipeline_editor.as_ref().unwrap().document.pipeline,
    );
    let status = app.status.clone();
    assert!(app.project_editing_enabled());
    for action in 0..3 {
        let _unused_dialog = dialog_override(&mut app);
        if action == 0 {
            app.begin_mfd_pipeline_import();
        } else if action == 1 {
            app.begin_pipeline_mfd_export(ExportProfile::FerruleExtensions);
        } else {
            app.begin_pipeline_mfd_export(ExportProfile::NativeMfd);
        }
        assert!(app.pending_dialog.is_none());
        assert!(app.pending_pipeline_editor_action.is_none());
        assert_pipeline_dialog_cleared(&app);
        assert!(app.pending_pipeline_run.is_none());
        let runner = app.pipeline_run_draft.as_ref().unwrap();
        assert_eq!(runner.path, runner_path);
        assert_eq!(
            crate::project_state::pipeline_snapshot_key(&runner.pipeline),
            runner_model
        );
        assert_eq!(
            runner
                .inputs
                .iter()
                .map(|input| (input.name.clone(), input.path.clone()))
                .collect::<Vec<_>>(),
            input_paths
        );
        assert_eq!(
            runner
                .outputs
                .iter()
                .map(|output| (
                    output.stage.clone(),
                    output.target.clone(),
                    output.path.clone()
                ))
                .collect::<Vec<_>>(),
            output_paths
        );
        assert_eq!(
            crate::project_state::pipeline_snapshot_key(
                &app.pipeline_editor.as_ref().unwrap().document.pipeline
            ),
            original
        );
        assert_eq!(app.status, status);
        assert_eq!(main_state(&app), main);
    }
    app.pipeline_run_draft = None;
    let output = dialog_override(&mut app);
    app.begin_pipeline_mfd_export(ExportProfile::NativeMfd);
    assert!(app.pending_pipeline_mfd_export.is_some());
    assert!(matches!(
        app.pending_dialog.as_ref(),
        Some((DialogKind::ExportPipelineMfd, _))
    ));
    app.cancel_pending_file_dialog();
    assert!(output.send(None).is_err());
    assert_eq!(main_state(&app), main);
}
