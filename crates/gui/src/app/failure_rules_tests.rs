use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, DynamicSourcePath, NamedSource, SequenceExpr};

fn fixture() -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Code", ScalarType::String),
                    SchemaNode::scalar("Valid", ScalarType::Bool),
                ],
            )
            .repeating(),
        ],
    );
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Result", ScalarType::String)],
    );
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    app.project.graph.nodes.clear();
    app.project.graph.nodes.extend([
        (
            0,
            Node::SourceField {
                path: vec!["Valid".into()],
                frame: None,
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["Code".into()],
                frame: None,
            },
        ),
        (
            2,
            Node::Call {
                function: "concat".into(),
                args: vec![3, 1],
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String("invalid:".into()),
            },
        ),
        (
            4,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            5,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            6,
            Node::Const {
                value: Value::Int(0),
            },
        ),
        (
            7,
            Node::Const {
                value: Value::String("success".into()),
            },
        ),
        (
            8,
            Node::Call {
                function: "divide".into(),
                args: vec![5, 6],
            },
        ),
        (
            9,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            11,
            Node::Const {
                value: Value::String(String::new()),
            },
        ),
        (
            12,
            Node::Const {
                value: Value::String("second".into()),
            },
        ),
    ]);
    app.project.root = Scope {
        bindings: vec![Binding {
            target_field: "Result".into(),
            node: 7,
        }],
        ..Scope::default()
    };
    app.project.failure_rules.clear();
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    assert!(cli::validate(&app.project).is_empty());
    app
}

fn row(code: &str, valid: bool) -> Instance {
    Instance::Group(
        vec![
            ("Code".into(), Instance::Scalar(Value::String(code.into()))),
            ("Valid".into(), Instance::Scalar(Value::Bool(valid))),
        ]
        .into(),
    )
}

fn source() -> Instance {
    Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(vec![row("A", true), row("B", false), row("C", false)]),
        )]
        .into(),
    )
}

fn encoded(app: &FerruleApp) -> String {
    mapping::project_file::encode_pretty(&app.project).expect("project encoding")
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
                egui::vec2(1200.0, 1400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_failure_rules(ui, true),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn label_position(shape: &egui::epaint::Shape, wanted: &str, positions: &mut Vec<egui::Pos2>) {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == wanted => {
            positions.push(text.visual_bounding_rect().center());
        }
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                label_position(shape, wanted, positions);
            }
        }
        _ => {}
    }
}

fn click_nth(app: &mut FerruleApp, context: &egui::Context, label: &str, occurrence: usize) {
    let mut output = frame(app, context, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, Vec::new());
    }
    let mut positions = Vec::new();
    for shape in &output.shapes {
        label_position(&shape.shape, label, &mut positions);
    }
    let position = *positions
        .get(occurrence)
        .unwrap_or_else(|| panic!("visible failure-rule control {label:?} #{occurrence}"));
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

fn click(app: &mut FerruleApp, context: &egui::Context, label: &str) {
    click_nth(app, context, label, 0);
}

