use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, NamedTarget, ScopeConstruction, ScopeIteration};

fn copy_app(document: MappingDocument) -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::group(
                        "Children",
                        vec![
                            SchemaNode::scalar("Value", ScalarType::String),
                            SchemaNode::group(
                                "Details",
                                vec![SchemaNode::scalar("Value", ScalarType::String)],
                            ),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
            SchemaNode::group(
                "Editable",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::group(
                        "Details",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    ),
                ],
            ),
        ],
    );
    app.project.target = app.project.source.clone();
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.graph.nodes.extend([
        (
            0,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (
            2,
            Node::SourceField {
                path: vec!["Rows".into(), "Name".into()],
                frame: None,
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String("edited".into()),
            },
        ),
    ]);
    app.project.root.children = vec![
        Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            construction: ScopeConstruction::CopyCurrentSource,
            sort_by: Some(2),
            ..Scope::default()
        },
        Scope {
            target_field: "Editable".into(),
            ..Scope::default()
        },
    ];
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

fn source() -> Instance {
    let row = |name: &str, children: &[&str]| {
        Instance::Group(vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            (
                "Children".into(),
                Instance::Repeated(
                    children
                        .iter()
                        .map(|value| {
                            Instance::Group(vec![(
                                "Value".into(),
                                Instance::Scalar(Value::String((*value).into())),
                            )])
                        })
                        .collect(),
                ),
            ),
        ])
    };
    Instance::Group(vec![
        (
            "Rows".into(),
            Instance::Repeated(vec![row("B", &["b1"]), row("A", &["a1", "a2"])]),
        ),
        (
            "Editable".into(),
            Instance::Group(vec![(
                "Value".into(),
                Instance::Scalar(Value::String("source".into())),
            )]),
        ),
    ])
}

fn project_state(project: &Project) -> String {
    mapping::project_file::encode_pretty(project).expect("project state")
}

fn active_root(app: &FerruleApp) -> &Scope {
    match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.project.extra_targets[index].root,
        _ => &app.project.root,
    }
}

fn active_output<'a>(app: &FerruleApp, outputs: &'a engine::ExecutionOutputs) -> &'a Instance {
    match app.mapping_workspace.active {
        MappingDocument::Target(index) => &outputs.extras[index].instance,
        _ => &outputs.primary,
    }
}

fn outputs(app: &FerruleApp) -> engine::ExecutionOutputs {
    let issues = cli::validate(&app.project);
    assert!(issues.is_empty(), "valid copy mapping: {issues:?}");
    engine::run_outputs(&app.project, &source()).expect("execute copy mapping")
}

fn assert_rows(app: &FerruleApp, expected: &[&str]) {
    let output = outputs(app);
    let rows = active_output(app, &output)
        .field("Rows")
        .and_then(Instance::as_repeated)
        .expect("Rows output");
    let names = rows
        .iter()
        .map(
            |row| match row.field("Name").and_then(Instance::as_scalar) {
                Some(Value::String(name)) => name.as_str(),
                other => panic!("Name output string, got {other:?}"),
            },
        )
        .collect::<Vec<_>>();
    assert_eq!(names, expected);
    let original = source();
    let original_rows = original
        .field("Rows")
        .and_then(Instance::as_repeated)
        .expect("source rows");
    for row in rows {
        let matching = original_rows
            .iter()
            .find(|candidate| candidate.field("Name") == row.field("Name"))
            .expect("original copied row");
        assert_eq!(row, matching, "complete nested copy survives controls");
    }
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
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 1600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_inspector(ui, true),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn rendered_labels(shape: &egui::epaint::Shape, label: &str, positions: &mut Vec<egui::Pos2>) {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
            positions.push(text.visual_bounding_rect().center())
        }
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                rendered_labels(shape, label, positions);
            }
        }
        _ => {}
    }
}

