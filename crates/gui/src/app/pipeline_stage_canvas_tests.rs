use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, NamedSource, NamedTarget, Pipeline, PipelineInput, PipelineStage};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-stage-canvas-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        for folder in ["flows", "data", "outputs", "designs"] {
            std::fs::create_dir(path.join(folder)).unwrap();
        }
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Ok("1")
        {
            eprintln!("retained stage canvas originals: {}", self.0.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn fixture(directory: &Directory) -> FerruleApp {
    let schema = SchemaNode::group(
        "Record",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let mut project = blank_project();
    project.source = schema.clone();
    project.target = schema.clone();
    project.source_path = Some("../data/orders.json".into());
    project.target_path = Some("../outputs/primary.json".into());
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project.graph.nodes.clear();
    project.graph.nodes.extend([
        (
            0,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: None,
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("before".into()),
            },
        ),
    ]);
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 1,
        }],
        ..Default::default()
    };
    project.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "../data/lookup.json".into(),
        schema: schema.clone(),
        options: project.source_options.clone(),
        dynamic_path: None,
    });
    project.extra_targets.push(NamedTarget {
        name: "selected".into(),
        path: Some("../outputs/selected.json".into()),
        schema,
        options: project.target_options.clone(),
        root: project.root.clone(),
    });
    let mut body = Graph::default();
    body.nodes.insert(
        0,
        Node::Const {
            value: Value::String("function-value".into()),
        },
    );
    project.user_functions.insert(
        FunctionId::new(7),
        UserFunction {
            library: "local".into(),
            name: "helper".into(),
            description: None,
            parameters: Vec::new(),
            output_name: "result".into(),
            output_type: ScalarType::String,
            body,
            output: 0,
        },
    );
    let mut finish = project.clone();
    finish.root.bindings[0].node = 0;
    finish.extra_targets.clear();
    finish.target_path = Some("../outputs/finish.json".into());
    let pipeline = Pipeline {
        main_mapping_path: Some("../designs/main.mfd".into()),
        stages: vec![
            PipelineStage {
                id: "prepare".into(),
                mapping_path: Some("../designs/prepare.mfd".into()),
                project,
                source: PipelineInput::Host {
                    name: "orders".into(),
                },
                extra_sources: vec![mapping::PipelineNamedInput {
                    name: "lookup".into(),
                    from: PipelineInput::Host {
                        name: "lookup".into(),
                    },
                }],
            },
            PipelineStage {
                id: "finish".into(),
                mapping_path: Some("../designs/finish.mfd".into()),
                project: finish,
                source: PipelineInput::StageTarget {
                    stage: "prepare".into(),
                    target: Some("selected".into()),
                },
                extra_sources: vec![mapping::PipelineNamedInput {
                    name: "lookup".into(),
                    from: PipelineInput::Host {
                        name: "lookup".into(),
                    },
                }],
            },
        ],
    };
    assert!(engine::validate_pipeline(&pipeline).is_empty());
    for (path, content) in [
        ("data/orders.json", r#"{"Value":"source"}"#),
        ("data/lookup.json", r#"{"Value":"lookup"}"#),
        ("outputs/primary.json", "old primary"),
        ("outputs/selected.json", "old selected"),
        ("outputs/finish.json", "old finish"),
        ("designs/main.mfd", "main identity"),
        ("designs/prepare.mfd", "stage identity"),
        ("designs/finish.mfd", "stage identity"),
    ] {
        std::fs::write(directory.0.join(path), content).unwrap();
    }
    let path = directory.0.join("flows/pipeline.json");
    std::fs::write(
        &path,
        mapping::pipeline_file::encode_pretty(&pipeline).unwrap(),
    )
    .unwrap();
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        42,
        Node::Const {
            value: Value::String("unsaved main".into()),
        },
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    app.request_pipeline_editor_action(pipeline_editor_ui::PipelineEditorAction::Open(path));
    app
}

fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1800.0, 1100.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            if app.pipeline_stage_canvas.is_some() {
                app.show_pipeline_stage_canvas(ui);
                app.show_pipeline_stage_close_guard(ui.ctx());
            } else {
                app.show_pipeline_editor(ui.ctx());
            }
        },
    )
}

fn text_positions(shape: &egui::epaint::Shape, label: &str, positions: &mut Vec<egui::Pos2>) {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
            positions.push(text.visual_bounding_rect().center())
        }
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                text_positions(shape, label, positions);
            }
        }
        _ => {}
    }
}

