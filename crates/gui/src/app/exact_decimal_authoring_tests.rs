//! Exact decimal drafts and real constant controls, without a live desktop.
use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use ir::{Decimal96, ScalarType, SchemaNode, Value};
use mapping::{Binding, NamedTarget};

const EXACT: &str = "9007199254740993.1";
const FUNCTION: FunctionId = FunctionId::new(7);
const DOCUMENTS: [MappingDocument; 3] = [
    MappingDocument::Main,
    MappingDocument::Target(0),
    MappingDocument::Function(FUNCTION),
];

fn decimal(text: &str) -> Value {
    Value::decimal(Decimal96::parse_exact(text).unwrap())
}

fn replace<S>(harness: &mut Harness<'_, S>, label: &str, text: &str) {
    harness.get_by_label(label).click();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    harness.key_press(egui::Key::Backspace);
    harness.run();
    harness.event(egui::Event::Text(text.into()));
    harness.run();
}

fn click<S>(harness: &mut Harness<'_, S>, label: &str) {
    harness.get_by_label(label).click();
    harness.run();
}

fn draft<S>(harness: &Harness<'_, S>) -> String {
    harness
        .get_by_label("Exact decimal value")
        .accesskit_node()
        .value()
        .unwrap()
}

fn value_harness(value: Value) -> Harness<'static, Value> {
    Harness::builder()
        .with_size((650.0, 260.0))
        // The unchanged legacy integer/float DragValue has no accessible name.
        .with_accessibility_check(false)
        .with_os(egui::os::OperatingSystem::Nix)
        .build_ui_state(crate::value_editor::show_value_editor, value)
}

#[test]
fn exact_decimal_drafts_apply_only_on_request_and_canonicalize_without_float() {
    let mut harness = value_harness(decimal("5"));
    for (text, expected) in [
        (EXACT, EXACT),
        ("+00012.34000", "12.34"),
        ("-.1250", "-0.125"),
        ("-0.000", "0"),
        (
            "0.0000000000000000000000000001",
            "0.0000000000000000000000000001",
        ),
        (
            "79228162514264337593543950335",
            "79228162514264337593543950335",
        ),
    ] {
        let original = harness.state().clone();
        replace(&mut harness, "Exact decimal value", text);
        harness.run_steps(5);
        assert_eq!(harness.state(), &original, "uncommitted {text}");
        assert_eq!(draft(&harness), text);
        click(&mut harness, "Apply");
        assert_eq!(harness.state(), &decimal(expected));
        assert_eq!(draft(&harness), expected);
    }
    replace(&mut harness, "Exact decimal value", "-9007199254740993.1");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state(), &decimal("-9007199254740993.1"));
}

#[test]
fn exact_decimal_invalid_overflow_precision_and_lexical_drafts_never_change_value() {
    let mut harness = value_harness(decimal(EXACT));
    for text in [
        "",
        "-",
        "+",
        ".",
        "-.",
        "1..2",
        "1e2",
        " 1",
        "1 ",
        "NaN",
        "−1",
        "79228162514264337593543950336",
        "0.00000000000000000000000000001",
    ] {
        replace(&mut harness, "Exact decimal value", text);
        click(&mut harness, "Apply");
        assert_eq!(harness.state(), &decimal(EXACT), "rejected {text}");
        assert_eq!(draft(&harness), text);
        harness.get_by_label("Exact decimal value").click();
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(harness.state(), &decimal(EXACT));
    }
    // Valid leading zeroes are accepted at the byte limit, not beyond it.
    let at_limit = format!("{}1", "0".repeat(ir::MAX_DECIMAL_LEXICAL_BYTES - 1));
    replace(&mut harness, "Exact decimal value", &at_limit);
    click(&mut harness, "Apply");
    assert_eq!(harness.state(), &decimal("1"));
    let oversized = "0".repeat(ir::MAX_DECIMAL_LEXICAL_BYTES * 2);
    replace(&mut harness, "Exact decimal value", &oversized);
    assert_eq!(draft(&harness).len(), ir::MAX_DECIMAL_LEXICAL_BYTES + 1);
    click(&mut harness, "Apply");
    assert_eq!(harness.state(), &decimal("1"));
    harness.get_by_label("decimal lexical exceeds the byte limit");
}

