use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, Binding, NamedTarget, ScopeIteration};

const INPUT: &str = r#"{"Price":999,"Orders":[{"Price":123,"Separator":";","Pick":2,"Items":[{"Price":3,"Quantity":2,"Label":"A"},{"Price":5,"Quantity":4,"Label":"B"}]},{"Price":456,"Separator":"|","Pick":1,"Items":[{"Price":7,"Quantity":1,"Label":"C"}]},{"Price":789,"Separator":"!","Pick":1,"Items":[]}]}"#;

struct Retained {
    path: PathBuf,
    complete: bool,
}
impl Retained {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-calculated-aggregate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self {
            path,
            complete: false,
        }
    }
    fn record(&self, name: &str, value: impl std::fmt::Debug) {
        std::fs::write(self.path.join(name), format!("{value:#?}\n")).unwrap();
    }
}
impl Drop for Retained {
    fn drop(&mut self) {
        if self.complete
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!(
                "Retained successful aggregate originals: {}",
                self.path.display()
            );
        } else if self.complete {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "Retained aggregate failure originals: {}",
                self.path.display()
            );
        }
    }
}

fn field(path: &str, frame: &[&str]) -> Node {
    Node::SourceField {
        path: vec![path.into()],
        frame: Some(frame.iter().map(|s| (*s).into()).collect()),
    }
}
fn constant(value: Value) -> Node {
    Node::Const { value }
}
fn aggregate(function: AggregateOp, value: &str, arg: Option<NodeId>) -> Node {
    Node::Aggregate {
        function,
        collection: vec!["Items".into()],
        value: if value.is_empty() {
            Vec::new()
        } else {
            vec![value.into()]
        },
        expression: None,
        arg,
    }
}
fn app() -> FerruleApp {
    let mut project = blank_project();
    project.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Price", ScalarType::Int),
            SchemaNode::group(
                "Orders",
                vec![
                    SchemaNode::scalar("Price", ScalarType::Int),
                    SchemaNode::scalar("Separator", ScalarType::String),
                    SchemaNode::scalar("Pick", ScalarType::Int),
                    SchemaNode::group(
                        "Items",
                        vec![
                            SchemaNode::scalar("Price", ScalarType::Int),
                            SchemaNode::scalar("Quantity", ScalarType::Int),
                            SchemaNode::scalar("Label", ScalarType::String),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Row",
                vec![
                    SchemaNode::scalar("Sum", ScalarType::Int),
                    SchemaNode::scalar("Joined", ScalarType::String),
                    SchemaNode::scalar("Picked", ScalarType::String),
                    SchemaNode::scalar("Count", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    );
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project.graph.nodes = [
        (0, aggregate(AggregateOp::Sum, "Price", None)),
        (1, field("Price", &["Orders", "Items"])),
        (2, field("Quantity", &["Orders", "Items"])),
        (
            3,
            Node::Call {
                function: "multiply".into(),
                args: vec![1, 2],
            },
        ),
        (4, aggregate(AggregateOp::Join, "Label", Some(8))),
        (5, field("Separator", &["Orders"])),
        (6, constant(Value::String(String::new()))),
        (
            7,
            Node::Call {
                function: "concat".into(),
                args: vec![12, 13],
            },
        ),
        (
            8,
            Node::Call {
                function: "concat".into(),
                args: vec![5, 6],
            },
        ),
        (12, field("Label", &["Orders", "Items"])),
        (
            13,
            Node::Position {
                collection: vec!["Items".into()],
            },
        ),
        (14, aggregate(AggregateOp::ItemAt, "Label", Some(15))),
        (
            15,
            Node::Call {
                function: "add".into(),
                args: vec![16, 17],
            },
        ),
        (16, field("Pick", &["Orders"])),
        (17, constant(Value::Int(0))),
        (18, aggregate(AggregateOp::Count, "", None)),
    ]
    .into_iter()
    .collect();
    project.root = Scope {
        children: vec![Scope {
            target_field: "Row".into(),
            iteration: ScopeIteration::Source(vec!["Orders".into()]),
            bindings: [("Sum", 0), ("Joined", 4), ("Picked", 14), ("Count", 18)]
                .into_iter()
                .map(|(name, node)| Binding {
                    target_field: name.into(),
                    node,
                })
                .collect(),
            ..Scope::default()
        }],
        ..Scope::default()
    };
    for name in ["audit", "copy"] {
        project.extra_targets.push(NamedTarget {
            name: name.into(),
            path: None,
            schema: project.target.clone(),
            options: project.target_options.clone(),
            root: project.root.clone(),
        });
    }
    let mut app = FerruleApp {
        project,
        ..FerruleApp::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    for index in 0..2 {
        app.open_target_tab(index);
        assert!(app.ensure_target_canvas(index));
    }
    app.mapping_workspace.active = MappingDocument::Main;
    app
}
fn canvas(app: &FerruleApp, document: MappingDocument) -> &CanvasDocumentState {
    match document {
        MappingDocument::Main => &app.main_canvas,
        MappingDocument::Target(i) => &app.mapping_workspace.target_canvases[&i],
        MappingDocument::Function(_) => panic!("ordinary aggregate fixture"),
    }
}
fn canvas_mut(app: &mut FerruleApp, document: MappingDocument) -> &mut CanvasDocumentState {
    match document {
        MappingDocument::Main => &mut app.main_canvas,
        MappingDocument::Target(i) => app.mapping_workspace.target_canvases.get_mut(&i).unwrap(),
        MappingDocument::Function(_) => panic!("ordinary aggregate fixture"),
    }
}
const DOCUMENTS: [MappingDocument; 3] = [
    MappingDocument::Main,
    MappingDocument::Target(0),
    MappingDocument::Target(1),
];
fn positions(app: &FerruleApp) -> Vec<String> {
    DOCUMENTS
        .into_iter()
        .map(|document| {
            format!(
                "{:?}",
                canvas(app, document)
                    .snarl
                    .nodes_pos()
                    .map(|(position, node)| (format!("{node:?}"), format!("{position:?}")))
                    .collect::<std::collections::BTreeMap<_, _>>()
            )
        })
        .collect()
}
fn raw(app: &FerruleApp) -> serde_json::Value {
    let observed = serde_json::json!({"project": app.project,
        "positions": positions(app),
        "wires": DOCUMENTS.into_iter().map(|d| format!("{:?}", canvas(app,d).snarl.wires().collect::<Vec<_>>())).collect::<Vec<_>>(),
        "undo": app.history.undo_len(), "redo": app.history.redo_len(), "dirty": app.is_dirty(),
        "status": app.status, "diagnostics": format!("{:?}", app.diagnostics.items())});
    eprintln!("Calculated aggregate app originals={observed}");
    observed
}
fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    editing: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            time: Some(context.cumulative_frame_nr() as f64 / 10.0),
            events,
            ..Default::default()
        },
        |ui| app.show_mapping_workspace_canvas(ui, editing),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}
fn settle(app: &mut FerruleApp, context: &egui::Context, editing: bool) -> egui::FullOutput {
    let mut output = frame(app, context, editing, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, editing, Vec::new());
    }
    output
}
fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}
fn click(app: &mut FerruleApp, context: &egui::Context, editing: bool, pos: egui::Pos2) {
    frame(app, context, editing, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(app, context, editing, vec![pointer(pos, pressed)]);
    }
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
fn close(app: &mut FerruleApp, context: &egui::Context) {
    frame(app, context, true, vec![key(egui::Key::Escape)]);
    settle(app, context, true);
}
fn visible(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    fn visit(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        label: &str,
        found: &mut Vec<egui::Pos2>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                if clip.contains(text.visual_bounding_rect().center()) {
                    found.push(text.visual_bounding_rect().center());
                }
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, label, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, label, &mut found);
    }
    found
}
fn salt(app: &FerruleApp) -> egui::Id {
    app.embedded_canvas_id(match app.mapping_workspace.active {
        MappingDocument::Main => egui::Id::new("main_mapping_canvas"),
        MappingDocument::Target(i) => egui::Id::new(("named_target_canvas", i)),
        _ => panic!("mapping canvas"),
    })
}
fn responses(app: &FerruleApp, context: &egui::Context) -> Vec<egui::Response> {
    let ids = crate::canvas_keyboard::current_pin_interaction_ids(
        context,
        salt(app),
        canvas(app, app.mapping_workspace.active).view_generation,
    );
    assert!(!ids.is_empty(), "actual current native pins");
    ids.into_iter()
        .map(|id| {
            context
                .read_response(id)
                .expect("registered native pin response")
        })
        .collect()
}
fn bounds(app: &FerruleApp, context: &egui::Context, wanted: NodeId) -> egui::Rect {
    let snarl = &canvas(app, app.mapping_workspace.active).snarl;
    let id = snarl
        .node_ids()
        .find_map(|(id, node)| (*node == CanvasNode::Graph(wanted)).then_some(id))
        .unwrap();
    let response = responses(app, context)
        .iter()
        .find_map(|pin| {
            context.read_response(pin.layer_id.id.with(("snarl-node", id)).with("frame"))
        })
        .expect("actual node frame");
    context
        .layer_transform_to_global(response.layer_id)
        .unwrap_or_default()
        * response.rect
}
fn pin(
    app: &FerruleApp,
    context: &egui::Context,
    wanted: NodeId,
    output: bool,
    index: usize,
) -> egui::Pos2 {
    let bounds = bounds(app, context, wanted);
    let mut pins = responses(app, context)
        .into_iter()
        .filter_map(|response| {
            let center = context
                .layer_transform_to_global(response.layer_id)
                .unwrap_or_default()
                * response.rect.center();
            let edge = if output {
                bounds.right()
            } else {
                bounds.left()
            };
            (response.sense.senses_drag()
                && bounds.expand(16.0).contains(center)
                && (center.x - edge).abs() < 20.0)
                .then_some(center)
        })
        .collect::<Vec<_>>();
    pins.sort_by(|a, b| a.y.total_cmp(&b.y));
    eprintln!(
        "Aggregate actual pins node={wanted}, output={output}, bounds={bounds:?}, pins={pins:?}"
    );
    assert_eq!(
        pins.len(),
        if output {
            1
        } else {
            match &app.project.graph.nodes[&wanted] {
                Node::Aggregate {
                    expression, arg, ..
                } => usize::from(expression.is_some()) + usize::from(arg.is_some()),
                _ => panic!("aggregate input"),
            }
        }
    );
    let point = pins[index];
    assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0)).contains(point));
    point
}
fn wire_value(
    app: &mut FerruleApp,
    context: &egui::Context,
    aggregate: NodeId,
    expression: NodeId,
) {
    close(app, context);
    let from = pin(app, context, expression, true, 0);
    let to = pin(app, context, aggregate, false, 0);
    frame(app, context, true, vec![egui::Event::PointerMoved(from)]);
    frame(app, context, true, vec![pointer(from, true)]);
    frame(
        app,
        context,
        true,
        vec![egui::Event::PointerMoved(from.lerp(to, 0.5))],
    );
    frame(app, context, true, vec![egui::Event::PointerMoved(to)]);
    frame(app, context, true, vec![pointer(to, false)]);
    settle(app, context, true);
    raw(app);
    assert!(
        matches!(&app.project.graph.nodes[&aggregate],Node::Aggregate {expression:Some(id),..} if *id==expression)
    );
}
fn open(app: &mut FerruleApp, context: &egui::Context, node: NodeId) {
    let output = settle(app, context, true);
    let rect = bounds(app, context, node);
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    let candidates = visible(&output, &pencil)
        .into_iter()
        .filter(|p| rect.contains(*p))
        .collect::<Vec<_>>();
    eprintln!("Aggregate pencil node={node}, actual frame={rect:?}, candidates={candidates:?}");
    assert_eq!(candidates.len(), 1);
    click(app, context, true, candidates[0]);
    let output = settle(app, context, true);
    assert_eq!(visible(&output, "Calculate each value").len(), 1);
}
fn switch(app: &mut FerruleApp, context: &egui::Context, editing: bool) {
    let output = settle(app, context, editing);
    let candidates = visible(&output, "Calculate each value");
    assert_eq!(candidates.len(), 1);
    click(app, context, editing, candidates[0]);
    settle(app, context, editing);
    raw(app);
}
fn setup() -> (FerruleApp, egui::Context) {
    let mut app = app();
    let context = egui::Context::default();
    crate::icons::install(&context);
    for document in DOCUMENTS {
        app.mapping_workspace.active = document;
        let state = canvas_mut(&mut app, document);
        let nodes = state
            .snarl
            .node_ids()
            .map(|(id, node)| (id, *node))
            .collect::<Vec<_>>();
        for (id, node) in nodes {
            state.snarl.get_node_info_mut(id).unwrap().pos = match node {
                CanvasNode::SourceBlock(i) => egui::pos2(0.0, i as f32 * 170.0),
                CanvasNode::TargetBlock(i) => egui::pos2(1000.0, i as f32 * 170.0),
                CanvasNode::Graph(0) => egui::pos2(650.0, 0.0),
                CanvasNode::Graph(4) => egui::pos2(650.0, 180.0),
                CanvasNode::Graph(14) => egui::pos2(650.0, 360.0),
                CanvasNode::Graph(18) => egui::pos2(650.0, 540.0),
                CanvasNode::Graph(3) => egui::pos2(300.0, 0.0),
                CanvasNode::Graph(7) => egui::pos2(300.0, 180.0),
                CanvasNode::Graph(8) => egui::pos2(300.0, 350.0),
                CanvasNode::Graph(15) => egui::pos2(300.0, 540.0),
                _ => egui::pos2(300.0, 720.0),
            };
        }
        state.reset_view();
        settle(&mut app, &context, true);
    }
    app.mapping_workspace.active = MappingDocument::Main;
    settle(&mut app, &context, true);
    app.mark_clean();
    app.rebase_history();
    (app, context)
}
fn expected(calculated_sum: bool, calculated_labels: bool) -> Instance {
    let row = |sum, joined: &str, picked: Value, count| {
        Instance::Group(
            vec![
                ("Sum".into(), Instance::Scalar(Value::Int(sum))),
                (
                    "Joined".into(),
                    Instance::Scalar(Value::String(joined.into())),
                ),
                ("Picked".into(), Instance::Scalar(picked)),
                ("Count".into(), Instance::Scalar(Value::Int(count))),
            ]
            .into(),
        )
    };
    Instance::Group(
        vec![(
            "Row".into(),
            Instance::Repeated(vec![
                row(
                    if calculated_sum { 26 } else { 8 },
                    if calculated_labels { "A1;B2" } else { "A;B" },
                    Value::String(if calculated_labels { "B2" } else { "B" }.into()),
                    2,
                ),
                row(
                    7,
                    if calculated_labels { "C1" } else { "C" },
                    Value::String(if calculated_labels { "C1" } else { "C" }.into()),
                    1,
                ),
                row(0, "", Value::Null, 0),
            ]),
        )]
        .into(),
    )
}
fn outputs(app: &FerruleApp, retained: &Retained, name: &str, sum: bool, labels: bool) {
    let parsed = format_json::from_str(INPUT, &app.project.source);
    retained.record(&format!("{name}.input.txt"), (&app.project, INPUT, &parsed));
    let input = parsed.unwrap();
    let validation = engine::validate(&app.project);
    let result = engine::run_outputs(&app.project, &input);
    retained.record(name, (&app.project, &input, &validation, &result));
    assert!(validation.is_empty());
    let result = result.unwrap();
    let expected = expected(sum, labels);
    assert_eq!(result.primary, expected);
    assert_eq!(
        result.extras,
        vec![
            engine::NamedOutput {
                name: "audit".into(),
                instance: expected.clone()
            },
            engine::NamedOutput {
                name: "copy".into(),
                instance: expected
            }
        ]
    );
}
fn assert_edges(app: &FerruleApp, aggregate: NodeId, expression: Option<NodeId>, argument: NodeId) {
    for document in DOCUMENTS {
        let snarl = &canvas(app, document).snarl;
        let id = snarl
            .node_ids()
            .find_map(|(id, node)| (*node == CanvasNode::Graph(aggregate)).then_some(id))
            .unwrap();
        let incoming = snarl
            .wires()
            .filter(|(_, to)| to.node == id)
            .map(|(from, to)| (snarl[from.node], to.input))
            .collect::<Vec<_>>();
        eprintln!("Aggregate complete incoming {document:?} = {incoming:?}");
        let mut expected = vec![(
            CanvasNode::Graph(argument),
            usize::from(expression.is_some()),
        )];
        if let Some(expression) = expression {
            expected.push((CanvasNode::Graph(expression), 0));
        }
        assert_eq!(incoming.len(), expected.len());
        for edge in expected {
            assert!(incoming.contains(&edge));
        }
    }
}
fn visit_all(app: &mut FerruleApp, context: &egui::Context) {
    close(app, context);
    for document in DOCUMENTS {
        app.mapping_workspace.active = document;
        settle(app, context, true);
    }
    app.mapping_workspace.active = MappingDocument::Main;
    settle(app, context, true);
}

