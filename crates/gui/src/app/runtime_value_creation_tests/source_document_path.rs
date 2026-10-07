use super::*;
use ir::DocumentMember;
use mapping::{FormatOptions, ScopeIteration};

const DOCUMENT_PATH: &str = "Source document path";
const MAPPINGS: [MappingDocument; 2] = [MappingDocument::Main, MappingDocument::Target(0)];
const PRESERVED: &str = "keep primary and named output";

fn scope_mut(project: &mut Project, document: MappingDocument) -> &mut Scope {
    match document {
        MappingDocument::Main => &mut project.root,
        MappingDocument::Target(index) => &mut project.extra_targets[index].root,
        MappingDocument::Function(_) => panic!("isolated functions do not own a source scope"),
    }
}

fn setup(
    document: MappingDocument,
    maximum: Option<NodeId>,
    options: FormatOptions,
    editing: bool,
) -> (FerruleApp, egui::Context) {
    let mut app = super::app(document, maximum);
    app.project.source_options = options;
    if !matches!(document, MappingDocument::Function(_)) {
        scope_mut(&mut app.project, document).iteration = ScopeIteration::Source(Vec::new());
    }
    reset_canvases(&mut app, document);
    let context = egui::Context::default();
    crate::icons::install(&context);
    settle(&mut app, &context, editing);
    app.mark_clean();
    app.rebase_history();
    (app, context)
}

fn file_set_options() -> FormatOptions {
    FormatOptions {
        xml_document: true,
        local_xml_file_set: true,
        ..Default::default()
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
        MappingDocument::Function(_) => panic!("document path wiring is not a function operation"),
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
    eprintln!("Document-path cached native pin IDs: {ids:?}");
    assert!(!ids.is_empty());
    let responses = ids
        .into_iter()
        .map(|id| context.read_response(id).unwrap())
        .collect::<Vec<_>>();
    eprintln!("Document-path original native pin responses: {responses:#?}");
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
        "Document-path actual frame={bounds:?}, wanted={wanted:?}, output={output}, pins={pins:?}"
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

fn single(value: &str) -> Instance {
    Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        )]
        .into(),
    )
}

fn documents() -> Instance {
    Instance::DocumentSet(vec![
        DocumentMember::new_source("portable-a.xml", "/source/A 雪.xml", single("first input"))
            .unwrap(),
        DocumentMember::new("b.xml", single("second input")).unwrap(),
    ])
}

fn assert_outputs(app: &FerruleApp, document: MappingDocument, paths: bool) {
    let input = documents();
    let issues = engine::validate(&app.project);
    let result = engine::run_outputs(&app.project, &input);
    eprintln!(
        "Document-path complete original input={input:#?}, validation={issues:?}, outputs={result:#?}"
    );
    let selected = if paths {
        Instance::Repeated(vec![single("/source/A 雪.xml"), single("b.xml")])
    } else {
        Instance::Repeated(vec![single(PRESERVED), single(PRESERVED)])
    };
    let (primary, named) = if document == MappingDocument::Main {
        (selected, single(PRESERVED))
    } else {
        (single(PRESERVED), selected)
    };
    assert!(issues.is_empty());
    let actual = result.expect("real wired document-path node executes in each active document");
    assert_eq!(actual.primary, primary);
    assert_eq!(
        actual.extras,
        vec![engine::NamedOutput {
            name: "audit".into(),
            instance: named
        }]
    );
}

