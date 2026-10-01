use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, NamedTarget, Scope, ScopeIteration, SequenceWindow};

fn window_app(document: MappingDocument) -> FerruleApp {
    let mut app = FerruleApp::default();
    let rows =
        SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)]).repeating();
    app.project.source = SchemaNode::group("Source", vec![rows.clone()]);
    app.project.target = SchemaNode::group("Target", vec![rows]);
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.graph.nodes.clear();
    for (id, value) in [(1, 2), (2, 3), (3, 1), (4, 6)] {
        app.project.graph.nodes.insert(
            id,
            Node::Const {
                value: Value::Int(value),
            },
        );
    }
    app.project.graph.nodes.insert(
        10,
        Node::SourceField {
            path: vec!["Rows".into(), "Value".into()],
            frame: None,
        },
    );
    app.project.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            windows: original_windows(),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 10,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    app.project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: Some("other.json".into()),
        schema: app.project.target.clone(),
        options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        root: app.project.root.clone(),
    });
    app.main_canvas = CanvasDocumentState::main(&app.project);
    if let MappingDocument::Target(index) = document {
        app.open_target_tab(index);
    }
    app.selected_scope = vec![0];
    app.mark_clean();
    app.rebase_history();
    assert!(cli::validate(&app.project).is_empty());
    app
}

fn original_windows() -> Vec<SequenceWindow> {
    vec![
        SequenceWindow::SkipFirst { count: 1 },
        SequenceWindow::First { count: 2 },
    ]
}

fn source() -> Instance {
    Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(
            (1..=8)
                .map(|value| {
                    Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(value)))])
                })
                .collect(),
        ),
    )])
}

fn active_scope(app: &FerruleApp) -> &Scope {
    let root = match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.project.extra_targets[index].root,
        _ => &app.project.root,
    };
    &root.children[0]
}

fn values(instance: &Instance) -> Vec<i64> {
    instance
        .field("Rows")
        .and_then(Instance::as_repeated)
        .expect("repeated Rows output")
        .iter()
        .map(
            |row| match row.field("Value").and_then(Instance::as_scalar) {
                Some(Value::Int(value)) => *value,
                other => panic!("integer Value output, got {other:?}"),
            },
        )
        .collect()
}

fn assert_outputs(app: &FerruleApp, active: &[i64], other: &[i64]) {
    assert!(
        cli::validate(&app.project).is_empty(),
        "valid authored mapping"
    );
    let outputs = engine::run_outputs(&app.project, &source()).expect("execute authored mapping");
    match app.mapping_workspace.active {
        MappingDocument::Target(_) => {
            assert_eq!(values(&outputs.extras[0].instance), active);
            assert_eq!(values(&outputs.primary), other);
        }
        _ => {
            assert_eq!(values(&outputs.primary), active);
            assert_eq!(values(&outputs.extras[0].instance), other);
        }
    }
}

fn project_state(project: &Project) -> String {
    mapping::project_file::encode_pretty(project).expect("project state")
}

fn gui_context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    editing_enabled: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 1600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_inspector(ui, editing_enabled),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn rendered_labels(shape: &egui::epaint::Shape, label: &str, positions: &mut Vec<egui::Pos2>) {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
            positions.push(text.visual_bounding_rect().center());
        }
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                rendered_labels(shape, label, positions);
            }
        }
        _ => {}
    }
}

