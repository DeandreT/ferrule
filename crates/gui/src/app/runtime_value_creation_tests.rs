use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, NamedTarget, RuntimeValue};

const FUNCTION: FunctionId = FunctionId::new(7);
const DOCUMENTS: [MappingDocument; 3] = [
    MappingDocument::Main,
    MappingDocument::Target(0),
    MappingDocument::Function(FUNCTION),
];

fn app(document: MappingDocument, maximum: Option<NodeId>) -> FerruleApp {
    let mut app = FerruleApp {
        project: blank_project(),
        ..FerruleApp::default()
    };
    app.project.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("keep primary and named output".into()),
        },
    );
    app.project.root.bindings.push(Binding {
        target_field: "Value".into(),
        node: 0,
    });
    app.project.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: None,
        schema: app.project.target.clone(),
        options: Default::default(),
        root: app.project.root.clone(),
    });
    let mut body = Graph::default();
    body.nodes.insert(
        0,
        Node::Const {
            value: Value::String("keep function output".into()),
        },
    );
    app.project.user_functions.insert(
        FUNCTION,
        UserFunction {
            library: "local".into(),
            name: "helper".into(),
            description: None,
            parameters: Vec::new(),
            output_name: "result".into(),
            output_type: ScalarType::String,
            body,
            output: 0,
        },
    );
    if let Some(maximum) = maximum {
        graph_mut(&mut app.project, document).nodes.insert(
            maximum,
            Node::Const {
                value: Value::String("capacity marker".into()),
            },
        );
    }
    app.main_canvas = CanvasDocumentState::main(&app.project);
    match document {
        MappingDocument::Main => {}
        MappingDocument::Target(index) => app.open_target_tab(index),
        MappingDocument::Function(id) => app.open_function_tab(id),
    }
    app
}

fn graph_mut(project: &mut Project, document: MappingDocument) -> &mut Graph {
    match document {
        MappingDocument::Main | MappingDocument::Target(_) => &mut project.graph,
        MappingDocument::Function(id) => &mut project.user_functions.get_mut(&id).unwrap().body,
    }
}

fn canvas(app: &FerruleApp, document: MappingDocument) -> &CanvasDocumentState {
    match document {
        MappingDocument::Main => &app.main_canvas,
        MappingDocument::Target(index) => &app.mapping_workspace.target_canvases[&index],
        MappingDocument::Function(id) => &app.mapping_workspace.function_canvases[&id],
    }
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

fn prepared(
    document: MappingDocument,
    maximum: Option<NodeId>,
    editing: bool,
) -> (FerruleApp, egui::Context) {
    let mut app = app(document, maximum);
    let context = egui::Context::default();
    crate::icons::install(&context);
    for _ in 0..4 {
        frame(&mut app, &context, editing, Vec::new());
    }
    app.mark_clean();
    app.rebase_history();
    (app, context)
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

fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn text_rect(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
    fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(text.visual_bounding_rect())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, label))
}

fn context_click(app: &mut FerruleApp, context: &egui::Context, editing: bool) -> egui::FullOutput {
    // The preceding prepared frames register the real canvas before this press.
    let pos = egui::pos2(1100.0, 160.0);
    frame(app, context, editing, vec![egui::Event::PointerMoved(pos)]);
    frame(
        app,
        context,
        editing,
        vec![pointer(pos, egui::PointerButton::Secondary, true)],
    );
    frame(
        app,
        context,
        editing,
        vec![pointer(pos, egui::PointerButton::Secondary, false)],
    );
    let output = frame(app, context, editing, Vec::new());
    eprintln!(
        "Runtime value canvas context click: document={:?}, editing={editing}, add_node={:?}",
        app.mapping_workspace.active,
        text_rect(&output, "Add node")
    );
    eprintln!(
        "Runtime value menu painted labels={:?}",
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => Some((
                    text.galley.text().chars().take(80).collect::<String>(),
                    text.visual_bounding_rect()
                )),
                _ => None,
            })
            .take(32)
            .collect::<Vec<_>>()
    );
    output
}

// Each expected value is authored independently of the node factory.
const VALUES: [(RuntimeValue, &str, &str, &str); 3] = [
    (
        RuntimeValue::MappingFilePath,
        "Mapping path",
        "active mapping file",
        "/maps/library.json",
    ),
    (
        RuntimeValue::MainMappingFilePath,
        "Main mapping path",
        "top level caller",
        "/maps/main.json",
    ),
    (
        RuntimeValue::CurrentDateTime,
        "Run date and time",
        "run timestamp",
        "2026-08-21T09:10:11-07:00",
    ),
];

