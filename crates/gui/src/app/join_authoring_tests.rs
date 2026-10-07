use super::*;
use ir::{Instance, ScalarType, Value};
use mapping::{
    FailureIteration, FailureRule, FailureSelection, NamedSource, NamedTarget, SequenceExpr,
};

fn row_schema(name: &str, value: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Key", ScalarType::String),
            SchemaNode::scalar(value, ScalarType::String),
        ],
    )
    .repeating()
}

fn fixture(document: MappingDocument, named_source: bool) -> FerruleApp {
    let mut app = FerruleApp::default();
    let mut sources = vec![row_schema("Orders", "Code")];
    if !named_source {
        sources.push(row_schema("Products", "Name"));
    }
    app.project.source = SchemaNode::group("Source", sources);
    app.project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Code", ScalarType::String),
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::scalar("Tuple", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    );
    app.project.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    app.project.graph.nodes.clear();
    app.project.failure_rules.clear();
    app.project.extra_sources.clear();
    app.project.extra_targets.clear();
    app.project.user_functions.clear();
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    if named_source {
        app.project.extra_sources.push(NamedSource {
            name: "Reference".into(),
            path: "reference.json".into(),
            schema: SchemaNode::group("Reference", vec![row_schema("Products", "Name")]),
            options: mapping::FormatOptions {
                json_document: true,
                ..Default::default()
            },
            dynamic_path: None,
        });
    }
    if matches!(document, MappingDocument::Target(_)) {
        app.project.extra_targets.push(NamedTarget {
            name: "Joined".into(),
            path: Some("joined.json".into()),
            schema: app.project.target.clone(),
            root: app.project.root.clone(),
            options: app.project.target_options.clone(),
        });
    }
    app.main_canvas = CanvasDocumentState::main(&app.project);
    if let MappingDocument::Target(index) = document {
        app.open_target_tab(index);
        assert!(app.ensure_target_canvas(index));
    }
    app.selected_scope = vec![0];
    app.mark_clean();
    app.rebase_history();
    assert!(cli::validate(&app.project).is_empty());
    app
}

fn row(key: Value, field: &str, value: &str) -> Instance {
    Instance::Group(
        vec![
            ("Key".into(), Instance::Scalar(key)),
            (field.into(), Instance::Scalar(Value::String(value.into()))),
        ]
        .into(),
    )
}

fn orders() -> Instance {
    Instance::Repeated(vec![
        row(Value::String("same".into()), "Code", "O1"),
        row(Value::Null, "Code", "absent"),
        row(Value::XmlNil(ir::XmlNil), "Code", "nil"),
        row(Value::String("same".into()), "Code", "O2"),
    ])
}

fn products() -> Instance {
    Instance::Repeated(vec![
        row(Value::String("same".into()), "Name", "P1"),
        row(Value::Null, "Name", "absent"),
        row(Value::XmlNil(ir::XmlNil), "Name", "nil"),
        row(Value::String("same".into()), "Name", "P2"),
    ])
}

fn source(named: bool) -> Instance {
    let mut fields = vec![("Orders".into(), orders())];
    if !named {
        fields.push(("Products".into(), products()));
    }
    Instance::Group(fields.into())
}

fn state(app: &FerruleApp) -> String {
    mapping::project_file::encode_pretty(&app.project).expect("project encoding")
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    enabled: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_ui(
        egui::RawInput {
            time: Some(context.cumulative_frame_nr() as f64 / 10.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 2400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let editing = enabled && app.ui_project_editing_enabled();
            ui.add_enabled_ui(editing, |ui| app.show_scope_controls(ui));
            app.show_join_authoring(ui, enabled);
        },
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn positions(shape: &egui::epaint::Shape, wanted: &str, found: &mut Vec<egui::Pos2>) {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == wanted => {
            found.push(text.visual_bounding_rect().center())
        }
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                positions(shape, wanted, found);
            }
        }
        _ => {}
    }
}

