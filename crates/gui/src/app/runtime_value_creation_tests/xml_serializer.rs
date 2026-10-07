use super::*;

const SERIALIZE: &str = "Serialize source as XML";
const MAPPINGS: [MappingDocument; 2] = [MappingDocument::Main, MappingDocument::Target(0)];
const PRESERVED: &str = "keep primary and named output";
const ROOT_XML: &str = "<Input>\n  <Payload Code=\"A&amp;B\">\n    <Name>Ada 雪 &lt;x&gt;</Name>\n    <Tag>one</Tag>\n    <Tag>two</Tag>\n  </Payload>\n  <Other>\n    <Name>second</Name>\n  </Other>\n</Input>";
const PAYLOAD_XML: &str =
    "<Payload Code=\"A&amp;B\"><Name>Ada 雪 &lt;x&gt;</Name><Tag>one</Tag><Tag>two</Tag></Payload>";

fn payload_schema() -> SchemaNode {
    let mut code = SchemaNode::scalar("Code", ScalarType::String);
    code.attribute = true;
    SchemaNode::group(
        "Payload",
        vec![
            code,
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Tag", ScalarType::String).repeating(),
        ],
    )
}

fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            payload_schema(),
            SchemaNode::group(
                "Other",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            ),
        ],
    )
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn scalar(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn input() -> Instance {
    group(vec![
        (
            "Payload",
            group(vec![
                ("Code", scalar("A&B")),
                ("Name", scalar("Ada 雪 <x>")),
                (
                    "Tag",
                    Instance::Repeated(vec![scalar("one"), scalar("two")]),
                ),
            ]),
        ),
        ("Other", group(vec![("Name", scalar("second"))])),
    ])
}

fn serializer(path: Vec<String>, schema: SchemaNode) -> Node {
    Node::XmlSerialize {
        path,
        frame: None,
        schema: Box::new(schema),
        declaration: false,
        indent: true,
        namespace: None,
    }
}

fn setup(
    document: MappingDocument,
    schema: SchemaNode,
    maximum: Option<NodeId>,
    editing: bool,
) -> (FerruleApp, egui::Context) {
    let mut app = super::app(document, maximum);
    app.project.source = schema;
    reset_canvases(&mut app, document);
    let context = egui::Context::default();
    crate::icons::install(&context);
    settle(&mut app, &context, editing);
    app.mark_clean();
    app.rebase_history();
    (app, context)
}

fn scope_mut(project: &mut Project, document: MappingDocument) -> &mut Scope {
    match document {
        MappingDocument::Main => &mut project.root,
        MappingDocument::Target(index) => &mut project.extra_targets[index].root,
        MappingDocument::Function(_) => panic!("isolated functions do not own a source scope"),
    }
}

fn reset_canvases(app: &mut FerruleApp, document: MappingDocument) {
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mapping_workspace.target_canvases.clear();
    if let MappingDocument::Target(index) = document {
        assert!(app.ensure_target_canvas(index));
    }
}

fn settle(app: &mut FerruleApp, context: &egui::Context, editing: bool) -> egui::FullOutput {
    let mut output = frame(app, context, editing, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, editing, Vec::new());
    }
    output
}

fn click(app: &mut FerruleApp, context: &egui::Context, pos: egui::Pos2) {
    frame(app, context, true, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            app,
            context,
            true,
            vec![pointer(pos, egui::PointerButton::Primary, pressed)],
        );
    }
    settle(app, context, true);
}

fn canvas_mut(app: &mut FerruleApp, document: MappingDocument) -> &mut CanvasDocumentState {
    match document {
        MappingDocument::Main => &mut app.main_canvas,
        MappingDocument::Target(index) => app
            .mapping_workspace
            .target_canvases
            .get_mut(&index)
            .unwrap(),
        MappingDocument::Function(_) => panic!("XML serializer wiring is not a function operation"),
    }
}

fn arrange(app: &mut FerruleApp, context: &egui::Context, document: MappingDocument) {
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
            CanvasNode::Graph(1) => egui::pos2(400.0, 0.0),
            _ => egui::pos2(400.0, 250.0),
        };
    }
    state.reset_view();
    settle(app, context, true);
}

