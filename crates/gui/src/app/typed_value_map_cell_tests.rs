use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, NamedTarget};

const FUNCTION: FunctionId = FunctionId::new(7);
const DOCUMENTS: [MappingDocument; 3] = [
    MappingDocument::Main,
    MappingDocument::Target(0),
    MappingDocument::Function(FUNCTION),
];
const INPUT: &str = r#"{"Value":"unused"}"#;

struct Retained {
    path: PathBuf,
    next: std::cell::Cell<u64>,
    complete: bool,
}
impl Retained {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-typed-value-map-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self {
            path,
            next: std::cell::Cell::new(0),
            complete: false,
        }
    }
    fn record(&self, name: &str, value: impl std::fmt::Debug) {
        let next = self.next.get();
        self.next.set(next + 1);
        std::fs::write(
            self.path.join(format!("{next:04}-{name}.txt")),
            format!("{value:#?}\n"),
        )
        .unwrap();
    }
}
impl Drop for Retained {
    fn drop(&mut self) {
        if self.complete
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                != Some(std::ffi::OsStr::new("1"))
        {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "Retained typed Value map originals: {}",
                self.path.display()
            );
        }
    }
}
fn graph(input: Value, table: Vec<(Value, Value)>, default: Option<Value>) -> Graph {
    Graph {
        nodes: [
            (0, Node::Const { value: input }),
            (
                1,
                Node::ValueMap {
                    input: 0,
                    input_type: None,
                    table,
                    default,
                },
            ),
        ]
        .into_iter()
        .collect(),
    }
}
fn fixture(
    input: Value,
    table: Vec<(Value, Value)>,
    default: Option<Value>,
    ty: ScalarType,
) -> FerruleApp {
    let mut project = blank_project();
    project.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Result", ty),
            SchemaNode::scalar("Function", ty),
        ],
    );
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project.graph = graph(input.clone(), table.clone(), default.clone());
    project.graph.nodes.insert(
        2,
        Node::UserFunctionCall {
            function: FUNCTION,
            args: Vec::new(),
        },
    );
    project.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::String("unrelated".into()),
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
        path: None,
        schema: project.target.clone(),
        options: project.target_options.clone(),
        root: project.root.clone(),
    });
    project.user_functions.insert(
        FUNCTION,
        UserFunction {
            library: "local".into(),
            name: "translate".into(),
            description: None,
            parameters: Vec::new(),
            output_name: "result".into(),
            output_type: ty,
            body: graph(input, table, default),
            output: 1,
        },
    );
    let mut app = FerruleApp {
        project,
        ..FerruleApp::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    app.open_function_tab(FUNCTION);
    assert!(app.ensure_function_canvas(FUNCTION));
    app.mapping_workspace.active = MappingDocument::Main;
    app
}
fn selected_graph(app: &FerruleApp, document: MappingDocument) -> &Graph {
    match document {
        MappingDocument::Main | MappingDocument::Target(_) => &app.project.graph,
        MappingDocument::Function(id) => &app.project.user_functions[&id].body,
    }
}
fn selected_graph_mut(app: &mut FerruleApp, document: MappingDocument) -> &mut Graph {
    match document {
        MappingDocument::Main | MappingDocument::Target(_) => &mut app.project.graph,
        MappingDocument::Function(id) => &mut app.project.user_functions.get_mut(&id).unwrap().body,
    }
}
fn cells(app: &FerruleApp, document: MappingDocument) -> (Vec<(Value, Value)>, Option<Value>) {
    let Node::ValueMap { table, default, .. } = &selected_graph(app, document).nodes[&1] else {
        panic!("Value map identity")
    };
    (table.clone(), default.clone())
}
fn float_bits(app: &FerruleApp, document: MappingDocument) -> Vec<u64> {
    let (table, default) = cells(app, document);
    table
        .into_iter()
        .flat_map(|(from, to)| [from, to])
        .chain(default)
        .filter_map(|value| match value {
            Value::Float(number) => Some(number.to_bits()),
            _ => None,
        })
        .collect()
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
fn layout(app: &FerruleApp) -> Vec<String> {
    DOCUMENTS
        .into_iter()
        .map(|document| {
            let snarl = &canvas(app, document).snarl;
            let mut wires = snarl
                .wires()
                .map(|(from, to)| {
                    (
                        snarl[from.node],
                        from.node.0,
                        from.output,
                        snarl[to.node],
                        to.node.0,
                        to.input,
                    )
                })
                .collect::<Vec<_>>();
            wires.sort_unstable();
            let mut nodes = snarl
                .node_ids()
                .map(|(id, node)| (*node, id.0))
                .collect::<Vec<_>>();
            nodes.sort_unstable();
            format!(
                "{document:?}: nodes={nodes:?}; wires={wires:?}; positions={:?}",
                CanvasLayout::capture_nodes(snarl)
            )
        })
        .collect()
}
fn original(app: &FerruleApp, retained: &Retained, label: &str) -> serde_json::Value {
    retained.record(
        label,
        (
            &app.project,
            CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace),
            layout(app),
            app.is_dirty(),
            app.can_undo(),
            app.history.can_redo(),
        ),
    );
    let encoded = serde_json::to_value(&app.project);
    retained.record("complete-project-codec-result", &encoded);
    encoded.unwrap()
}
fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
    editing: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    retained.record("actual-input-events", &events);
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
        |ui| {
            app.handle_history_shortcuts(ui.ctx(), editing);
            app.show_mapping_workspace_canvas(ui, editing);
        },
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    retained.record(
        "full-widget-shapes-events-accessibility-scale",
        (
            &output.shapes,
            &output.platform_output.events,
            &output.platform_output.accesskit_update,
            output.pixels_per_point,
        ),
    );
    output
}
fn settle(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
    editing: bool,
) -> egui::FullOutput {
    let mut output = frame(app, context, retained, editing, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, retained, editing, Vec::new());
    }
    output
}
fn setup(
    retained: &Retained,
    input: Value,
    table: Vec<(Value, Value)>,
    default: Option<Value>,
    ty: ScalarType,
) -> (FerruleApp, egui::Context) {
    let mut app = fixture(input, table, default, ty);
    let context = egui::Context::default();
    context.enable_accesskit();
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
                CanvasNode::SourceBlock(_) => egui::pos2(0.0, 350.0),
                CanvasNode::TargetBlock(_) => egui::pos2(850.0, 0.0),
                CanvasNode::Graph(0) => egui::pos2(0.0, 0.0),
                CanvasNode::Graph(1) if matches!(document, MappingDocument::Function(_)) => {
                    egui::pos2(150.0, 0.0)
                }
                CanvasNode::Graph(1) => egui::pos2(350.0, 0.0),
                CanvasNode::Graph(2) => egui::pos2(350.0, 300.0),
                _ => egui::pos2(0.0, 650.0),
            };
        }
        state.reset_view();
        settle(&mut app, &context, retained, true);
    }
    app.mapping_workspace.active = MappingDocument::Main;
    settle(&mut app, &context, retained, true);
    app.mark_clean();
    app.rebase_history();
    (app, context)
}
fn visible(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    fn visit(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        label: &str,
        out: &mut Vec<egui::Pos2>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text)
                if text.galley.text() == label
                    && clip.contains_rect(text.visual_bounding_rect()) =>
            {
                out.push(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, label, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, label, &mut out);
    }
    out
}
fn rect(node: &egui::accesskit::Node) -> egui::Rect {
    let bounds = node.bounds().unwrap();
    egui::Rect::from_min_max(
        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
    )
}
fn control(
    output: &egui::FullOutput,
    role: egui::accesskit::Role,
    label: &str,
    enabled: bool,
) -> egui::Pos2 {
    let update = output.platform_output.accesskit_update.as_ref().unwrap();
    let nodes = update
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            (node.role() == role && node.label() == Some(label)).then_some(node)
        })
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1, "one actual labelled {role:?}: {label}");
    let node = nodes[0];
    assert_eq!(!node.is_disabled(), enabled);
    let bounds = rect(node);
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))
            .contains_rect(bounds),
        "control fully visible: {label} {bounds:?}"
    );
    bounds.center()
}
fn key_event(key: egui::Key, modifiers: egui::Modifiers, pressed: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    }
}
fn click(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
    editing: bool,
    point: egui::Pos2,
) {
    frame(
        app,
        context,
        retained,
        editing,
        vec![egui::Event::PointerMoved(point)],
    );
    for pressed in [true, false] {
        frame(
            app,
            context,
            retained,
            editing,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    settle(app, context, retained, editing);
}
fn canvas_salt(app: &FerruleApp) -> egui::Id {
    app.embedded_canvas_id(match app.mapping_workspace.active {
        MappingDocument::Main => egui::Id::new("main_mapping_canvas"),
        MappingDocument::Target(i) => egui::Id::new(("named_target_canvas", i)),
        MappingDocument::Function(id) => egui::Id::new(("function_mapping_canvas", id.get())),
    })
}
fn bounds(app: &FerruleApp, context: &egui::Context) -> egui::Rect {
    let state = canvas(app, app.mapping_workspace.active);
    let node = state
        .snarl
        .node_ids()
        .find_map(|(id, node)| (*node == CanvasNode::Graph(1)).then_some(id))
        .unwrap();
    let ids = crate::canvas_keyboard::current_pin_interaction_ids(
        context,
        canvas_salt(app),
        state.view_generation,
    );
    assert!(!ids.is_empty(), "real warmed pin responses");
    let response = ids
        .into_iter()
        .find_map(|id| {
            context.read_response(id).and_then(|pin| {
                context.read_response(pin.layer_id.id.with(("snarl-node", node)).with("frame"))
            })
        })
        .expect("actual Value map frame");
    context
        .layer_transform_to_global(response.layer_id)
        .unwrap_or_default()
        * response.rect
}
fn open(app: &mut FerruleApp, context: &egui::Context, retained: &Retained) {
    let output = settle(app, context, retained, true);
    let bounds = bounds(app, context);
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))
            .contains_rect(bounds)
    );
    let points = visible(&output, &char::from(lucide_icons::Icon::Pencil).to_string())
        .into_iter()
        .filter(|point| bounds.contains(*point))
        .collect::<Vec<_>>();
    assert_eq!(points.len(), 1, "actual node pencil");
    click(app, context, retained, true, points[0]);
    let output = settle(app, context, retained, true);
    assert_eq!(visible(&output, "Input conversion").len(), 1);
}
fn close(app: &mut FerruleApp, context: &egui::Context, retained: &Retained) {
    frame(
        app,
        context,
        retained,
        true,
        vec![
            key_event(egui::Key::Escape, egui::Modifiers::NONE, true),
            key_event(egui::Key::Escape, egui::Modifiers::NONE, false),
        ],
    );
    settle(app, context, retained, true);
}
fn choose_kind(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
    label: &str,
    kind: &str,
) {
    let output = settle(app, context, retained, true);
    let point = control(
        &output,
        egui::accesskit::Role::ComboBox,
        &format!("{label} type"),
        true,
    );
    click(app, context, retained, true, point);
    let output = settle(app, context, retained, true);
    assert!(egui::Popup::is_any_open(context));
    let point = control(&output, egui::accesskit::Role::Button, kind, true);
    assert!(visible(&output, kind).iter().any(|p| {
        rect(
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find_map(|(_, n)| {
                    (n.role() == egui::accesskit::Role::Button && n.label() == Some(kind))
                        .then_some(n)
                })
                .unwrap(),
        )
        .contains(*p)
    }));
    click(app, context, retained, true, point);
}
fn enter_text(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
    label: &str,
    text: &str,
) {
    let output = settle(app, context, retained, true);
    let point = control(
        &output,
        egui::accesskit::Role::TextInput,
        &format!("{label} value"),
        true,
    );
    click(app, context, retained, true, point);
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    frame(
        app,
        context,
        retained,
        true,
        vec![
            key_event(egui::Key::A, modifiers, true),
            key_event(egui::Key::A, modifiers, false),
        ],
    );
    frame(
        app,
        context,
        retained,
        true,
        vec![egui::Event::Text(text.into())],
    );
    settle(app, context, retained, true);
}
fn apply(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
    label: &str,
    enter: bool,
) {
    if enter {
        frame(
            app,
            context,
            retained,
            true,
            vec![
                key_event(egui::Key::Enter, egui::Modifiers::NONE, true),
                key_event(egui::Key::Enter, egui::Modifiers::NONE, false),
            ],
        );
        settle(app, context, retained, true);
    } else {
        let output = settle(app, context, retained, true);
        let point = control(
            &output,
            egui::accesskit::Role::Button,
            &format!("Apply {label}"),
            true,
        );
        click(app, context, retained, true, point);
    }
}
fn checkbox(app: &mut FerruleApp, context: &egui::Context, retained: &Retained) {
    let output = settle(app, context, retained, true);
    let point = control(&output, egui::accesskit::Role::CheckBox, "Default", true);
    click(app, context, retained, true, point);
}
fn conversion_string(app: &mut FerruleApp, context: &egui::Context, retained: &Retained) {
    let output = settle(app, context, retained, true);
    let update = output.platform_output.accesskit_update.as_ref().unwrap();
    let label = update
        .nodes
        .iter()
        .find_map(|(id, n)| {
            (n.role() == egui::accesskit::Role::Label && n.value() == Some("Input conversion"))
                .then_some(*id)
        })
        .unwrap();
    let nodes = update
        .nodes
        .iter()
        .filter_map(|(_, n)| {
            (n.role() == egui::accesskit::Role::ComboBox && n.labelled_by().contains(&label))
                .then_some(n)
        })
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    click(app, context, retained, true, rect(nodes[0]).center());
    let output = settle(app, context, retained, true);
    let point = control(&output, egui::accesskit::Role::Button, "string", true);
    click(app, context, retained, true, point);
}
fn expected(result: Value, function: Value) -> Instance {
    Instance::Group(
        vec![
            ("Result".into(), Instance::Scalar(result)),
            ("Function".into(), Instance::Scalar(function)),
        ]
        .into(),
    )
}
fn outcomes(
    app: &FerruleApp,
    retained: &Retained,
    result: Value,
    function: Value,
    primary: &str,
) -> anyhow::Result<()> {
    let parsed = format_json::from_str(INPUT, &app.project.source);
    retained.record("full-parse-original", &parsed);
    let input = parsed?;
    let validation = engine::validate(&app.project);
    let actual = engine::run_outputs(&app.project, &input);
    retained.record(
        "full-engine-original",
        (&app.project, &input, &validation, &actual),
    );
    assert!(validation.is_empty(), "{validation:?}");
    let actual = actual?;
    let written = format_json::to_string(&app.project.target, &actual.primary);
    let named = format_json::to_string(
        &app.project.extra_targets[0].schema,
        &actual.extras[0].instance,
    );
    retained.record("complete-writer-results", (&written, &named));
    let expected = expected(result, function);
    assert_eq!(actual.primary, expected);
    assert_eq!(
        actual.extras,
        vec![engine::NamedOutput {
            name: "audit".into(),
            instance: expected
        }]
    );
    assert_eq!(written?, primary);
    assert_eq!(named?, primary);
    Ok(())
}
fn pair(document: MappingDocument, selected: Value, untouched: Value) -> (Value, Value) {
    match document {
        MappingDocument::Main | MappingDocument::Target(_) => (selected, untouched),
        MappingDocument::Function(_) => (untouched, selected),
    }
}
fn history_key(app: &mut FerruleApp, context: &egui::Context, retained: &Retained, redo: bool) {
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        shift: redo,
        ..Default::default()
    };
    frame(
        app,
        context,
        retained,
        true,
        vec![
            key_event(egui::Key::Z, modifiers, true),
            key_event(egui::Key::Z, modifiers, false),
        ],
    );
    settle(app, context, retained, true);
}

