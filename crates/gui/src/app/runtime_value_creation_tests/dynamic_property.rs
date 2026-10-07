use super::*;

const SOURCE_PROPERTY: &str = "Source property by name";
const MAPPING_DOCUMENTS: [MappingDocument; 2] = [MappingDocument::Main, MappingDocument::Target(0)];

fn open_object(name: &str) -> SchemaNode {
    SchemaNode::group(name, Vec::new())
        .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String))
        .unwrap()
}

fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("selectedName", ScalarType::String),
            open_object("values"),
            open_object("z-values"),
        ],
    )
}

fn source_instance(key: Value) -> Instance {
    Instance::Group(
        vec![
            ("selectedName".into(), Instance::Scalar(key)),
            (
                "values".into(),
                Instance::Group(
                    vec![
                        (
                            "country".into(),
                            Instance::Scalar(Value::String("Canada".into())),
                        ),
                        ("city".into(), Instance::Scalar(Value::String("雪".into()))),
                    ]
                    .into(),
                ),
            ),
            (
                "z-values".into(),
                Instance::Group(
                    vec![(
                        "country".into(),
                        Instance::Scalar(Value::String("France".into())),
                    )]
                    .into(),
                ),
            ),
        ]
        .into(),
    )
}

fn reset_source_canvases(app: &mut FerruleApp, document: MappingDocument) {
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mapping_workspace.target_canvases.clear();
    if let MappingDocument::Target(index) = document {
        assert!(app.ensure_target_canvas(index));
    }
}

fn setup(
    document: MappingDocument,
    source: SchemaNode,
    maximum: Option<NodeId>,
    editing: bool,
) -> (FerruleApp, egui::Context) {
    let mut app = super::app(document, maximum);
    app.project.source = source;
    reset_source_canvases(&mut app, document);
    let context = egui::Context::default();
    crate::icons::install(&context);
    for _ in 0..4 {
        frame(&mut app, &context, editing, Vec::new());
    }
    app.mark_clean();
    app.rebase_history();
    (app, context)
}

fn click(app: &mut FerruleApp, context: &egui::Context, editing: bool, pos: egui::Pos2) {
    frame(app, context, editing, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            app,
            context,
            editing,
            vec![pointer(pos, egui::PointerButton::Primary, pressed)],
        );
    }
}

fn canvas_mut(app: &mut FerruleApp, document: MappingDocument) -> &mut CanvasDocumentState {
    match document {
        MappingDocument::Main => &mut app.main_canvas,
        MappingDocument::Target(index) => app
            .mapping_workspace
            .target_canvases
            .get_mut(&index)
            .unwrap(),
        MappingDocument::Function(id) => app
            .mapping_workspace
            .function_canvases
            .get_mut(&id)
            .unwrap(),
    }
}

fn settle(app: &mut FerruleApp, context: &egui::Context, editing: bool) -> egui::FullOutput {
    let mut output = frame(app, context, editing, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, editing, Vec::new());
    }
    output
}

// Only fixture geometry changes here. Creation and both connections use real
// pointer/key events on the production canvas; no graph edit is seeded.
fn arrange_fixture(app: &mut FerruleApp, context: &egui::Context, document: MappingDocument) {
    let state = canvas_mut(app, document);
    let nodes = state
        .snarl
        .node_ids()
        .map(|(id, node)| (id, *node))
        .collect::<Vec<_>>();
    for (id, node) in nodes {
        state.snarl.get_node_info_mut(id).unwrap().pos = match node {
            CanvasNode::SourceBlock(_) => egui::pos2(0.0, 0.0),
            CanvasNode::TargetBlock(_) => egui::pos2(800.0, 0.0),
            CanvasNode::Graph(2) => egui::pos2(400.0, 0.0),
            _ => egui::pos2(400.0, 250.0),
        };
    }
    state.reset_view();
    settle(app, context, true);
}