fn scroll_expression_picker(app: &mut FerruleApp, context: &egui::Context) {
    let output = frame(app, context, Vec::new());
    let mut positions = Vec::new();
    for shape in &output.shapes {
        label_position(
            &shape.shape,
            "3: constant String(\"invalid:\")",
            &mut positions,
        );
    }
    let position = *positions
        .first()
        .expect("visible upper expression popup entry");
    let _ = frame(
        app,
        context,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -600.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    for _ in 0..7 {
        let _ = frame(app, context, Vec::new());
    }
}

fn author_rule(app: &mut FerruleApp, context: &egui::Context) {
    click(app, context, "Add source rule");
    click(app, context, "<current rows>");
    click(app, context, "Rows");
    click(app, context, "All items");
    click(app, context, "Expression is false");
    click(app, context, "Custom message expression");
    // Both selected predicate and the newly selected message initially show node0.
    click_nth(app, context, "0: field Valid", 1);
    click(app, context, "2: concat");
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule-failure-rule-editor-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("test directory");
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!(
                "retained failure-rule editor test artifacts: {}",
                self.0.display()
            );
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn source_failure_rule_real_controls_reject_first_row_and_unsaved_preview_never_writes() {
    let mut app = fixture();
    let original_graph = serde_json::to_value(&app.project.graph).unwrap();
    let original_wires = app.main_canvas.snarl.wires().collect::<Vec<_>>();
    let context = egui::Context::default();
    crate::icons::install(&context);
    author_rule(&mut app, &context);
    let actual = engine::run_outputs(&app.project, &source());
    eprintln!(
        "failure-rule authored original project={} outcome={actual:?}",
        encoded(&app)
    );
    assert_eq!(
        app.project.failure_rules,
        vec![FailureRule {
            iteration: FailureIteration::Source {
                collection: vec!["Rows".into()]
            },
            selection: FailureSelection::WhenFalse { predicate: 0 },
            message: Some(2),
        }]
    );
    assert_eq!(
        actual.err(),
        Some(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("invalid:B".into())
        })
    );
    assert_eq!(
        serde_json::to_value(&app.project.graph).unwrap(),
        original_graph
    );
    assert_eq!(
        app.main_canvas.snarl.wires().collect::<Vec<_>>(),
        original_wires
    );
    assert!(app.is_dirty());
    assert!(app.document.saved_path().is_none());
    assert!(cli::validate(&app.project).is_empty());
    let empty = Instance::Group(vec![("Rows".into(), Instance::Repeated(Vec::new()))].into());
    assert!(engine::run_outputs(&app.project, &empty).is_ok());
    click(&mut app, &context, "Expression is false");
    click(&mut app, &context, "Expression is true");
    let when_true = engine::run(&app.project, &source());
    eprintln!("failure-rule true selection original={when_true:?}");
    assert_eq!(
        when_true,
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("invalid:A".into())
        })
    );
    click(&mut app, &context, "Expression is true");
    click(&mut app, &context, "All items");
    let all = engine::run(&app.project, &source());
    eprintln!("failure-rule all selection original={all:?}");
    assert_eq!(
        all,
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("invalid:A".into())
        })
    );
    click(&mut app, &context, "All items");
    click(&mut app, &context, "Expression is false");

    let directory = Directory::new();
    let output = directory.path("must-not-be-written.json");
    app.preview_draft = Some(crate::preview::PreviewDraft {
        target: crate::preview::PreviewTarget::Primary,
        input_identity: "input.json".into(), output_identity: output.display().to_string(),
        input_text: r#"{"Rows":[{"Code":"A","Valid":true},{"Code":"B","Valid":false},{"Code":"C","Valid":false}]}"#.into(),
        debug_breakpoint: None,
    });
    app.execute_preview();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.pending_preview.is_some() && std::time::Instant::now() < deadline {
        app.poll_preview(&context);
        if app.pending_preview.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    eprintln!(
        "failure-rule preview original status={} diagnostics={:?}",
        app.status,
        app.diagnostics.items()
    );
    assert!(app.pending_preview.is_none());
    assert_eq!(app.status, "preview failed");
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("invalid:B"))
    );
    assert!(!output.exists());
    assert!(app.document.saved_path().is_none());
    assert!(app.is_dirty());
}

#[test]
fn source_failure_rule_reordering_changes_first_error_and_keeps_later_message_lazy() {
    let mut app = fixture();
    let context = egui::Context::default();
    crate::icons::install(&context);
    author_rule(&mut app, &context);
    author_rule(&mut app, &context);
    click(&mut app, &context, "2: concat");
    scroll_expression_picker(&mut app, &context);
    click(&mut app, &context, "12: constant String(\"second\")");
    app.project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: vec!["Rows".into()],
        },
        selection: FailureSelection::All,
        message: Some(8),
    });
    app.observe_editor_history(std::time::Instant::now(), false);
    assert!(cli::validate(&app.project).is_empty());
    let first = engine::run_outputs(&app.project, &source());
    eprintln!("failure-rule ordered original={first:?}");
    let first = first.expect_err("first source rule fails");
    assert_eq!(
        first,
        engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("invalid:B".into())
        }
    );
    click(&mut app, &context, "Move up");
    let reordered = engine::run_outputs(&app.project, &source());
    eprintln!("failure-rule reordered original={reordered:?}");
    let reordered = reordered.expect_err("reordered source rule fails");
    assert_eq!(
        reordered,
        engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("second".into())
        }
    );
    app.undo_project();
    assert_eq!(
        engine::run_outputs(&app.project, &source()).err(),
        Some(first)
    );
    app.redo_project();
    assert_eq!(
        engine::run_outputs(&app.project, &source()).err(),
        Some(reordered)
    );
}