fn assert_creation(
    app: &FerruleApp,
    document: MappingDocument,
    mut expected: Project,
    created: NodeId,
) {
    observe(app, document);
    let issues = engine::validate(&app.project);
    eprintln!("Document-path creation original validation={issues:?}");
    expected
        .graph
        .nodes
        .insert(created, Node::SourceDocumentPath);
    assert_eq!(
        serde_json::to_value(&app.project).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(issues.is_empty());
    assert_eq!(
        canvas(app, document)
            .snarl
            .nodes()
            .filter(|node| **node == CanvasNode::Graph(created))
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
    assert_eq!(app.history.undo_len(), 1);
    assert!(app.is_dirty());
    assert!(app.diagnostics.is_empty());
}

#[test]
fn source_document_path_real_creation_wiring_and_history_preserve_ordered_complete_outputs() {
    for document in MAPPINGS {
        let (mut app, context) = setup(document, None, file_set_options(), true);
        let before = app.project.clone();
        let nodes = canvas(&app, document).snarl.nodes().count();
        let wires = canvas(&app, document).snarl.wires().count();
        let rendered = search(&mut app, &context, "provenance", DOCUMENT_PATH);
        if document == MappingDocument::Main {
            frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
        } else {
            click(
                &mut app,
                &context,
                text_rect(&rendered, DOCUMENT_PATH).unwrap().center(),
            );
        }
        assert_creation(&app, document, before.clone(), 1);
        assert_eq!(canvas(&app, document).snarl.nodes().count(), nodes + 1);
        assert_eq!(canvas(&app, document).snarl.wires().count(), wires);
        let created = app.project.clone();
        let position = canvas(&app, document)
            .snarl
            .nodes_pos()
            .find_map(|(position, node)| (*node == CanvasNode::Graph(1)).then_some(position))
            .unwrap();
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
        assert_eq!(
            canvas(&app, document)
                .snarl
                .nodes_pos()
                .find_map(|(point, node)| (*node == CanvasNode::Graph(1)).then_some(point)),
            Some(position)
        );
        arrange(&mut app, &context, document);
        app.mark_clean();
        app.rebase_history();
        let unwired = app.project.clone();
        let from = native_pin(&app, &context, document, CanvasNode::Graph(1), true);
        let to = native_pin(&app, &context, document, CanvasNode::TargetBlock(0), false);
        wire(&mut app, &context, from, to);
        observe(&app, document);
        let mut expected = unwired.clone();
        scope_mut(&mut expected, document).bindings[0].node = 1;
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert_eq!(canvas(&app, document).snarl.wires().count(), wires);
        assert_outputs(&app, document, true);
        app.undo_project();
        observe(&app, document);
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&unwired).unwrap()
        );
        assert_outputs(&app, document, false);
        app.redo_project();
        observe(&app, document);
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert_outputs(&app, document, true);
        let encoded = mapping::project_file::encode_pretty(&app.project);
        eprintln!("Document-path full saved codec original: {encoded:?}");
        let reopened = mapping::project_file::decode_str(&encoded.unwrap());
        eprintln!("Document-path full reopened codec original: {reopened:#?}");
        assert_eq!(
            serde_json::to_value(reopened.unwrap()).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
}

#[test]
fn source_document_path_real_palette_refuses_ordinary_named_only_function_and_locked_contexts() {
    for document in MAPPINGS {
        for options in [
            FormatOptions::default(),
            FormatOptions {
                xml_document: true,
                ..Default::default()
            },
        ] {
            let (mut app, context) = setup(document, None, options, true);
            let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
            context_click(&mut app, &context, true);
            frame(
                &mut app,
                &context,
                true,
                vec![egui::Event::Text(DOCUMENT_PATH.into())],
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
        let (mut app, context) = setup(document, None, FormatOptions::default(), true);
        app.project.extra_sources.push(mapping::NamedSource {
            name: "lookup".into(),
            path: "other-*.xml".into(),
            schema: app.project.source.clone(),
            options: file_set_options(),
            dynamic_path: None,
        });
        // An existing imported node remains intact on inspection; it does not enable creation.
        app.project.graph.nodes.insert(1, Node::SourceDocumentPath);
        reset_canvases(&mut app, document);
        settle(&mut app, &context, true);
        app.mark_clean();
        app.rebase_history();
        let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
        context_click(&mut app, &context, true);
        frame(
            &mut app,
            &context,
            true,
            vec![egui::Event::Text(DOCUMENT_PATH.into())],
        );
        let output = settle(&mut app, &context, true);
        frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
        observe(&app, document);
        assert!(text_rect(&output, "No matching nodes").is_some());
        assert!(
            editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace) == before
        );
        assert!(matches!(
            app.project.graph.nodes[&1],
            Node::SourceDocumentPath
        ));
        assert_eq!(app.history.undo_len(), 0);
        assert!(!app.is_dirty());
    }
    let (mut app, context) = setup(
        MappingDocument::Function(FUNCTION),
        None,
        file_set_options(),
        true,
    );
    let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
    context_click(&mut app, &context, true);
    frame(
        &mut app,
        &context,
        true,
        vec![egui::Event::Text(DOCUMENT_PATH.into())],
    );
    let output = settle(&mut app, &context, true);
    frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
    observe(&app, MappingDocument::Function(FUNCTION));
    assert!(text_rect(&output, "No matching nodes").is_some());
    assert!(editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace) == before);
    assert_eq!(app.history.undo_len(), 0);
    assert!(!app.is_dirty());
    for document in MAPPINGS {
        let (mut app, context) = setup(document, None, file_set_options(), false);
        let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
        let output = context_click(&mut app, &context, false);
        frame(
            &mut app,
            &context,
            false,
            vec![
                egui::Event::Text(DOCUMENT_PATH.into()),
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
fn source_document_path_created_node_keeps_missing_metadata_and_lazy_fallback_semantics() {
    let (mut app, context) = setup(MappingDocument::Main, None, file_set_options(), true);
    search(&mut app, &context, DOCUMENT_PATH, DOCUMENT_PATH);
    frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
    assert!(matches!(
        app.project.graph.nodes[&1],
        Node::SourceDocumentPath
    ));
    let mut project = app.project.clone();
    project.root.iteration = ScopeIteration::None;
    project.root.bindings[0].node = 1;
    for input in [
        Instance::DocumentSet(Vec::new()),
        single("no document metadata"),
    ] {
        let result = engine::run_outputs(&project, &input);
        eprintln!("Document-path original missing metadata input={input:#?}, result={result:#?}");
        assert!(
            matches!(result, Err(engine::EngineError::MissingSourceField(ref path)) if path == "<document-path>")
        );
    }
    let result = engine::run_outputs(&project, &documents());
    eprintln!("Document-path original parent-context first-member fallback: {result:#?}");
    let actual = result.unwrap();
    assert_eq!(actual.primary, single("/source/A 雪.xml"));
    assert_eq!(
        actual.extras,
        vec![engine::NamedOutput {
            name: "audit".into(),
            instance: single(PRESERVED)
        }]
    );
    project.root.bindings[0].node = 0;
    let result = engine::run_outputs(&project, &Instance::DocumentSet(Vec::new()));
    eprintln!("Document-path original unreachable read on empty source: {result:#?}");
    let actual = result.unwrap();
    assert_eq!(actual.primary, single(PRESERVED));
    assert_eq!(
        actual.extras,
        vec![engine::NamedOutput {
            name: "audit".into(),
            instance: single(PRESERVED)
        }]
    );
}

#[test]
fn source_document_path_real_menu_reserves_one_final_id_or_refuses_atomically() {
    for document in MAPPINGS {
        for available in [true, false] {
            let maximum = if available {
                NodeId::MAX - 1
            } else {
                NodeId::MAX
            };
            let (mut app, context) = setup(document, Some(maximum), file_set_options(), true);
            let expected = app.project.clone();
            let before = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
            let nodes = canvas(&app, document).snarl.nodes().count();
            let wires = canvas(&app, document).snarl.wires().count();
            search(&mut app, &context, DOCUMENT_PATH, DOCUMENT_PATH);
            frame(&mut app, &context, true, vec![key(egui::Key::Enter)]);
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