#[test]
fn typed_value_map_real_key_and_value_commits_all_four_types_keep_warm_three_canvas_identity()
-> anyhow::Result<()> {
    let mut retained = Retained::new();
    for (kind, input, output, ty, input_text, value_text, edited_main, edited_function) in [
        (
            "string",
            Value::String("配送".into()),
            Value::String("kept 雪".into()),
            ScalarType::String,
            "配送",
            "kept 雪",
            "{\n  \"Result\": \"kept 雪\"\n}\n",
            "{\n  \"Function\": \"kept 雪\"\n}\n",
        ),
        (
            "int",
            Value::Int(7),
            Value::Int(-9),
            ScalarType::Int,
            "7",
            "-9",
            "{\n  \"Result\": -9\n}\n",
            "{\n  \"Function\": -9\n}\n",
        ),
        (
            "float",
            Value::Float(2.5),
            Value::Float(1.25),
            ScalarType::Float,
            "2.5",
            "1.25",
            "{\n  \"Result\": 1.25\n}\n",
            "{\n  \"Function\": 1.25\n}\n",
        ),
        (
            "bool",
            Value::Bool(true),
            Value::Bool(false),
            ScalarType::Bool,
            "true",
            "false",
            "{\n  \"Result\": false\n}\n",
            "{\n  \"Function\": false\n}\n",
        ),
    ] {
        for document in DOCUMENTS {
            let (mut app, context) = setup(
                &retained,
                input.clone(),
                vec![(
                    Value::String("not-matched".into()),
                    Value::String("uncommitted".into()),
                )],
                None,
                ty,
            );
            app.mapping_workspace.active = document;
            settle(&mut app, &context, &retained, true);
            let baseline = layout(&app);
            let frame_before = bounds(&app, &context);
            let pin_before = crate::canvas_keyboard::current_pin_interaction_ids(
                &context,
                canvas_salt(&app),
                canvas(&app, document).view_generation,
            );
            open(&mut app, &context, &retained);
            app.mark_clean();
            app.rebase_history();
            let before = cells(&app, document);
            choose_kind(&mut app, &context, &retained, "Entry 1 key", kind);
            enter_text(&mut app, &context, &retained, "Entry 1 key", input_text);
            retained.record(
                "complete-key-draft-or-live-text-original",
                (&app.project, cells(&app, document)),
            );
            if kind == "string" {
                assert_eq!(
                    cells(&app, document),
                    (
                        vec![(input.clone(), Value::String("uncommitted".into()))],
                        None
                    )
                );
                assert!(app.is_dirty() && app.can_undo());
            } else {
                assert_eq!(cells(&app, document), before);
                assert!(!app.is_dirty() && !app.can_undo());
            }
            apply(&mut app, &context, &retained, "Entry 1 key", true);
            choose_kind(&mut app, &context, &retained, "Entry 1 value", kind);
            enter_text(&mut app, &context, &retained, "Entry 1 value", value_text);
            let before_value = cells(&app, document);
            apply(&mut app, &context, &retained, "Entry 1 value", false);
            retained.record("full-authored-table-original", cells(&app, document));
            assert_eq!(
                cells(&app, document),
                (vec![(input.clone(), output.clone())], None)
            );
            if kind == "string" {
                assert_eq!(cells(&app, document), before_value);
            } else {
                assert_ne!(cells(&app, document), before_value);
            }
            close(&mut app, &context, &retained);
            assert_eq!(layout(&app), baseline);
            assert_eq!(bounds(&app, &context), frame_before);
            assert_eq!(
                crate::canvas_keyboard::current_pin_interaction_ids(
                    &context,
                    canvas_salt(&app),
                    canvas(&app, document).view_generation
                ),
                pin_before
            );
            let (result, function) = pair(document, output.clone(), Value::Null);
            outcomes(
                &app,
                &retained,
                result,
                function,
                if matches!(document, MappingDocument::Function(_)) {
                    edited_function
                } else {
                    edited_main
                },
            )?;
        }
    }
    retained.complete = true;
    Ok(())
}

