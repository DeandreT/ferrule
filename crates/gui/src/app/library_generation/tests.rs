use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, NamedTarget, Node, Scope};

use super::*;
use crate::app::{CanvasDocumentState, CanvasLayout, DestructiveAction, MappingDocument};
use crate::document::DocumentLocation;

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-library-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!(
                "Retained library generation test artifacts: {}",
                self.0.display()
            );
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn mapped_app(directory: &Path) -> anyhow::Result<FerruleApp> {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("before generation".into()),
        },
    );
    app.project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Scope::default()
    };
    app.project.extra_targets.push(NamedTarget {
        name: "Audit".into(),
        schema: app.project.target.clone(),
        root: app.project.root.clone(),
        path: Some("audit.json".into()),
        options: Default::default(),
    });
    app.main_canvas = CanvasDocumentState::main(&app.project);
    assert!(app.ensure_target_canvas(0));
    for (canvas, x) in [
        (&mut app.main_canvas, 511.0),
        (
            app.mapping_workspace.target_canvases.get_mut(&0).unwrap(),
            733.0,
        ),
    ] {
        let id = canvas
            .snarl
            .node_ids()
            .find(|(_, node)| **node == crate::canvas::CanvasNode::Graph(0))
            .unwrap()
            .0;
        canvas.snarl.get_node_info_mut(id).unwrap().pos = egui::pos2(x, 149.0);
    }
    app.document = DocumentLocation::untitled(directory.join("mapping.json"));
    app.save_document_to(&directory.join("mapping.json"))?;
    app.rebase_history();
    assert!(cli::validate(&app.project).is_empty());
    Ok(app)
}

fn layout(app: &FerruleApp) -> CanvasLayout {
    CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace)
}

fn positions(
    app: &FerruleApp,
) -> BTreeMap<MappingDocument, Vec<(crate::canvas::CanvasNode, egui::Pos2)>> {
    let collect = |canvas: &CanvasDocumentState| {
        canvas
            .snarl
            .nodes_pos()
            .map(|(pos, node)| (*node, pos))
            .collect()
    };
    BTreeMap::from([
        (MappingDocument::Main, collect(&app.main_canvas)),
        (
            MappingDocument::Target(0),
            collect(&app.mapping_workspace.target_canvases[&0]),
        ),
    ])
}

fn wires(
    app: &FerruleApp,
) -> BTreeMap<MappingDocument, Vec<(egui_snarl::OutPinId, egui_snarl::InPinId)>> {
    BTreeMap::from([
        (
            MappingDocument::Main,
            app.main_canvas.snarl.wires().collect(),
        ),
        (
            MappingDocument::Target(0),
            app.mapping_workspace.target_canvases[&0]
                .snarl
                .wires()
                .collect(),
        ),
    ])
}

fn choose_csharp(app: &mut FerruleApp, destination: &str) {
    app.begin_library_generation();
    let draft = app.library_generation_draft.as_mut().unwrap();
    assert!(!draft.include_csv_output);
    draft.language = LibraryLanguage::CSharp;
    draft.runtime_path = "unused C# runtime setting".into();
    draft.destination = destination.into();
}