fn native_pin(
    app: &FerruleApp,
    context: &egui::Context,
    document: MappingDocument,
    wanted: CanvasNode,
    output: bool,
) -> egui::Pos2 {
    let salt = match document {
        MappingDocument::Main => egui::Id::new("main_mapping_canvas"),
        MappingDocument::Target(index) => egui::Id::new(("named_target_canvas", index)),
        MappingDocument::Function(_) => unreachable!(),
    };
    let ids = crate::canvas_keyboard::current_pin_interaction_ids(
        context,
        app.embedded_canvas_id(salt),
        canvas(app, document).view_generation,
    );
    eprintln!("XML-serializer cached native pin IDs: {ids:?}");
    assert!(!ids.is_empty());
    let responses = ids
        .into_iter()
        .map(|id| context.read_response(id).unwrap())
        .collect::<Vec<_>>();
    eprintln!("XML-serializer original native pin responses: {responses:#?}");
    let node = canvas(app, document)
        .snarl
        .node_ids()
        .find_map(|(id, node)| (*node == wanted).then_some(id))
        .unwrap();
    let response = responses
        .iter()
        .find_map(|pin| {
            context.read_response(pin.layer_id.id.with(("snarl-node", node)).with("frame"))
        })
        .expect("registered native node frame");
    let bounds = context
        .layer_transform_to_global(response.layer_id)
        .unwrap_or_default()
        * response.rect;
    let pins = responses
        .into_iter()
        .filter_map(|pin| {
            let center = context
                .layer_transform_to_global(pin.layer_id)
                .unwrap_or_default()
                * pin.rect.center();
            let edge = if output {
                bounds.right()
            } else {
                bounds.left()
            };
            (pin.sense.senses_drag()
                && bounds.expand(16.0).contains(center)
                && (center.x - edge).abs() < 20.0)
                .then_some(center)
        })
        .collect::<Vec<_>>();
    eprintln!(
        "XML-serializer actual frame={bounds:?}, wanted={wanted:?}, output={output}, pins={pins:?}"
    );
    assert_eq!(
        pins.len(),
        1,
        "one actual native pin on the requested fixture edge"
    );
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0)).contains(pins[0])
    );
    pins[0]
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

fn node_rect(
    app: &FerruleApp,
    context: &egui::Context,
    document: MappingDocument,
    wanted: CanvasNode,
) -> egui::Rect {
    let node = canvas(app, document)
        .snarl
        .node_ids()
        .find_map(|(id, node)| (*node == wanted).then_some(id))
        .unwrap();
    let salt = match document {
        MappingDocument::Main => egui::Id::new("main_mapping_canvas"),
        MappingDocument::Target(index) => egui::Id::new(("named_target_canvas", index)),
        MappingDocument::Function(_) => unreachable!(),
    };
    let ids = crate::canvas_keyboard::current_pin_interaction_ids(
        context,
        app.embedded_canvas_id(salt),
        canvas(app, document).view_generation,
    );
    let response = ids
        .into_iter()
        .filter_map(|id| context.read_response(id))
        .find_map(|pin| {
            context.read_response(pin.layer_id.id.with(("snarl-node", node)).with("frame"))
        })
        .expect("existing registered serializer frame");
    eprintln!("XML serializer original frame response={response:?}");
    context
        .layer_transform_to_global(response.layer_id)
        .unwrap_or_default()
        * response.rect
}

fn open_editor(
    app: &mut FerruleApp,
    context: &egui::Context,
    document: MappingDocument,
) -> egui::FullOutput {
    let rendered = settle(app, context, true);
    if text_rect(&rendered, "XML declaration").is_some() {
        return rendered;
    }
    let bounds = node_rect(app, context, document, CanvasNode::Graph(1));
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    let positions = visible_texts(&rendered, &pencil)
        .into_iter()
        .filter(|pos| bounds.contains(*pos))
        .collect::<Vec<_>>();
    eprintln!("XML serializer original pencil positions={positions:?}, frame={bounds:?}");
    assert_eq!(positions.len(), 1);
    click(app, context, positions[0]);
    settle(app, context, true)
}