#[test]
fn typed_value_map_invalid_drafts_imported_markers_and_locked_actual_controls_never_commit()
-> anyhow::Result<()> {
    let mut retained = Retained::new();
    for document in DOCUMENTS {
        let table = vec![
            (Value::Int(7), Value::String("first".into())),
            (Value::Int(7), Value::String("second".into())),
            (Value::json_null(), Value::xml_nil()),
            (Value::Float(-0.0), Value::Bool(true)),
        ];
        let (mut app, context) = setup(
            &retained,
            Value::Int(7),
            table,
            Some(Value::xml_nil()),
            ScalarType::String,
        );
        app.mapping_workspace.active = document;
        open(&mut app, &context, &retained);
        app.mark_clean();
        app.rebase_history();
        let original_cells = cells(&app, document);
        let original_bits = float_bits(&app, document);
        retained.record("full-imported-float-bits", &original_bits);
        let bytes = original(&app, &retained, "passive-imported-original");
        settle(&mut app, &context, &retained, true);
        assert_eq!(original(&app, &retained, "passive-imported-after"), bytes);
        assert_eq!(cells(&app, document), original_cells);
        for (kind, text) in [
            ("int", "12x"),
            ("int", "9223372036854775808"),
            ("float", "NaN"),
            ("float", "inf"),
            ("float", "1e999"),
            ("bool", "truth"),
        ] {
            choose_kind(&mut app, &context, &retained, "Entry 1 key", kind);
            enter_text(&mut app, &context, &retained, "Entry 1 key", text);
            let output = settle(&mut app, &context, &retained, true);
            let point = control(
                &output,
                egui::accesskit::Role::Button,
                "Apply Entry 1 key",
                false,
            );
            apply(&mut app, &context, &retained, "Entry 1 key", true);
            assert_eq!(cells(&app, document), original_cells);
            click(&mut app, &context, &retained, true, point);
            assert_eq!(cells(&app, document), original_cells);
            assert_eq!(
                original(&app, &retained, "full-invalid-draft-original"),
                bytes
            );
            assert!(!app.is_dirty() && !app.can_undo());
        }
        choose_kind(&mut app, &context, &retained, "Entry 1 key", "int");
        enter_text(&mut app, &context, &retained, "Entry 1 key", "7");
        apply(&mut app, &context, &retained, "Entry 1 key", false);
        assert_eq!(cells(&app, document), original_cells);
        conversion_string(&mut app, &context, &retained);
        assert_eq!(cells(&app, document), original_cells);
        assert_eq!(float_bits(&app, document), original_bits);
        choose_kind(&mut app, &context, &retained, "Entry 1 key", "string");
        enter_text(&mut app, &context, &retained, "Entry 1 key", "7");
        assert_eq!(cells(&app, document), original_cells);
        apply(&mut app, &context, &retained, "Entry 1 key", true);
        let mut replaced = original_cells.clone();
        replaced.0[0].0 = Value::String("7".into());
        assert_eq!(cells(&app, document), replaced);
        assert_eq!(float_bits(&app, document), original_bits);
        outcomes(
            &app,
            &retained,
            Value::String("first".into()),
            Value::String("first".into()),
            "{\n  \"Result\": \"first\",\n  \"Function\": \"first\"\n}\n",
        )?;
        close(&mut app, &context, &retained);
        open(&mut app, &context, &retained);
        app.mark_clean();
        app.rebase_history();
        let before = original(&app, &retained, "locked-full-original");
        let before_layout = layout(&app);
        if matches!(document, MappingDocument::Function(_)) {
            app.mapping_workspace.active = MappingDocument::Main;
        }
        app.begin_preview();
        app.mapping_workspace.active = document;
        retained.record(
            "full-preview-lock-original",
            (&app.preview_draft, &app.status),
        );
        assert!(app.preview_draft.is_some());
        assert_eq!(app.mapping_workspace.active, document);
        assert!(!app.ui_project_editing_enabled());
        let output = settle(&mut app, &context, &retained, false);
        for (role, label) in [
            (egui::accesskit::Role::ComboBox, "Entry 1 key type"),
            (egui::accesskit::Role::TextInput, "Entry 1 key value"),
            (egui::accesskit::Role::Button, "Apply Entry 1 key"),
        ] {
            let point = control(&output, role, label, false);
            click(&mut app, &context, &retained, false, point);
        }
        assert_eq!(
            original(&app, &retained, "locked-after-real-clicks"),
            before
        );
        assert_eq!(cells(&app, document), replaced);
        assert_eq!(float_bits(&app, document), original_bits);
        assert_eq!(layout(&app), before_layout);
        assert!(!app.is_dirty() && !app.can_undo());
    }
    retained.complete = true;
    Ok(())
}