fn finish_worker(app: &mut FerruleApp) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let context = egui::Context::default();
    while app.pending_library_generation.is_some() {
        assert!(Instant::now() < deadline, "library worker must finish");
        app.poll_library_generation(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn set_save_as_result(app: &mut FerruleApp, result: Option<&Path>) {
    let (sender, receiver) = mpsc::channel();
    sender
        .send(result.map(|path| path.display().to_string()))
        .unwrap();
    app.save_as_dialog_override = Some(receiver);
}

fn returned_save_dialog(app: &mut FerruleApp, result: Option<&Path>) {
    set_save_as_result(app, result);
    app.request_library_generation(&egui::Context::default());
    assert!(matches!(
        app.pending_dialog,
        Some((DialogKind::SaveProjectAs, _))
    ));
    assert_eq!(
        app.pending_save_continuation,
        Some(SaveContinuation::GenerateLibrary)
    );
    assert!(app.pending_library_generation.is_none());
    assert!(app.save_as_dialog_override.is_none());
    app.poll_dialog(&egui::Context::default());
}

#[test]
fn generation_saves_latest_value_and_retains_shared_owners_positions_and_history()
-> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = mapped_app(&temp.0)?;
    let original_positions = positions(&app);
    let original_wires = wires(&app);
    assert!(
        original_positions
            .values()
            .all(|positions| !positions.is_empty())
    );
    assert!(original_wires.values().all(|wires| !wires.is_empty()));
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("latest generation value".into()),
        },
    );
    app.observe_editor_history(Instant::now(), false);
    let before = crate::project_state::project_snapshot_key(&app.project);
    let undo_count = app.history.undo_len();
    assert!(app.is_dirty() && undo_count > 0);
    choose_csharp(&mut app, "generated-csharp");
    app.request_library_generation(&egui::Context::default());
    assert!(app.pending_library_generation.is_some());
    finish_worker(&mut app);
    let output = temp.0.join("generated-csharp");
    assert!(output.join("Ferrule.Generated.csproj").is_file());
    let generated = std::fs::read_to_string(output.join("GeneratedMapping.cs"))?;
    assert!(generated.contains("latest generation value"));
    assert!(!generated.contains("before generation"));
    assert!(!output.join("GeneratedMapping.Csv.cs").exists());
    assert!(!output.join("Runtime/FerruleCsv.cs").exists());
    assert!(app.library_generation_draft.is_none());
    assert!(app.diagnostics.is_empty());
    assert!(app.status.contains("generated C# library"));
    assert_eq!(
        crate::project_state::project_snapshot_key(&app.project),
        before
    );
    assert_eq!(positions(&app), original_positions);
    assert_eq!(wires(&app), original_wires);
    assert_eq!(app.history.undo_len(), undo_count);
    assert!(!app.is_dirty());
    let saved = mapping::project_file::decode_bytes(&std::fs::read(temp.0.join("mapping.json"))?)?;
    assert_eq!(crate::project_state::project_snapshot_key(&saved), before);
    app.undo_project();
    assert!(
        matches!(&app.project.graph.nodes[&0], Node::Const { value: Value::String(value) } if value == "before generation")
    );
    app.redo_project();
    assert_eq!(
        crate::project_state::project_snapshot_key(&app.project),
        before
    );
    assert_eq!(positions(&app), original_positions);
    assert_eq!(wires(&app), original_wires);
    Ok(())
}

fn csv_mapped_app(directory: &Path) -> anyhow::Result<FerruleApp> {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
    .repeating();
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.csv".into());
    app.project.target_options.delimiter = Some(';');
    app.project.target_options.csv_quote = Some('\'');
    app.project.target_options.csv_utf8_bom = true;
    app.project.target_options.has_header_row = Some(false);
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("before CSV generation".into()),
        },
    );
    app.project.root = Scope {
        iteration: mapping::ScopeIteration::Source(Vec::new()),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Scope::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.document = DocumentLocation::untitled(directory.join("csv-mapping.json"));
    app.save_document_to(&directory.join("csv-mapping.json"))?;
    app.rebase_history();
    assert!(cli::validate(&app.project).is_empty());
    Ok(app)
}