fn choose_source(app: &mut FerruleApp, context: &egui::Context, desired: &str) {
    let rendered = settle(app, context, true);
    let button = visible_texts(&rendered, "Choose source element");
    eprintln!("XML serializer original source picker={button:?}");
    assert_eq!(button.len(), 1);
    click(app, context, button[0]);
    let rendered = settle(app, context, true);
    let choices = visible_texts(&rendered, desired);
    eprintln!("XML serializer original supported source choice {desired:?}={choices:?}");
    assert_eq!(choices.len(), 1);
    click(app, context, choices[0]);
}

fn create(app: &mut FerruleApp, context: &egui::Context, document: MappingDocument) {
    // A distinct query avoids confusing the editable search text with its row.
    let rendered = search(app, context, "xml fragment", SERIALIZE);
    if document == MappingDocument::Main {
        frame(app, context, true, vec![key(egui::Key::Enter)]);
    } else {
        click(
            app,
            context,
            text_rect(&rendered, SERIALIZE).unwrap().center(),
        );
    }
    observe(app, document);
}

fn assert_creation(app: &FerruleApp, document: MappingDocument, mut expected: Project, id: NodeId) {
    expected
        .graph
        .nodes
        .insert(id, serializer(Vec::new(), source_schema()));
    observe(app, document);
    let issues = engine::validate(&app.project);
    eprintln!("XML serializer original creation validation={issues:?}");
    assert_eq!(
        serde_json::to_value(&app.project).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(issues.is_empty());
    assert_eq!(
        canvas(app, document)
            .snarl
            .nodes()
            .filter(|node| **node == CanvasNode::Graph(id))
            .count(),
        1
    );
    assert!(
        !app.project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Unconnected))
    );
    assert!(
        !canvas(app, document)
            .snarl
            .nodes()
            .any(|node| matches!(node, CanvasNode::Placeholder(_)))
    );
    assert!(app.is_dirty());
    assert_eq!(app.history.undo_len(), 1);
    assert!(app.diagnostics.is_empty());
}

fn assert_outputs(app: &FerruleApp, document: MappingDocument, expected: &str) {
    let input = input();
    let issues = engine::validate(&app.project);
    let outputs = engine::run_outputs(&app.project, &input);
    eprintln!(
        "XML serializer full original input={input:#?}, validation={issues:?}, outputs={outputs:#?}"
    );
    let selected = group(vec![("Value", scalar(expected))]);
    let preserved = group(vec![("Value", scalar(PRESERVED))]);
    assert!(issues.is_empty());
    let outputs = outputs.unwrap();
    assert_eq!(
        outputs.primary,
        if document == MappingDocument::Main {
            selected.clone()
        } else {
            preserved.clone()
        }
    );
    assert_eq!(
        outputs.extras,
        vec![engine::NamedOutput {
            name: "audit".into(),
            instance: if document == MappingDocument::Target(0) {
                selected
            } else {
                preserved
            }
        }]
    );
}