fn click(app: &mut FerruleApp, context: &egui::Context, enabled: bool, wanted: &str, last: bool) {
    let mut output = frame(app, context, enabled, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, enabled, Vec::new());
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        positions(&shape.shape, wanted, &mut found);
    }
    let point = if last { found.last() } else { found.first() }
        .copied()
        .unwrap_or_else(|| panic!("visible join control {wanted:?}"));
    for pressed in [true, false] {
        let _ = frame(
            app,
            context,
            enabled,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

fn choose(app: &mut FerruleApp, context: &egui::Context, selected: &str, desired: &str) {
    click(app, context, true, selected, false);
    click(app, context, true, desired, true);
}

fn author(app: &mut FerruleApp, context: &egui::Context, named: bool) {
    click(app, context, true, "Join two collections", false);
    click(app, context, true, "Create inner join", false);
    click(app, context, true, "Add joined output", false);
    choose(app, context, "Key", "Code");
    click(app, context, true, "Bind joined output", false);
    click(app, context, true, "Add joined output", false);
    choose(
        app,
        context,
        "Orders",
        if named {
            "Reference / Products"
        } else {
            "Products"
        },
    );
    choose(app, context, "Key", "Name");
    click(app, context, true, "Bind joined output", false);
    click(app, context, true, "Add joined output", false);
    click(app, context, true, "Tuple position", false);
    click(app, context, true, "Bind joined output", false);
}

fn active_scope(app: &FerruleApp) -> &Scope {
    let (root, _) = target_root(&app.project, app.mapping_workspace.active).unwrap();
    crate::auto_connect::scope_at(root, &[0]).unwrap()
}

fn actual(app: &FerruleApp, named: bool) -> Instance {
    let extras = if named {
        vec![(
            "Reference".into(),
            Instance::Group(vec![("Products".into(), products())].into()),
        )]
    } else {
        Vec::new()
    };
    let context = engine::ExecutionContext::new(Path::new("mapping.json"));
    let result = engine::run_outputs_with_sources_and_context(
        &app.project,
        &source(named),
        extras,
        &context,
    );
    eprintln!("joined original project={} outcome={result:?}", state(app));
    let output = result.expect("authored join executes");
    match app.mapping_workspace.active {
        MappingDocument::Target(index) => output.extras[index].instance.clone(),
        _ => output.primary,
    }
}

fn assert_rows(output: &Instance) {
    let rows = output
        .field("Rows")
        .and_then(Instance::as_repeated)
        .expect("joined rows");
    let values = rows
        .iter()
        .map(|row| {
            (
                row.field("Code").and_then(Instance::as_scalar).cloned(),
                row.field("Name").and_then(Instance::as_scalar).cloned(),
                row.field("Tuple").and_then(Instance::as_scalar).cloned(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![
            (
                Some(Value::String("O1".into())),
                Some(Value::String("P1".into())),
                Some(Value::Int(1))
            ),
            (
                Some(Value::String("O1".into())),
                Some(Value::String("P2".into())),
                Some(Value::Int(2))
            ),
            (
                Some(Value::String("O2".into())),
                Some(Value::String("P1".into())),
                Some(Value::Int(3))
            ),
            (
                Some(Value::String("O2".into())),
                Some(Value::String("P2".into())),
                Some(Value::Int(4))
            ),
        ]
    );
}

fn assert_wires(app: &FerruleApp) {
    let canvas = match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.mapping_workspace.target_canvases[&index],
        _ => &app.main_canvas,
    };
    let nodes = active_scope(app)
        .bindings
        .iter()
        .map(|binding| binding.node)
        .collect::<std::collections::BTreeSet<_>>();
    let count = canvas.snarl.wires().filter(|(from, to)|
        matches!(canvas.snarl.get_node(from.node), Some(CanvasNode::Graph(node)) if nodes.contains(node))
            && matches!(canvas.snarl.get_node(to.node), Some(CanvasNode::TargetBlock(_)))
    ).count();
    assert_eq!(
        count, 3,
        "all three projections have actual canvas target wires"
    );
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule-join-authoring-{}-{next}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("test directory");
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!(
                "retained join-authoring test artifacts: {}",
                self.0.display()
            );
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn join_real_controls_preserve_duplicate_order_nil_exclusion_and_unsaved_preview() {
    let mut app = fixture(MappingDocument::Main, false);
    let context = egui::Context::default();
    crate::icons::install(&context);
    let inspector = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 2600.0),
            )),
            ..Default::default()
        },
        |ui| app.show_inspector(ui, true),
    );
    let mut found = Vec::new();
    for shape in &inspector.shapes {
        positions(&shape.shape, "Join two collections", &mut found);
    }
    assert!(
        !found.is_empty(),
        "ordinary Inspector exposes the creation route"
    );
    author(&mut app, &context, false);
    assert!(cli::validate(&app.project).is_empty());
    assert_rows(&actual(&app, false));
    assert_wires(&app);
    assert!(app.is_dirty());
    assert!(app.document.saved_path().is_none());
    let directory = Directory::new();
    let output = directory.0.join("must-not-write.json");
    app.preview_draft = Some(crate::preview::PreviewDraft {
        target: crate::preview::PreviewTarget::Primary,
        input_identity: "input.json".into(), output_identity: output.display().to_string(),
        input_text: r#"{"Orders":[{"Key":"same","Code":"O1"},{"Key":"same","Code":"O2"}],"Products":[{"Key":"same","Name":"P1"},{"Key":"same","Name":"P2"}]}"#.into(),
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
        "joined Preview original status={} diagnostics={:?}",
        app.status,
        app.diagnostics.items()
    );
    assert!(app.pending_preview.is_none());
    let report = app.run_report.as_mut().expect("successful Preview report");
    let crate::run_report::OutputPreview::Text { content, .. } = report.report.outputs[0].preview()
    else {
        panic!("JSON preview");
    };
    std::fs::write(
        directory.0.join("preview-original.json"),
        content.as_bytes(),
    )
    .unwrap();
    eprintln!("joined Preview original={content}");
    let value: serde_json::Value = serde_json::from_str(content).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"Rows":[
            {"Code":"O1","Name":"P1","Tuple":1},{"Code":"O1","Name":"P2","Tuple":2},
            {"Code":"O2","Name":"P1","Tuple":3},{"Code":"O2","Name":"P2","Tuple":4}
        ]})
    );
    assert!(!output.exists());
    assert!(app.document.saved_path().is_none());
}

#[test]
fn join_named_input_target_real_controls_undo_save_and_reopen_keep_exact_owner_wires() {
    let directory = Directory::new();
    let origin = directory.0.join("origin");
    let destination = directory.0.join("saved");
    std::fs::create_dir_all(&origin).unwrap();
    std::fs::create_dir_all(&destination).unwrap();
    for name in ["input.json", "reference.json", "output.json", "joined.json"] {
        std::fs::write(origin.join(name), b"{}").unwrap();
    }
    let mut app = fixture(MappingDocument::Target(0), true);
    app.document = DocumentLocation::untitled(origin.join("mapping.json"));
    let context = egui::Context::default();
    crate::icons::install(&context);
    let initial = state(&app);
    author(&mut app, &context, true);
    let authored = state(&app);
    let id = active_scope(&app).join().unwrap().0;
    assert_rows(&actual(&app, true));
    assert_wires(&app);
    app.undo_project();
    assert_eq!(
        active_scope(&app).bindings.len(),
        2,
        "one user action undoes only the tuple projection"
    );
    app.redo_project();
    assert_eq!(state(&app), authored);
    assert_eq!(active_scope(&app).join().unwrap().0, id);
    assert_wires(&app);
    let path = destination.join("mapping.json");
    std::fs::write(directory.0.join("before.json"), initial).unwrap();
    std::fs::write(directory.0.join("authored-original.json"), &authored).unwrap();
    let before_save = app.project.clone();
    app.save_document_to(&path).unwrap();
    let saved = state(&app);
    let saved_file = std::fs::read_to_string(&path).unwrap();
    eprintln!("joined Save As original saved={saved} file={saved_file}");
    assert_eq!(
        saved_file, saved,
        "saved bytes match the rebased editor snapshot"
    );
    let mut unchanged = app.project.clone();
    unchanged.source_path = before_save.source_path.clone();
    unchanged.extra_sources[0].path = before_save.extra_sources[0].path.clone();
    unchanged.target_path = before_save.target_path.clone();
    unchanged.extra_targets[0].path = before_save.extra_targets[0].path.clone();
    assert_eq!(
        mapping::project_file::encode_pretty(&unchanged).unwrap(),
        authored,
        "Save As changes only four boundary paths, preserving exact join IDs, graph, rules and bindings"
    );
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    reopened.open_target_tab(0);
    assert!(reopened.ensure_target_canvas(0));
    reopened.selected_scope = vec![0];
    assert_eq!(state(&reopened), saved);
    for (stored, name) in [
        (
            reopened.project.source_path.as_deref().unwrap(),
            "input.json",
        ),
        (
            reopened.project.extra_sources[0].path.as_str(),
            "reference.json",
        ),
        (
            reopened.project.target_path.as_deref().unwrap(),
            "output.json",
        ),
        (
            reopened.project.extra_targets[0].path.as_deref().unwrap(),
            "joined.json",
        ),
    ] {
        assert!(
            !Path::new(stored).is_absolute(),
            "Save As retains relative boundary paths"
        );
        assert_eq!(
            std::fs::canonicalize(destination.join(stored)).unwrap(),
            std::fs::canonicalize(origin.join(name)).unwrap(),
            "{name} still names its original physical boundary after Save As"
        );
        assert_eq!(std::fs::read(origin.join(name)).unwrap(), b"{}");
    }
    assert_eq!(active_scope(&reopened).join().unwrap().0, id);
    assert_rows(&actual(&reopened, true));
    assert_wires(&reopened);
    assert!(!reopened.is_dirty());
}