fn generated_files(directory: &Path) -> anyhow::Result<BTreeMap<PathBuf, Vec<u8>>> {
    fn collect(
        directory: &Path,
        relative: &Path,
        files: &mut BTreeMap<PathBuf, Vec<u8>>,
    ) -> anyhow::Result<()> {
        for entry in std::fs::read_dir(directory.join(relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            let kind = entry.file_type()?;
            if kind.is_dir() {
                collect(directory, &path, files)?;
            } else {
                assert!(kind.is_file(), "generated artifacts are ordinary files");
                files.insert(path, std::fs::read(entry.path())?);
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    collect(directory, Path::new(""), &mut files)?;
    Ok(files)
}

#[test]
fn selected_csv_generation_saves_the_latest_mapping_and_matches_the_public_writer()
-> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = csv_mapped_app(&temp.0)?;
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("latest CSV generation value".into()),
        },
    );
    app.observe_editor_history(Instant::now(), false);
    let before = crate::project_state::project_snapshot_key(&app.project);
    let before_layout = layout(&app);
    let before_history = app.history.undo_len();
    assert!(app.is_dirty());
    choose_csharp(&mut app, "generated-csv");
    app.library_generation_draft
        .as_mut()
        .unwrap()
        .include_csv_output = true;
    app.request_library_generation(&egui::Context::default());
    assert!(app.pending_library_generation.is_some());
    // The worker owns the selected setting even if the editor draft changes.
    app.library_generation_draft
        .as_mut()
        .unwrap()
        .include_csv_output = false;
    finish_worker(&mut app);
    let destination = temp.0.join("generated-csv");
    assert!(destination.join("GeneratedMapping.Csv.cs").is_file());
    assert!(destination.join("Runtime/FerruleCsv.cs").is_file());
    let generated = std::fs::read_to_string(destination.join("GeneratedMapping.cs"))?;
    assert!(generated.contains("latest CSV generation value"));
    assert!(!generated.contains("before CSV generation"));
    let saved_path = temp.0.join("csv-mapping.json");
    let saved = mapping::project_file::decode_bytes(&std::fs::read(&saved_path)?)?;
    assert_eq!(crate::project_state::project_snapshot_key(&saved), before);
    assert_eq!(saved.target_options, app.project.target_options);
    let expected = temp.0.join("expected-csv");
    cli::generate_project_with_csv_output(&saved_path, &expected, cli::GenerateTarget::CSharp)?;
    assert_eq!(generated_files(&destination)?, generated_files(&expected)?);
    assert_eq!(layout(&app), before_layout);
    assert_eq!(app.history.undo_len(), before_history);
    assert!(!app.is_dirty());
    assert!(app.library_generation_draft.is_none());
    assert!(app.diagnostics.is_empty());
    Ok(())
}

#[test]
fn csv_selection_survives_save_cancel_and_failure_then_uses_the_captured_save_as_settings()
-> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = csv_mapped_app(&temp.0)?;
    app.document = DocumentLocation::untitled(temp.0.join("untitled-csv.json"));
    app.history.mark_unsaved();
    choose_csharp(&mut app, "captured-csv-library");
    app.library_generation_draft
        .as_mut()
        .unwrap()
        .include_csv_output = true;
    returned_save_dialog(&mut app, None);
    let draft = app.library_generation_draft.as_ref().unwrap();
    assert!(draft.include_csv_output);
    assert!(draft.pending_settings.is_none());
    assert!(app.pending_library_generation.is_none());

    returned_save_dialog(&mut app, Some(&temp.0.join("missing-parent/mapping.json")));
    let draft = app.library_generation_draft.as_ref().unwrap();
    assert!(draft.include_csv_output);
    assert!(draft.pending_settings.is_none());
    assert!(
        draft
            .error
            .as_deref()
            .unwrap()
            .contains("could not be saved")
    );
    assert!(app.pending_library_generation.is_none());

    let saved_parent = temp.0.join("saved-csv-location");
    std::fs::create_dir(&saved_parent)?;
    let saved_path = saved_parent.join("mapping.json");
    set_save_as_result(&mut app, Some(&saved_path));
    app.request_library_generation(&egui::Context::default());
    assert!(matches!(
        app.pending_dialog,
        Some((DialogKind::SaveProjectAs, _))
    ));
    let draft = app.library_generation_draft.as_mut().unwrap();
    assert!(draft.pending_settings.as_ref().unwrap().include_csv_output);
    // These edits simulate outside changes while the dialog is pending. The
    // requested language, relative folder, runtime and CSV choice are frozen.
    draft.language = LibraryLanguage::Rust;
    draft.destination = "changed-library".into();
    draft.runtime_path.clear();
    draft.include_csv_output = false;
    app.poll_dialog(&egui::Context::default());
    assert!(app.pending_library_generation.is_some());
    finish_worker(&mut app);
    let destination = saved_parent.join("captured-csv-library");
    assert!(destination.join("GeneratedMapping.Csv.cs").is_file());
    assert!(destination.join("Runtime/FerruleCsv.cs").is_file());
    assert!(!saved_parent.join("changed-library").exists());
    assert!(!temp.0.join("captured-csv-library").exists());
    assert_eq!(app.document.saved_path(), Some(saved_path.as_path()));
    assert!(app.status.contains("generated C# library"));
    assert!(app.library_generation_draft.is_none());
    assert!(app.diagnostics.is_empty());
    Ok(())
}

