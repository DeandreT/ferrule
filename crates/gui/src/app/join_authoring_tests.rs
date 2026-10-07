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