fn click(app: &mut FerruleApp, context: &egui::Context, label: &str) {
    let mut output = frame(app, context, Vec::new());
    for _ in 0..4 {
        output = frame(app, context, Vec::new());
    }
    let mut positions = Vec::new();
    for shape in &output.shapes {
        text_positions(&shape.shape, label, &mut positions);
    }
    let position = *positions
        .first()
        .unwrap_or_else(|| panic!("visible stage control {label:?}"));
    pointer_click(app, context, position);
}

fn pointer_click(app: &mut FerruleApp, context: &egui::Context, position: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            context,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn edit_constant(app: &mut FerruleApp, context: &egui::Context, old: &str) {
    context.enable_accesskit();
    let mut output = frame(app, context, Vec::new());
    for _ in 0..4 {
        output = frame(app, context, Vec::new());
    }
    let mut pencils = Vec::new();
    for shape in &output.shapes {
        text_positions(
            &shape.shape,
            &char::from(lucide_icons::Icon::Pencil).to_string(),
            &mut pencils,
        );
    }
    // The fixed named-target boundary button may also paint a disabled pencil.
    pointer_click(
        app,
        context,
        *pencils.last().expect("the node property pencil is visible"),
    );
    for _ in 0..5 {
        output = frame(app, context, Vec::new());
    }
    let inputs = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("the popup has accessible controls")
        .nodes
        .iter()
        .filter(|(_, node)| {
            node.role() == egui::accesskit::Role::TextInput && node.value() == Some(old)
        })
        .collect::<Vec<_>>();
    eprintln!("stage constant popup inputs for {old:?}: {inputs:?}");
    assert_eq!(inputs.len(), 1, "one real constant text input is visible");
    let (input_id, input) = inputs[0];
    let bounds = input.bounds().expect("the text input has response bounds");
    let position = egui::pos2(
        ((bounds.x0 + bounds.x1) * 0.5) as f32,
        ((bounds.y0 + bounds.y1) * 0.5) as f32,
    );
    pointer_click(app, context, position);
    let focused = context
        .memory(|memory| memory.focused())
        .expect("the pointer click focuses the text input");
    let response = context
        .read_response(focused)
        .expect("the focused input has a real response");
    let state = egui::TextEdit::load_state(context, focused);
    eprintln!(
        "stage constant pointer focus: expected={input_id:?}, actual={:?}, response={response:?}, text_state={}",
        focused.accesskit_id(),
        state.is_some()
    );
    assert_eq!(focused.accesskit_id(), *input_id);
    assert!(response.enabled() && response.has_focus());
    assert!(state.is_some(), "the focused control is a real TextEdit");
    frame(
        app,
        context,
        vec![
            key(egui::Key::End, egui::Modifiers::NONE),
            egui::Event::Text("-edited".into()),
        ],
    );
    frame(
        app,
        context,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
}

fn child(app: &FerruleApp) -> &FerruleApp {
    &app.pipeline_stage_canvas.as_ref().unwrap().editor
}
fn child_mut(app: &mut FerruleApp) -> &mut FerruleApp {
    &mut app.pipeline_stage_canvas.as_mut().unwrap().editor
}
fn pipeline(app: &FerruleApp) -> &Pipeline {
    &app.pipeline_editor.as_ref().unwrap().document.pipeline
}
fn parent_state(app: &FerruleApp) -> EditorSnapshot {
    editor_snapshot(&app.project, &app.main_canvas.snarl, &app.mapping_workspace)
}
fn const_text(project: &Project, id: NodeId) -> &str {
    let Node::Const {
        value: Value::String(value),
    } = &project.graph.nodes[&id]
    else {
        panic!("string constant");
    };
    value
}

#[test]
fn stage_canvas_real_property_wire_history_and_apply_leave_parent_and_other_stage_exact() {
    let directory = Directory::new();
    let mut app = fixture(&directory);
    let context = context();
    let parent = parent_state(&app);
    let document = app.document.clone();
    let main_undo = app.can_undo();
    let original = pipeline(&app).clone();
    click(&mut app, &context, "Edit mapping on canvas");
    edit_constant(&mut app, &context, "before");
    eprintln!(
        "stage edit original: {}",
        crate::project_state::project_snapshot_key(&child(&app).project)
    );
    assert_eq!(const_text(&child(&app).project, 1), "before-edited");
    assert!(child(&app).can_undo());
    let command = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    frame(&mut app, &context, vec![key(egui::Key::Z, command)]);
    assert_eq!(const_text(&child(&app).project, 1), "before");
    frame(
        &mut app,
        &context,
        vec![key(
            egui::Key::Z,
            egui::Modifiers {
                shift: true,
                ..command
            },
        )],
    );
    assert_eq!(const_text(&child(&app).project, 1), "before-edited");
    for _ in 0..8 {
        frame(&mut app, &context, Vec::new());
    }
    // Capture real drag widgets emitted by the existing main GraphViewer, not synthetic pin coordinates.
    let mut pins = child(&app)
        .embedded_stage_pin_ids
        .iter()
        .map(|id| {
            let response = context
                .read_response(*id)
                .expect("current-frame pin response");
            let center = context
                .layer_transform_to_global(response.layer_id)
                .unwrap_or_default()
                * response.rect.center();
            (response.id, center)
        })
        .collect::<Vec<_>>();
    eprintln!("stage wire pin originals: {pins:?}");
    assert_eq!(
        pins.len(),
        3,
        "one source output, constant output and target input"
    );
    pins.sort_by(|left, right| left.1.x.total_cmp(&right.1.x));
    let from = pins[0].1;
    let to = pins[2].1;
    frame(
        &mut app,
        &context,
        vec![
            egui::Event::PointerMoved(from),
            egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(
        &mut app,
        &context,
        vec![egui::Event::PointerMoved(from + egui::vec2(12.0, 0.0))],
    );
    frame(&mut app, &context, vec![egui::Event::PointerMoved(to)]);
    frame(
        &mut app,
        &context,
        vec![egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    frame(&mut app, &context, Vec::new());
    eprintln!(
        "stage wire result: {}",
        crate::project_state::project_snapshot_key(&child(&app).project)
    );
    assert_eq!(child(&app).project.root.bindings[0].node, 0);
    assert_eq!(child(&app).main_canvas.snarl.wires().count(), 1);
    let source = Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String("source".into())),
        )]
        .into(),
    );
    let result = engine::run_with_sources(
        &child(&app).project,
        &source,
        vec![("lookup".into(), source.clone())],
    )
    .unwrap();
    assert_eq!(
        result, source,
        "the real wire determines the native mapping result"
    );
    click(&mut app, &context, "Apply stage mapping");
    assert!(app.pipeline_stage_canvas.is_none());
    assert_eq!(pipeline(&app).stages[0].project.root.bindings[0].node, 0);
    assert_eq!(
        const_text(&pipeline(&app).stages[0].project, 1),
        "before-edited"
    );
    let mut expected = original;
    expected.stages[0].project = pipeline(&app).stages[0].project.clone();
    assert_eq!(
        crate::project_state::pipeline_snapshot_key(pipeline(&app)),
        crate::project_state::pipeline_snapshot_key(&expected)
    );
    assert!(parent_state(&app) == parent);
    assert_eq!(app.document, document);
    assert_eq!(app.can_undo(), main_undo);
    assert!(app.is_dirty());
    assert!(app.pipeline_editor.as_ref().unwrap().document.is_dirty());
}

#[test]
fn stage_named_and_function_canvases_edit_same_owned_snapshot_and_cancel_discards_it() {
    let directory = Directory::new();
    let mut app = fixture(&directory);
    let context = context();
    let original = crate::project_state::pipeline_snapshot_key(pipeline(&app));
    let parent = parent_state(&app);
    click(&mut app, &context, "Edit mapping on canvas");
    let ordinary = egui::Id::new("main_mapping_canvas");
    assert_eq!(app.embedded_canvas_id(ordinary), ordinary);
    assert_ne!(child(&app).embedded_canvas_id(ordinary), ordinary);
    let first_session_id = child(&app).embedded_canvas_id(ordinary);
    click(&mut app, &context, "Primary");
    click(&mut app, &context, "selected");
    assert_eq!(
        child(&app).mapping_workspace.active,
        MappingDocument::Target(0)
    );
    edit_constant(&mut app, &context, "before");
    assert_eq!(const_text(&child(&app).project, 1), "before-edited");
    click(&mut app, &context, "Existing functions");
    click(&mut app, &context, "local:helper");
    assert_eq!(
        child(&app).mapping_workspace.active,
        MappingDocument::Function(FunctionId::new(7))
    );
    edit_constant(&mut app, &context, "function-value");
    assert_eq!(
        const_text_in_function(&child(&app).project),
        "function-value-edited"
    );
    click(&mut app, &context, "Cancel stage editing");
    click(&mut app, &context, "Keep editing stage");
    assert!(app.pipeline_stage_canvas.is_some());
    click(&mut app, &context, "Cancel stage editing");
    click(&mut app, &context, "Discard stage changes");
    assert!(app.pipeline_stage_canvas.is_none());
    assert_eq!(
        crate::project_state::pipeline_snapshot_key(pipeline(&app)),
        original
    );
    assert!(parent_state(&app) == parent);
    click(&mut app, &context, "Edit mapping on canvas");
    assert_ne!(
        child(&app).embedded_canvas_id(ordinary),
        first_session_id,
        "reopening receives a fresh session namespace"
    );
    click(&mut app, &context, "Cancel stage editing");
    assert!(
        app.pipeline_stage_canvas.is_none(),
        "a clean reopened draft closes without confirmation"
    );
}

fn const_text_in_function(project: &Project) -> &str {
    let Node::Const {
        value: Value::String(value),
    } = &project.user_functions[&FunctionId::new(7)].body.nodes[&0]
    else {
        panic!("function string constant");
    };
    value
}

#[test]
fn stage_apply_refuses_stale_owner_boundaries_and_full_pipeline_error_atomically() {
    let directory = Directory::new();
    let mut app = fixture(&directory);
    let original = crate::project_state::pipeline_snapshot_key(pipeline(&app));
    app.begin_pipeline_stage_canvas(0);
    child_mut(&mut app).project.source_path = Some("changed.json".into());
    assert!(!app.apply_pipeline_stage_canvas());
    assert!(
        app.pipeline_stage_canvas
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("boundary")
    );
    assert_eq!(
        crate::project_state::pipeline_snapshot_key(pipeline(&app)),
        original
    );
    child_mut(&mut app).project.source_path = Some("../data/orders.json".into());
    // Removing a graph node owned by the named target is invalid even when primary output remains valid.
    child_mut(&mut app).project.extra_targets[0].root.bindings[0].node = 999;
    assert!(!app.apply_pipeline_stage_canvas());
    let error = app
        .pipeline_stage_canvas
        .as_ref()
        .unwrap()
        .error
        .as_ref()
        .unwrap();
    eprintln!("complete candidate validation refusal: {error}");
    assert!(error.contains("invalid") && error.contains("prepare"));
    assert_eq!(
        crate::project_state::pipeline_snapshot_key(pipeline(&app)),
        original
    );
    child_mut(&mut app).project.extra_targets[0].root.bindings[0].node = 1;
    app.pipeline_editor
        .as_mut()
        .unwrap()
        .document
        .rename_stage(1, "changed-finish")
        .unwrap();
    let changed = crate::project_state::pipeline_snapshot_key(pipeline(&app));
    assert!(!app.apply_pipeline_stage_canvas());
    assert!(
        app.pipeline_stage_canvas
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("pipeline changed")
    );
    assert_eq!(
        crate::project_state::pipeline_snapshot_key(pipeline(&app)),
        changed
    );
    app.pipeline_editor.as_mut().unwrap().document.path = directory.0.join("different.json");
    assert!(!app.apply_pipeline_stage_canvas());
    assert!(
        app.pipeline_stage_canvas
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("document changed")
    );
    app.pipeline_stage_canvas = None;
    app.pipeline_editor.as_mut().unwrap().document.path = directory.0.join("flows/pipeline.json");
    app.pipeline_editor
        .as_mut()
        .unwrap()
        .document
        .pipeline
        .stages[1]
        .project
        .root
        .bindings[0]
        .node = 999;
    let unrelated_invalid = crate::project_state::pipeline_snapshot_key(pipeline(&app));
    app.begin_pipeline_stage_canvas(0);
    assert!(
        cli::validate(&child(&app).project).is_empty(),
        "selected mapping is valid"
    );
    child_mut(&mut app).project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("changed".into()),
        },
    );
    assert!(
        !app.apply_pipeline_stage_canvas(),
        "the other stage must also be validated"
    );
    assert!(
        app.pipeline_stage_canvas
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("changed-finish")
    );
    assert_eq!(
        crate::project_state::pipeline_snapshot_key(pipeline(&app)),
        unrelated_invalid
    );
}