#[test]
fn csv_selection_rejects_a_stored_non_csv_target_without_publishing_a_tree() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = csv_mapped_app(&temp.0)?;
    app.project.target_path = Some("output.json".into());
    choose_csharp(&mut app, "rejected-csv-library");
    app.library_generation_draft
        .as_mut()
        .unwrap()
        .include_csv_output = true;
    app.request_library_generation(&egui::Context::default());
    assert!(app.pending_library_generation.is_some());
    finish_worker(&mut app);
    assert!(!temp.0.join("rejected-csv-library").exists());
    let draft = app.library_generation_draft.as_ref().unwrap();
    assert!(draft.include_csv_output);
    assert!(draft.pending_settings.is_none());
    assert!(
        draft
            .error
            .as_deref()
            .unwrap()
            .contains("requires a CSV target")
    );
    assert!(std::fs::read_dir(&temp.0)?.all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("ferrule-stage")
    }));
    Ok(())
}

#[test]
fn csv_checkbox_toggles_only_the_generation_draft_and_is_disabled_while_waiting()
-> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = csv_mapped_app(&temp.0)?;
    app.begin_library_generation();
    assert!(
        !app.library_generation_draft
            .as_ref()
            .unwrap()
            .include_csv_output
    );
    let before = crate::project_state::project_snapshot_key(&app.project);
    let before_layout = layout(&app);
    let before_history = app.history.undo_len();
    let saved_before = std::fs::read(temp.0.join("csv-mapping.json"))?;
    let context = egui::Context::default();
    let frame = |app: &mut FerruleApp, events: Vec<egui::Event>| {
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.show_library_generation(ui.ctx()),
        )
    };
    fn label_center(shape: &egui::epaint::Shape) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == "Include CSV output" => {
                Some(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(label_center),
            _ => None,
        }
    }
    let mut output = frame(&mut app, Vec::new());
    for _ in 0..3 {
        output = frame(&mut app, Vec::new());
    }
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| label_center(&shape.shape))
        .expect("actual Include CSV output checkbox");
    let click = |app: &mut FerruleApp| {
        for pressed in [true, false] {
            let _ = frame(
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    };
    click(&mut app);
    assert!(
        app.library_generation_draft
            .as_ref()
            .unwrap()
            .include_csv_output
    );
    click(&mut app);
    assert!(
        !app.library_generation_draft
            .as_ref()
            .unwrap()
            .include_csv_output
    );
    let (_dialog_sender, dialog_receiver) = mpsc::channel();
    app.pending_dialog = Some((DialogKind::BrowseLibraryParent, dialog_receiver));
    let _ = frame(&mut app, Vec::new());
    click(&mut app);
    assert!(
        !app.library_generation_draft
            .as_ref()
            .unwrap()
            .include_csv_output
    );
    app.pending_dialog = None;
    let (_worker_sender, worker_receiver) = mpsc::channel();
    app.pending_library_generation = Some(PendingLibraryGeneration {
        receiver: worker_receiver,
        language: LibraryLanguage::Rust,
    });
    let _ = frame(&mut app, Vec::new());
    click(&mut app);
    assert!(
        !app.library_generation_draft
            .as_ref()
            .unwrap()
            .include_csv_output
    );
    assert_eq!(
        crate::project_state::project_snapshot_key(&app.project),
        before
    );
    assert_eq!(layout(&app), before_layout);
    assert_eq!(app.history.undo_len(), before_history);
    assert_eq!(
        std::fs::read(temp.0.join("csv-mapping.json"))?,
        saved_before
    );
    assert!(!app.is_dirty());
    Ok(())
}

#[test]
fn cancelled_failed_and_successful_save_as_only_launch_after_a_real_saved_project()
-> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = mapped_app(&temp.0)?;
    app.document = DocumentLocation::untitled(temp.0.join("unsaved.json"));
    app.history.mark_unsaved();
    choose_csharp(&mut app, "generated-after-save");
    let old_destination = temp.0.join("generated-after-save");
    std::fs::create_dir(&old_destination)?;
    std::fs::write(old_destination.join("sentinel.txt"), "keep the old folder")?;
    let project_before = crate::project_state::project_snapshot_key(&app.project);
    let layout_before = layout(&app);
    let original_positions = positions(&app);
    returned_save_dialog(&mut app, None);
    assert!(app.pending_library_generation.is_none());
    assert!(app.pending_save_continuation.is_none());
    assert!(app.library_generation_draft.is_some());
    assert!(app.document.saved_path().is_none());
    assert!(app.is_dirty());
    assert_eq!(layout(&app), layout_before);
    assert_eq!(
        crate::project_state::project_snapshot_key(&app.project),
        project_before
    );
    assert_eq!(
        std::fs::read_to_string(old_destination.join("sentinel.txt"))?,
        "keep the old folder"
    );

    let invalid_path = temp.0.join("missing-parent/mapping.json");
    returned_save_dialog(&mut app, Some(&invalid_path));
    assert!(app.pending_library_generation.is_none());
    assert!(!invalid_path.exists());
    assert!(app.document.saved_path().is_none());
    assert_eq!(layout(&app), layout_before);
    assert_eq!(
        crate::project_state::project_snapshot_key(&app.project),
        project_before
    );
    assert!(
        app.library_generation_draft
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("could not be saved")
    );
    assert_eq!(
        std::fs::read_to_string(old_destination.join("sentinel.txt"))?,
        "keep the old folder"
    );

    let new_parent = temp.0.join("saved-elsewhere");
    std::fs::create_dir(&new_parent)?;
    let saved_path = new_parent.join("mapping.json");
    returned_save_dialog(&mut app, Some(&saved_path));
    assert!(app.pending_library_generation.is_some());
    finish_worker(&mut app);
    assert_eq!(app.document.saved_path(), Some(saved_path.as_path()));
    assert!(
        new_parent
            .join("generated-after-save/GeneratedMapping.cs")
            .is_file()
    );
    assert_eq!(
        std::fs::read_to_string(old_destination.join("sentinel.txt"))?,
        "keep the old folder"
    );
    assert_eq!(std::fs::read_dir(&old_destination)?.count(), 1);
    assert_eq!(positions(&app), original_positions);
    assert_eq!(app.project.source_path.as_deref(), Some("../input.json"));
    assert_eq!(app.project.target_path.as_deref(), Some("../output.json"));
    assert_eq!(
        app.project.extra_targets[0].path.as_deref(),
        Some("../audit.json")
    );
    assert!(app.diagnostics.is_empty());
    Ok(())
}

