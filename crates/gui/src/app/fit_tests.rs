use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, NamedTarget, Node, UserFunction};

fn fixture() -> FerruleApp {
    let schema = SchemaNode::group(
        "Record",
        vec![
            SchemaNode::scalar("out", ScalarType::String),
            SchemaNode::scalar("a", ScalarType::String),
            SchemaNode::scalar("b", ScalarType::String),
            SchemaNode::scalar("c", ScalarType::String),
            SchemaNode::scalar("d", ScalarType::String),
            SchemaNode::scalar("e", ScalarType::String),
        ],
    );
    let mut app = FerruleApp {
        show_minimap: false,
        ..Default::default()
    };
    app.project.source = schema.clone();
    app.project.target = schema.clone();
    app.project.graph.nodes.clear();
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::String("one".to_owned()),
        },
    );
    app.project.graph.nodes.insert(
        2,
        Node::Call {
            function: "concat".to_owned(),
            args: vec![1],
        },
    );
    app.project.root = Scope {
        bindings: vec![Binding {
            target_field: "out".to_owned(),
            node: 2,
        }],
        ..Default::default()
    };
    app.project.extra_targets.push(NamedTarget {
        name: "archive".to_owned(),
        path: None,
        schema,
        options: FormatOptions::default(),
        root: app.project.root.clone(),
    });
    let mut body = Graph::default();
    body.nodes.insert(
        10,
        Node::Const {
            value: ir::Value::String("result".to_owned()),
        },
    );
    app.project.user_functions.insert(
        FunctionId::new(42),
        UserFunction {
            library: "local".to_owned(),
            name: "label".to_owned(),
            description: None,
            parameters: Vec::new(),
            output_name: "result".to_owned(),
            output_type: ScalarType::String,
            body,
            output: 10,
        },
    );
    app.main_canvas.snarl = build_snarl(&app.project);
    eprintln!(
        "fit fixture complete project before lazy canvas initialization: {}",
        serde_json::to_string(&app.project).expect("fixture project original")
    );
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    app.open_function_tab(FunctionId::new(42));
    assert!(app.ensure_function_canvas(FunctionId::new(42)));
    for document in documents() {
        let canvas = canvas_mut(&mut app, document);
        let positions: Vec<_> = canvas
            .snarl
            .node_ids()
            .map(|(id, &value)| {
                let position = match value {
                    CanvasNode::SourceBlock(_) => egui::pos2(-200.0, -100.0),
                    CanvasNode::TargetBlock(_) => egui::pos2(700.0, 350.0),
                    CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => {
                        egui::pos2(id as f32 * 160.0, (id % 3) as f32 * 320.0)
                    }
                };
                (id, position)
            })
            .collect();
        for (id, position) in positions {
            canvas
                .snarl
                .get_node_info_mut(id)
                .expect("fixture node")
                .pos = position;
        }
    }
    app.mark_clean();
    app.rebase_history();
    app
}

fn documents() -> [MappingDocument; 3] {
    [
        MappingDocument::Main,
        MappingDocument::Target(0),
        MappingDocument::Function(FunctionId::new(42)),
    ]
}

fn canvas_mut(app: &mut FerruleApp, document: MappingDocument) -> &mut CanvasDocumentState {
    match document {
        MappingDocument::Main => &mut app.main_canvas,
        MappingDocument::Target(id) => app
            .mapping_workspace
            .target_canvases
            .get_mut(&id)
            .expect("target canvas"),
        MappingDocument::Function(id) => app
            .mapping_workspace
            .function_canvases
            .get_mut(&id)
            .expect("function canvas"),
    }
}

fn canvas(app: &FerruleApp, document: MappingDocument) -> &CanvasDocumentState {
    match document {
        MappingDocument::Main => &app.main_canvas,
        MappingDocument::Target(id) => &app.mapping_workspace.target_canvases[&id],
        MappingDocument::Function(id) => &app.mapping_workspace.function_canvases[&id],
    }
}