#[test]
fn stage_busy_actions_and_close_resolve_draft_before_pipeline_before_parent() {
    let directory = Directory::new();
    let mut app = fixture(&directory);
    let context = context();
    click(&mut app, &context, "Edit mapping on canvas");
    edit_constant(&mut app, &context, "before");
    let original = crate::project_state::pipeline_snapshot_key(pipeline(&app));
    let parent = parent_state(&app);
    let pipeline_path = directory.0.join("flows/pipeline.json");
    let disk = std::fs::read(&pipeline_path).unwrap();
    assert!(!app.ui_project_editing_enabled());
    assert!(child(&app).ui_project_editing_enabled());
    app.begin_preview();
    app.begin_library_generation();
    app.begin_rest_run();
    app.begin_mfd_pipeline_import();
    app.begin_pipeline_mfd_export(mfd::ExportProfile::FerruleExtensions);
    app.load_pipeline_for_run(&pipeline_path);
    app.add_pipeline_stage_from_path(&directory.0.join("missing.json"));
    app.request_pipeline_editor_action(pipeline_editor_ui::PipelineEditorAction::Close);
    assert!(
        app.save_document_to(&directory.0.join("forbidden-main.json"))
            .is_err()
    );
    let enabled = app.ui_project_editing_enabled();
    app.handle_history_shortcuts(&context, enabled);
    assert!(
        app.preview_draft.is_none()
            && app.library_generation_draft.is_none()
            && app.rest_run_draft.is_none()
    );
    assert!(app.pending_dialog.is_none() && app.pipeline_run_draft.is_none());
    assert_eq!(std::fs::read(&pipeline_path).unwrap(), disk);
    assert_eq!(
        crate::project_state::pipeline_snapshot_key(pipeline(&app)),
        original
    );
    assert!(parent_state(&app) == parent);
    app.guard_app_close_requested(&context, true);
    assert!(
        app.pending_pipeline_editor_action.is_none() && app.pending_destructive_action.is_none()
    );
    click(&mut app, &context, "Keep editing stage");
    assert!(!app.allow_close);
    app.guard_app_close_requested(&context, true);
    click(&mut app, &context, "Apply stage changes");
    assert!(app.pipeline_stage_canvas.is_none());
    assert!(matches!(
        app.pending_pipeline_editor_action,
        Some(pipeline_editor_ui::PipelineEditorAction::CloseApp)
    ));
    assert!(app.pending_destructive_action.is_none());
    app.discard_pending_pipeline_editor_action(&context);
    assert_eq!(
        app.pending_destructive_action,
        Some(DestructiveAction::Close)
    );
    assert!(!app.allow_close);
}

