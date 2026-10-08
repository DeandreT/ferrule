use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, NamedTarget};

const FUNCTION: FunctionId = FunctionId::new(7);
const DOCUMENTS: [MappingDocument; 3] = [
    MappingDocument::Main,
    MappingDocument::Target(0),
    MappingDocument::Function(FUNCTION),
];
const INPUT: &str = r#"{"Value":"unused source value"}"#;

struct Retained {
    path: PathBuf,
    next: std::cell::Cell<u64>,
    complete: bool,
}
impl Retained {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-value-map-input-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
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
                "Retained Value map input originals: {}",
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
fn fixture(input: Value, table: Vec<(Value, Value)>, default: Option<Value>) -> FerruleApp {
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
            output_type: ScalarType::String,
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
fn mode(app: &FerruleApp, document: MappingDocument) -> Option<ScalarType> {
    let Node::ValueMap { input_type, .. } = &selected_graph(app, document).nodes[&1] else {
        panic!("Value map identity changed")
    };
    *input_type
}
fn label(mode: Option<ScalarType>) -> &'static str {
    match mode {
        None => "unchanged",
        Some(ScalarType::String) => "string",
        Some(ScalarType::Int) => "int",
        Some(ScalarType::Float) => "float",
        Some(ScalarType::Bool) => "bool",
    }
}
fn typed_values(app: &FerruleApp) -> Vec<String> {
    [
        &app.project.graph,
        &app.project.user_functions[&FUNCTION].body,
    ]
    .into_iter()
    .map(|graph| {
        let Node::ValueMap {
            input,
            table,
            default,
            ..
        } = &graph.nodes[&1]
        else {
            panic!("Value map")
        };
        format!("input={input};table={table:?};default={default:?}")
    })
    .collect()
}
fn project_except_mode(app: &FerruleApp, document: MappingDocument) -> serde_json::Value {
    let mut project = app.project.clone();
    let graph = match document {
        MappingDocument::Main | MappingDocument::Target(_) => &mut project.graph,
        MappingDocument::Function(id) => &mut project.user_functions.get_mut(&id).unwrap().body,
    };
    let Node::ValueMap { input_type, .. } = graph.nodes.get_mut(&1).unwrap() else {
        panic!("Value map")
    };
    *input_type = None;
    serde_json::to_value(project).unwrap()
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
                        format!("{:?}", snarl[from.node]),
                        from.output,
                        format!("{:?}", snarl[to.node]),
                        to.input,
                    )
                })
                .collect::<Vec<_>>();
            // Canvas rebuilds may change hash iteration order, not the edges.
            wires.sort();
            format!(
                "{document:?}: positions={:?};wires={:?}",
                snarl
                    .nodes_pos()
                    .map(|(pos, node)| (format!("{node:?}"), format!("{pos:?}")))
                    .collect::<std::collections::BTreeMap<_, _>>(),
                wires,
            )
        })
        .collect()
}
fn raw(app: &FerruleApp) -> serde_json::Value {
    serde_json::json!({"project":app.project,"typed_values":typed_values(app),"layout":layout(app),
        "active":format!("{:?}",app.mapping_workspace.active),"dirty":app.is_dirty(),
        "undo":app.history.undo_len(),"redo":app.history.redo_len(),"status":app.status})
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
    eprintln!(
        "Value map input frame: state={};paint={:?}",
        raw(app),
        texts(&output)
    );
    output
}
fn settle(app: &mut FerruleApp, context: &egui::Context, editing: bool) -> egui::FullOutput {
    let mut output = frame(app, context, editing, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, editing, Vec::new());
    }
    output
}
fn setup(
    input: Value,
    table: Vec<(Value, Value)>,
    default: Option<Value>,
) -> (FerruleApp, egui::Context) {
    let mut app = fixture(input, table, default);
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
                // Initial function fit uses the two node anchors at scale2;
                // leave space for the full output frame to the right.
                CanvasNode::Graph(1) if matches!(document, MappingDocument::Function(_)) => {
                    egui::pos2(150.0, 0.0)
                }
                CanvasNode::Graph(1) => egui::pos2(350.0, 0.0),
                CanvasNode::Graph(2) => egui::pos2(350.0, 300.0),
                _ => egui::pos2(0.0, 650.0),
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
fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
    fn visit(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        found: &mut Vec<(String, egui::Rect, egui::Rect)>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) => found.push((
                text.galley.text().to_owned(),
                text.visual_bounding_rect(),
                clip,
            )),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, &mut found);
    }
    found
}
fn visible(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    texts(output)
        .into_iter()
        .filter_map(|(text, rect, clip)| {
            (text == label && clip.contains(rect.center())).then_some(rect.center())
        })
        .collect()
}
// These controls live in foreground Popup Areas, so their AccessKit bounds
// and painted glyph positions share screen coordinates. Match the actual
// ComboBox and its associated label, never a table TextInput's equal text.
fn conversion_control(
    output: &egui::FullOutput,
    current: Option<ScalarType>,
    enabled: bool,
) -> Vec<egui::Pos2> {
    let update = output.platform_output.accesskit_update.as_ref().unwrap();
    let labels = update
        .nodes
        .iter()
        .filter_map(|(id, node)| {
            (node.role() == egui::accesskit::Role::Label
                && node.value() == Some("Input conversion"))
            .then_some(*id)
        })
        .collect::<Vec<_>>();
    eprintln!("Value map actual conversion label identities: {labels:?}");
    assert_eq!(labels.len(), 1, "one actual properties label");
    let controls = update
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            (node.role() == egui::accesskit::Role::ComboBox
                && node.value() == Some(label(current))
                && node.labelled_by().contains(&labels[0]))
            .then_some(node)
        })
        .collect::<Vec<_>>();
    eprintln!("Value map actual labelled conversion controls: {controls:?}");
    controls
        .into_iter()
        .map(|node| {
            assert_eq!(
                !node.is_disabled(),
                enabled,
                "actual conversion enabled state"
            );
            let bounds = node.bounds().unwrap();
            let rect = egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            );
            assert!(
                visible(output, label(current))
                    .iter()
                    .any(|point| rect.contains(*point)),
                "conversion value must be painted inside its actual ComboBox"
            );
            rect.center()
        })
        .collect()
}
fn conversion_options(output: &egui::FullOutput, option: &str) -> Vec<egui::Pos2> {
    let update = output.platform_output.accesskit_update.as_ref().unwrap();
    let options = update
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            (node.role() == egui::accesskit::Role::Button && node.label() == Some(option))
                .then_some(node)
        })
        .collect::<Vec<_>>();
    eprintln!("Value map actual selectable option {option}: {options:?}");
    options
        .into_iter()
        .map(|node| {
            assert!(!node.is_disabled(), "actual dropdown option enabled");
            let bounds = node.bounds().unwrap();
            let rect = egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            );
            assert!(
                visible(output, option)
                    .iter()
                    .any(|point| rect.contains(*point)),
                "option must be painted inside its actual selectable widget"
            );
            rect.center()
        })
        .collect()
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
fn click(app: &mut FerruleApp, context: &egui::Context, editing: bool, pos: egui::Pos2) {
    frame(app, context, editing, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            app,
            context,
            editing,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}
fn salt(app: &FerruleApp) -> egui::Id {
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
        salt(app),
        state.view_generation,
    );
    assert!(!ids.is_empty(), "current warmed canvas pin responses");
    let response = ids
        .into_iter()
        .find_map(|id| {
            context.read_response(id).and_then(|pin| {
                context.read_response(pin.layer_id.id.with(("snarl-node", node)).with("frame"))
            })
        })
        .expect("existing registered Value map frame");
    context
        .layer_transform_to_global(response.layer_id)
        .unwrap_or_default()
        * response.rect
}
fn open(app: &mut FerruleApp, context: &egui::Context) {
    let output = settle(app, context, true);
    let rect = bounds(app, context);
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0)).contains_rect(rect),
        "fixture Value map frame must be fully visible"
    );
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    let points = visible(&output, &pencil)
        .into_iter()
        .filter(|p| rect.contains(*p))
        .collect::<Vec<_>>();
    eprintln!("Value map actual pencil: frame={rect:?},points={points:?}");
    assert_eq!(points.len(), 1);
    click(app, context, true, points[0]);
    let output = settle(app, context, true);
    assert_eq!(visible(&output, "Input conversion").len(), 1);
    assert_eq!(
        visible(
            &output,
            "Edited keys are text. Conversion applies to the incoming value."
        )
        .len(),
        1
    );
}
fn choose(app: &mut FerruleApp, context: &egui::Context, candidate: Option<ScalarType>) {
    let document = app.mapping_workspace.active;
    let output = settle(app, context, true);
    let current = conversion_control(&output, mode(app, document), true);
    assert_eq!(current.len(), 1, "one actual conversion combo");
    click(app, context, true, current[0]);
    let output = settle(app, context, true);
    assert!(
        egui::Popup::is_any_open(context),
        "actual conversion dropdown opened"
    );
    for option in ["unchanged", "string", "int", "float", "bool"] {
        assert_eq!(
            conversion_options(&output, option).len(),
            1,
            "one actual dropdown option {option}"
        );
    }
    let options = conversion_options(&output, label(candidate));
    assert_eq!(options.len(), 1);
    click(app, context, true, options[0]);
    settle(app, context, true);
    eprintln!("Value map dropdown selected: {}", raw(app));
    assert_eq!(mode(app, document), candidate);
}
fn close(app: &mut FerruleApp, context: &egui::Context) {
    frame(app, context, true, vec![key(egui::Key::Escape)]);
    settle(app, context, true);
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
fn outputs(app: &FerruleApp, retained: &Retained, result: Value, function: Value) {
    let parsed = format_json::from_str(INPUT, &app.project.source);
    retained.record("parse-original", (&app.project, INPUT, &parsed));
    let input = parsed.unwrap();
    let validation = engine::validate(&app.project);
    let actual = engine::run_outputs(&app.project, &input);
    retained.record(
        "engine-original",
        (&app.project, &input, &validation, &actual),
    );
    assert!(validation.is_empty(), "{validation:?}");
    let actual = actual.unwrap();
    let expected = expected(result, function);
    assert_eq!(actual.primary, expected);
    assert_eq!(
        actual.extras,
        vec![engine::NamedOutput {
            name: "audit".into(),
            instance: expected
        }]
    );
}
fn pair(document: MappingDocument, selected: Value, untouched: Value) -> (Value, Value) {
    match document {
        MappingDocument::Main | MappingDocument::Target(_) => (selected, untouched),
        MappingDocument::Function(_) => (untouched, selected),
    }
}
fn imported_table() -> Vec<(Value, Value)> {
    vec![
        (Value::Int(1), Value::String("typed".into())),
        (Value::String("1".into()), Value::String("text".into())),
        (Value::Float(1.0), Value::String("float".into())),
        (Value::Bool(true), Value::String("bool".into())),
        (Value::Null, Value::String("null".into())),
        (Value::xml_nil(), Value::String("nil".into())),
    ]
}

#[test]
fn value_map_actual_pencil_dropdown_all_choices_keep_typed_values_and_warmed_canvas_identity() {
    let mut retained = Retained::new();
    for document in DOCUMENTS {
        let (mut app, context) = setup(
            Value::Int(1),
            imported_table(),
            Some(Value::String("fallback".into())),
        );
        let before_layout = layout(&app);
        let before_values = typed_values(&app);
        let stable_project = project_except_mode(&app, document);
        app.mapping_workspace.active = document;
        settle(&mut app, &context, true);
        let before_rect = bounds(&app, &context);
        outputs(
            &app,
            &retained,
            Value::String("typed".into()),
            Value::String("typed".into()),
        );
        open(&mut app, &context);
        for (candidate, literal) in [
            (Some(ScalarType::String), "text"),
            (Some(ScalarType::Int), "typed"),
            (Some(ScalarType::Float), "float"),
            // An integer cannot be converted to Bool; original input is retained.
            (Some(ScalarType::Bool), "typed"),
            (None, "typed"),
        ] {
            let before = serde_json::to_value(&app.project).unwrap();
            choose(&mut app, &context, candidate);
            retained.record("mode-and-canvas-original", (&before, raw(&app)));
            assert_eq!(typed_values(&app), before_values);
            assert_eq!(project_except_mode(&app, document), stable_project);
            assert_eq!(layout(&app), before_layout);
            let (result, function) = pair(
                document,
                Value::String(literal.into()),
                Value::String("typed".into()),
            );
            outputs(&app, &retained, result, function);
            // Graph and isolated function modes belong to their own graphs.
            match document {
                MappingDocument::Main | MappingDocument::Target(_) => {
                    assert_eq!(mode(&app, MappingDocument::Function(FUNCTION)), None)
                }
                MappingDocument::Function(_) => assert_eq!(mode(&app, MappingDocument::Main), None),
            }
        }
        close(&mut app, &context);
        assert_eq!(
            bounds(&app, &context),
            before_rect,
            "closed compact geometry changed"
        );
        assert_eq!(layout(&app), before_layout);
    }
    retained.complete = true;
}

#[test]
fn value_map_real_conversion_choices_preserve_literal_first_match_invalid_coercion_null_and_default_outcomes()
 {
    let mut retained = Retained::new();
    let cases = vec![
        (
            Value::Int(7),
            Some(ScalarType::String),
            vec![
                (Value::String("7".into()), Value::String("first".into())),
                (Value::String("7".into()), Value::String("second".into())),
            ],
            Some(Value::String("fallback".into())),
            Value::String("first".into()),
            Value::String("fallback".into()),
        ),
        (
            Value::String(" 7 ".into()),
            Some(ScalarType::Int),
            vec![(Value::Int(7), Value::String("integer".into()))],
            Some(Value::String("fallback".into())),
            Value::String("integer".into()),
            Value::String("fallback".into()),
        ),
        (
            Value::String("1.5".into()),
            Some(ScalarType::Float),
            vec![(Value::Float(1.5), Value::String("decimal".into()))],
            Some(Value::String("fallback".into())),
            Value::String("decimal".into()),
            Value::String("fallback".into()),
        ),
        (
            Value::String("true".into()),
            Some(ScalarType::Bool),
            vec![(Value::Bool(true), Value::String("boolean".into()))],
            Some(Value::String("fallback".into())),
            Value::String("boolean".into()),
            Value::String("fallback".into()),
        ),
        (
            Value::String("not-an-int".into()),
            Some(ScalarType::Int),
            vec![(
                Value::String("not-an-int".into()),
                Value::String("unchanged input".into()),
            )],
            None,
            Value::String("unchanged input".into()),
            Value::String("unchanged input".into()),
        ),
        (
            Value::String("missing".into()),
            Some(ScalarType::Int),
            vec![(Value::Int(7), Value::String("integer".into()))],
            Some(Value::String("fallback".into())),
            Value::String("fallback".into()),
            Value::String("fallback".into()),
        ),
        (
            Value::Null,
            Some(ScalarType::String),
            vec![(Value::Null, Value::String("null match".into()))],
            None,
            Value::String("null match".into()),
            Value::String("null match".into()),
        ),
        (
            Value::Null,
            None,
            vec![(Value::Null, Value::String("unchanged null".into()))],
            None,
            Value::String("unchanged null".into()),
            Value::String("unchanged null".into()),
        ),
        (
            Value::String("not-a-bool".into()),
            Some(ScalarType::Bool),
            vec![(
                Value::String("not-a-bool".into()),
                Value::String("unchanged bool input".into()),
            )],
            None,
            Value::String("unchanged bool input".into()),
            Value::String("unchanged bool input".into()),
        ),
        (
            Value::Int(7),
            Some(ScalarType::String),
            vec![(Value::Int(7), Value::String("typed".into()))],
            None,
            Value::Null,
            Value::String("typed".into()),
        ),
        (
            Value::Int(7),
            Some(ScalarType::String),
            vec![(Value::Int(7), Value::String("typed".into()))],
            Some(Value::Null),
            Value::Null,
            Value::String("typed".into()),
        ),
        (
            Value::Int(7),
            Some(ScalarType::String),
            vec![(Value::Int(7), Value::String("typed".into()))],
            Some(Value::String(String::new())),
            Value::String(String::new()),
            Value::String("typed".into()),
        ),
    ];
    for (input, candidate, table, default, literal, unchanged) in cases {
        for document in DOCUMENTS {
            let (mut app, context) = setup(input.clone(), table.clone(), default.clone());
            let values = typed_values(&app);
            let before_layout = layout(&app);
            let stable_project = project_except_mode(&app, document);
            app.mapping_workspace.active = document;
            open(&mut app, &context);
            choose(&mut app, &context, candidate);
            retained.record(
                "literal-case-original",
                (
                    raw(&app),
                    &input,
                    &candidate,
                    &table,
                    &default,
                    &literal,
                    &unchanged,
                ),
            );
            let (result, function) = pair(document, literal.clone(), unchanged.clone());
            outputs(&app, &retained, result, function);
            assert_eq!(typed_values(&app), values);
            assert_eq!(project_except_mode(&app, document), stable_project);
            assert_eq!(layout(&app), before_layout);
        }
    }
    retained.complete = true;
}

#[test]
fn value_map_passive_imported_values_and_locked_open_dropdown_keep_all_table_tags_defaults_and_project_bytes()
 {
    let mut retained = Retained::new();
    for document in DOCUMENTS {
        for (index, default) in [
            None,
            Some(Value::Null),
            Some(Value::String(String::new())),
            Some(Value::Bool(true)),
        ]
        .into_iter()
        .enumerate()
        {
            let imported_mode = if index % 2 == 0 {
                None
            } else {
                Some(ScalarType::Float)
            };
            let mut table = imported_table();
            table.push((Value::Float(-0.0), Value::Bool(false)));
            let (mut app, context) = setup(Value::Int(1), table, default);
            app.mapping_workspace.active = document;
            let graph = match document {
                MappingDocument::Main | MappingDocument::Target(_) => &mut app.project.graph,
                MappingDocument::Function(id) => {
                    &mut app.project.user_functions.get_mut(&id).unwrap().body
                }
            };
            let Node::ValueMap { input_type, .. } = graph.nodes.get_mut(&1).unwrap() else {
                panic!("Value map")
            };
            *input_type = imported_mode;
            let before = serde_json::to_vec(&app.project).unwrap();
            let values = typed_values(&app);
            let before_layout = layout(&app);
            open(&mut app, &context);
            // Establish the inspected document as the clean baseline; tab navigation
            // is itself persisted layout state in the real application.
            app.mark_clean();
            app.rebase_history();
            retained.record("passive-open-original", raw(&app));
            assert_eq!(serde_json::to_vec(&app.project).unwrap(), before);
            assert_eq!(typed_values(&app), values);
            let output = settle(&mut app, &context, false);
            let points = conversion_control(&output, imported_mode, false);
            assert_eq!(
                points.len(),
                1,
                "locked imported conversion remains inspectable"
            );
            click(&mut app, &context, false, points[0]);
            let output = settle(&mut app, &context, false);
            retained.record("locked-choice-original", (raw(&app), texts(&output)));
            assert!(
                !egui::Popup::is_any_open(&context),
                "disabled conversion popup opened"
            );
            assert!(conversion_options(&output, "int").is_empty());
            assert!(
                visible(&output, "int").is_empty(),
                "disabled conversion combo opened"
            );
            assert_eq!(serde_json::to_vec(&app.project).unwrap(), before);
            assert_eq!(typed_values(&app), values);
            assert_eq!(layout(&app), before_layout);
            assert_eq!(app.history.undo_len(), 0);
            assert!(!app.is_dirty());
            close(&mut app, &context);
            open(&mut app, &context);
            close(&mut app, &context);
            retained.record("reinspected-original", raw(&app));
            assert_eq!(serde_json::to_vec(&app.project).unwrap(), before);
            assert_eq!(typed_values(&app), values);
        }
    }
    retained.complete = true;
}

#[test]
fn value_map_actual_conversion_history_saved_reopen_and_preview_keep_complete_outputs_and_no_publication()
-> anyhow::Result<()> {
    let mut retained = Retained::new();
    for (index, document) in DOCUMENTS.into_iter().enumerate() {
        let (mut app, context) = setup(
            Value::Int(7),
            vec![(
                Value::String("7".into()),
                Value::String("text match".into()),
            )],
            Some(Value::String("fallback".into())),
        );
        app.mapping_workspace.active = document;
        open(&mut app, &context);
        app.mark_clean();
        app.rebase_history();
        let before = serde_json::to_value(&app.project)?;
        let values = typed_values(&app);
        let before_layout = layout(&app);
        outputs(
            &app,
            &retained,
            Value::String("fallback".into()),
            Value::String("fallback".into()),
        );
        choose(&mut app, &context, Some(ScalarType::String));
        close(&mut app, &context);
        let after = serde_json::to_value(&app.project)?;
        let (result, function) = pair(
            document,
            Value::String("text match".into()),
            Value::String("fallback".into()),
        );
        retained.record("edited-history-original", raw(&app));
        assert!(app.is_dirty());
        assert_eq!(app.history.undo_len(), 1);
        assert_eq!(typed_values(&app), values);
        outputs(&app, &retained, result.clone(), function.clone());
        app.undo_project();
        settle(&mut app, &context, true);
        retained.record("undo-original", raw(&app));
        assert_eq!(serde_json::to_value(&app.project)?, before);
        assert!(!app.is_dirty());
        outputs(
            &app,
            &retained,
            Value::String("fallback".into()),
            Value::String("fallback".into()),
        );
        app.redo_project();
        settle(&mut app, &context, true);
        retained.record("redo-original", raw(&app));
        assert_eq!(serde_json::to_value(&app.project)?, after);
        assert_eq!(layout(&app), before_layout);
        outputs(&app, &retained, result.clone(), function.clone());
        let project_path = retained.path.join(format!("project-{index}.json"));
        let saved = app.save_document_to(&project_path);
        match &saved {
            Ok(saved) => retained.record(
                "save-original",
                (&saved.validation_issues, &saved.layout_warning),
            ),
            Err(error) => retained.record("save-original", format!("{error:#?}\n{error:#}")),
        }
        let saved = saved?;
        assert!(saved.validation_issues.is_empty());
        assert!(saved.layout_warning.is_none());
        let project_bytes = std::fs::read(&project_path)?;
        let layout_path = crate::layout_store::layout_path(&project_path);
        let layout_bytes = std::fs::read(&layout_path)?;
        retained.record("saved-full-originals", (&project_bytes, &layout_bytes));
        let saved_project = serde_json::to_value(&app.project)?;
        let saved_layout = layout(&app);
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&project_path);
        retained.record(
            "load-project-full-original",
            (
                &reopened.project,
                &reopened.status,
                reopened.diagnostics.items(),
            ),
        );
        reopened.open_target_tab(0);
        assert!(reopened.ensure_target_canvas(0));
        reopened.open_function_tab(FUNCTION);
        assert!(reopened.ensure_function_canvas(FUNCTION));
        for opened in DOCUMENTS {
            reopened.mapping_workspace.active = opened;
            settle(&mut reopened, &context, true);
        }
        reopened.mapping_workspace.active = document;
        retained.record("reopened-original", raw(&reopened));
        assert_eq!(serde_json::to_value(&reopened.project)?, saved_project);
        assert_eq!(layout(&reopened), saved_layout);
        assert_eq!(typed_values(&reopened), values);
        outputs(&reopened, &retained, result.clone(), function.clone());
        let output_path = retained.path.join(format!("must-not-write-{index}.json"));
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
        retained.record("preview-full-original", &reopened.run_report);
        assert!(reopened.pending_preview.is_none());
        assert!(!output_path.exists());
        let report = reopened
            .run_report
            .as_mut()
            .expect("completed real Preview report");
        assert_eq!(report.report.outputs.len(), 1);
        let crate::run_report::OutputPreview::Text { content, .. } =
            report.report.outputs[0].preview()
        else {
            panic!("JSON Preview")
        };
        std::fs::write(
            retained.path.join(format!("preview-{index}-original.json")),
            content.as_bytes(),
        )?;
        let parsed = serde_json::from_str::<serde_json::Value>(content);
        retained.record("preview-parse-original", &parsed);
        let (Value::String(result_text), Value::String(function_text)) = (result, function) else {
            panic!("independent Preview string oracle")
        };
        assert_eq!(
            parsed?,
            serde_json::json!({"Result":result_text,"Function":function_text})
        );
        assert_eq!(std::fs::read(&project_path)?, project_bytes);
        assert_eq!(std::fs::read(&layout_path)?, layout_bytes);
    }
    retained.complete = true;
    Ok(())
}