fn complete_original(app: &FerruleApp) -> (String, String, Vec<String>, usize, usize) {
    (
        serde_json::to_string(&app.project).expect("project original"),
        serde_json::to_string(&CanvasLayout::capture(
            &app.project,
            &app.main_canvas.snarl,
            &app.mapping_workspace,
        ))
        .expect("layout original"),
        documents()
            .into_iter()
            .map(|document| format!("{:#?}", canvas(app, document).snarl))
            .collect(),
        app.history.undo_len(),
        app.history.redo_len(),
    )
}

fn settle(app: &mut FerruleApp, context: &egui::Context, size: egui::Vec2) {
    for _ in 0..12 {
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| app.show_mapping_workspace_canvas(ui, false));
            },
        );
    }
}

fn assert_complete_visible_frames(app: &FerruleApp, context: &egui::Context) {
    let document = app.mapping_workspace.active;
    let salt = app.embedded_canvas_id(match document {
        MappingDocument::Main => egui::Id::new("main_mapping_canvas"),
        MappingDocument::Target(id) => egui::Id::new(("named_target_canvas", id)),
        MappingDocument::Function(id) => egui::Id::new(("function_mapping_canvas", id.get())),
    });
    let canvas = canvas(app, document);
    let actual = crate::canvas_keyboard::current_fit_rects(
        context,
        salt,
        canvas.view_generation,
        &canvas.snarl,
    );
    eprintln!("fit complete original document={document:?}, actual={actual:#?}");
    let (viewport, frames) = actual.expect("every live node has a current rendered frame");
    assert_eq!(frames.len(), canvas.snarl.node_ids().count());
    assert!(!frames.is_empty());
    for (id, frame) in frames {
        assert!(
            frame.is_finite() && frame.is_positive(),
            "invalid frame {id:?}: {frame:?}"
        );
        assert!(
            viewport.shrink(11.9).contains_rect(frame),
            "Fit clipped {id:?}: {frame:?} outside {viewport:?}"
        );
    }
}

#[test]
fn fit_rendered_frames_main_named_function_and_resize_preserve_complete_editor_state() {
    let mut app = fixture();
    let context = egui::Context::default();
    crate::icons::install(&context);
    for document in documents() {
        app.mapping_workspace.active = document;
        app.mapping_workspace.focused = document;
        // Warm the ordinary wire reconciliation before freezing editor state.
        settle(&mut app, &context, egui::vec2(1200.0, 900.0));
        let before = complete_original(&app);
        eprintln!("fit editor before {document:?}: {before:#?}");
        assert!(app.fit_document(document));
        for size in [
            egui::vec2(900.0, 700.0),
            egui::vec2(1200.0, 900.0),
            egui::vec2(900.0, 700.0),
        ] {
            settle(&mut app, &context, size);
            let after = complete_original(&app);
            eprintln!("fit editor after {document:?}/{size:?}: {after:#?}");
            assert_complete_visible_frames(&app, &context);
            assert_eq!(after, before, "Fit changed saved editor state");
        }
    }
    app.mapping_workspace.active = MappingDocument::Main;
    app.mapping_workspace.focused = MappingDocument::Main;
    settle(&mut app, &context, egui::vec2(900.0, 700.0));
    assert_complete_visible_frames(&app, &context);
}

#[test]
fn fit_document_only_resets_the_requested_camera() {
    let mut app = fixture();
    for document in documents() {
        let before = complete_original(&app);
        let generations = documents().map(|document| canvas(&app, document).view_generation);
        eprintln!(
            "fit reset original document={document:?}, before={before:#?}, generations={generations:?}"
        );
        assert!(app.fit_document(document));
        let after = complete_original(&app);
        let actual = documents().map(|document| canvas(&app, document).view_generation);
        eprintln!(
            "fit reset actual document={document:?}, after={after:#?}, generations={actual:?}"
        );
        assert_eq!(before, after);
        for (index, candidate) in documents().into_iter().enumerate() {
            let expected = generations[index].wrapping_add(u64::from(candidate == document));
            assert_eq!(actual[index], expected);
        }
    }
}