#[test]
fn stage_apply_save_reopen_and_pipeline_preview_preserve_physical_paths_and_edge_value() {
    let directory = Directory::new();
    let mut app = fixture(&directory);
    let context = context();
    let parent = parent_state(&app);
    let original = pipeline(&app).clone();
    click(&mut app, &context, "Edit mapping on canvas");
    edit_constant(&mut app, &context, "before");
    // The live stage context must retain its stage and main mapping identities after Apply.
    child_mut(&mut app).project.graph.nodes.extend([
        (
            2,
            Node::RuntimeValue {
                value: mapping::RuntimeValue::MappingFilePath,
            },
        ),
        (
            3,
            Node::RuntimeValue {
                value: mapping::RuntimeValue::MainMappingFilePath,
            },
        ),
        (
            4,
            Node::Const {
                value: Value::String("|".into()),
            },
        ),
        (
            5,
            Node::Call {
                function: "concat".into(),
                args: vec![1, 4, 2, 4, 3],
            },
        ),
    ]);
    child_mut(&mut app).project.root.bindings[0].node = 5;
    child_mut(&mut app).project.extra_targets[0].root.bindings[0].node = 5;
    click(&mut app, &context, "Apply stage mapping");
    let path = directory.0.join("flows/pipeline.json");
    let expected_value = format!(
        "before-edited|{}|{}",
        path.parent()
            .unwrap()
            .join("../designs/prepare.mfd")
            .display(),
        path.parent().unwrap().join("../designs/main.mfd").display()
    );
    app.load_pipeline_for_run(&path);
    assert!(
        app.pipeline_run_draft.is_none(),
        "unsaved applied content cannot launch saved content"
    );
    app.pipeline_editor
        .as_mut()
        .unwrap()
        .document
        .save()
        .unwrap();
    let saved = crate::project_state::pipeline_snapshot_key(pipeline(&app));
    let reopened = crate::pipeline_edit::PipelineEditorDocument::load(&path).unwrap();
    assert_eq!(
        crate::project_state::pipeline_snapshot_key(&reopened.pipeline),
        saved
    );
    for (old, new) in original.stages.iter().zip(&reopened.pipeline.stages) {
        assert_eq!(old.mapping_path, new.mapping_path);
        assert_eq!(old.project.source_path, new.project.source_path);
        assert_eq!(old.project.target_path, new.project.target_path);
        assert_eq!(
            old.project.extra_sources[0].path,
            new.project.extra_sources[0].path
        );
        for hint in [
            new.project.source_path.as_deref().unwrap(),
            new.project.target_path.as_deref().unwrap(),
            &new.project.extra_sources[0].path,
            new.mapping_path.as_deref().unwrap(),
        ] {
            assert_eq!(
                std::fs::canonicalize(path.parent().unwrap().join(hint)).unwrap(),
                std::fs::canonicalize(directory.0.join(hint.strip_prefix("../").unwrap())).unwrap()
            );
        }
    }
    assert_eq!(
        original.main_mapping_path,
        reopened.pipeline.main_mapping_path
    );
    assert_eq!(
        reopened.pipeline.stages[0].project.extra_targets[0].path,
        original.stages[0].project.extra_targets[0].path
    );
    for (hint, physical) in [
        (
            reopened.pipeline.main_mapping_path.as_deref().unwrap(),
            "designs/main.mfd",
        ),
        (
            reopened.pipeline.stages[0].project.extra_targets[0]
                .path
                .as_deref()
                .unwrap(),
            "outputs/selected.json",
        ),
    ] {
        assert_eq!(
            std::fs::canonicalize(path.parent().unwrap().join(hint)).unwrap(),
            std::fs::canonicalize(directory.0.join(physical)).unwrap()
        );
    }
    app.load_pipeline_for_run(&path);
    assert!(app.pipeline_run_draft.as_ref().unwrap().issues.is_empty());
    app.start_pipeline_preview();
    wait(&mut app, &context);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    let report = &mut app.run_report.as_mut().unwrap().report;
    assert_eq!(
        report.kind,
        crate::run_report::RunReportKind::PipelinePreview
    );
    assert_eq!(report.outputs.len(), 3);
    for output in &mut report.outputs {
        let crate::run_report::OutputPreview::Text { content, .. } = output.preview() else {
            panic!("JSON preview");
        };
        let value: serde_json::Value = serde_json::from_str(content).unwrap();
        assert_eq!(value["Value"], expected_value);
    }
    for (file, expected) in [
        ("primary.json", "old primary"),
        ("selected.json", "old selected"),
        ("finish.json", "old finish"),
    ] {
        assert_eq!(
            std::fs::read_to_string(directory.0.join("outputs").join(file)).unwrap(),
            expected
        );
    }
    assert!(parent_state(&app) == parent);
}

fn wait(app: &mut FerruleApp, context: &egui::Context) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.pending_pipeline_run.is_some() && std::time::Instant::now() < deadline {
        app.poll_pipeline_run(context);
        if app.pending_pipeline_run.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    assert!(
        app.pending_pipeline_run.is_none(),
        "owned Preview worker completes"
    );
}

#[test]
fn clean_stage_close_and_pending_run_admission_do_not_displace_owned_state() {
    let directory = Directory::new();
    let mut app = fixture(&directory);
    let path = directory.0.join("flows/pipeline.json");
    app.load_pipeline_for_run(&path);
    app.begin_pipeline_stage_canvas(0);
    assert!(app.pipeline_stage_canvas.is_none() && app.pipeline_run_draft.is_some());
    app.pipeline_run_draft = None;
    app.begin_pipeline_stage_canvas(0);
    let context = context();
    app.guard_app_close_requested(&context, true);
    assert!(app.pipeline_stage_canvas.is_none());
    assert_eq!(
        app.pending_destructive_action,
        Some(DestructiveAction::Close)
    );
    assert!(app.pending_pipeline_editor_action.is_none());
}