fn click(
    app: &mut FerruleApp,
    context: &egui::Context,
    editing_enabled: bool,
    label: &str,
    occurrence: usize,
) {
    let mut output = frame(app, context, editing_enabled, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, editing_enabled, Vec::new());
    }
    let mut positions = Vec::new();
    for shape in &output.shapes {
        rendered_labels(&shape.shape, label, &mut positions);
    }
    // Dropdown items are painted after their selected text in the editor.
    let position = if occurrence == usize::MAX {
        positions.last()
    } else {
        positions.get(occurrence)
    }
    .copied()
    .unwrap_or_else(|| panic!("rendered Inspector control {label:?} occurrence {occurrence}"));
    for pressed in [true, false] {
        let _ = frame(
            app,
            context,
            editing_enabled,
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

fn choose(app: &mut FerruleApp, context: &egui::Context, selected: &str, desired: &str) {
    click(app, context, true, selected, 0);
    click(app, context, true, desired, usize::MAX);
}

#[test]
fn inspector_reorders_windows_without_detaching_bounds_or_touching_other_target() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = window_app(document);
        let original = project_state(&app.project);
        let graph = serde_json::to_value(&app.project.graph).expect("graph state");
        let context = gui_context();
        assert_outputs(&app, &[3, 4, 5], &[3, 4, 5]);
        click(&mut app, &context, true, "Up", 0);
        click(&mut app, &context, true, "Down", 1);
        assert_eq!(project_state(&app.project), original, "disabled endpoints");
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
        assert!(!app.history.can_redo());
        assert!(app.pending_history.is_none());

        click(&mut app, &context, true, "Up", 1);
        assert_eq!(
            active_scope(&app).windows,
            [
                SequenceWindow::First { count: 2 },
                SequenceWindow::SkipFirst { count: 1 },
            ]
        );
        assert_outputs(&app, &[3], &[3, 4, 5]);
        assert!(app.is_dirty());
        assert!(app.can_undo());
        click(&mut app, &context, true, "Down", 0);
        assert_eq!(active_scope(&app).windows, original_windows());
        assert_outputs(&app, &[3, 4, 5], &[3, 4, 5]);
        assert_eq!(project_state(&app.project), original);
        assert_eq!(serde_json::to_value(&app.project.graph).unwrap(), graph);
    }
}

#[test]
fn reordered_windows_support_undo_redo_and_saved_reopen() -> anyhow::Result<()> {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = window_app(document);
        let original = project_state(&app.project);
        let context = gui_context();
        click(&mut app, &context, true, "Down", 0);
        let reordered = project_state(&app.project);
        let windows = active_scope(&app).windows.clone();
        assert_outputs(&app, &[3], &[3, 4, 5]);
        app.undo_project();
        assert_eq!(project_state(&app.project), original);
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
        assert!(app.history.can_redo());
        assert_outputs(&app, &[3, 4, 5], &[3, 4, 5]);
        app.redo_project();
        assert_eq!(project_state(&app.project), reordered);
        assert!(app.is_dirty());
        assert!(app.can_undo());
        assert!(!app.history.can_redo());
        assert_outputs(&app, &[3], &[3, 4, 5]);

        let path = temporary_project_path();
        let outcome = app.save_document_to(&path)?;
        assert!(outcome.validation_issues.is_empty());
        assert!(outcome.layout_warning.is_none());
        assert!(!app.is_dirty());
        let saved = project_state(&app.project);
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&path);
        if let MappingDocument::Target(index) = document {
            reopened.open_target_tab(index);
        }
        reopened.selected_scope = vec![0];
        assert_eq!(project_state(&reopened.project), saved);
        assert_eq!(active_scope(&reopened).windows, windows);
        assert_outputs(&reopened, &[3], &[3, 4, 5]);
        assert!(!reopened.is_dirty());
        assert!(!reopened.can_undo());
        let context = gui_context();
        click(&mut reopened, &context, true, "Up", 1);
        assert_outputs(&reopened, &[3, 4, 5], &[3, 4, 5]);
        std::fs::remove_dir_all(path.parent().expect("temporary project directory"))?;
    }
    Ok(())
}

fn temporary_project_path() -> PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "ferrule-gui-window-controls-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("temporary test directory");
    directory.join("project.json")
}