#[test]
fn join_locked_widgets_stale_navigation_and_cancel_preserve_project_history() {
    let mut app = fixture(MappingDocument::Main, false);
    let context = egui::Context::default();
    crate::icons::install(&context);
    let original = state(&app);
    click(&mut app, &context, false, "Join two collections", false);
    assert!(app.join_authoring_draft.is_none());
    assert_eq!(state(&app), original);
    click(&mut app, &context, true, "Join two collections", false);
    let draft = app.join_authoring_draft.clone().unwrap();
    app.begin_preview();
    click(&mut app, &context, true, "Create inner join", false);
    app.apply_join_draft(&draft, true);
    assert_eq!(state(&app), original);
    assert!(!app.is_dirty());
    assert!(!app.can_undo());
    app.preview_draft = None;
    click(&mut app, &context, true, "Cancel join edit", false);
    assert!(app.join_authoring_draft.is_none());
    assert_eq!(state(&app), original);
    app.selected_scope.clear();
    app.apply_join_draft(&draft, true);
    assert_eq!(state(&app), original);
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("selected scope changed"))
    );
    let _ = frame(&mut app, &context, true, Vec::new());
    assert!(
        app.join_authoring_draft.is_none(),
        "stale selection discards only transient draft"
    );
    assert!(!app.can_undo());
}

#[test]
fn join_invalid_choices_and_both_identity_exhaustion_paths_are_atomic() {
    let mut app = fixture(MappingDocument::Main, false);
    let context = egui::Context::default();
    crate::icons::install(&context);
    click(&mut app, &context, true, "Join two collections", false);
    choose(&mut app, &context, "Products", "Orders");
    let original = state(&app);
    click(&mut app, &context, true, "Create inner join", false);
    assert_eq!(state(&app), original);
    assert!(!app.can_undo());
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("used more than once"))
    );
    click(&mut app, &context, true, "Cancel join edit", false);
    app.project.graph.nodes.insert(
        0,
        Node::JoinPosition {
            join: JoinId::new(u64::MAX),
        },
    );
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    click(&mut app, &context, true, "Join two collections", false);
    let original = state(&app);
    click(&mut app, &context, true, "Create inner join", false);
    assert_eq!(state(&app), original);
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("Join IDs are exhausted"))
    );
    let mut app = fixture(MappingDocument::Main, false);
    click(&mut app, &context, true, "Join two collections", false);
    click(&mut app, &context, true, "Create inner join", false);
    app.project.graph.nodes.insert(
        NodeId::MAX,
        Node::Const {
            value: Value::String("retained".into()),
        },
    );
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    click(&mut app, &context, true, "Add joined output", false);
    let original = state(&app);
    click(&mut app, &context, true, "Bind joined output", false);
    assert_eq!(state(&app), original);
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("mapping node IDs are exhausted"))
    );
    assert!(!app.is_dirty());
    assert!(!app.can_undo());
}

#[test]
fn join_sequence_items_and_imported_plans_survive_creation_and_removal_refusal() {
    let mut app = fixture(MappingDocument::Main, false);
    app.project.graph.nodes.extend([
        (
            0,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            1,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::Int(0),
            },
        ),
    ]);
    app.project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 2,
                item: 0,
            },
        },
        selection: FailureSelection::WhenFalse { predicate: 1 },
        message: Some(0),
    });
    let owners = crate::graph_viewer::project_sequence_item_ids(&app.project);
    let rule = serde_json::to_value(&app.project.failure_rules).unwrap();
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    let context = egui::Context::default();
    crate::icons::install(&context);
    author(&mut app, &context, false);
    assert_eq!(
        crate::graph_viewer::project_sequence_item_ids(&app.project),
        owners
    );
    assert_eq!(
        serde_json::to_value(&app.project.failure_rules).unwrap(),
        rule
    );
    assert!(
        matches!(app.project.graph.nodes.get(&0), Some(Node::SourceField { path, frame: None }) if path.is_empty())
    );
    assert_rows(&actual(&app, false));
    let before = state(&app);
    click(&mut app, &context, true, "Remove scope", false);
    app.remove_selected_target_scope();
    assert_eq!(
        state(&app),
        before,
        "UI and central action refuse joined-owner retirement"
    );
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains(JOIN_REMOVAL_REASON))
    );
    let preserved = active_scope(&app)
        .join()
        .unwrap()
        .1
        .clone()
        .then(
            JoinSource::new(vec!["Third".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["Orders".into()],
                vec!["Key".into()],
                vec!["Key".into()],
            )),
        )
        .unwrap();
    app.project.root.children[0].iteration = ScopeIteration::InnerJoin {
        id: JoinId::new(77),
        plan: preserved,
    };
    let imported = state(&app);
    click(&mut app, &context, true, "Join two collections", false);
    click(&mut app, &context, true, "Add joined output", false);
    assert!(app.join_authoring_draft.is_none());
    assert_eq!(
        state(&app),
        imported,
        "unsupported imported plans stay untouched"
    );
}