#[test]
fn exact_decimal_type_is_distinct_and_external_changes_reset_its_draft() {
    let mut harness = value_harness(Value::Int(42));
    click(&mut harness, "Constant type");
    click(&mut harness, "Exact decimal");
    assert_eq!(harness.state(), &decimal("0"));
    replace(&mut harness, "Exact decimal value", "unfinished");
    *harness.state_mut() = decimal("23.5");
    harness.run();
    assert_eq!(draft(&harness), "23.5");
    click(&mut harness, "Constant type");
    click(&mut harness, "Exact decimal");
    assert_eq!(harness.state(), &decimal("23.5"));
    click(&mut harness, "Constant type");
    click(&mut harness, "Decimal");
    assert_eq!(harness.state(), &Value::Float(0.0));
    click(&mut harness, "Constant type");
    click(&mut harness, "Exact decimal");
    assert_eq!(draft(&harness), "0");
    click(&mut harness, "Constant type");
    click(&mut harness, "Text");
    assert_eq!(harness.state(), &Value::String(String::new()));
}

struct Editor {
    app: FerruleApp,
    editing: bool,
}

fn graph() -> Graph {
    Graph {
        nodes: [
            (
                0,
                Node::Const {
                    value: Value::Int(0),
                },
            ),
            (
                1,
                Node::Call {
                    function: "string".into(),
                    args: vec![0],
                },
            ),
        ]
        .into_iter()
        .collect(),
    }
}

fn fixture() -> FerruleApp {
    let mut project = blank_project();
    project.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Result", ScalarType::String),
            SchemaNode::scalar("Function", ScalarType::String),
        ],
    );
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project.source_path = Some("input.json".into());
    project.target_path = Some("output.json".into());
    project.graph = graph();
    project.graph.nodes.insert(
        2,
        Node::UserFunctionCall {
            function: FUNCTION,
            args: vec![],
        },
    );
    project.root.bindings = vec![
        Binding {
            target_field: "Result".into(),
            node: 1,
        },
        Binding {
            target_field: "Function".into(),
            node: 2,
        },
    ];
    project.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: Some("audit.json".into()),
        schema: project.target.clone(),
        options: project.target_options.clone(),
        root: project.root.clone(),
    });
    project.user_functions.insert(
        FUNCTION,
        UserFunction {
            library: "local".into(),
            name: "exact_text".into(),
            description: None,
            parameters: vec![],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: graph(),
            output: 1,
        },
    );
    let mut app = FerruleApp {
        project,
        ..Default::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    app.open_function_tab(FUNCTION);
    assert!(app.ensure_function_canvas(FUNCTION));
    for document in DOCUMENTS {
        let state = canvas_mut(&mut app, document);
        let nodes = state
            .snarl
            .node_ids()
            .map(|(id, node)| (id, *node))
            .collect::<Vec<_>>();
        for (id, node) in nodes {
            state.snarl.get_node_info_mut(id).unwrap().pos = match node {
                CanvasNode::Graph(0) => egui::pos2(230.0, 40.0),
                CanvasNode::Graph(1) => egui::pos2(560.0, 40.0),
                CanvasNode::Graph(2) => egui::pos2(560.0, 330.0),
                CanvasNode::TargetBlock(_) => egui::pos2(890.0, 40.0),
                _ => egui::pos2(0.0, 330.0),
            };
        }
        state.reset_view();
    }
    app.mapping_workspace.active = MappingDocument::Main;
    app.mark_clean();
    app.rebase_history();
    app
}

fn canvas(app: &FerruleApp, document: MappingDocument) -> &CanvasDocumentState {
    match document {
        MappingDocument::Main => &app.main_canvas,
        MappingDocument::Target(i) => &app.mapping_workspace.target_canvases[&i],
        MappingDocument::Function(id) => &app.mapping_workspace.function_canvases[&id],
    }
}

fn canvas_mut(app: &mut FerruleApp, document: MappingDocument) -> &mut CanvasDocumentState {
    match document {
        MappingDocument::Main => &mut app.main_canvas,
        MappingDocument::Target(i) => app.mapping_workspace.target_canvases.get_mut(&i).unwrap(),
        MappingDocument::Function(id) => app
            .mapping_workspace
            .function_canvases
            .get_mut(&id)
            .unwrap(),
    }
}

fn constant(app: &FerruleApp, document: MappingDocument) -> Value {
    let graph = match document {
        MappingDocument::Main | MappingDocument::Target(_) => &app.project.graph,
        MappingDocument::Function(id) => &app.project.user_functions[&id].body,
    };
    let Node::Const { value } = &graph.nodes[&0] else {
        panic!("constant")
    };
    value.clone()
}