fn canvas_salt(app: &FerruleApp, document: MappingDocument) -> egui::Id {
    let salt = match document {
        MappingDocument::Main => egui::Id::new("main_mapping_canvas"),
        MappingDocument::Target(index) => egui::Id::new(("named_target_canvas", index)),
        MappingDocument::Function(_) => {
            panic!("source-property fixtures never wire an isolated function")
        }
    };
    app.embedded_canvas_id(salt)
}

fn native_responses(
    app: &FerruleApp,
    context: &egui::Context,
    document: MappingDocument,
) -> Vec<egui::Response> {
    let ids = crate::canvas_keyboard::current_pin_interaction_ids(
        context,
        canvas_salt(app, document),
        canvas(app, document).view_generation,
    );
    eprintln!("Source-property native cached pin IDs: {ids:?}");
    assert!(!ids.is_empty(), "the actual Snarl canvas registered pins");
    let responses = ids
        .into_iter()
        .map(|id| {
            context
                .read_response(id)
                .expect("actual native pin response")
        })
        .collect::<Vec<_>>();
    eprintln!("Source-property full native pin responses: {responses:#?}");
    responses
}

fn node_rect(
    app: &FerruleApp,
    context: &egui::Context,
    document: MappingDocument,
    wanted: CanvasNode,
) -> egui::Rect {
    let snarl_id = canvas(app, document)
        .snarl
        .node_ids()
        .find_map(|(id, node)| (*node == wanted).then_some(id))
        .unwrap();
    let registered = native_responses(app, context, document);
    let response = registered
        .iter()
        .find_map(|pin| {
            context.read_response(pin.layer_id.id.with(("snarl-node", snarl_id)).with("frame"))
        })
        .expect("existing registered Snarl node frame");
    let transform = context
        .layer_transform_to_global(response.layer_id)
        .unwrap_or_default();
    let rect = transform * response.rect;
    eprintln!(
        "Source-property actual node={wanted:?}, frame={response:?}, transform={transform:?}, global={rect:?}"
    );
    rect
}

fn native_pin(
    app: &FerruleApp,
    context: &egui::Context,
    document: MappingDocument,
    wanted: CanvasNode,
    output: bool,
) -> egui::Pos2 {
    let bounds = node_rect(app, context, document, wanted);
    let pins = native_responses(app, context, document)
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
                .then_some((response.id, center))
        })
        .collect::<Vec<_>>();
    eprintln!(
        "Source-property actual native {wanted:?} output={output}, frame={bounds:?}, pins={pins:?}"
    );
    assert_eq!(
        pins.len(),
        1,
        "one exact native pin on this fixture's endpoint or property node"
    );
    let center = pins[0].1;
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0)).contains(center)
    );
    center
}

fn wire(app: &mut FerruleApp, context: &egui::Context, from: egui::Pos2, to: egui::Pos2) {
    frame(app, context, true, vec![egui::Event::PointerMoved(from)]);
    frame(
        app,
        context,
        true,
        vec![pointer(from, egui::PointerButton::Primary, true)],
    );
    frame(
        app,
        context,
        true,
        vec![egui::Event::PointerMoved(from.lerp(to, 0.5))],
    );
    frame(app, context, true, vec![egui::Event::PointerMoved(to)]);
    frame(
        app,
        context,
        true,
        vec![pointer(to, egui::PointerButton::Primary, false)],
    );
    settle(app, context, true);
}

