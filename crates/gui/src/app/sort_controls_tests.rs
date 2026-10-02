use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, NamedTarget, Scope, ScopeIteration, SortFilterOrder, SortKey,
};

fn sort_app(document: MappingDocument, ties: bool, filtered: bool) -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Score", ScalarType::Int),
                    SchemaNode::scalar("Last", ScalarType::String),
                    SchemaNode::scalar("First", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    app.project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("First", ScalarType::String),
                    SchemaNode::scalar("Position", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    );
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    for (id, field) in [(0, "Score"), (1, "Last"), (2, "First")] {
        app.project.graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec!["Rows".into(), field.into()],
                frame: None,
            },
        );
    }
    app.project.graph.nodes.insert(
        3,
        Node::Position {
            collection: vec!["Rows".into()],
        },
    );
    app.project.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::Int(2),
        },
    );
    app.project.graph.nodes.insert(
        5,
        Node::Call {
            function: "less_or_equal".into(),
            args: vec![3, 4],
        },
    );
    app.project.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            sort_by: Some(0),
            sort_descending: true,
            sort_then_by: if ties {
                vec![
                    SortKey {
                        node: 1,
                        descending: false,
                    },
                    SortKey {
                        node: 2,
                        descending: true,
                    },
                ]
            } else {
                Vec::new()
            },
            filter: filtered.then_some(5),
            bindings: vec![
                Binding {
                    target_field: "First".into(),
                    node: 2,
                },
                Binding {
                    target_field: "Position".into(),
                    node: 3,
                },
            ],
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

fn project_state(project: &Project) -> String {
    mapping::project_file::encode_pretty(project).expect("project state")
}

fn scope_state(scope: &Scope) -> serde_json::Value {
    serde_json::to_value(scope).expect("scope state")
}

fn temporary_project_path() -> PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "ferrule-gui-sort-controls-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("temporary test directory");
    directory.join("project.json")
}