#[test]
fn join_projections_refresh_main_and_two_open_named_canvases_preserving_positions_and_history() {
    let mut app = fixture(MappingDocument::Main, false);
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("shared".into()),
        },
    );
    for name in ["First", "Second"] {
        app.project.extra_targets.push(NamedTarget {
            name: name.into(),
            path: Some(format!("{name}.json")),
            schema: app.project.target.clone(),
            root: app.project.root.clone(),
            options: app.project.target_options.clone(),
        });
    }
    app.main_canvas = CanvasDocumentState::main(&app.project);
    for index in 0..2 {
        app.open_target_tab(index);
        assert!(app.ensure_target_canvas(index));
    }
    app.mapping_workspace.active = MappingDocument::Main;
    app.mapping_workspace.focused = MappingDocument::Main;
    app.selected_scope = vec![0];
    let place = |snarl: &mut egui_snarl::Snarl<CanvasNode>, position| {
        let node = snarl
            .node_ids()
            .find_map(|(id, node)| (*node == CanvasNode::Graph(0)).then_some(id))
            .unwrap();
        snarl.get_node_info_mut(node).unwrap().pos = position;
    };
    place(&mut app.main_canvas.snarl, egui::pos2(125.0, 85.0));
    for (index, canvas) in &mut app.mapping_workspace.target_canvases {
        place(
            &mut canvas.snarl,
            egui::pos2(425.0 + *index as f32 * 200.0, 185.0),
        );
    }
    app.mark_clean();
    app.rebase_history();
    let original = state(&app);
    let original_positions = [
        CanvasLayout::capture_nodes(&app.main_canvas.snarl),
        CanvasLayout::capture_nodes(&app.mapping_workspace.target_canvases[&0].snarl),
        CanvasLayout::capture_nodes(&app.mapping_workspace.target_canvases[&1].snarl),
    ];
    let verify = |app: &FerruleApp| {
        let expected = app
            .project
            .graph
            .nodes
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let owner_binding_count = active_scope(app).bindings.len();
        for (index, snarl) in [
            &app.main_canvas.snarl,
            &app.mapping_workspace.target_canvases[&0].snarl,
            &app.mapping_workspace.target_canvases[&1].snarl,
        ]
        .into_iter()
        .enumerate()
        {
            let actual = snarl
                .nodes()
                .filter_map(|node| match node {
                    CanvasNode::Graph(id) => Some(*id),
                    _ => None,
                })
                .collect::<std::collections::BTreeSet<_>>();
            let wires = snarl.wires().collect::<Vec<_>>();
            eprintln!(
                "joined all-canvas original phase project={} canvas={index} graph={actual:?} wires={wires:?}",
                state(app)
            );
            assert_eq!(
                actual, expected,
                "every open mapping canvas reflects the shared graph"
            );
            let after = CanvasLayout::capture_nodes(snarl);
            for node in &original_positions[index] {
                assert_eq!(
                    after.iter().find(|current| current.node == node.node),
                    Some(node),
                    "existing positions remain exact in canvas {index}"
                );
            }
            let target_wires = wires
                .iter()
                .filter(|(from, to)| {
                    matches!(snarl.get_node(from.node), Some(CanvasNode::Graph(_)))
                        && matches!(snarl.get_node(to.node), Some(CanvasNode::TargetBlock(_)))
                })
                .count();
            assert_eq!(
                target_wires,
                if index == 0 { owner_binding_count } else { 0 },
                "only the owning target gains projection wires"
            );
        }
    };
    let context = egui::Context::default();
    crate::icons::install(&context);
    verify(&app);
    author(&mut app, &context, false);
    let authored = state(&app);
    let join = active_scope(&app).join().unwrap().0;
    verify(&app);
    assert_rows(&actual(&app, false));
    for remaining in (0..3).rev() {
        app.undo_project();
        assert_eq!(active_scope(&app).bindings.len(), remaining);
        assert_eq!(active_scope(&app).join().unwrap().0, join);
        verify(&app);
    }
    app.undo_project();
    assert_eq!(
        state(&app),
        original,
        "undoing creation restores the entire original project"
    );
    verify(&app);
    app.redo_project();
    assert_eq!(active_scope(&app).join().unwrap().0, join);
    verify(&app);
    for restored in 1..=3 {
        app.redo_project();
        assert_eq!(active_scope(&app).bindings.len(), restored);
        verify(&app);
    }
    assert_eq!(state(&app), authored);
    assert_rows(&actual(&app, false));
}

fn composite_schema(name: &str, value: &str, key: ScalarType) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Key", key),
            SchemaNode::scalar("Tenant", ScalarType::String),
            SchemaNode::scalar("Batch", ScalarType::Int),
            SchemaNode::scalar(value, ScalarType::String),
        ],
    )
    .repeating()
}

fn composite_fixture(document: MappingDocument, named: bool) -> FerruleApp {
    let mut app = fixture(document, named);
    let mut children = vec![composite_schema("Orders", "Code", ScalarType::Int)];
    if named {
        app.project.extra_sources[0].schema = SchemaNode::group(
            "Reference",
            vec![composite_schema("Products", "Name", ScalarType::String)],
        );
    } else {
        children.push(composite_schema("Products", "Name", ScalarType::String));
    }
    app.project.source = SchemaNode::group("Source", children);
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mapping_workspace.target_canvases.clear();
    if let MappingDocument::Target(index) = document {
        app.open_target_tab(index);
        assert!(app.ensure_target_canvas(index));
    }
    app.selected_scope = vec![0];
    app.mark_clean();
    app.rebase_history();
    assert!(cli::validate(&app.project).is_empty());
    app
}

fn composite_row(key: Value, tenant: Value, batch: Value, name: &str, text: &str) -> Instance {
    Instance::Group(
        vec![
            ("Key".into(), Instance::Scalar(key)),
            ("Tenant".into(), Instance::Scalar(tenant)),
            ("Batch".into(), Instance::Scalar(batch)),
            (name.into(), Instance::Scalar(Value::String(text.into()))),
        ]
        .into(),
    )
}

fn composite_inputs() -> (Vec<Instance>, Vec<Instance>) {
    let left = [("A", "L1"), ("B", "L2"), ("A", "L3")]
        .into_iter()
        .map(|(tenant, code)| {
            composite_row(
                Value::Int(7),
                Value::String(tenant.into()),
                Value::Int(1),
                "Code",
                code,
            )
        })
        .collect();
    let right = [
        ("A", 1, "R1"),
        ("A", 2, "R2"),
        ("B", 1, "R3"),
        ("C", 1, "R4"),
    ]
    .into_iter()
    .map(|(tenant, batch, name)| {
        composite_row(
            Value::String("7".into()),
            Value::String(tenant.into()),
            Value::Int(batch),
            "Name",
            name,
        )
    })
    .collect();
    (left, right)
}