#[test]
fn xml_serializer_real_creation_and_wire_keep_complete_primary_named_outputs_and_history() {
    for document in MAPPINGS {
        let (mut app, context) = setup(document, source_schema(), None, true);
        let before = app.project.clone();
        let before_nodes = canvas(&app, document).snarl.nodes().count();
        let before_wires = canvas(&app, document).snarl.wires().count();
        create(&mut app, &context, document);
        assert_creation(&app, document, before.clone(), 1);
        assert_eq!(
            canvas(&app, document).snarl.nodes().count(),
            before_nodes + 1
        );
        assert_eq!(canvas(&app, document).snarl.wires().count(), before_wires);
        let created = app.project.clone();
        app.undo_project();
        observe(&app, document);
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        assert!(!app.is_dirty());
        app.redo_project();
        observe(&app, document);
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&created).unwrap()
        );
        arrange(&mut app, &context, document);
        app.mark_clean();
        app.rebase_history();
        let unwired = app.project.clone();
        let from = native_pin(&app, &context, document, CanvasNode::Graph(1), true);
        let to = native_pin(&app, &context, document, CanvasNode::TargetBlock(0), false);
        wire(&mut app, &context, from, to);
        observe(&app, document);
        let mut wired = unwired.clone();
        scope_mut(&mut wired, document).bindings[0].node = 1;
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&wired).unwrap()
        );
        assert_eq!(canvas(&app, document).snarl.wires().count(), before_wires);
        assert_outputs(&app, document, ROOT_XML);
        app.undo_project();
        observe(&app, document);
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&unwired).unwrap()
        );
        assert_outputs(&app, document, PRESERVED);
        app.redo_project();
        observe(&app, document);
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&wired).unwrap()
        );
        assert_outputs(&app, document, ROOT_XML);
        let encoded = mapping::project_file::encode_pretty(&app.project);
        eprintln!("XML serializer complete saved project original={encoded:?}");
        let reopened = mapping::project_file::decode_str(&encoded.unwrap());
        eprintln!("XML serializer complete reopened project original={reopened:#?}");
        assert_eq!(
            serde_json::to_value(reopened.unwrap()).unwrap(),
            serde_json::to_value(&wired).unwrap()
        );
    }
}