fn search(
    app: &mut FerruleApp,
    context: &egui::Context,
    query: &str,
    label: &str,
) -> egui::FullOutput {
    let output = context_click(app, context, true);
    assert!(
        text_rect(&output, "Add node").is_some(),
        "real graph context menu"
    );
    frame(app, context, true, vec![egui::Event::Text(query.into())]);
    let output = frame(app, context, true, Vec::new());
    eprintln!(
        "Runtime value query={query:?}, action={label:?}, rect={:?}",
        text_rect(&output, label)
    );
    assert!(
        text_rect(&output, label).is_some(),
        "visible creation action"
    );
    output
}

fn observe(app: &FerruleApp, document: MappingDocument) {
    let canvas = canvas(app, document);
    eprintln!(
        "Runtime value creation raw: document={document:?}, project={}, nodes={:?}, wires={}, status={:?}, diagnostics={:?}, undo={}, redo={}",
        serde_json::to_string(&app.project).unwrap(),
        canvas.snarl.nodes_pos().collect::<Vec<_>>(),
        canvas.snarl.wires().count(),
        app.status,
        app.diagnostics.items(),
        app.history.undo_len(),
        app.history.redo_len()
    );
}

fn assert_created(
    app: &FerruleApp,
    document: MappingDocument,
    mut expected: Project,
    created: NodeId,
    value: RuntimeValue,
    canvas_before: usize,
    wires_before: usize,
) {
    observe(app, document);
    let issues = engine::validate(&app.project);
    eprintln!("Runtime value validation original: {issues:?}");
    graph_mut(&mut expected, document)
        .nodes
        .insert(created, Node::RuntimeValue { value });
    assert_eq!(
        serde_json::to_value(&app.project).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert!(issues.is_empty());
    let canvas = canvas(app, document);
    assert_eq!(canvas.snarl.nodes().count(), canvas_before + 1);
    assert_eq!(canvas.snarl.wires().count(), wires_before);
    assert_eq!(
        canvas
            .snarl
            .nodes()
            .filter(|node| **node == CanvasNode::Graph(created))
            .count(),
        1
    );
    assert!(
        !canvas
            .snarl
            .nodes()
            .any(|node| matches!(node, CanvasNode::Placeholder(_)))
    );
    assert!(app.diagnostics.is_empty());
    assert_eq!(app.history.undo_len(), 1);
    assert!(app.is_dirty());
}

fn output(value: &str) -> Instance {
    Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        )]
        .into(),
    )
}

fn assert_created_value_reaches_its_canvas_output(
    app: &FerruleApp,
    document: MappingDocument,
    expected: &str,
) {
    // Bind only a disposable project clone. The real GUI edit remains one node.
    let mut project = app.project.clone();
    match document {
        MappingDocument::Main => project.root.bindings[0].node = 1,
        MappingDocument::Target(index) => project.extra_targets[index].root.bindings[0].node = 1,
        MappingDocument::Function(id) => {
            project.user_functions.get_mut(&id).unwrap().output = 1;
            project.graph.nodes.insert(
                2,
                Node::UserFunctionCall {
                    function: id,
                    args: Vec::new(),
                },
            );
            project.root.bindings[0].node = 2;
        }
    }
    let context = engine::ExecutionContext::with_main_mapping_file_path(
        Path::new("/maps/library.json"),
        Path::new("/maps/main.json"),
    )
    .with_current_datetime("2026-08-21T09:10:11-07:00");
    let input = Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String("source unchanged".into())),
        )]
        .into(),
    );
    let result =
        engine::run_outputs_with_sources_and_context(&project, &input, Vec::new(), &context);
    eprintln!(
        "Runtime value bound output original: document={document:?}, expected={expected:?}, result={result:?}"
    );
    let actual = result.expect("created runtime value executes in its selected canvas context");
    let primary = if matches!(document, MappingDocument::Target(_)) {
        "keep primary and named output"
    } else {
        expected
    };
    let named = if matches!(document, MappingDocument::Target(_)) {
        expected
    } else {
        "keep primary and named output"
    };
    assert_eq!(actual.primary, output(primary));
    assert_eq!(
        actual.extras,
        vec![engine::NamedOutput {
            name: "audit".into(),
            instance: output(named)
        }]
    );
}

#[test]
fn runtime_value_search_return_creates_and_reads_each_value_on_all_three_canvases() {
    for document in DOCUMENTS {
        for (value, label, _, expected) in VALUES {
            let (mut app, context) = prepared(document, None, true);
            let before = app.project.clone();
            let nodes = canvas(&app, document).snarl.nodes().count();
            let wires = canvas(&app, document).snarl.wires().count();
            search(&mut app, &context, label, label);
            frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
            assert_created(&app, document, before, 1, value, nodes, wires);
            assert_created_value_reaches_its_canvas_output(&app, document, expected);
        }
    }
}