#[test]
fn untitled_rust_checks_presence_then_resolves_the_runtime_only_beside_the_saved_project()
-> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = mapped_app(&temp.0)?;
    app.document = DocumentLocation::untitled(temp.0.join("untitled-rust.json"));
    app.history.mark_unsaved();
    app.begin_library_generation();
    app.library_generation_draft
        .as_mut()
        .unwrap()
        .destination
        .clear();
    set_save_as_result(&mut app, None);
    app.request_library_generation(&egui::Context::default());
    assert!(app.pending_dialog.is_none());
    assert!(app.pending_library_generation.is_none());
    assert!(
        app.library_generation_draft
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("new folder")
    );

    app.library_generation_draft.as_mut().unwrap().destination = "rust-library".into();
    set_save_as_result(&mut app, None);
    app.request_library_generation(&egui::Context::default());
    assert!(app.pending_dialog.is_none());
    assert!(app.pending_library_generation.is_none());
    assert!(
        app.library_generation_draft
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("codegen-runtime")
    );

    let new_parent = temp.0.join("saved-rust-location");
    let runtime = new_parent.join("selected-runtime");
    std::fs::create_dir_all(&runtime)?;
    std::fs::write(
        runtime.join("Cargo.toml"),
        "[package]\nname = \"codegen-runtime\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    assert!(!temp.0.join("selected-runtime").exists());
    app.library_generation_draft.as_mut().unwrap().runtime_path = "selected-runtime".into();
    let before = crate::project_state::project_snapshot_key(&app.project);
    let original_positions = positions(&app);
    returned_save_dialog(&mut app, None);
    assert!(app.pending_library_generation.is_none());
    assert!(app.document.saved_path().is_none());
    assert_eq!(
        crate::project_state::project_snapshot_key(&app.project),
        before
    );
    assert!(!new_parent.join("rust-library").exists());

    let saved_path = new_parent.join("mapping.json");
    set_save_as_result(&mut app, Some(&saved_path));
    app.request_library_generation(&egui::Context::default());
    assert!(matches!(
        app.pending_dialog,
        Some((DialogKind::SaveProjectAs, _))
    ));
    let draft = app.library_generation_draft.as_mut().unwrap();
    assert_eq!(
        draft.pending_settings.as_ref().unwrap().runtime_path,
        "selected-runtime"
    );
    draft.runtime_path = "changed-runtime".into();
    draft.language = LibraryLanguage::CSharp;
    draft.destination = "changed-rust-library".into();
    draft.include_csv_output = true;
    app.poll_dialog(&egui::Context::default());
    assert!(app.pending_library_generation.is_some());
    finish_worker(&mut app);
    let destination = new_parent.join("rust-library");
    let manifest = std::fs::read_to_string(destination.join("Cargo.toml"))?;
    assert!(manifest.contains("saved-rust-location"));
    assert!(manifest.contains("selected-runtime"));
    assert!(destination.join("src/lib.rs").is_file());
    assert!(!std::fs::read_to_string(destination.join("src/lib.rs"))?.contains("execute_csv"));
    assert!(!new_parent.join("changed-rust-library").exists());
    assert!(!temp.0.join("rust-library").exists());
    assert!(!temp.0.join("selected-runtime").exists());
    assert_eq!(app.document.saved_path(), Some(saved_path.as_path()));
    assert_eq!(positions(&app), original_positions);
    assert!(app.diagnostics.is_empty());
    Ok(())
}