fn composite_expected(pairs: &[(&str, &str)]) -> Instance {
    Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(
                pairs
                    .iter()
                    .enumerate()
                    .map(|(index, (code, name))| {
                        Instance::Group(
                            vec![
                                (
                                    "Code".into(),
                                    Instance::Scalar(Value::String((*code).into())),
                                ),
                                (
                                    "Name".into(),
                                    Instance::Scalar(Value::String((*name).into())),
                                ),
                                (
                                    "Tuple".into(),
                                    Instance::Scalar(Value::Int(index as i64 + 1)),
                                ),
                            ]
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    )
}

fn composite_run(
    directory: &Directory,
    label: &str,
    app: &FerruleApp,
    named: bool,
    left: Vec<Instance>,
    right: Vec<Instance>,
) -> Result<Instance, engine::EngineError> {
    let mut fields = vec![("Orders".into(), Instance::Repeated(left))];
    let extras = if named {
        vec![(
            "Reference".into(),
            Instance::Group(vec![("Products".into(), Instance::Repeated(right))].into()),
        )]
    } else {
        fields.push(("Products".into(), Instance::Repeated(right)));
        Vec::new()
    };
    let source = Instance::Group(fields.into());
    std::fs::write(
        directory.0.join(format!("{label}-project.json")),
        state(app),
    )
    .unwrap();
    std::fs::write(
        directory.0.join(format!("{label}-input.txt")),
        format!("source={source:#?}\nextras={extras:#?}\n"),
    )
    .unwrap();
    let context = engine::ExecutionContext::new(Path::new("mapping.json"));
    let original =
        engine::run_outputs_with_sources_and_context(&app.project, &source, extras, &context);
    std::fs::write(
        directory.0.join(format!("{label}-outcome.txt")),
        format!("{original:#?}\n"),
    )
    .unwrap();
    eprintln!("composite join original {label}={original:?}");
    original.map(|outputs| match app.mapping_workspace.active {
        MappingDocument::Target(index) => outputs.extras[index].instance.clone(),
        _ => outputs.primary,
    })
}

fn composite_choose_nth(
    app: &mut FerruleApp,
    context: &egui::Context,
    selected: &str,
    occurrence: usize,
    desired: &str,
) {
    let mut output = frame(app, context, true, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, true, Vec::new());
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        let mut points = Vec::new();
        positions(&shape.shape, selected, &mut points);
        found.extend(
            points
                .into_iter()
                .filter(|point| shape.clip_rect.contains(*point)),
        );
    }
    let point = *found
        .get(occurrence)
        .unwrap_or_else(|| panic!("visible composite picker {selected:?} occurrence {occurrence}"));
    for pressed in [true, false] {
        let _ = frame(
            app,
            context,
            true,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    click(app, context, true, desired, true);
}

fn composite_prepare(app: &mut FerruleApp, context: &egui::Context, count: usize) {
    assert!((1..=3).contains(&count));
    click(app, context, true, "Join two collections", false);
    for field in ["Tenant", "Batch"].into_iter().take(count - 1) {
        click(app, context, true, "Add equality pair", false);
        // Both first-pair Key choices remain; the next two belong to the new pair.
        composite_choose_nth(app, context, "Key", 2, field);
        composite_choose_nth(app, context, "Key", 2, field);
    }
}

fn composite_bind(app: &mut FerruleApp, context: &egui::Context, named: bool) {
    click(app, context, true, "Add joined output", false);
    choose(app, context, "Key", "Code");
    click(app, context, true, "Bind joined output", false);
    click(app, context, true, "Add joined output", false);
    choose(
        app,
        context,
        "Orders",
        if named {
            "Reference / Products"
        } else {
            "Products"
        },
    );
    choose(app, context, "Key", "Name");
    click(app, context, true, "Bind joined output", false);
    click(app, context, true, "Add joined output", false);
    click(app, context, true, "Tuple position", false);
    click(app, context, true, "Bind joined output", false);
}

type CompositeKeyPaths = Vec<(Vec<String>, Vec<String>)>;
type CompositeDraftPaths = (Vec<String>, Vec<String>, CompositeKeyPaths);

fn composite_draft(app: &FerruleApp) -> CompositeDraftPaths {
    let Edit::Create { left, right, keys } = &app
        .join_authoring_draft
        .as_ref()
        .expect("creation draft")
        .edit
    else {
        panic!("creation draft");
    };
    (
        left.clone(),
        right.clone(),
        keys.iter()
            .map(|key| (key.left.clone(), key.right.clone()))
            .collect(),
    )
}

fn composite_key_names(app: &FerruleApp) -> CompositeKeyPaths {
    active_scope(app)
        .join()
        .unwrap()
        .1
        .stages()
        .flat_map(|(_, conditions)| {
            conditions
                .iter()
                .map(|key| (key.left_path().to_vec(), key.right_path().to_vec()))
        })
        .collect()
}

#[test]
fn join_composite_real_pair_controls_have_exact_one_two_and_three_key_oracles() {
    let directory = Directory::new();
    let expected = [
        vec![
            ("L1", "R1"),
            ("L1", "R2"),
            ("L1", "R3"),
            ("L1", "R4"),
            ("L2", "R1"),
            ("L2", "R2"),
            ("L2", "R3"),
            ("L2", "R4"),
            ("L3", "R1"),
            ("L3", "R2"),
            ("L3", "R3"),
            ("L3", "R4"),
        ],
        vec![
            ("L1", "R1"),
            ("L1", "R2"),
            ("L2", "R3"),
            ("L3", "R1"),
            ("L3", "R2"),
        ],
        vec![("L1", "R1"), ("L2", "R3"), ("L3", "R1")],
    ];
    for count in 1..=3 {
        let mut app = composite_fixture(MappingDocument::Main, false);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let before = state(&app);
        composite_prepare(&mut app, &context, count);
        assert_eq!(
            state(&app),
            before,
            "draft key edits do not publish a partial plan"
        );
        assert!(!app.can_undo());
        if count == 1 {
            click(&mut app, &context, true, "Remove key pair 1", false);
            assert_eq!(
                composite_draft(&app).2.len(),
                1,
                "the last equality pair cannot be removed"
            );
        } else if count == 3 {
            let draft = composite_draft(&app);
            click(&mut app, &context, true, "Move key pair 3 up", false);
            assert_eq!(composite_draft(&app).2[1].0, vec!["Batch".to_string()]);
            click(&mut app, &context, true, "Move key pair 2 down", false);
            assert_eq!(composite_draft(&app), draft);
            click(&mut app, &context, true, "Remove key pair 3", false);
            assert_eq!(composite_draft(&app).2.len(), 2);
            click(&mut app, &context, true, "Add equality pair", false);
            composite_choose_nth(&mut app, &context, "Key", 2, "Batch");
            composite_choose_nth(&mut app, &context, "Key", 2, "Batch");
            assert_eq!(composite_draft(&app), draft);
        }
        click(&mut app, &context, true, "Create inner join", false);
        assert!(cli::validate(&app.project).is_empty());
        let wanted = ["Key", "Tenant", "Batch"]
            .into_iter()
            .take(count)
            .map(|name| (vec![name.to_string()], vec![name.to_string()]))
            .collect::<Vec<_>>();
        assert_eq!(composite_key_names(&app), wanted);
        composite_bind(&mut app, &context, false);
        assert_wires(&app);
        let (left, right) = composite_inputs();
        let actual = composite_run(
            &directory,
            &format!("key-count-{count}"),
            &app,
            false,
            left,
            right,
        )
        .expect("authored composite mapping");
        assert_eq!(actual, composite_expected(&expected[count - 1]));
    }
}

fn composite_replace_field(row: &mut Instance, name: &str, value: Option<Value>) {
    let Instance::Group(fields) = row else {
        panic!("test row");
    };
    if let Some(value) = value {
        let field = fields
            .iter_mut()
            .find(|(field, _)| field == name)
            .expect("test field");
        field.1 = Instance::Scalar(value);
    } else {
        fields.retain(|(field, _)| field != name);
    }
}

#[test]
fn join_composite_any_key_absence_null_or_nil_and_empty_inputs_do_not_match() {
    let directory = Directory::new();
    let mut app = composite_fixture(MappingDocument::Main, false);
    let context = egui::Context::default();
    crate::icons::install(&context);
    composite_prepare(&mut app, &context, 3);
    click(&mut app, &context, true, "Create inner join", false);
    composite_bind(&mut app, &context, false);
    let base_left = composite_row(
        Value::Int(7),
        Value::String("A".into()),
        Value::Int(1),
        "Code",
        "L",
    );
    let base_right = composite_row(
        Value::String("7".into()),
        Value::String("A".into()),
        Value::Int(1),
        "Name",
        "R",
    );
    let actual = composite_run(
        &directory,
        "all-three-match",
        &app,
        false,
        vec![base_left.clone()],
        vec![base_right.clone()],
    )
    .unwrap();
    assert_eq!(actual, composite_expected(&[("L", "R")]));
    for side in 0..2 {
        for name in ["Key", "Tenant", "Batch"] {
            for (kind, value) in [
                ("absent", None),
                ("null", Some(Value::Null)),
                ("json-null", Some(Value::JsonNull(ir::JsonNull))),
                ("xml-nil", Some(Value::XmlNil(ir::XmlNil))),
            ] {
                let mut left = base_left.clone();
                let mut right = base_right.clone();
                composite_replace_field(
                    if side == 0 { &mut left } else { &mut right },
                    name,
                    value,
                );
                let actual = composite_run(
                    &directory,
                    &format!("side-{side}-{name}-{kind}"),
                    &app,
                    false,
                    vec![left],
                    vec![right],
                )
                .unwrap();
                assert_eq!(actual, composite_expected(&[]), "{side} {name} {kind}");
            }
        }
    }
    for (name, left, right) in [
        ("empty-left", Vec::new(), vec![base_right.clone()]),
        ("empty-right", vec![base_left.clone()], Vec::new()),
        ("both-empty", Vec::new(), Vec::new()),
    ] {
        assert_eq!(
            composite_run(&directory, name, &app, false, left, right).unwrap(),
            composite_expected(&[])
        );
    }
    let mut lexical = base_right;
    composite_replace_field(&mut lexical, "Key", Some(Value::String("07".into())));
    assert_eq!(
        composite_run(
            &directory,
            "lexical-not-numeric-parse",
            &app,
            false,
            vec![base_left],
            vec![lexical]
        )
        .unwrap(),
        composite_expected(&[]),
        "mixed string equality retains native scalar lexical comparison"
    );
}

#[test]
fn join_composite_pair_order_preserves_native_reached_and_skipped_typed_comparison_errors() {
    let directory = Directory::new();
    for reversed in [false, true] {
        let mut app = composite_fixture(MappingDocument::Main, false);
        let SchemaKind::Group { children, .. } = &mut app.project.source.kind else {
            panic!("source");
        };
        for child in children {
            let ty = if child.name == "Orders" {
                ScalarType::Bool
            } else {
                ScalarType::Int
            };
            let SchemaKind::Group { children, .. } = &mut child.kind else {
                panic!("row schema");
            };
            *children
                .iter_mut()
                .find(|field| field.name == "Tenant")
                .unwrap() = SchemaNode::scalar("Tenant", ty);
        }
        app.main_canvas = CanvasDocumentState::main(&app.project);
        app.mark_clean();
        app.rebase_history();
        let context = egui::Context::default();
        crate::icons::install(&context);
        composite_prepare(&mut app, &context, 3);
        if reversed {
            click(&mut app, &context, true, "Move key pair 2 up", false);
        }
        click(&mut app, &context, true, "Create inner join", false);
        assert!(cli::validate(&app.project).is_empty());
        composite_bind(&mut app, &context, false);
        let left = composite_row(Value::Int(7), Value::Bool(true), Value::Int(1), "Code", "L");
        for matched_first in [false, true] {
            let right = composite_row(
                Value::String(if matched_first { "7" } else { "8" }.into()),
                Value::Int(1),
                Value::Int(1),
                "Name",
                "R",
            );
            let actual = composite_run(
                &directory,
                &format!("reversed-{reversed}-key-match-{matched_first}"),
                &app,
                false,
                vec![left.clone()],
                vec![right],
            );
            if reversed || matched_first {
                assert!(
                    matches!(
                        &actual,
                        Err(engine::EngineError::Function(
                            functions::FunctionError::TypeMismatch {
                                function: "equal",
                                got: "int"
                            }
                        ))
                    ),
                    "native error identity/order must survive: {actual:?}"
                );
            } else {
                assert_eq!(
                    actual.unwrap(),
                    composite_expected(&[]),
                    "an earlier false key skips the later invalid comparison"
                );
            }
        }
    }
}

#[test]
fn join_composite_named_output_save_history_and_all_open_canvases_preserve_complete_plan() {
    let directory = Directory::new();
    let origin = directory.0.join("origin");
    let saved_dir = directory.0.join("saved");
    std::fs::create_dir_all(&origin).unwrap();
    std::fs::create_dir_all(&saved_dir).unwrap();
    for name in ["input.json", "reference.json", "output.json", "joined.json"] {
        std::fs::write(origin.join(name), b"{}").unwrap();
    }
    let mut app = composite_fixture(MappingDocument::Target(0), true);
    app.document = DocumentLocation::untitled(origin.join("mapping.json"));
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("preserved".into()),
        },
    );
    for name in ["Other", "Third"] {
        app.project.extra_targets.push(NamedTarget {
            name: name.into(),
            path: None,
            schema: app.project.target.clone(),
            root: app.project.root.clone(),
            options: app.project.target_options.clone(),
        });
    }
    app.rebuild_mapping_canvases_after_retirement();
    for index in 0..3 {
        app.open_target_tab(index);
        assert!(app.ensure_target_canvas(index));
    }
    app.mapping_workspace.active = MappingDocument::Target(0);
    app.mapping_workspace.focused = MappingDocument::Target(0);
    app.selected_scope = vec![0];
    for (index, canvas) in std::iter::once(&mut app.main_canvas)
        .chain(app.mapping_workspace.target_canvases.values_mut())
        .enumerate()
    {
        let node = canvas
            .snarl
            .node_ids()
            .find_map(|(id, node)| (*node == CanvasNode::Graph(0)).then_some(id))
            .unwrap();
        canvas.snarl.get_node_info_mut(node).unwrap().pos =
            egui::pos2(115.0 + index as f32 * 200.0, 90.0);
    }
    app.mark_clean();
    app.rebase_history();
    let original = state(&app);
    let original_positions = std::iter::once(&app.main_canvas)
        .chain(app.mapping_workspace.target_canvases.values())
        .map(|canvas| CanvasLayout::capture_nodes(&canvas.snarl))
        .collect::<Vec<_>>();
    let verify = |app: &FerruleApp| {
        let expected = app
            .project
            .graph
            .nodes
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        for (index, canvas) in std::iter::once(&app.main_canvas)
            .chain(app.mapping_workspace.target_canvases.values())
            .enumerate()
        {
            let actual = canvas
                .snarl
                .nodes()
                .filter_map(|node| {
                    if let CanvasNode::Graph(id) = node {
                        Some(*id)
                    } else {
                        None
                    }
                })
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(actual, expected, "shared graph in canvas {index}");
            let current = CanvasLayout::capture_nodes(&canvas.snarl);
            for node in &original_positions[index] {
                assert_eq!(
                    current.iter().find(|after| after.node == node.node),
                    Some(node)
                );
            }
            let wires = canvas
                .snarl
                .wires()
                .filter(|(from, to)| {
                    matches!(canvas.snarl.get_node(from.node), Some(CanvasNode::Graph(_)))
                        && matches!(
                            canvas.snarl.get_node(to.node),
                            Some(CanvasNode::TargetBlock(_))
                        )
                })
                .count();
            assert_eq!(
                wires,
                if index == 1 {
                    active_scope(app).bindings.len()
                } else {
                    0
                },
                "only target 0 gains joined output wires"
            );
        }
    };
    let context = egui::Context::default();
    crate::icons::install(&context);
    composite_prepare(&mut app, &context, 3);
    click(&mut app, &context, true, "Create inner join", false);
    composite_bind(&mut app, &context, true);
    verify(&app);
    let authored = state(&app);
    let id = active_scope(&app).join().unwrap().0;
    let keys = composite_key_names(&app);
    for _ in 0..4 {
        app.undo_project();
        verify(&app);
    }
    assert_eq!(state(&app), original);
    for _ in 0..4 {
        app.redo_project();
        verify(&app);
    }
    assert_eq!(state(&app), authored);
    assert_eq!(active_scope(&app).join().unwrap().0, id);
    assert_eq!(composite_key_names(&app), keys);
    let (left, right) = composite_inputs();
    assert_eq!(
        composite_run(&directory, "named-before-save", &app, true, left, right).unwrap(),
        composite_expected(&[("L1", "R1"), ("L2", "R3"), ("L3", "R1")])
    );
    let before_save = app.project.clone();
    let path = saved_dir.join("mapping.json");
    app.save_document_to(&path).unwrap();
    let saved = state(&app);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    let mut logical = app.project.clone();
    logical.source_path = before_save.source_path.clone();
    logical.extra_sources[0].path = before_save.extra_sources[0].path.clone();
    logical.target_path = before_save.target_path.clone();
    logical.extra_targets[0].path = before_save.extra_targets[0].path.clone();
    assert_eq!(
        mapping::project_file::encode_pretty(&logical).unwrap(),
        authored
    );
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    reopened.open_target_tab(0);
    assert!(reopened.ensure_target_canvas(0));
    reopened.selected_scope = vec![0];
    assert_eq!(state(&reopened), saved);
    assert_eq!(composite_key_names(&reopened), keys);
    assert_eq!(active_scope(&reopened).join().unwrap().0, id);
    assert_wires(&reopened);
    for (stored, name) in [
        (
            reopened.project.source_path.as_deref().unwrap(),
            "input.json",
        ),
        (
            reopened.project.extra_sources[0].path.as_str(),
            "reference.json",
        ),
        (
            reopened.project.target_path.as_deref().unwrap(),
            "output.json",
        ),
        (
            reopened.project.extra_targets[0].path.as_deref().unwrap(),
            "joined.json",
        ),
    ] {
        assert!(!Path::new(stored).is_absolute());
        assert_eq!(
            std::fs::canonicalize(saved_dir.join(stored)).unwrap(),
            std::fs::canonicalize(origin.join(name)).unwrap()
        );
        assert_eq!(std::fs::read(origin.join(name)).unwrap(), b"{}");
    }
    let (left, right) = composite_inputs();
    assert_eq!(
        composite_run(
            &directory,
            "named-after-reopen",
            &reopened,
            true,
            left,
            right
        )
        .unwrap(),
        composite_expected(&[("L1", "R1"), ("L2", "R3"), ("L3", "R1")])
    );
    assert!(!reopened.is_dirty());
}

#[test]
fn join_composite_draft_reset_lock_cancel_stale_and_invalid_pairs_are_atomic() {
    let mut app = composite_fixture(MappingDocument::Main, false);
    let SchemaKind::Group { children, .. } = &mut app.project.source.kind else {
        panic!("source");
    };
    children.push(
        SchemaNode::group(
            "ZArchive",
            vec![
                SchemaNode::scalar("Alternate", ScalarType::Int),
                SchemaNode::scalar("Name", ScalarType::String),
            ],
        )
        .repeating(),
    );
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    let context = egui::Context::default();
    crate::icons::install(&context);
    let original = state(&app);
    let layout =
        CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
    composite_prepare(&mut app, &context, 3);
    let draft = app.join_authoring_draft.clone().unwrap();
    let signature = composite_draft(&app);
    app.begin_preview();
    for control in [
        "Add equality pair",
        "Remove key pair 3",
        "Move key pair 2 up",
        "Create inner join",
        "Cancel join edit",
    ] {
        click(&mut app, &context, true, control, false);
    }
    app.apply_join_draft(&draft, true);
    assert_eq!(composite_draft(&app), signature);
    assert_eq!(state(&app), original);
    assert_eq!(
        CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace),
        layout
    );
    assert!(!app.can_undo());
    assert!(!app.is_dirty());
    app.preview_draft = None;
    choose(&mut app, &context, "Products", "ZArchive");
    assert!(
        composite_draft(&app)
            .2
            .iter()
            .all(|(_, right)| right == &vec!["Alternate".to_string()])
    );
    choose(&mut app, &context, "Orders", "Products");
    assert!(
        composite_draft(&app)
            .2
            .iter()
            .all(|(left, _)| left == &vec!["Key".to_string()])
    );
    assert_eq!(state(&app), original);
    click(&mut app, &context, true, "Cancel join edit", false);
    assert!(app.join_authoring_draft.is_none());
    assert_eq!(state(&app), original);
    assert!(!app.can_undo());
    for empty in [false, true] {
        let mut invalid = draft.clone();
        let Edit::Create { keys, .. } = &mut invalid.edit else {
            panic!("create");
        };
        if empty {
            keys.clear();
        } else {
            keys[2].right = vec!["missing".into()];
        }
        app.apply_join_draft(&invalid, true);
        assert_eq!(state(&app), original);
        assert!(!app.can_undo());
        assert!(!app.is_dirty());
        let expected = if empty {
            "at least one equality pair"
        } else {
            "Equality pair 3"
        };
        assert!(
            app.diagnostics
                .items()
                .iter()
                .any(|item| item.message.contains(expected))
        );
    }
    app.selected_scope.clear();
    app.apply_join_draft(&draft, true);
    assert_eq!(state(&app), original);
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("selected scope changed"))
    );
    let _ = frame(&mut app, &context, true, Vec::new());
    assert!(app.join_authoring_draft.is_none());
    let mut exhausted = composite_fixture(MappingDocument::Main, false);
    exhausted.project.graph.nodes.insert(
        0,
        Node::JoinPosition {
            join: JoinId::new(u64::MAX),
        },
    );
    exhausted.main_canvas = CanvasDocumentState::main(&exhausted.project);
    exhausted.mark_clean();
    exhausted.rebase_history();
    let original = state(&exhausted);
    composite_prepare(&mut exhausted, &context, 3);
    click(&mut exhausted, &context, true, "Create inner join", false);
    assert_eq!(state(&exhausted), original);
    assert!(!exhausted.is_dirty());
    assert!(!exhausted.can_undo());
    assert!(
        exhausted
            .diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("Join IDs are exhausted"))
    );
    let mut graph_exhausted = composite_fixture(MappingDocument::Main, false);
    graph_exhausted.project.graph.nodes.insert(
        NodeId::MAX,
        Node::Const {
            value: Value::String("retained".into()),
        },
    );
    graph_exhausted.main_canvas = CanvasDocumentState::main(&graph_exhausted.project);
    graph_exhausted.mark_clean();
    graph_exhausted.rebase_history();
    composite_prepare(&mut graph_exhausted, &context, 3);
    click(
        &mut graph_exhausted,
        &context,
        true,
        "Create inner join",
        false,
    );
    assert_eq!(
        composite_key_names(&graph_exhausted).len(),
        3,
        "key creation does not need new graph IDs"
    );
    graph_exhausted.mark_clean();
    graph_exhausted.rebase_history();
    let original = state(&graph_exhausted);
    click(
        &mut graph_exhausted,
        &context,
        true,
        "Add joined output",
        false,
    );
    click(
        &mut graph_exhausted,
        &context,
        true,
        "Bind joined output",
        false,
    );
    assert_eq!(state(&graph_exhausted), original);
    assert!(!graph_exhausted.is_dirty());
    assert!(!graph_exhausted.can_undo());
    assert!(
        graph_exhausted
            .diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("mapping node IDs are exhausted"))
    );
}