fn layout(app: &FerruleApp) -> Vec<String> {
    DOCUMENTS
        .into_iter()
        .map(|document| {
            let snarl = &canvas(app, document).snarl;
            let mut wires = snarl
                .wires()
                .map(|(from, to)| (snarl[from.node], from.output, snarl[to.node], to.input))
                .collect::<Vec<_>>();
            wires.sort_unstable();
            format!(
                "{document:?}: {:?}; {wires:?}",
                CanvasLayout::capture_nodes(snarl)
            )
        })
        .collect()
}

fn editor(app: FerruleApp) -> Harness<'static, Editor> {
    let mut initialized = false;
    let mut harness = Harness::builder()
        .with_size((1450.0, 1000.0))
        .with_os(egui::os::OperatingSystem::Nix)
        .with_accessibility_check(false)
        .with_max_steps(30)
        .build_ui_state(
            move |ui, host: &mut Editor| {
                if !initialized {
                    crate::fonts::install(ui.ctx());
                    crate::icons::install(ui.ctx());
                    ui.ctx().all_styles_mut(|style| {
                        style.visuals.text_cursor.blink = false;
                        style.scroll_animation = egui::style::ScrollAnimation::none();
                        style.animation_time = 0.0;
                    });
                    initialized = true;
                }
                host.app.handle_history_shortcuts(ui.ctx(), host.editing);
                host.app.show_mapping_workspace_canvas(ui, host.editing);
                host.app.show_preview_setup(ui.ctx());
                host.app
                    .observe_editor_history(std::time::Instant::now(), false);
            },
            Editor { app, editing: true },
        );
    for document in DOCUMENTS {
        harness.state_mut().app.mapping_workspace.active = document;
        harness.run();
    }
    harness
}

fn open_constant(harness: &mut Harness<'_, Editor>) {
    harness.run();
    let app = &harness.state().app;
    let document = app.mapping_workspace.active;
    let state = canvas(app, document);
    let salt = match document {
        MappingDocument::Main => egui::Id::unique("main_mapping_canvas"),
        MappingDocument::Target(i) => egui::Id::unique(("named_target_canvas", i)),
        MappingDocument::Function(id) => egui::Id::unique(("function_mapping_canvas", id.get())),
    };
    let (_, rects) = crate::canvas_keyboard::current_fit_rects(
        &harness.ctx,
        salt,
        state.view_generation,
        &state.snarl,
    )
    .unwrap();
    let id = state
        .snarl
        .node_ids()
        .find_map(|(id, node)| (*node == CanvasNode::Graph(0)).then_some(id))
        .unwrap();
    let rect = rects.iter().find(|(node, _)| *node == id).unwrap().1;
    harness.event(egui::Event::PointerMoved(egui::pos2(
        rect.center().x,
        rect.top() + 12.0,
    )));
    harness.run();
    harness.get_by_label_contains("Edit Const:").click();
    harness.run();
    harness.get_by_label("Constant type");
}

fn escape(harness: &mut Harness<'_, Editor>) {
    harness.key_press(egui::Key::Escape);
    harness.run();
}

fn capture(harness: &mut Harness<'_, Editor>, name: &str) {
    if let Some(directory) = std::env::var_os("FERRULE_DECIMAL_SCREENSHOTS") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        harness
            .render()
            .unwrap()
            .save(directory.join(format!("{name}.png")))
            .unwrap();
    }
}