#[test]
fn runtime_value_pointer_alias_creation_roundtrips_history_and_canvas_position() {
    for document in DOCUMENTS {
        for (value, label, query, _) in VALUES {
            let (mut app, context) = prepared(document, None, true);
            let before = serde_json::to_value(&app.project).unwrap();
            let project_before = app.project.clone();
            let nodes = canvas(&app, document).snarl.nodes().count();
            let wires = canvas(&app, document).snarl.wires().count();
            // The query differs from the action label, so text_rect cannot pick the search field.
            let rendered = search(&mut app, &context, query, label);
            let pos = text_rect(&rendered, label).unwrap().center();
            frame(
                &mut app,
                &context,
                true,
                vec![egui::Event::PointerMoved(pos)],
            );
            for pressed in [true, false] {
                frame(
                    &mut app,
                    &context,
                    true,
                    vec![pointer(pos, egui::PointerButton::Primary, pressed)],
                );
            }
            assert_created(&app, document, project_before, 1, value, nodes, wires);
            let after = serde_json::to_value(&app.project).unwrap();
            let position = canvas(&app, document)
                .snarl
                .nodes_pos()
                .find(|(_, node)| **node == CanvasNode::Graph(1))
                .unwrap()
                .0;
            app.undo_project();
            observe(&app, document);
            assert_eq!(serde_json::to_value(&app.project).unwrap(), before);
            assert!(
                !canvas(&app, document)
                    .snarl
                    .nodes()
                    .any(|node| *node == CanvasNode::Graph(1))
            );
            assert!(!app.is_dirty());
            assert_eq!(app.history.redo_len(), 1);
            app.redo_project();
            observe(&app, document);
            assert_eq!(serde_json::to_value(&app.project).unwrap(), after);
            assert_eq!(
                canvas(&app, document)
                    .snarl
                    .nodes_pos()
                    .find(|(_, node)| **node == CanvasNode::Graph(1))
                    .unwrap()
                    .0,
                position
            );
            assert_eq!(app.history.undo_len(), 1);
            assert!(app.is_dirty());
        }
    }
}

#[test]
fn locked_canvases_refuse_all_three_runtime_actions_without_history_or_project_edits() {
    for document in DOCUMENTS {
        for (_, label, _, _) in VALUES {
            let (mut app, context) = prepared(document, None, false);
            let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
            let rendered = context_click(&mut app, &context, false);
            frame(
                &mut app,
                &context,
                false,
                vec![egui::Event::Text(label.into())],
            );
            frame(&mut app, &context, false, vec![key(egui::Key::Enter)]);
            observe(&app, document);
            assert!(text_rect(&rendered, "Add node").is_none());
            assert!(
                editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace)
                    == before
            );
            assert_eq!(app.history.undo_len(), 0);
            assert!(app.diagnostics.is_empty());
            assert!(!app.is_dirty());
        }
    }
}

#[test]
fn runtime_value_menu_reserves_one_final_id_or_refuses_without_hidden_nodes() {
    for document in DOCUMENTS {
        for available in [true, false] {
            let maximum = if available {
                NodeId::MAX - 1
            } else {
                NodeId::MAX
            };
            let (mut app, context) = prepared(document, Some(maximum), true);
            let project_before = app.project.clone();
            let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
            let nodes = canvas(&app, document).snarl.nodes().count();
            let wires = canvas(&app, document).snarl.wires().count();
            search(&mut app, &context, "Run date and time", "Run date and time");
            frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
            if available {
                assert_created(
                    &app,
                    document,
                    project_before,
                    NodeId::MAX,
                    RuntimeValue::CurrentDateTime,
                    nodes,
                    wires,
                );
            } else {
                observe(&app, document);
                assert!(
                    editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace)
                        == before
                );
                assert_eq!(canvas(&app, document).snarl.nodes().count(), nodes);
                assert_eq!(canvas(&app, document).snarl.wires().count(), wires);
                assert_eq!(app.history.undo_len(), 0);
                assert!(!app.is_dirty());
                let errors = app.diagnostics.items();
                assert_eq!(errors.len(), 1);
                assert_eq!(errors[0].level, crate::diagnostics::DiagnosticLevel::Error);
                assert_eq!(errors[0].message, "mapping node IDs are exhausted");
            }
        }
    }
}

mod dynamic_property;
mod source_document_path;
mod xml_serializer;