#[test]
fn source_failure_rule_remove_undo_and_save_keep_graph_and_message_presence() {
    let mut app = fixture();
    let context = egui::Context::default();
    crate::icons::install(&context);
    author_rule(&mut app, &context);
    click(&mut app, &context, "2: concat");
    scroll_expression_picker(&mut app, &context);
    click(&mut app, &context, "11: constant String(\"\")");
    let with_empty_message = engine::run(&app.project, &source());
    assert_eq!(
        with_empty_message,
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some(String::new())
        })
    );
    let graph = serde_json::to_value(&app.project.graph).unwrap();
    click(&mut app, &context, "Custom message expression");
    let absent = engine::run(&app.project, &source());
    assert_eq!(
        absent,
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: None
        })
    );
    app.undo_project();
    assert_eq!(engine::run(&app.project, &source()), with_empty_message);
    app.redo_project();
    assert_eq!(engine::run(&app.project, &source()), absent);
    click(&mut app, &context, "Rule 1: Rows");
    let rules = app.project.failure_rules.clone();
    click(&mut app, &context, "Remove rule");
    assert!(app.project.failure_rules.is_empty());
    assert_eq!(app.selected_failure_rule, None);
    assert_eq!(serde_json::to_value(&app.project.graph).unwrap(), graph);
    assert!(engine::run(&app.project, &source()).is_ok());
    app.undo_project();
    assert_eq!(app.project.failure_rules, rules);
    let directory = Directory::new();
    let path = directory.path("mapping.json");
    app.save_document_to(&path).unwrap();
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    assert_eq!(reopened.project.failure_rules, rules);
    assert_eq!(
        serde_json::to_value(&reopened.project.graph).unwrap(),
        graph
    );
    assert_eq!(engine::run(&reopened.project, &source()), absent);
    assert!(!reopened.is_dirty());
}

#[test]
fn source_failure_rule_locked_widgets_preserve_project_and_history() {
    let mut app = fixture();
    let context = egui::Context::default();
    crate::icons::install(&context);
    author_rule(&mut app, &context);
    app.mark_clean();
    app.rebase_history();
    click(&mut app, &context, "Rule 1: Rows");
    app.begin_preview();
    assert!(app.preview_draft.is_some());
    let before = encoded(&app);
    let expected = engine::run(&app.project, &source());
    for label in [
        "Add source rule",
        "Move down",
        "Remove rule",
        "Custom message expression",
    ] {
        click(&mut app, &context, label);
        assert_eq!(encoded(&app), before, "locked control {label}");
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
        assert!(!app.history.can_redo());
        assert!(app.preview_draft.is_some());
    }
    app.apply_failure_rule_action(RuleAction::Remove(0), true);
    app.apply_failure_rule_action(RuleAction::Add, true);
    assert_eq!(
        encoded(&app),
        before,
        "central lock guards direct actions too"
    );
    assert_eq!(engine::run(&app.project, &source()), expected);
}