#[test]
fn explicit_rust_runtime_is_required_and_its_selected_path_reaches_the_public_writer()
-> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = mapped_app(&temp.0)?;
    let before = std::fs::read(temp.0.join("mapping.json"))?;
    app.begin_library_generation();
    let draft = app.library_generation_draft.as_mut().unwrap();
    assert!(draft.runtime_path.is_empty());
    draft.destination = "generated-rust".into();
    app.request_library_generation(&egui::Context::default());
    assert!(app.pending_library_generation.is_none());
    assert!(
        app.library_generation_draft
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("codegen-runtime")
    );
    assert_eq!(std::fs::read(temp.0.join("mapping.json"))?, before);
    assert!(!temp.0.join("generated-rust").exists());

    let runtime = temp.0.join("selected-runtime");
    std::fs::create_dir(&runtime)?;
    std::fs::write(
        runtime.join("Cargo.toml"),
        "[package]\nname = \"codegen-runtime\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    app.stage_library_runtime_folder(runtime.display().to_string());
    app.request_library_generation(&egui::Context::default());
    finish_worker(&mut app);
    let generated = temp.0.join("generated-rust");
    let manifest = std::fs::read_to_string(generated.join("Cargo.toml"))?;
    assert!(
        manifest.contains("selected-runtime"),
        "the explicit runtime choice reaches the Cargo dependency"
    );
    assert!(generated.join("src/lib.rs").is_file());
    assert!(app.status.contains("generated Rust library"));
    assert!(app.library_generation_draft.is_none());
    assert!(app.diagnostics.is_empty());
    Ok(())
}