fn click(app: &mut FerruleApp, context: &egui::Context, label: &str, popup: bool) {
    let mut output = frame(app, context, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, Vec::new());
    }
    let mut positions = Vec::new();
    for shape in &output.shapes {
        rendered_labels(&shape.shape, label, &mut positions);
    }
    let position = if popup {
        positions.last()
    } else {
        positions.first()
    }
    .copied()
    .unwrap_or_else(|| panic!("rendered Inspector control {label:?}"));
    for pressed in [true, false] {
        let _ = frame(
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

fn select_node(app: &mut FerruleApp, context: &egui::Context, desired: &str) {
    click(app, context, "0: constant Int(1)", false);
    click(app, context, desired, true);
}

fn temporary_project_path() -> PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "ferrule-gui-copy-scope-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("temporary project directory");
    directory.join("project.json")
}

#[test]
fn primary_and_named_copy_inspectors_block_incompatible_pointer_edits() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = copy_app(document);
        let before = project_state(&app.project);
        let expected = outputs(&app);
        let context = context();
        let output = frame(&mut app, &context, Vec::new());
        let mut reasons = Vec::new();
        for shape in &output.shapes {
            rendered_labels(&shape.shape, "Whole source group copy", &mut reasons);
        }
        assert!(!reasons.is_empty(), "copy mode is explained");
        for label in ["+ binding", "Add child", "none"] {
            click(&mut app, &context, label, false);
            assert_eq!(project_state(&app.project), before, "copy control {label}");
            assert!(!app.is_dirty());
            assert!(!app.can_undo());
            assert!(app.pending_history.is_none());
        }
        let actual = outputs(&app);
        assert_eq!(actual.primary, expected.primary);
        assert_eq!(actual.extras, expected.extras);
        assert_rows(&app, &["A", "B"]);
    }
}

#[test]
fn copy_scope_child_helpers_reject_copied_ancestors_without_mutation() {
    let mut app = copy_app(MappingDocument::Main);
    let before = project_state(&app.project);
    assert!(matches!(
        available_static_child_scopes(&app.project.root, &app.project.target, &[0]),
        Err(crate::scope_editor::ScopeTreeError::WholeGroupCopyContent(
            _
        )),
    ));
    assert!(matches!(
        create_static_child_scope(&mut app.project.root, &app.project.target, &[0], "Children"),
        Err(crate::scope_editor::ScopeTreeError::WholeGroupCopyContent(
            _
        )),
    ));
    assert_eq!(project_state(&app.project), before);
    assert_rows(&app, &["A", "B"]);

    // Invalid saved descendants must not let direct child creation bypass the copy owner.
    app.project.root.children[0].children.push(Scope {
        target_field: "Children".into(),
        ..Scope::default()
    });
    let invalid = project_state(&app.project);
    assert!(matches!(
        create_static_child_scope(
            &mut app.project.root,
            &app.project.target,
            &[0, 0],
            "Details"
        ),
        Err(crate::scope_editor::ScopeTreeError::WholeGroupCopyContent(
            _
        )),
    ));
    assert!(matches!(
        expand_static_target_subtree(&mut app.project.root, &app.project.target, &[0, 0]),
        Err(crate::scope_editor::ScopeTreeError::WholeGroupCopyContent(
            _
        )),
    ));
    assert_eq!(project_state(&app.project), invalid);
    app.selected_scope = vec![0, 0];
    app.mark_clean();
    app.rebase_history();
    click(&mut app, &context(), "Expand subtree", false);
    assert_eq!(project_state(&app.project), invalid);
    assert!(!app.is_dirty());
    assert!(!app.can_undo());
    assert!(app.pending_history.is_none());

    // Scope selection uses indices, so a same-named constructed sibling must
    // not hide the selected copied scope in an invalid saved tree.
    let mut duplicate = copy_app(MappingDocument::Main);
    duplicate.project.root.children.insert(
        0,
        Scope {
            target_field: "Rows".into(),
            ..Scope::default()
        },
    );
    let duplicate_before = project_state(&duplicate.project);
    assert!(matches!(
        create_static_child_scope(
            &mut duplicate.project.root,
            &duplicate.project.target,
            &[1],
            "Children"
        ),
        Err(crate::scope_editor::ScopeTreeError::WholeGroupCopyContent(
            _
        )),
    ));
    assert_eq!(project_state(&duplicate.project), duplicate_before);
}