#[test]
fn join_composite_unsaved_preview_and_imported_plan_display_keep_exact_keys_without_files() {
    let directory = Directory::new();
    let mut app = composite_fixture(MappingDocument::Main, false);
    let context = egui::Context::default();
    crate::icons::install(&context);
    composite_prepare(&mut app, &context, 3);
    click(&mut app, &context, true, "Create inner join", false);
    composite_bind(&mut app, &context, false);
    let authored = state(&app);
    let keys = composite_key_names(&app);
    // Treat the saved shape as an imported plan: display cannot rewrite or recreate it.
    let _ = frame(&mut app, &context, true, Vec::new());
    click(&mut app, &context, true, "Join two collections", false);
    assert!(app.join_authoring_draft.is_none());
    assert_eq!(state(&app), authored);
    assert_eq!(composite_key_names(&app), keys);
    let output = directory.0.join("must-not-publish.json");
    let input = r#"{"Orders":[{"Key":7,"Tenant":"A","Batch":1,"Code":"L1"},{"Key":7,"Tenant":"B","Batch":1,"Code":"L2"},{"Key":7,"Tenant":"A","Batch":1,"Code":"L3"}],"Products":[{"Key":"7","Tenant":"A","Batch":1,"Name":"R1"},{"Key":"7","Tenant":"A","Batch":2,"Name":"R2"},{"Key":"7","Tenant":"B","Batch":1,"Name":"R3"},{"Key":"7","Tenant":"C","Batch":1,"Name":"R4"}]}"#;
    std::fs::write(directory.0.join("preview-input.json"), input).unwrap();
    std::fs::write(directory.0.join("preview-project.json"), &authored).unwrap();
    app.preview_draft = Some(crate::preview::PreviewDraft {
        target: crate::preview::PreviewTarget::Primary,
        input_identity: "input.json".into(),
        output_identity: output.display().to_string(),
        input_text: input.into(),
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
    std::fs::write(
        directory.0.join("preview-status.txt"),
        format!(
            "status={} diagnostics={:?}\n",
            app.status,
            app.diagnostics.items()
        ),
    )
    .unwrap();
    assert!(app.pending_preview.is_none());
    let report = app.run_report.as_mut().expect("composite Preview result");
    let crate::run_report::OutputPreview::Text { content, .. } = report.report.outputs[0].preview()
    else {
        panic!("JSON Preview");
    };
    std::fs::write(
        directory.0.join("preview-original.json"),
        content.as_bytes(),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(content).unwrap(),
        serde_json::json!({"Rows":[{"Code":"L1","Name":"R1","Tuple":1},{"Code":"L2","Name":"R3","Tuple":2},{"Code":"L3","Name":"R1","Tuple":3}]})
    );
    assert_eq!(state(&app), authored);
    assert_eq!(composite_key_names(&app), keys);
    assert!(!output.exists());
    assert!(app.document.saved_path().is_none());
}