fn source(rows: &[(&str, &str, i64)]) -> Instance {
    Instance::Group(
        (vec![(
            "Rows".into(),
            Instance::Repeated(
                rows.iter()
                    .map(|(first, last, score)| {
                        Instance::Group(
                            (vec![
                                (
                                    "First".into(),
                                    Instance::Scalar(Value::String((*first).into())),
                                ),
                                (
                                    "Last".into(),
                                    Instance::Scalar(Value::String((*last).into())),
                                ),
                                ("Score".into(), Instance::Scalar(Value::Int(*score))),
                            ])
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )])
        .into(),
    )
}

fn tied_source() -> Instance {
    source(&[
        ("Susan", "Schmitt", 2),
        ("Alex", "Martin", 2),
        ("Fred", "Landis", 2),
        ("Joe", "Martin", 2),
        ("Lower", "Able", 1),
    ])
}

fn active_scope(app: &FerruleApp) -> &Scope {
    let root = match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.project.extra_targets[index].root,
        _ => &app.project.root,
    };
    &root.children[0]
}

fn active_output<'a>(app: &FerruleApp, outputs: &'a engine::ExecutionOutputs) -> &'a Instance {
    match app.mapping_workspace.active {
        MappingDocument::Target(index) => &outputs.extras[index].instance,
        _ => &outputs.primary,
    }
}

fn names(instance: &Instance) -> Vec<&str> {
    instance
        .field("Rows")
        .and_then(Instance::as_repeated)
        .expect("repeated Rows output")
        .iter()
        .map(
            |row| match row.field("First").and_then(Instance::as_scalar) {
                Some(Value::String(name)) => name.as_str(),
                other => panic!("First output string, got {other:?}"),
            },
        )
        .collect()
}

fn assert_names(app: &FerruleApp, source: &Instance, expected: &[&str]) {
    assert!(
        cli::validate(&app.project).is_empty(),
        "valid authored project"
    );
    let outputs = engine::run_outputs(&app.project, source).expect("execute authored mapping");
    assert_eq!(names(active_output(app, &outputs)), expected);
}

fn context() -> egui::Context {
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
                egui::vec2(1100.0, 1500.0),
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
    // A dropdown's item is painted after its selected text in the underlying editor.
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

fn pick_node(
    app: &mut FerruleApp,
    context: &egui::Context,
    selected: &str,
    occurrence: usize,
    desired: &str,
) {
    click(app, context, true, selected, occurrence);
    click(app, context, true, desired, usize::MAX);
}

#[test]
fn inspector_authors_orders_and_removes_secondary_sort_keys() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = sort_app(document, false, false);
        let primary_before = scope_state(&app.project.root);
        let source = tied_source();
        let context = context();
        assert_names(&app, &source, &["Susan", "Alex", "Fred", "Joe", "Lower"]);
        click(&mut app, &context, true, "Add sort key", 0);
        pick_node(
            &mut app,
            &context,
            "0: field Rows/Score",
            1,
            "1: field Rows/Last",
        );
        click(&mut app, &context, true, "Add sort key", 0);
        pick_node(
            &mut app,
            &context,
            "0: field Rows/Score",
            1,
            "2: field Rows/First",
        );
        click(&mut app, &context, true, "descending", 2);
        assert_eq!(
            active_scope(&app).sort_then_by,
            [
                SortKey {
                    node: 1,
                    descending: false
                },
                SortKey {
                    node: 2,
                    descending: true
                }
            ],
        );
        assert_names(&app, &source, &["Fred", "Joe", "Alex", "Susan", "Lower"]);
        click(&mut app, &context, true, "Up", 1);
        assert_names(&app, &source, &["Susan", "Joe", "Fred", "Alex", "Lower"]);
        click(&mut app, &context, true, "Down", 0);
        assert_names(&app, &source, &["Fred", "Joe", "Alex", "Susan", "Lower"]);
        click(&mut app, &context, true, "descending", 2);
        assert_names(&app, &source, &["Fred", "Alex", "Joe", "Susan", "Lower"]);
        click(&mut app, &context, true, "Remove key", 0);
        assert_names(&app, &source, &["Alex", "Fred", "Joe", "Susan", "Lower"]);
        assert_eq!(
            active_scope(&app).sort_then_by,
            [SortKey {
                node: 2,
                descending: false
            }]
        );
        if matches!(document, MappingDocument::Target(_)) {
            assert_eq!(
                scope_state(&app.project.root),
                primary_before,
                "named editor preserves primary scope"
            );
            let outputs = engine::run_outputs(&app.project, &source).expect("all outputs");
            assert_eq!(
                names(&outputs.primary),
                ["Susan", "Alex", "Fred", "Joe", "Lower"]
            );
        }
        let saved = mapping::project_file::encode_pretty(&app.project).expect("save mapping");
        let reopened = mapping::project_file::decode_str(&saved).expect("reopen mapping");
        assert_eq!(project_state(&reopened), project_state(&app.project));
        let output = engine::run_outputs(&reopened, &source).expect("saved mapping executes");
        assert_eq!(
            names(active_output(&app, &output)),
            ["Alex", "Fred", "Joe", "Susan", "Lower"]
        );
    }
}

#[test]
fn disabling_imported_multi_key_sort_is_valid_and_undo_restores_ties() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = sort_app(document, true, false);
        let original = project_state(&app.project);
        let source = tied_source();
        let context = context();
        for _ in 0..4 {
            let _ = frame(&mut app, &context, true, Vec::new());
        }
        assert_eq!(
            project_state(&app.project),
            original,
            "display does not normalize imported metadata"
        );
        assert!(!app.can_undo());
        assert_names(&app, &source, &["Fred", "Joe", "Alex", "Susan", "Lower"]);
        click(&mut app, &context, true, "sorted", 0);
        assert_eq!(active_scope(&app).sort_by, None);
        assert!(active_scope(&app).sort_then_by.is_empty());
        assert_names(&app, &source, &["Susan", "Alex", "Fred", "Joe", "Lower"]);
        assert!(app.is_dirty());
        assert!(app.can_undo());
        app.undo_project();
        assert_eq!(project_state(&app.project), original);
        assert!(!app.is_dirty());
        assert_names(&app, &source, &["Fred", "Joe", "Alex", "Susan", "Lower"]);
        app.redo_project();
        assert_eq!(active_scope(&app).sort_by, None);
        assert!(active_scope(&app).sort_then_by.is_empty());
        assert_names(&app, &source, &["Susan", "Alex", "Fred", "Joe", "Lower"]);
    }
}