#[test]
fn copied_rows_retain_filter_sort_and_window_authoring_with_undo() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = copy_app(document);
        let original = project_state(&app.project);
        let context = context();
        assert_rows(&app, &["A", "B"]);
        click(&mut app, &context, "sorted", false);
        assert_rows(&app, &["B", "A"]);
        app.undo_project();
        assert_eq!(project_state(&app.project), original);
        app.selected_scope = vec![0];

        click(&mut app, &context, "+", false);
        assert_rows(&app, &["A"]);
        app.undo_project();
        assert_eq!(project_state(&app.project), original);
        app.selected_scope = vec![0];

        click(&mut app, &context, "filtered", false);
        select_node(&mut app, &context, "1: constant Bool(false)");
        assert_rows(&app, &[]);
        app.undo_project(); // node choice
        app.undo_project(); // enable filter
        assert_eq!(project_state(&app.project), original);
        assert!(!app.is_dirty());
        assert_rows(&app, &["A", "B"]);
    }
}

#[test]
fn constructed_sibling_accepts_bindings_and_children_and_keeps_copy_output() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = copy_app(document);
        let original = project_state(&app.project);
        let original_copy =
            serde_json::to_value(&active_root(&app).children[0]).expect("copy state");
        let context = context();
        app.selected_scope = vec![1];
        click(&mut app, &context, "+ binding", false);
        select_node(&mut app, &context, "3: constant String(\"edited\")");
        let output = outputs(&app);
        assert_eq!(
            active_output(&app, &output)
                .field("Editable")
                .and_then(|value| value.field("Value"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("edited".into()))
        );
        assert_rows(&app, &["A", "B"]);
        assert_eq!(
            serde_json::to_value(&active_root(&app).children[0]).expect("copy state"),
            original_copy
        );
        app.undo_project();
        app.undo_project();
        assert_eq!(project_state(&app.project), original);
        app.selected_scope = vec![1];

        click(&mut app, &context, "Add child", false);
        click(&mut app, &context, "Details", true);
        assert_eq!(
            active_root(&app).children[1].children[0].target_field,
            "Details"
        );
        let output = outputs(&app);
        assert_eq!(
            active_output(&app, &output)
                .field("Editable")
                .and_then(|value| value.field("Details")),
            Some(&Instance::Group(Vec::new()))
        );
        assert_rows(&app, &["A", "B"]);
        app.undo_project();
        assert_eq!(project_state(&app.project), original);
    }
}

#[test]
fn saved_copy_scope_reopens_with_protection_and_invalid_metadata_is_not_normalized()
-> anyhow::Result<()> {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = copy_app(document);
        let expected = outputs(&app);
        let path = temporary_project_path();
        let outcome = app.save_document_to(&path)?;
        assert!(outcome.validation_issues.is_empty());
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&path);
        if let MappingDocument::Target(index) = document {
            reopened.open_target_tab(index);
        }
        reopened.selected_scope = vec![0];
        let before = project_state(&reopened.project);
        let context = context();
        click(&mut reopened, &context, "+ binding", false);
        click(&mut reopened, &context, "Add child", false);
        assert_eq!(project_state(&reopened.project), before);
        assert!(!reopened.can_undo());
        let actual = outputs(&reopened);
        assert_eq!(actual.primary, expected.primary);
        assert_eq!(actual.extras, expected.extras);
        std::fs::remove_dir_all(path.parent().expect("temporary directory"))?;
    }

    let mut invalid = copy_app(MappingDocument::Main);
    let copied = &mut invalid.project.root.children[0];
    copied.bindings.push(Binding {
        target_field: "Name".into(),
        node: 3,
    });
    copied.children.push(Scope {
        target_field: "Children".into(),
        ..Scope::default()
    });
    copied.group_by = Some(2);
    invalid.mark_clean();
    invalid.rebase_history();
    let before = project_state(&invalid.project);
    let errors = cli::validate(&invalid.project)
        .into_iter()
        .map(|issue| issue.to_string())
        .collect::<Vec<_>>();
    assert!(!errors.is_empty());
    let context = context();
    for _ in 0..4 {
        let _ = frame(&mut invalid, &context, Vec::new());
    }
    assert_eq!(project_state(&invalid.project), before);
    assert!(!invalid.is_dirty());
    assert!(!invalid.can_undo());
    assert_eq!(
        cli::validate(&invalid.project)
            .into_iter()
            .map(|issue| issue.to_string())
            .collect::<Vec<_>>(),
        errors
    );
    Ok(())
}