#[test]
fn typed_value_map_real_default_and_absent_keys_keep_none_null_empty_and_ordered_first_match()
-> anyhow::Result<()> {
    let mut retained = Retained::new();
    for document in DOCUMENTS {
        let (mut app, context) = setup(
            &retained,
            Value::String("missing".into()),
            vec![
                (Value::String("other".into()), Value::String("first".into())),
                (
                    Value::String("other".into()),
                    Value::String("second".into()),
                ),
            ],
            None,
            ScalarType::String,
        );
        app.mapping_workspace.active = document;
        open(&mut app, &context, &retained);
        outcomes(&app, &retained, Value::Null, Value::Null, "{}\n")?;
        checkbox(&mut app, &context, &retained);
        assert_eq!(cells(&app, document).1, Some(Value::String(String::new())));
        let (result, function) = pair(document, Value::String(String::new()), Value::Null);
        outcomes(
            &app,
            &retained,
            result,
            function,
            if matches!(document, MappingDocument::Function(_)) {
                "{\n  \"Function\": \"\"\n}\n"
            } else {
                "{\n  \"Result\": \"\"\n}\n"
            },
        )?;
        choose_kind(&mut app, &context, &retained, "Default", "absent");
        apply(&mut app, &context, &retained, "Default", false);
        assert_eq!(cells(&app, document).1, Some(Value::Null));
        outcomes(&app, &retained, Value::Null, Value::Null, "{}\n")?;
        checkbox(&mut app, &context, &retained);
        assert_eq!(cells(&app, document).1, None);
        checkbox(&mut app, &context, &retained);
        assert_eq!(cells(&app, document).1, Some(Value::String(String::new())));
        let output = settle(&mut app, &context, &retained, true);
        control(
            &output,
            egui::accesskit::Role::TextInput,
            "Default value",
            true,
        );
        enter_text(&mut app, &context, &retained, "Default", "fallback");
        apply(&mut app, &context, &retained, "Default", true);
        assert_eq!(
            cells(&app, document).1,
            Some(Value::String("fallback".into()))
        );
        choose_kind(&mut app, &context, &retained, "Entry 1 key", "absent");
        apply(&mut app, &context, &retained, "Entry 1 key", false);
        let Node::Const { value } = selected_graph_mut(&mut app, document)
            .nodes
            .get_mut(&0)
            .unwrap()
        else {
            panic!("input")
        };
        *value = Value::Null;
        let (result, function) = pair(document, Value::String("first".into()), Value::Null);
        outcomes(
            &app,
            &retained,
            result,
            function,
            if matches!(document, MappingDocument::Function(_)) {
                "{\n  \"Function\": \"first\"\n}\n"
            } else {
                "{\n  \"Result\": \"first\"\n}\n"
            },
        )?;
        choose_kind(&mut app, &context, &retained, "Entry 2 key", "absent");
        apply(&mut app, &context, &retained, "Entry 2 key", false);
        assert_eq!(
            cells(&app, document).0,
            vec![
                (Value::Null, Value::String("first".into())),
                (Value::Null, Value::String("second".into()))
            ]
        );
        let (result, function) = pair(document, Value::String("first".into()), Value::Null);
        outcomes(
            &app,
            &retained,
            result,
            function,
            if matches!(document, MappingDocument::Function(_)) {
                "{\n  \"Function\": \"first\"\n}\n"
            } else {
                "{\n  \"Result\": \"first\"\n}\n"
            },
        )?;
    }
    retained.complete = true;
    Ok(())
}