fn visible_texts(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    fn visit(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        label: &str,
        found: &mut Vec<egui::Pos2>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                let rect = text.visual_bounding_rect();
                if clip.contains(rect.center()) {
                    found.push(rect.center());
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

fn open_editor(
    app: &mut FerruleApp,
    context: &egui::Context,
    document: MappingDocument,
) -> egui::FullOutput {
    let output = settle(app, context, true);
    let Node::DynamicSourceField {
        object,
        frame: stored_frame,
        ..
    } = &app.project.graph.nodes[&2]
    else {
        unreachable!()
    };
    let identity = format!(
        "open source object: {}{}",
        stored_frame
            .as_ref()
            .map(|path| format!("{}/", path.join("/")))
            .unwrap_or_default(),
        object.join("/")
    );
    if text_rect(&output, &identity).is_some() {
        return output;
    }
    let bounds = node_rect(app, context, document, CanvasNode::Graph(2));
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    let candidates = visible_texts(&output, &pencil)
        .into_iter()
        .filter(|pos| bounds.contains(*pos))
        .collect::<Vec<_>>();
    eprintln!("Source-property real pencil candidates={candidates:?}, node={bounds:?}");
    assert_eq!(candidates.len(), 1);
    click(app, context, true, candidates[0]);
    settle(app, context, true)
}

fn choose_object(
    app: &mut FerruleApp,
    context: &egui::Context,
    current: &str,
    desired: &str,
    editing: bool,
) {
    let output = settle(app, context, editing);
    let selected = visible_texts(&output, current);
    eprintln!("Source-property actual selector text={selected:?}");
    assert_eq!(
        selected.len(),
        1,
        "selected object label belongs only to its existing picker"
    );
    click(app, context, editing, selected[0]);
    let opened = settle(app, context, editing);
    let choices = visible_texts(&opened, desired)
        .into_iter()
        .filter(|pos| current != desired || pos.distance(selected[0]) > 2.0)
        .collect::<Vec<_>>();
    eprintln!("Source-property actual choice={desired:?}, positions={choices:?}");
    assert_eq!(choices.len(), 1, "actual supported choice is visible");
    click(app, context, editing, choices[0]);
    settle(app, context, editing);
}

fn output_value(value: Value) -> Instance {
    Instance::Group(vec![("Value".into(), Instance::Scalar(value))].into())
}

fn assert_outputs(app: &FerruleApp, document: MappingDocument, input: &Instance, expected: Value) {
    let validation = engine::validate(&app.project);
    let result = engine::run_outputs(&app.project, input);
    eprintln!(
        "Source-property full original validation={validation:?}, outputs={result:#?}, input={input:#?}"
    );
    assert!(validation.is_empty());
    let actual = result.expect("real wired property executes");
    let preserved = Value::String("keep primary and named output".into());
    let primary = if document == MappingDocument::Main {
        expected.clone()
    } else {
        preserved.clone()
    };
    let named = if document == MappingDocument::Target(0) {
        expected
    } else {
        preserved
    };
    assert_eq!(actual.primary, output_value(primary));
    assert_eq!(
        actual.extras,
        vec![engine::NamedOutput {
            name: "audit".into(),
            instance: output_value(named)
        }]
    );
}

fn assert_creation(
    app: &FerruleApp,
    document: MappingDocument,
    mut expected: Project,
    created: NodeId,
    key: NodeId,
) {
    observe(app, document);
    expected.graph.nodes.insert(key, Node::Unconnected);
    expected.graph.nodes.insert(
        created,
        Node::DynamicSourceField {
            object: vec!["values".into()],
            frame: None,
            key,
        },
    );
    assert_eq!(
        serde_json::to_value(&app.project).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(
        canvas(app, document)
            .snarl
            .nodes()
            .filter(|node| **node == CanvasNode::Graph(created))
            .count(),
        1
    );
    assert!(
        !canvas(app, document)
            .snarl
            .nodes()
            .any(|node| matches!(node, CanvasNode::Placeholder(_)))
    );
    assert!(
        !canvas(app, document)
            .snarl
            .nodes()
            .any(|node| *node == CanvasNode::Graph(key))
    );
    assert_eq!(app.history.undo_len(), 1);
    assert!(app.is_dirty());
    assert!(app.diagnostics.is_empty());
}

#[test]
fn source_property_real_creation_and_wires_preserve_complete_primary_named_outputs_and_key_errors()
{
    for document in MAPPING_DOCUMENTS {
        let (mut app, context) = setup(document, source_schema(), None, true);
        let before = app.project.clone();
        let rendered = search(&mut app, &context, "dynamic property", SOURCE_PROPERTY);
        if document == MappingDocument::Main {
            frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
        } else {
            let action = text_rect(&rendered, SOURCE_PROPERTY).unwrap().center();
            click(&mut app, &context, true, action);
        }
        assert_creation(&app, document, before.clone(), 2, 1);
        let created = serde_json::to_value(&app.project).unwrap();
        app.undo_project();
        observe(&app, document);
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        assert!(!app.is_dirty());
        app.redo_project();
        observe(&app, document);
        assert_eq!(serde_json::to_value(&app.project).unwrap(), created);
        assert_eq!(app.history.undo_len(), 1);
        arrange_fixture(&mut app, &context, document);
        let source = native_pin(&app, &context, document, CanvasNode::SourceBlock(0), true);
        let property_key = native_pin(&app, &context, document, CanvasNode::Graph(2), false);
        wire(&mut app, &context, source, property_key);
        observe(&app, document);
        let mut expected = before.clone();
        expected.graph.nodes.insert(
            3,
            Node::SourceField {
                path: vec!["selectedName".into()],
                frame: None,
            },
        );
        expected.graph.nodes.insert(
            2,
            Node::DynamicSourceField {
                object: vec!["values".into()],
                frame: None,
                key: 3,
            },
        );
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        let output = native_pin(&app, &context, document, CanvasNode::Graph(2), true);
        let target = native_pin(&app, &context, document, CanvasNode::TargetBlock(0), false);
        wire(&mut app, &context, output, target);
        observe(&app, document);
        match document {
            MappingDocument::Main => expected.root.bindings[0].node = 2,
            MappingDocument::Target(index) => {
                expected.extra_targets[index].root.bindings[0].node = 2
            }
            MappingDocument::Function(_) => unreachable!(),
        }
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert_eq!(canvas(&app, document).snarl.wires().count(), 2);
        assert_outputs(
            &app,
            document,
            &source_instance(Value::String("country".into())),
            Value::String("Canada".into()),
        );
        assert_outputs(
            &app,
            document,
            &source_instance(Value::String("city".into())),
            Value::String("雪".into()),
        );
        assert_outputs(
            &app,
            document,
            &source_instance(Value::String("missing".into())),
            Value::Null,
        );
        // The typed engine input control exercises existing non-string-key semantics,
        // separately from schema-valid String inputs used by the real source pin.
        assert_outputs(&app, document, &source_instance(Value::Int(7)), Value::Null);
        assert_outputs(&app, document, &source_instance(Value::Null), Value::Null);
        let mut failing = app.project.clone();
        failing.graph.nodes.insert(
            10,
            Node::Const {
                value: Value::Int(1),
            },
        );
        failing.graph.nodes.insert(
            11,
            Node::Const {
                value: Value::Int(0),
            },
        );
        failing.graph.nodes.insert(
            3,
            Node::Call {
                function: "divide".into(),
                args: vec![10, 11],
            },
        );
        let error =
            engine::run_outputs(&failing, &source_instance(Value::String("country".into())));
        eprintln!("Source-property original key evaluation failure: {error:#?}");
        assert!(matches!(
            error,
            Err(engine::EngineError::Function(
                functions::FunctionError::DivideByZero
            ))
        ));
        let encoded = mapping::project_file::encode_pretty(&app.project);
        eprintln!("Source-property complete saved codec original: {encoded:?}");
        let reopened = mapping::project_file::decode_str(&encoded.unwrap());
        eprintln!("Source-property reopened codec original: {reopened:#?}");
        assert_eq!(
            serde_json::to_value(reopened.unwrap()).unwrap(),
            serde_json::to_value(&app.project).unwrap()
        );
        let before_edit = serde_json::to_value(&app.project).unwrap();
        open_editor(&mut app, &context, document);
        choose_object(&mut app, &context, "values", "z-values", true);
        observe(&app, document);
        expected.graph.nodes.insert(
            2,
            Node::DynamicSourceField {
                object: vec!["z-values".into()],
                frame: None,
                key: 3,
            },
        );
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert_eq!(canvas(&app, document).snarl.wires().count(), 2);
        assert_outputs(
            &app,
            document,
            &source_instance(Value::String("country".into())),
            Value::String("France".into()),
        );
        let after_edit = serde_json::to_value(&app.project).unwrap();
        app.undo_project();
        observe(&app, document);
        assert_eq!(serde_json::to_value(&app.project).unwrap(), before_edit);
        assert_outputs(
            &app,
            document,
            &source_instance(Value::String("country".into())),
            Value::String("Canada".into()),
        );
        app.redo_project();
        observe(&app, document);
        assert_eq!(serde_json::to_value(&app.project).unwrap(), after_edit);
        assert_outputs(
            &app,
            document,
            &source_instance(Value::String("country".into())),
            Value::String("France".into()),
        );
    }
}

#[test]
fn source_property_existing_metadata_stays_exact_until_real_supported_selection_and_history_reversal()
 {
    for document in MAPPING_DOCUMENTS {
        for frame_path in [
            None,
            Some(Vec::new()),
            Some(vec!["legacy".into(), "rows".into()]),
        ] {
            let (mut app, context) = setup(document, source_schema(), None, true);
            app.project.graph.nodes.insert(1, Node::Unconnected);
            app.project.graph.nodes.insert(
                2,
                Node::DynamicSourceField {
                    object: vec!["imported".into(), "未知📦".into()],
                    frame: frame_path.clone(),
                    key: 1,
                },
            );
            let original = mapping::project_file::encode_pretty(&app.project).unwrap();
            app.project = mapping::project_file::decode_str(&original).unwrap();
            reset_source_canvases(&mut app, document);
            arrange_fixture(&mut app, &context, document);
            app.mark_clean();
            app.rebase_history();
            let before = serde_json::to_value(&app.project).unwrap();
            let wires = canvas(&app, document).snarl.wires().count();
            let output = open_editor(&mut app, &context, document);
            observe(&app, document);
            assert!(text_rect(&output, "imported / 未知📦").is_some());
            assert_eq!(serde_json::to_value(&app.project).unwrap(), before);
            assert_eq!(app.history.undo_len(), 0);
            assert!(!app.is_dirty());
            choose_object(&mut app, &context, "imported / 未知📦", "values", true);
            observe(&app, document);
            let mut expected: Project = mapping::project_file::decode_str(&original).unwrap();
            expected.graph.nodes.insert(
                2,
                Node::DynamicSourceField {
                    object: vec!["values".into()],
                    frame: None,
                    key: 1,
                },
            );
            assert_eq!(
                serde_json::to_value(&app.project).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(canvas(&app, document).snarl.wires().count(), wires);
            assert_eq!(app.history.undo_len(), 1);
            assert!(app.is_dirty());
            let after = serde_json::to_value(&app.project).unwrap();
            app.undo_project();
            observe(&app, document);
            assert_eq!(serde_json::to_value(&app.project).unwrap(), before);
            assert!(!app.is_dirty());
            app.redo_project();
            observe(&app, document);
            assert_eq!(serde_json::to_value(&app.project).unwrap(), after);
        }
    }
    // Both selecting a different supported object and explicitly selecting the
    // same empty path clear a stored frame; inspection alone never does.
    let root = SchemaNode::group(
        "Input",
        vec![SchemaNode::group("envelope", vec![open_object("nested")])],
    )
    .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String))
    .unwrap();
    let (mut app, context) = setup(MappingDocument::Main, root, None, true);
    search(&mut app, &context, "dynamic property", SOURCE_PROPERTY);
    frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
    observe(&app, MappingDocument::Main);
    assert!(
        matches!(&app.project.graph.nodes[&2], Node::DynamicSourceField { object, frame: None, key: 1 } if object.is_empty())
    );
    let Node::DynamicSourceField { frame: stored, .. } =
        app.project.graph.nodes.get_mut(&2).unwrap()
    else {
        unreachable!()
    };
    *stored = Some(Vec::new());
    arrange_fixture(&mut app, &context, MappingDocument::Main);
    app.mark_clean();
    app.rebase_history();
    let before = serde_json::to_value(&app.project).unwrap();
    open_editor(&mut app, &context, MappingDocument::Main);
    observe(&app, MappingDocument::Main);
    assert_eq!(serde_json::to_value(&app.project).unwrap(), before);
    // A locked already-open popover keeps this exact stored frame and graph.
    let locked = settle(&mut app, &context, false);
    let selector = visible_texts(&locked, "<source root>");
    assert_eq!(selector.len(), 1);
    click(&mut app, &context, false, selector[0]);
    frame(&mut app, &context, false, vec![key(egui::Key::Enter)]);
    observe(&app, MappingDocument::Main);
    assert_eq!(serde_json::to_value(&app.project).unwrap(), before);
    assert_eq!(app.history.undo_len(), 0);
    // The locked click may close the popover; close it deliberately before reopening.
    frame(&mut app, &context, true, vec![key(egui::Key::Escape)]);
    open_editor(&mut app, &context, MappingDocument::Main);
    choose_object(
        &mut app,
        &context,
        "<source root>",
        "envelope / nested",
        true,
    );
    observe(&app, MappingDocument::Main);
    assert!(
        matches!(&app.project.graph.nodes[&2], Node::DynamicSourceField { object, frame: None, key: 1 } if object == &["envelope".to_owned(), "nested".to_owned()])
    );
    frame(&mut app, &context, true, vec![key(egui::Key::Escape)]);
    let Node::DynamicSourceField {
        object,
        frame: stored,
        ..
    } = app.project.graph.nodes.get_mut(&2).unwrap()
    else {
        unreachable!()
    };
    object.clear();
    *stored = Some(Vec::new());
    settle(&mut app, &context, true);
    app.mark_clean();
    app.rebase_history();
    open_editor(&mut app, &context, MappingDocument::Main);
    choose_object(&mut app, &context, "<source root>", "<source root>", true);
    observe(&app, MappingDocument::Main);
    assert!(
        matches!(&app.project.graph.nodes[&2], Node::DynamicSourceField { object, frame: None, key: 1 } if object.is_empty())
    );
    assert_eq!(app.history.undo_len(), 1);
}

#[test]
fn source_property_palette_refuses_closed_repeated_complex_function_and_locked_contexts() {
    let schemas = [
        SchemaNode::group(
            "Input",
            vec![SchemaNode::scalar("closed", ScalarType::String)],
        ),
        source_schema().repeating(),
        SchemaNode::group("Input", vec![open_object("rows").repeating()]),
        SchemaNode::group("Input", Vec::new())
            .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String).repeating())
            .unwrap(),
        SchemaNode::group("Input", Vec::new())
            .with_dynamic_fields(SchemaNode::group(
                "value",
                vec![SchemaNode::scalar("leaf", ScalarType::String)],
            ))
            .unwrap(),
    ];
    for document in MAPPING_DOCUMENTS {
        for schema in schemas.clone() {
            let (mut app, context) = setup(document, schema, None, true);
            let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
            context_click(&mut app, &context, true);
            frame(
                &mut app,
                &context,
                true,
                vec![egui::Event::Text("Source property by name".into())],
            );
            let output = settle(&mut app, &context, true);
            frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
            observe(&app, document);
            assert!(text_rect(&output, "No matching nodes").is_some());
            assert!(
                editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace)
                    == before
            );
            assert_eq!(app.history.undo_len(), 0);
            assert!(!app.is_dirty());
        }
    }
    // A named secondary open object cannot make a closed primary source eligible.
    let (mut secondary, secondary_context) = setup(
        MappingDocument::Main,
        SchemaNode::group("Input", Vec::new()),
        None,
        true,
    );
    secondary.project.extra_sources.push(mapping::NamedSource {
        name: "lookup".into(),
        path: "unused.json".into(),
        schema: open_object("Other"),
        options: Default::default(),
        dynamic_path: None,
    });
    settle(&mut secondary, &secondary_context, true);
    secondary.mark_clean();
    secondary.rebase_history();
    let secondary_before = editor_state(
        &secondary.project,
        &secondary.main_canvas.snarl,
        &secondary.mapping_workspace,
    );
    context_click(&mut secondary, &secondary_context, true);
    frame(
        &mut secondary,
        &secondary_context,
        true,
        vec![egui::Event::Text(SOURCE_PROPERTY.into())],
    );
    let secondary_output = settle(&mut secondary, &secondary_context, true);
    frame(
        &mut secondary,
        &secondary_context,
        true,
        vec![key(egui::Key::Enter)],
    );
    observe(&secondary, MappingDocument::Main);
    assert!(text_rect(&secondary_output, "No matching nodes").is_some());
    assert!(
        editor_state(
            &secondary.project,
            &secondary.main_canvas.snarl,
            &secondary.mapping_workspace
        ) == secondary_before
    );
    assert_eq!(secondary.history.undo_len(), 0);
    // An eligible primary schema still does not expose source-frame reads inside an isolated function.
    let (mut app, context) = setup(
        MappingDocument::Function(FUNCTION),
        source_schema(),
        None,
        true,
    );
    let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
    context_click(&mut app, &context, true);
    frame(
        &mut app,
        &context,
        true,
        vec![egui::Event::Text(SOURCE_PROPERTY.into())],
    );
    let output = settle(&mut app, &context, true);
    frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
    observe(&app, MappingDocument::Function(FUNCTION));
    assert!(text_rect(&output, "No matching nodes").is_some());
    assert!(editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace) == before);
    assert_eq!(app.history.undo_len(), 0);
    for document in MAPPING_DOCUMENTS {
        let (mut app, context) = setup(document, source_schema(), None, false);
        let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
        let output = context_click(&mut app, &context, false);
        frame(
            &mut app,
            &context,
            false,
            vec![
                egui::Event::Text(SOURCE_PROPERTY.into()),
                key(egui::Key::Enter),
            ],
        );
        observe(&app, document);
        assert!(text_rect(&output, "Add node").is_none());
        assert!(
            editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace) == before
        );
        assert_eq!(app.history.undo_len(), 0);
        assert!(!app.is_dirty());
    }
}

#[test]
fn source_property_real_menu_reserves_two_final_ids_or_refuses_without_partial_key_or_canvas() {
    for document in MAPPING_DOCUMENTS {
        for available in [true, false] {
            let maximum = if available {
                NodeId::MAX - 2
            } else {
                NodeId::MAX - 1
            };
            let (mut app, context) = setup(document, source_schema(), Some(maximum), true);
            let expected = app.project.clone();
            let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
            let nodes = canvas(&app, document).snarl.nodes().count();
            let wires = canvas(&app, document).snarl.wires().count();
            search(&mut app, &context, SOURCE_PROPERTY, SOURCE_PROPERTY);
            frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
            if available {
                assert_creation(&app, document, expected, NodeId::MAX, NodeId::MAX - 1);
                assert_eq!(canvas(&app, document).snarl.nodes().count(), nodes + 1);
                assert_eq!(canvas(&app, document).snarl.wires().count(), wires);
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