#[test]
fn exact_decimal_real_controls_history_locks_and_drafts_on_all_canvases() {
    for document in DOCUMENTS {
        let mut harness = editor(fixture());
        harness.state_mut().app.mapping_workspace.active = document;
        harness.run();
        let initial_layout = layout(&harness.state().app);
        let initial = crate::project_state::project_snapshot_key(&harness.state().app.project);
        open_constant(&mut harness);
        click(&mut harness, "Constant type");
        click(&mut harness, "Exact decimal");
        assert_eq!(constant(&harness.state().app, document), decimal("0"));
        let before_draft = harness.state().app.history.undo_len();
        replace(&mut harness, "Exact decimal value", EXACT);
        harness.run_steps(4);
        assert_eq!(constant(&harness.state().app, document), decimal("0"));
        assert_eq!(harness.state().app.history.undo_len(), before_draft);
        assert_eq!(draft(&harness), EXACT);
        click(&mut harness, "Apply");
        assert_eq!(constant(&harness.state().app, document), decimal(EXACT));
        assert!(matches!(
            harness.state().app.history.next_undo(),
            Some(HistoryRecord::Command(_))
        ));
        assert_eq!(
            harness.state().app.history.undo_label(),
            Some("Edit constant")
        );
        let Some(HistoryRecord::Command(record)) = harness.state().app.history.next_undo() else {
            panic!("typed constant command")
        };
        assert!(record.commands().iter().all(|command| matches!(
            command.action(),
            editor_ui::GraphAction::EditNode {
                node: 0,
                property: editor_ui::NodeProperty::Constant
            }
        )));
        let applied_history = harness.state().app.history.undo_len();
        capture(&mut harness, &format!("{document:?}-applied"));
        replace(&mut harness, "Exact decimal value", "-");
        click(&mut harness, "Apply");
        assert_eq!(constant(&harness.state().app, document), decimal(EXACT));
        assert_eq!(harness.state().app.history.undo_len(), applied_history);
        escape(&mut harness);
        open_constant(&mut harness);
        assert_eq!(
            draft(&harness),
            "-",
            "draft survives popup close and frames"
        );
        replace(&mut harness, "Exact decimal value", "23.5");
        harness.state_mut().editing = false;
        harness.run();
        assert!(
            harness
                .get_by_label("Exact decimal value")
                .accesskit_node()
                .is_disabled()
        );
        assert!(harness.get_by_label("Apply").accesskit_node().is_disabled());
        click(&mut harness, "Apply");
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(constant(&harness.state().app, document), decimal(EXACT));
        harness.state_mut().editing = true;
        harness.run();
        replace(&mut harness, "Exact decimal value", "-0.2500");
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(constant(&harness.state().app, document), decimal("-0.25"));
        escape(&mut harness);
        let after = crate::project_state::project_snapshot_key(&harness.state().app.project);
        for _ in 0..4 {
            if !harness.state().app.can_undo() {
                break;
            }
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
            harness.run();
        }
        assert!(!harness.state().app.can_undo());
        assert_eq!(
            crate::project_state::project_snapshot_key(&harness.state().app.project),
            initial
        );
        for _ in 0..4 {
            if !harness.state().app.history.can_redo() {
                break;
            }
            harness.key_press_modifiers(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            );
            harness.run();
        }
        assert!(!harness.state().app.history.can_redo());
        assert_eq!(
            crate::project_state::project_snapshot_key(&harness.state().app.project),
            after
        );
        assert_eq!(layout(&harness.state().app), initial_layout);
    }
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-exact-decimal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn exact_decimal_drafts_do_not_cross_same_app_document_loads() {
    let directory = Directory::new();
    let a = directory.0.join("a.json");
    let b = directory.0.join("b.json");
    let mut saved = fixture();
    for graph in [
        &mut saved.project.graph,
        &mut saved
            .project
            .user_functions
            .get_mut(&FUNCTION)
            .unwrap()
            .body,
    ] {
        graph.nodes.insert(
            0,
            Node::Const {
                value: decimal("7.25"),
            },
        );
    }
    saved.document = DocumentLocation::untitled(a.clone());
    saved.save_document_to(&a).unwrap();
    std::fs::copy(&a, &b).unwrap();
    std::fs::copy(
        crate::layout_store::layout_path(&a),
        crate::layout_store::layout_path(&b),
    )
    .unwrap();
    let mut app = FerruleApp::default();
    app.load_project_from(&a);
    // Keep this exact application and egui Context for every replacement.
    let mut harness = editor(app);
    let unrelated = egui::Id::unique("unrelated-document-load-state");
    harness
        .ctx
        .data_mut(|data| data.insert_temp(unrelated, "retain me".to_owned()));
    let error = ir::DecimalError::InvalidLexical.to_string();
    for document in DOCUMENTS {
        for (text, apply_invalid, destination) in [
            ("-", true, &b),
            (EXACT, false, &b),
            // Reloading the same file is also a new document instance.
            (EXACT, false, &a),
        ] {
            harness.state_mut().app.load_project_from(&a);
            harness.state_mut().app.mapping_workspace.active = document;
            harness.run();
            if harness.query_by_label("Exact decimal value").is_none() {
                open_constant(&mut harness);
            }
            replace(&mut harness, "Exact decimal value", text);
            if apply_invalid {
                click(&mut harness, "Apply");
                harness.get_by_label(&error);
            }
            assert_eq!(draft(&harness), text);
            assert_eq!(constant(&harness.state().app, document), decimal("7.25"));
            let previous_owner = canvas(&harness.state().app, document).tools_id;
            harness.state_mut().app.load_project_from(destination);
            assert_eq!(
                harness.state().app.document.saved_path(),
                Some(destination.as_path())
            );
            harness.state_mut().app.mapping_workspace.active = document;
            harness.run();
            if harness.query_by_label("Exact decimal value").is_none() {
                open_constant(&mut harness);
            }
            assert_ne!(
                canvas(&harness.state().app, document).tools_id,
                previous_owner
            );
            assert_eq!(
                draft(&harness),
                "7.25",
                "stale draft on {document:?}: {text}"
            );
            assert!(
                harness.query_by_label(&error).is_none(),
                "stale error on {document:?}"
            );
            let expected = crate::project_state::project_snapshot_key(&harness.state().app.project);
            click(&mut harness, "Apply");
            assert_eq!(
                crate::project_state::project_snapshot_key(&harness.state().app.project),
                expected
            );
            assert!(!harness.state().app.can_undo());
            assert!(!harness.state().app.is_dirty());
            assert_eq!(
                harness
                    .ctx
                    .data(|data| data.get_temp::<String>(unrelated))
                    .as_deref(),
                Some("retain me")
            );
            escape(&mut harness);
        }
    }
}

#[test]
fn exact_decimal_save_reopen_and_preview_keep_all_digits_wires_and_layout() {
    let directory = Directory::new();
    let path = directory.0.join("project.json");
    let mut app = fixture();
    app.document = DocumentLocation::untitled(path.clone());
    let mut harness = editor(app);
    for document in [MappingDocument::Main, MappingDocument::Function(FUNCTION)] {
        harness.state_mut().app.mapping_workspace.active = document;
        harness.run();
        open_constant(&mut harness);
        click(&mut harness, "Constant type");
        click(&mut harness, "Exact decimal");
        replace(&mut harness, "Exact decimal value", EXACT);
        click(&mut harness, "Apply");
        escape(&mut harness);
    }
    let expected_project = crate::project_state::project_snapshot_key(&harness.state().app.project);
    let expected_layout = layout(&harness.state().app);
    let saved = harness.state_mut().app.save_document_to(&path).unwrap();
    assert!(saved.validation_issues.is_empty());
    assert!(saved.layout_warning.is_none());
    let bytes = std::fs::read(&path).unwrap();
    let sidecar = crate::layout_store::layout_path(&path);
    let layout_bytes = std::fs::read(&sidecar).unwrap();
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    assert_eq!(
        crate::project_state::project_snapshot_key(&reopened.project),
        expected_project
    );
    assert_eq!(layout(&reopened), expected_layout);
    assert!(!reopened.is_dirty());
    let mut harness = editor(reopened);
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        harness.state_mut().app.mapping_workspace.active = document;
        harness.state_mut().app.begin_preview();
        harness.run();
        // The actual Preview dialog invokes the normal in-memory worker.
        harness
            .get_by_role_and_label(egui::accesskit::Role::MultilineTextInput, "Primary input")
            .click();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        harness.event(egui::Event::Text(r#"{"Value":"unused"}"#.into()));
        harness.run();
        harness.get_by_label("Preview").click();
        // A running worker deliberately repaints its spinner until polled.
        harness.run_steps(3);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while harness.state().app.pending_preview.is_some() && std::time::Instant::now() < deadline
        {
            let context = harness.ctx.clone();
            harness.state_mut().app.poll_preview(&context);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(harness.state().app.pending_preview.is_none());
        let report = harness
            .state_mut()
            .app
            .run_report
            .as_mut()
            .expect("completed Preview");
        assert_eq!(report.report.outputs.len(), 1);
        let crate::run_report::OutputPreview::Text { content, .. } =
            report.report.outputs[0].preview()
        else {
            panic!("JSON preview")
        };
        assert_eq!(
            content.as_bytes(),
            format!("{{\n  \"Result\": \"{EXACT}\",\n  \"Function\": \"{EXACT}\"\n}}\n").as_bytes()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(std::fs::read(&sidecar).unwrap(), layout_bytes);
        assert_eq!(layout(&harness.state().app), expected_layout);
    }
}