#[test]
fn calculated_sum_actual_pointer_wires_roundtrip_history_saved_project_and_unwritten_preview()
-> anyhow::Result<()> {
    let mut retained = Retained::new();
    std::fs::write(retained.path.join("input.json"), INPUT)?;
    let (mut app, context) = setup();
    outputs(&app, &retained, "field-original.txt", false, false);
    let before = serde_json::to_value(&app.project)?;
    let coordinates = positions(&app);
    open(&mut app, &context, 0);
    app.mark_clean();
    app.rebase_history();
    switch(&mut app, &context, true);
    let mode = serde_json::to_value(&app.project)?;
    let Node::Aggregate {
        expression: Some(empty),
        ..
    } = app.project.graph.nodes[&0]
    else {
        panic!("required calculated value")
    };
    assert!(matches!(app.project.graph.nodes[&empty], Node::Unconnected));
    wire_value(&mut app, &context, 0, 3);
    outputs(&app, &retained, "calculated-original.txt", true, false);
    assert!(!app.project.graph.nodes.contains_key(&empty));
    let wired = serde_json::to_value(&app.project)?;
    app.undo_project();
    raw(&app);
    assert_eq!(serde_json::to_value(&app.project)?, mode);
    app.undo_project();
    raw(&app);
    assert_eq!(serde_json::to_value(&app.project)?, before);
    assert!(!app.is_dirty());
    app.redo_project();
    app.redo_project();
    visit_all(&mut app, &context);
    assert_eq!(serde_json::to_value(&app.project)?, wired);
    assert_eq!(positions(&app), coordinates);
    outputs(&app, &retained, "redo-original.txt", true, false);
    let path = retained.path.join("project.json");
    let saved_result = app.save_document_to(&path);
    match &saved_result {
        Ok(saved) => retained.record(
            "save-result.txt",
            (&saved.validation_issues, &saved.layout_warning),
        ),
        Err(error) => retained.record("save-result.txt", format!("{error:#?}\n{error:#}")),
    }
    let saved = saved_result?;
    assert!(saved.validation_issues.is_empty());
    assert!(saved.layout_warning.is_none());
    let project_bytes = std::fs::read(&path)?;
    let layout_bytes = std::fs::read(crate::layout_store::layout_path(&path))?;
    retained.record("saved-originals.txt", (&project_bytes, &layout_bytes));
    let saved_project = serde_json::to_value(&app.project)?;
    let saved_coordinates = positions(&app);
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    for index in 0..2 {
        reopened.open_target_tab(index);
    }
    visit_all(&mut reopened, &context);
    retained.record("reopened-original.txt", raw(&reopened));
    assert_eq!(serde_json::to_value(&reopened.project)?, saved_project);
    assert_eq!(positions(&reopened), saved_coordinates);
    let logical_output = retained.path.join("must-not-write.json");
    reopened.preview_draft = Some(crate::preview::PreviewDraft {
        target: crate::preview::PreviewTarget::Primary,
        input_identity: "input.json".into(),
        output_identity: logical_output.display().to_string(),
        input_text: INPUT.into(),
        debug_breakpoint: None,
    });
    reopened.execute_preview();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while reopened.pending_preview.is_some() && std::time::Instant::now() < deadline {
        reopened.poll_preview(&context);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    retained.record("preview-original.txt", &reopened.run_report);
    assert!(reopened.pending_preview.is_none());
    assert!(!logical_output.exists());
    let report = reopened.run_report.as_mut().expect("real preview report");
    assert_eq!(report.report.outputs.len(), 1);
    let crate::run_report::OutputPreview::Text { content, .. } = report.report.outputs[0].preview()
    else {
        panic!("JSON preview")
    };
    std::fs::write(
        retained.path.join("complete-preview.json"),
        content.as_bytes(),
    )?;
    let actual: serde_json::Value = serde_json::from_str(content)?;
    assert_eq!(
        actual,
        serde_json::json!({"Row":[{"Sum":26,"Joined":"A;B","Picked":"B","Count":2},
        {"Sum":7,"Joined":"C","Picked":"C","Count":1},{"Sum":0,"Joined":"","Count":0}]})
    );
    assert_eq!(std::fs::read(&path)?, project_bytes);
    assert_eq!(
        std::fs::read(crate::layout_store::layout_path(&path))?,
        layout_bytes
    );
    retained.complete = true;
    Ok(())
}

#[test]
fn calculated_join_and_item_at_keep_parent_arguments_on_warmed_primary_and_named_canvases() {
    let mut retained = Retained::new();
    let (mut app, context) = setup();
    let coordinates = positions(&app);
    assert_edges(&app, 4, None, 8);
    assert_edges(&app, 14, None, 15);
    for aggregate in [4, 14] {
        app.mapping_workspace.active = MappingDocument::Target(0);
        settle(&mut app, &context, true);
        open(&mut app, &context, aggregate);
        switch(&mut app, &context, true);
        wire_value(&mut app, &context, aggregate, 7);
        visit_all(&mut app, &context);
    }
    assert_edges(&app, 4, Some(7), 8);
    assert_edges(&app, 14, Some(7), 15);
    outputs(
        &app,
        &retained,
        "calculated-labels-original.txt",
        false,
        true,
    );
    app.mapping_workspace.active = MappingDocument::Target(1);
    settle(&mut app, &context, true);
    open(&mut app, &context, 4);
    let before = raw(&app);
    switch(&mut app, &context, false);
    assert_eq!(
        raw(&app),
        before,
        "already-open locked editor cannot change mode or history"
    );
    switch(&mut app, &context, true);
    close(&mut app, &context);
    open(&mut app, &context, 14);
    switch(&mut app, &context, true);
    visit_all(&mut app, &context);
    assert_edges(&app, 4, None, 8);
    assert_edges(&app, 14, None, 15);
    assert!(matches!(&app.project.graph.nodes[&7],Node::Call{args,..} if args==&[12,13]));
    assert!(app.project.graph.nodes.contains_key(&12));
    assert!(app.project.graph.nodes.contains_key(&13));
    assert_eq!(positions(&app), coordinates);
    outputs(
        &app,
        &retained,
        "restored-labels-original.txt",
        false,
        false,
    );
    retained.complete = true;
}