#[test]
fn source_failure_rule_static_named_and_flat_collections_use_real_path_choices() {
    let mut app = fixture();
    let columns = vec![
        SchemaNode::scalar("Code", ScalarType::String),
        SchemaNode::scalar("Valid", ScalarType::Bool),
    ];
    for (name, dynamic_path) in [
        ("Reference", None),
        (
            "Dynamic",
            Some(DynamicSourcePath {
                node: 3,
                iteration: vec!["Rows".into()],
            }),
        ),
    ] {
        app.project.extra_sources.push(NamedSource {
            name: name.into(),
            path: format!("{name}.json"),
            schema: SchemaNode::group(
                name,
                vec![SchemaNode::group("Entries", columns.clone()).repeating()],
            ),
            options: mapping::FormatOptions {
                json_document: true,
                ..Default::default()
            },
            dynamic_path,
        });
    }
    let context = egui::Context::default();
    crate::icons::install(&context);
    author_rule(&mut app, &context);
    click(&mut app, &context, "Rows");
    let output = frame(&mut app, &context, Vec::new());
    let mut dynamic = Vec::new();
    for shape in &output.shapes {
        label_position(&shape.shape, "Dynamic / Entries", &mut dynamic);
    }
    assert!(
        dynamic.is_empty(),
        "dynamic documents are not authoring choices"
    );
    click(&mut app, &context, "Reference / Entries");
    assert_eq!(
        app.project.failure_rules[0].iteration,
        FailureIteration::Source {
            collection: vec!["Reference".into(), "Entries".into()]
        }
    );
    let named = Instance::Group(
        vec![(
            "Entries".into(),
            Instance::Repeated(vec![row("named", false)]),
        )]
        .into(),
    );
    let actual =
        engine::run_with_sources(&app.project, &source(), vec![("Reference".into(), named)]);
    eprintln!("failure-rule static named original={actual:?}");
    assert_eq!(
        actual,
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("invalid:named".into())
        })
    );
    click(&mut app, &context, "Reference / Entries");
    click(&mut app, &context, "<current rows>");
    app.project.extra_sources.clear();
    app.project.source = SchemaNode::group("Row", columns);
    assert_eq!(
        app.project.failure_rules[0].iteration,
        FailureIteration::Source {
            collection: Vec::new()
        }
    );
    assert_eq!(
        engine::run(&app.project, &Instance::Repeated(vec![row("flat", false)])),
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("invalid:flat".into())
        })
    );
}

#[test]
fn source_failure_rule_sequence_owners_and_stale_navigation_survive_list_changes() {
    let mut app = fixture();
    let context = egui::Context::default();
    crate::icons::install(&context);
    author_rule(&mut app, &context);
    app.project.graph.nodes.insert(
        10,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    let sequence_rule = FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 4,
                item: 10,
            },
        },
        selection: FailureSelection::WhenFalse { predicate: 9 },
        message: Some(10),
    };
    app.project.failure_rules.push(sequence_rule.clone());
    let graph = serde_json::to_value(&app.project.graph).unwrap();
    let owners = crate::graph_viewer::project_sequence_item_ids(&app.project);
    app.observe_editor_history(std::time::Instant::now(), false);
    click(&mut app, &context, "Rule 2: generated sequence");
    click(&mut app, &context, "Remove rule");
    assert_eq!(
        app.project.failure_rules[1], sequence_rule,
        "sequence removal is disabled"
    );
    click(&mut app, &context, "Move up");
    assert_eq!(app.project.failure_rules[0], sequence_rule);
    assert_eq!(
        crate::graph_viewer::project_sequence_item_ids(&app.project),
        owners
    );
    click(&mut app, &context, "Rule 2: Rows");
    let original = engine::run(&app.project, &source());
    assert_eq!(
        original,
        Err(engine::EngineError::MappingFailure {
            rule: 2,
            message: Some("invalid:B".into())
        })
    );
    app.project.failure_rules[1].message = Some(999);
    let issue = cli::validate(&app.project)
        .into_iter()
        .find(|issue| issue.owner == Some(engine::ValidationOwner::FailureRule { index: 1 }))
        .expect("typed failure-rule owner");
    let diagnostic = Diagnostic::validation(&app.project, issue);
    assert!(app.navigate_to_diagnostic(&diagnostic));
    assert_eq!(app.selected_failure_rule, Some(1));
    click(&mut app, &context, "Remove rule");
    assert_eq!(app.project.failure_rules, vec![sequence_rule]);
    assert_eq!(app.selected_failure_rule, Some(0));
    assert!(
        !app.navigate_to_diagnostic(&diagnostic),
        "stale owner must not select the shifted rule"
    );
    assert_eq!(serde_json::to_value(&app.project.graph).unwrap(), graph);
    assert_eq!(
        crate::graph_viewer::project_sequence_item_ids(&app.project),
        owners
    );
}