#[test]
fn existing_output_and_unsupported_mapping_keep_sentinels_and_publish_no_partial_tree()
-> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = mapped_app(&temp.0)?;
    let output = temp.0.join("keep-existing");
    std::fs::create_dir(&output)?;
    std::fs::write(output.join("sentinel.txt"), "keep this directory")?;
    choose_csharp(&mut app, "keep-existing");
    app.request_library_generation(&egui::Context::default());
    assert!(app.pending_library_generation.is_none());
    assert_eq!(
        std::fs::read_to_string(output.join("sentinel.txt"))?,
        "keep this directory"
    );
    assert_eq!(std::fs::read_dir(&output)?.count(), 1);
    assert!(
        app.diagnostics.items()[0]
            .message
            .contains("already exists")
    );

    app.library_generation_draft.as_mut().unwrap().destination = "unsupported-output".into();
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("unsupported descriptor".into()),
        },
    );
    app.project.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::String("Value".into()),
        },
    );
    app.project.graph.nodes.insert(
        2,
        Node::Call {
            function: "flextext_parse_field".into(),
            args: vec![0, 1, 3],
        },
    );
    app.project.root.bindings[0].node = 2;
    assert!(cli::validate(&app.project).is_empty());
    let before = crate::project_state::project_snapshot_key(&app.project);
    app.request_library_generation(&egui::Context::default());
    assert!(app.pending_library_generation.is_some());
    finish_worker(&mut app);
    assert!(!temp.0.join("unsupported-output").exists());
    assert!(
        app.library_generation_draft
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("flextext_parse_field")
    );
    assert!(
        app.diagnostics.items()[0]
            .message
            .contains("flextext_parse_field")
    );
    assert_eq!(
        crate::project_state::project_snapshot_key(&app.project),
        before
    );
    assert_eq!(
        std::fs::read_to_string(output.join("sentinel.txt"))?,
        "keep this directory"
    );
    assert!(std::fs::read_dir(&temp.0)?.all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("ferrule-stage")
    }));
    Ok(())
}

#[test]
fn close_waits_for_generation_then_uses_the_existing_dirty_project_guard() {
    let mut app = FerruleApp::default();
    let (sender, receiver) = mpsc::channel();
    app.pending_library_generation = Some(PendingLibraryGeneration {
        receiver,
        language: LibraryLanguage::CSharp,
    });
    let context = egui::Context::default();
    assert!(app.guard_library_generation_close_requested(&context, true));
    assert!(app.close_after_library_generation);
    app.poll_library_generation(&context);
    assert!(app.pending_library_generation.is_some());
    assert!(!app.allow_close);
    app.project
        .graph
        .nodes
        .insert(4, Node::Const { value: Value::Null });
    sender
        .send(Err("unsupported mapping detail".into()))
        .unwrap();
    app.poll_library_generation(&context);
    assert!(app.pending_library_generation.is_none());
    assert!(!app.close_after_library_generation);
    assert_eq!(
        app.pending_destructive_action,
        Some(DestructiveAction::Close)
    );
    assert!(!app.allow_close);
    assert!(
        app.diagnostics.items()[0]
            .message
            .contains("unsupported mapping detail")
    );
}

#[test]
fn setup_parent_choice_and_cancel_preserve_the_complete_editor_without_output() -> anyhow::Result<()>
{
    let temp = TestDir::new()?;
    let mut app = mapped_app(&temp.0)?;
    app.begin_library_generation();
    let draft = app.library_generation_draft.as_mut().unwrap();
    draft.destination = "nested/custom-library".into();
    let new_parent = temp.0.join("other-parent");
    std::fs::create_dir(&new_parent)?;
    app.stage_library_parent_folder(new_parent.clone());
    assert_eq!(
        Path::new(&app.library_generation_draft.as_ref().unwrap().destination),
        new_parent.join("custom-library")
    );
    assert!(!new_parent.join("custom-library").exists());
    let before = crate::project_state::project_snapshot_key(&app.project);
    let before_layout = layout(&app);
    let before_history = app.history.undo_len();
    let context = egui::Context::default();
    let frame = |app: &mut FerruleApp, events: Vec<egui::Event>| {
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.show_library_generation(ui.ctx()),
        )
    };
    let mut output = frame(&mut app, Vec::new());
    for _ in 0..3 {
        output = frame(&mut app, Vec::new());
    }
    fn cancel_center(shape: &egui::epaint::Shape) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == "Cancel" => {
                Some(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(cancel_center),
            _ => None,
        }
    }
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| cancel_center(&shape.shape))
        .expect("actual generation Cancel control");
    for pressed in [true, false] {
        let _ = frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(app.library_generation_draft.is_none());
    assert!(app.pending_library_generation.is_none());
    assert_eq!(
        crate::project_state::project_snapshot_key(&app.project),
        before
    );
    assert_eq!(layout(&app), before_layout);
    assert_eq!(app.history.undo_len(), before_history);
    assert!(!new_parent.join("custom-library").exists());
    assert!(!app.is_dirty());
    Ok(())
}