#[test]
fn typed_value_map_committed_key_edit_real_history_save_reopen_and_preview_keep_full_originals()
-> anyhow::Result<()> {
    let mut retained = Retained::new();
    for (index, document) in DOCUMENTS.into_iter().enumerate() {
        let (mut app, context) = setup(
            &retained,
            Value::Int(7),
            vec![(
                Value::String("7".into()),
                Value::String("typed match".into()),
            )],
            Some(Value::String("fallback".into())),
            ScalarType::String,
        );
        app.mapping_workspace.active = document;
        open(&mut app, &context, &retained);
        app.mark_clean();
        app.rebase_history();
        let before = original(&app, &retained, "history-before");
        let before_layout = layout(&app);
        choose_kind(&mut app, &context, &retained, "Entry 1 key", "int");
        enter_text(&mut app, &context, &retained, "Entry 1 key", "7");
        assert_eq!(original(&app, &retained, "staged-key-before-apply"), before);
        assert!(!app.is_dirty() && !app.can_undo());
        apply(&mut app, &context, &retained, "Entry 1 key", true);
        close(&mut app, &context, &retained);
        let after = original(&app, &retained, "history-after");
        assert_eq!(app.history.undo_len(), 1);
        assert!(app.is_dirty());
        let (result, function) = pair(
            document,
            Value::String("typed match".into()),
            Value::String("fallback".into()),
        );
        let literal = if matches!(document, MappingDocument::Function(_)) {
            "{\n  \"Result\": \"fallback\",\n  \"Function\": \"typed match\"\n}\n"
        } else {
            "{\n  \"Result\": \"typed match\",\n  \"Function\": \"fallback\"\n}\n"
        };
        outcomes(&app, &retained, result.clone(), function.clone(), literal)?;
        history_key(&mut app, &context, &retained, false);
        assert_eq!(original(&app, &retained, "after-real-undo"), before);
        assert!(!app.is_dirty());
        outcomes(
            &app,
            &retained,
            Value::String("fallback".into()),
            Value::String("fallback".into()),
            "{\n  \"Result\": \"fallback\",\n  \"Function\": \"fallback\"\n}\n",
        )?;
        history_key(&mut app, &context, &retained, true);
        assert_eq!(original(&app, &retained, "after-real-redo"), after);
        assert_eq!(layout(&app), before_layout);
        outcomes(&app, &retained, result.clone(), function.clone(), literal)?;
        let path = retained.path.join(format!("typed-{index}.json"));
        let saved = app.save_document_to(&path);
        match &saved {
            Ok(saved) => retained.record(
                "complete-save-result",
                (&saved.validation_issues, &saved.layout_warning),
            ),
            Err(error) => retained.record("complete-save-error", format!("{error:#?}\n{error:#}")),
        };
        let saved = saved?;
        assert!(saved.validation_issues.is_empty() && saved.layout_warning.is_none());
        let project_bytes = std::fs::read(&path)?;
        let layout_path = crate::layout_store::layout_path(&path);
        let layout_bytes = std::fs::read(&layout_path)?;
        retained.record(
            "full-saved-original-bodies",
            (&project_bytes, &layout_bytes),
        );
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&path);
        reopened.open_target_tab(0);
        assert!(reopened.ensure_target_canvas(0));
        reopened.open_function_tab(FUNCTION);
        assert!(reopened.ensure_function_canvas(FUNCTION));
        for document in DOCUMENTS {
            reopened.mapping_workspace.active = document;
            settle(&mut reopened, &context, &retained, true);
        }
        reopened.mapping_workspace.active = document;
        settle(&mut reopened, &context, &retained, true);
        assert_eq!(
            original(&reopened, &retained, "full-reopened-original"),
            after
        );
        assert_eq!(layout(&reopened), before_layout);
        outcomes(
            &reopened,
            &retained,
            result.clone(),
            function.clone(),
            literal,
        )?;
        let output_path = retained
            .path
            .join(format!("preview-must-not-write-{index}.json"));
        reopened.preview_draft = Some(crate::preview::PreviewDraft {
            target: crate::preview::PreviewTarget::Primary,
            input_identity: "input.json".into(),
            output_identity: output_path.display().to_string(),
            input_text: INPUT.into(),
            debug_breakpoint: None,
        });
        reopened.execute_preview();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while reopened.pending_preview.is_some() && std::time::Instant::now() < deadline {
            reopened.poll_preview(&context);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        retained.record(
            "full-preview-original",
            (
                &reopened.run_report,
                &reopened.status,
                reopened.diagnostics.items(),
            ),
        );
        assert!(reopened.pending_preview.is_none());
        assert!(!output_path.exists());
        let report = reopened.run_report.as_mut().expect("actual public Preview");
        assert_eq!(report.report.outputs.len(), 1);
        let crate::run_report::OutputPreview::Text { content, .. } =
            report.report.outputs[0].preview()
        else {
            panic!("JSON Preview")
        };
        retained.record("complete-preview-bytes", content.as_bytes());
        assert_eq!(content.as_str(), literal);
        assert_eq!(std::fs::read(&path)?, project_bytes);
        assert_eq!(std::fs::read(&layout_path)?, layout_bytes);
    }
    retained.complete = true;
    Ok(())
}

mod regressions;