#[test]
fn window_kind_arguments_add_and_remove_retain_existing_authoring_behavior() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = window_app(document);
        let original = project_state(&app.project);
        let context = gui_context();
        choose(&mut app, &context, "first", "last");
        assert_eq!(
            active_scope(&app).windows,
            [
                SequenceWindow::SkipFirst { count: 1 },
                SequenceWindow::Last { count: 2 },
            ]
        );
        assert_outputs(&app, &[6, 7, 8], &[3, 4, 5]);
        choose(
            &mut app,
            &context,
            "2: constant Int(3)",
            "1: constant Int(2)",
        );
        assert_outputs(&app, &[7, 8], &[3, 4, 5]);
        click(&mut app, &context, true, "Up", 1);
        assert_eq!(
            active_scope(&app).windows,
            [
                SequenceWindow::Last { count: 1 },
                SequenceWindow::SkipFirst { count: 1 },
            ]
        );
        assert_outputs(&app, &[], &[3, 4, 5]);
        app.undo_project();
        app.undo_project();
        app.undo_project();
        assert_eq!(project_state(&app.project), original);
        app.selected_scope = vec![0];
        click(&mut app, &context, true, "+", 0);
        assert_eq!(
            active_scope(&app).windows,
            [
                SequenceWindow::SkipFirst { count: 1 },
                SequenceWindow::First { count: 2 },
                SequenceWindow::First { count: 1 },
            ]
        );
        assert_outputs(&app, &[3, 4], &[3, 4, 5]);
        click(&mut app, &context, true, "x", 2);
        assert_eq!(active_scope(&app).windows, original_windows());
        assert_outputs(&app, &[3, 4, 5], &[3, 4, 5]);
        assert_eq!(project_state(&app.project), original);
    }
}

#[test]
fn window_reordering_preserves_each_kind_and_both_range_nodes() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = window_app(document);
        let windows = vec![
            SequenceWindow::SkipFirst { count: 1 },
            SequenceWindow::First { count: 2 },
            SequenceWindow::From { position: 3 },
            SequenceWindow::FromTo { first: 1, last: 4 },
            SequenceWindow::Last { count: 2 },
        ];
        match document {
            MappingDocument::Main => app.project.root.children[0].windows = windows.clone(),
            MappingDocument::Target(index) => {
                app.project.extra_targets[index].root.children[0].windows = windows.clone();
            }
            MappingDocument::Function(_) => unreachable!(),
        }
        app.mark_clean();
        app.rebase_history();
        let original = project_state(&app.project);
        let context = gui_context();
        assert_outputs(&app, &[4, 5], &[3, 4, 5]);
        click(&mut app, &context, true, "Up", 3);
        let mut reordered = windows.clone();
        reordered.swap(2, 3);
        assert_eq!(active_scope(&app).windows, reordered);
        assert_outputs(&app, &[4, 5], &[3, 4, 5]);
        click(&mut app, &context, true, "Down", 2);
        assert_eq!(active_scope(&app).windows, windows);
        assert_eq!(project_state(&app.project), original);
        assert_outputs(&app, &[4, 5], &[3, 4, 5]);
    }
}

#[test]
fn locked_window_controls_preserve_project_outputs_and_history() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = window_app(document);
        app.begin_preview();
        assert!(app.preview_draft.is_some(), "preview setup takes edit lock");
        let original = project_state(&app.project);
        let before = engine::run_outputs(&app.project, &source()).expect("valid mapping");
        let context = gui_context();
        for (label, occurrence) in [
            ("Up", 1),
            ("Down", 0),
            ("x", 0),
            ("+", 0),
            ("first", 0),
            ("2: constant Int(3)", 0),
        ] {
            click(&mut app, &context, false, label, occurrence);
            assert_eq!(project_state(&app.project), original, "locked {label}");
            assert!(!app.is_dirty());
            assert!(!app.can_undo());
            assert!(!app.history.can_redo());
            assert!(app.pending_history.is_none());
            assert!(app.preview_draft.is_some());
        }
        let after = engine::run_outputs(&app.project, &source()).expect("unchanged mapping");
        assert_eq!(after.primary, before.primary);
        assert_eq!(after.extras, before.extras);
    }
}