#[test]
fn xml_serializer_real_source_settings_preserve_imported_metadata_until_explicit_choice() {
    for document in MAPPINGS {
        let (mut app, context) = setup(document, source_schema(), None, true);
        create(&mut app, &context, document);
        scope_mut(&mut app.project, document).bindings[0].node = 1;
        arrange(&mut app, &context, document);
        app.mark_clean();
        app.rebase_history();
        open_editor(&mut app, &context, document);
        choose_source(&mut app, &context, "Payload");
        observe(&app, document);
        let Node::XmlSerialize {
            path,
            frame: stored_frame,
            schema,
            ..
        } = &app.project.graph.nodes[&1]
        else {
            panic!("serializer remains typed");
        };
        assert_eq!(path, &["Payload"]);
        assert_eq!(stored_frame, &None);
        assert_eq!(schema.as_ref(), &payload_schema());
        let rendered = open_editor(&mut app, &context, document);
        click(
            &mut app,
            &context,
            text_rect(&rendered, "indent output").unwrap().center(),
        );
        observe(&app, document);
        assert_outputs(&app, document, PAYLOAD_XML);
        let rendered = open_editor(&mut app, &context, document);
        click(
            &mut app,
            &context,
            text_rect(&rendered, "XML declaration").unwrap().center(),
        );
        observe(&app, document);
        assert_outputs(
            &app,
            document,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Payload Code=\"A&amp;B\"><Name>Ada 雪 &lt;x&gt;</Name><Tag>one</Tag><Tag>two</Tag></Payload>",
        );
        open_editor(&mut app, &context, document);
        choose_source(&mut app, &context, "Other");
        observe(&app, document);
        assert_outputs(
            &app,
            document,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Other><Name>second</Name></Other>",
        );
    }
    for stored_frame in [None, Some(Vec::new()), Some(vec!["legacy rows".into()])] {
        let (mut app, context) = setup(MappingDocument::Main, source_schema(), None, true);
        let imported = Node::XmlSerialize {
            path: vec!["legacy element".into()],
            frame: stored_frame,
            schema: Box::new(SchemaNode::group(
                "Unknown",
                vec![SchemaNode::scalar("Legacy", ScalarType::String)],
            )),
            declaration: true,
            indent: false,
            namespace: Some("urn:legacy".into()),
        };
        app.project.graph.nodes.insert(1, imported);
        reset_canvases(&mut app, MappingDocument::Main);
        arrange(&mut app, &context, MappingDocument::Main);
        app.mark_clean();
        app.rebase_history();
        let before = app.project.clone();
        let rendered = open_editor(&mut app, &context, MappingDocument::Main);
        observe(&app, MappingDocument::Main);
        assert!(text_rect(&rendered, "source: legacy element").is_some());
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        assert_eq!(app.history.undo_len(), 0);
        assert!(!app.is_dirty());
        choose_source(&mut app, &context, "Payload");
        observe(&app, MappingDocument::Main);
        let mut expected = before.clone();
        expected.graph.nodes.insert(
            1,
            Node::XmlSerialize {
                path: vec!["Payload".into()],
                frame: None,
                schema: Box::new(payload_schema()),
                declaration: true,
                indent: false,
                namespace: Some("urn:legacy".into()),
            },
        );
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        app.undo_project();
        observe(&app, MappingDocument::Main);
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        app.redo_project();
        observe(&app, MappingDocument::Main);
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
}

#[test]
fn xml_serializer_real_menu_refusals_and_created_node_keep_missing_lazy_and_writer_errors() {
    let mut repeated = source_schema();
    repeated.repeating = true;
    let mut deep = SchemaNode::scalar("Value", ScalarType::String);
    for _ in 0..17 {
        deep = SchemaNode::group("Level", vec![deep]);
    }
    let wide = SchemaNode::group(
        "Input",
        (0..256)
            .map(|index| SchemaNode::scalar(format!("Field{index}"), ScalarType::String))
            .collect(),
    );
    let open = SchemaNode::group("Input", Vec::new())
        .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String))
        .unwrap();
    let mut metadata = source_schema();
    metadata.default = Some("retained unsupported metadata".into());
    let many_choices = SchemaNode::group(
        "Input",
        (0..32)
            .map(|index| SchemaNode::group(format!("Group{index}"), Vec::new()))
            .collect(),
    );
    let text_budget = SchemaNode::group(
        "Input",
        (0..17)
            .map(|index| {
                SchemaNode::scalar(
                    format!("Field{index}{}", "x".repeat(4088)),
                    ScalarType::String,
                )
            })
            .collect(),
    );
    for schema in [
        SchemaNode::scalar("Input", ScalarType::String),
        repeated,
        deep,
        wide,
        open,
        metadata,
        many_choices,
        text_budget,
    ] {
        for document in MAPPINGS {
            let (mut app, context) = setup(document, schema.clone(), None, true);
            let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
            context_click(&mut app, &context, true);
            frame(
                &mut app,
                &context,
                true,
                vec![egui::Event::Text("xml fragment".into())],
            );
            let rendered = settle(&mut app, &context, true);
            frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
            observe(&app, document);
            assert!(text_rect(&rendered, "No matching nodes").is_some());
            assert!(
                editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace)
                    == before
            );
            assert_eq!(app.history.undo_len(), 0);
            assert!(!app.is_dirty());
        }
    }
    for (document, editing) in [
        (MappingDocument::Function(FUNCTION), true),
        (MappingDocument::Main, false),
        (MappingDocument::Target(0), false),
    ] {
        let (mut app, context) = setup(document, source_schema(), None, editing);
        let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
        context_click(&mut app, &context, editing);
        frame(
            &mut app,
            &context,
            editing,
            vec![egui::Event::Text("xml fragment".into())],
        );
        let rendered = settle(&mut app, &context, editing);
        frame(&mut app, &context, editing, vec![key(egui::Key::Enter)]);
        observe(&app, document);
        if editing {
            assert!(text_rect(&rendered, "No matching nodes").is_some());
        } else {
            assert!(text_rect(&rendered, "Add node").is_none());
        }
        assert!(
            editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace) == before
        );
        assert_eq!(app.history.undo_len(), 0);
        assert!(!app.is_dirty());
    }
    // Repetition inside a selected element is serializable; selecting a
    // descendant across that repetition would need a frame and is not offered.
    let rows = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Row",
                vec![SchemaNode::group(
                    "Detail",
                    vec![SchemaNode::scalar("Name", ScalarType::String)],
                )],
            )
            .repeating(),
        ],
    );
    let (mut nested, nested_context) = setup(MappingDocument::Main, rows, None, true);
    create(&mut nested, &nested_context, MappingDocument::Main);
    arrange(&mut nested, &nested_context, MappingDocument::Main);
    let rendered = open_editor(&mut nested, &nested_context, MappingDocument::Main);
    click(
        &mut nested,
        &nested_context,
        text_rect(&rendered, "Choose source element")
            .unwrap()
            .center(),
    );
    let choices = settle(&mut nested, &nested_context, true);
    observe(&nested, MappingDocument::Main);
    assert!(text_rect(&choices, "<source root>").is_some());
    assert!(text_rect(&choices, "Row").is_none());
    assert!(text_rect(&choices, "Row / Detail").is_none());
    frame(
        &mut nested,
        &nested_context,
        true,
        vec![key(egui::Key::Escape)],
    );
    nested.project.root.bindings[0].node = 1;
    let repeated_input = group(vec![(
        "Row",
        Instance::Repeated(vec![
            group(vec![("Detail", group(vec![("Name", scalar("first"))]))]),
            group(vec![("Detail", group(vec![("Name", scalar("second"))]))]),
        ]),
    )]);
    let repeated_result = engine::run_outputs(&nested.project, &repeated_input);
    eprintln!(
        "XML serializer full repeated descendants input={repeated_input:#?}, result={repeated_result:#?}"
    );
    let repeated_output = repeated_result.unwrap();
    assert_eq!(
        repeated_output.primary,
        group(vec![(
            "Value",
            scalar(
                "<Input>\n  <Row>\n    <Detail>\n      <Name>first</Name>\n    </Detail>\n  </Row>\n  <Row>\n    <Detail>\n      <Name>second</Name>\n    </Detail>\n  </Row>\n</Input>"
            )
        )])
    );
    assert_eq!(
        repeated_output.extras,
        vec![engine::NamedOutput {
            name: "audit".into(),
            instance: group(vec![("Value", scalar(PRESERVED))])
        }]
    );
    let (mut app, context) = setup(MappingDocument::Main, source_schema(), None, true);
    create(&mut app, &context, MappingDocument::Main);
    arrange(&mut app, &context, MappingDocument::Main);
    open_editor(&mut app, &context, MappingDocument::Main);
    choose_source(&mut app, &context, "Payload");
    scope_mut(&mut app.project, MappingDocument::Main).bindings[0].node = 1;
    let missing = engine::run_outputs(&app.project, &group(Vec::new()));
    eprintln!("XML serializer complete original absent element result={missing:#?}");
    assert!(
        matches!(missing, Err(engine::EngineError::MissingSourceField(ref path)) if path == "Payload")
    );
    let bad = group(vec![(
        "Payload",
        group(vec![
            ("Code", scalar("ok")),
            ("Name", scalar("invalid\u{1}")),
            ("Tag", Instance::Repeated(Vec::new())),
        ]),
    )]);
    let error = engine::run_outputs(&app.project, &bad);
    eprintln!(
        "XML serializer complete original illegal XML character result={error:#?}, input={bad:#?}"
    );
    assert!(matches!(
        error,
        Err(engine::EngineError::XmlSerialization { node: 1, .. })
    ));
    let mut lazy = app.project.clone();
    lazy.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    lazy.graph.nodes.insert(
        3,
        Node::If {
            condition: 2,
            then: 1,
            else_: 0,
        },
    );
    lazy.root.bindings[0].node = 3;
    let outcomes = engine::run_outputs(&lazy, &bad);
    eprintln!("XML serializer full original unselected serialization branch={outcomes:#?}");
    let outputs = outcomes.unwrap();
    assert_eq!(outputs.primary, group(vec![("Value", scalar(PRESERVED))]));
    assert_eq!(
        outputs.extras,
        vec![engine::NamedOutput {
            name: "audit".into(),
            instance: group(vec![("Value", scalar(PRESERVED))])
        }]
    );
}

#[test]
fn xml_serializer_real_menu_reserves_one_final_id_or_refuses_without_partial_graph_history() {
    for document in MAPPINGS {
        for available in [true, false] {
            let maximum = if available {
                NodeId::MAX - 1
            } else {
                NodeId::MAX
            };
            let (mut app, context) = setup(document, source_schema(), Some(maximum), true);
            let expected = app.project.clone();
            let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
            let nodes = canvas(&app, document).snarl.nodes().count();
            let wires = canvas(&app, document).snarl.wires().count();
            create(&mut app, &context, document);
            if available {
                assert_creation(&app, document, expected, NodeId::MAX);
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