#[test]
fn inspector_filter_order_changes_position_selection_and_survives_saved_reopen()
-> anyhow::Result<()> {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = sort_app(document, true, true);
        let original = project_state(&app.project);
        let original_primary = scope_state(&app.project.root);
        let source = source(&[("low", "A", 1), ("high", "B", 5), ("middle", "C", 3)]);
        let context = context();
        assert_names(&app, &source, &["high", "middle"]);
        click(&mut app, &context, true, "Filter before sorting", 0);
        assert_eq!(
            active_scope(&app).sort_filter_order,
            SortFilterOrder::FilterThenSort
        );
        assert_names(&app, &source, &["high", "low"]);
        app.undo_project();
        assert_eq!(project_state(&app.project), original);
        assert_names(&app, &source, &["high", "middle"]);
        app.redo_project();
        assert_names(&app, &source, &["high", "low"]);

        let path = temporary_project_path();
        let outcome = app.save_document_to(&path)?;
        assert!(outcome.validation_issues.is_empty());
        assert!(outcome.layout_warning.is_none());
        assert!(!app.is_dirty());
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&path);
        if let MappingDocument::Target(index) = document {
            reopened.open_target_tab(index);
        }
        assert_eq!(
            active_scope(&reopened).sort_filter_order,
            SortFilterOrder::FilterThenSort
        );
        assert_eq!(
            active_scope(&reopened).sort_then_by,
            active_scope(&app).sort_then_by
        );
        assert_names(&reopened, &source, &["high", "low"]);
        if matches!(document, MappingDocument::Target(_)) {
            assert_eq!(scope_state(&reopened.project.root), original_primary);
            let outputs = engine::run_outputs(&reopened.project, &source)?;
            assert_eq!(names(&outputs.primary), ["high", "middle"]);
        }
        std::fs::remove_dir_all(path.parent().expect("temporary project directory"))?;
    }
    Ok(())
}

#[test]
fn locked_inspector_sort_controls_preserve_both_outputs_and_history() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = sort_app(document, true, true);
        app.begin_preview();
        assert!(app.preview_draft.is_some());
        let original = project_state(&app.project);
        let source = tied_source();
        let before = engine::run_outputs(&app.project, &source).expect("valid mapping");
        let context = context();
        for (label, occurrence) in [
            ("sorted", 0),
            ("descending", 1),
            ("1: field Rows/Last", 0),
            ("Add sort key", 0),
            ("Remove key", 0),
            ("Up", 1),
            ("Down", 0),
            ("Filter before sorting", 0),
        ] {
            click(&mut app, &context, false, label, occurrence);
            assert_eq!(
                project_state(&app.project),
                original,
                "locked control {label}"
            );
            assert!(!app.is_dirty());
            assert!(!app.can_undo());
            assert!(!app.history.can_redo());
            assert!(app.pending_history.is_none());
            assert!(app.preview_draft.is_some());
        }
        let after = engine::run_outputs(&app.project, &source).expect("unchanged valid mapping");
        assert_eq!(after.primary, before.primary);
        assert_eq!(after.extras, before.extras);
    }
}

#[test]
fn secondary_only_sort_metadata_is_explicitly_repairable_without_render_mutation() {
    let mut app = sort_app(MappingDocument::Main, true, false);
    app.project.root.children[0].sort_by = None;
    app.mark_clean();
    app.rebase_history();
    let original = project_state(&app.project);
    assert!(cli::validate(&app.project).iter().any(|issue| {
        issue
            .to_string()
            .contains("secondary sort keys require a primary sort key")
    }));
    let context = context();
    for _ in 0..4 {
        let _ = frame(&mut app, &context, true, Vec::new());
    }
    assert_eq!(project_state(&app.project), original);
    assert!(!app.can_undo());
    click(&mut app, &context, true, "Set primary key", 0);
    assert_eq!(active_scope(&app).sort_by, Some(0));
    assert_names(
        &app,
        &tied_source(),
        &["Fred", "Joe", "Alex", "Susan", "Lower"],
    );
    app.undo_project();
    assert_eq!(project_state(&app.project), original);
    app.selected_scope = vec![0];
    click(&mut app, &context, true, "sorted", 0);
    assert_eq!(active_scope(&app).sort_by, None);
    assert!(active_scope(&app).sort_then_by.is_empty());
    assert_names(
        &app,
        &tied_source(),
        &["Susan", "Alex", "Fred", "Joe", "Lower"],
    );
}
