use super::*;
use ir::Instance;
use mapping::{
    AggregateOp, Binding, DynamicChild, FailureIteration, FailureRule, FailureSelection,
    NamedTarget, ScopeIteration, ScopeSequence, SequenceExpr,
};
use std::collections::BTreeSet;

fn draft(name: &str, argument_count: usize) -> NewFunctionDraft {
    NewFunctionDraft {
        library: "local".into(),
        name: name.into(),
        description: String::new(),
        parameters: (0..argument_count)
            .map(|index| ParameterDraft {
                name: format!("arg{index}"),
                ty: ScalarType::String,
            })
            .collect(),
        output_name: "result".into(),
        output_type: ScalarType::String,
        error: None,
    }
}

fn call_app(argument_count: usize, highest: NodeId) -> (FerruleApp, FunctionId) {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group("Input", Vec::new());
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Keep", ScalarType::String)],
    );
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    let function = app
        .create_function(&draft("callee", argument_count))
        .unwrap();
    let definition = app.project.user_functions.get_mut(&function).unwrap();
    if argument_count == 0 {
        definition.body.nodes.insert(
            definition.output,
            Node::Const {
                value: Value::String("called".into()),
            },
        );
    } else {
        definition.output = 0;
    }
    app.project.graph.nodes = [
        (
            0,
            Node::Const {
                value: Value::String("zero".into()),
            },
        ),
        (
            highest,
            Node::Const {
                value: Value::String("keep".into()),
            },
        ),
    ]
    .into();
    app.project.root = Scope {
        bindings: vec![Binding {
            target_field: "Keep".into(),
            node: highest,
        }],
        ..Scope::default()
    };
    app.project.extra_targets = ["Before", "Current", "After"]
        .into_iter()
        .map(|name| NamedTarget {
            name: name.into(),
            path: None,
            schema: app.project.target.clone(),
            options: app.project.target_options.clone(),
            root: app.project.root.clone(),
        })
        .collect();
    app.main_canvas = CanvasDocumentState::main(&app.project);
    (app, function)
}

fn select_document(app: &mut FerruleApp, document: MappingDocument) {
    match document {
        MappingDocument::Main => app.mapping_workspace.active = document,
        MappingDocument::Target(index) => {
            app.open_target_tab(index);
            assert!(app.ensure_target_canvas(index));
        }
        MappingDocument::Function(function) => {
            app.open_function_tab(function);
            assert!(app.ensure_function_canvas(function));
        }
    }
}

fn clean_baseline(app: &mut FerruleApp) {
    app.mark_clean();
    app.rebase_history();
    assert!(!app.is_dirty());
    assert_eq!(app.history.undo_len(), 0);
}

fn project_state(app: &FerruleApp) -> String {
    mapping::project_file::encode_pretty(&app.project).unwrap()
}

fn canvas_state(snarl: &Snarl<CanvasNode>) -> (String, String) {
    (
        format!("{:?}", snarl.nodes_pos_ids().collect::<Vec<_>>()),
        format!("{:?}", snarl.wires().collect::<Vec<_>>()),
    )
}

fn selected_canvas(app: &FerruleApp) -> &Snarl<CanvasNode> {
    match app.mapping_workspace.active {
        MappingDocument::Main => &app.main_canvas.snarl,
        MappingDocument::Target(index) => &app.mapping_workspace.target_canvases[&index].snarl,
        MappingDocument::Function(function) => {
            &app.mapping_workspace.function_canvases[&function].snarl
        }
    }
}

fn assert_keep_outputs(app: &FerruleApp, active: &str, other: &str) {
    let issues = engine::validate(&app.project);
    assert!(issues.is_empty(), "{issues:#?}");
    let output = engine::run_outputs(&app.project, &Instance::Group((Vec::new()).into())).unwrap();
    let keep = |instance: &Instance| {
        instance
            .field("Keep")
            .and_then(Instance::as_scalar)
            .cloned()
    };
    assert_eq!(
        keep(&output.primary),
        Some(Value::String(
            if app.mapping_workspace.active == MappingDocument::Main {
                active.into()
            } else {
                other.into()
            }
        )),
    );
    for (index, target) in output.extras.iter().enumerate() {
        assert_eq!(
            keep(&target.instance),
            Some(Value::String(
                if app.mapping_workspace.active == MappingDocument::Target(index) {
                    active.into()
                } else {
                    other.into()
                }
            )),
        );
    }
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_function_navigator(ui.ctx(), true),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn label_center(shape: &egui::epaint::Shape) -> Option<egui::Pos2> {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == "Add call" => {
            Some(text.visual_bounding_rect().center())
        }
        egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(label_center),
        _ => None,
    }
}

fn click_add_call(app: &mut FerruleApp) {
    app.show_function_navigator = true;
    app.function_search = "callee".into();
    let context = egui::Context::default();
    let mut output = frame(app, &context, Vec::new());
    for _ in 0..3 {
        output = frame(app, &context, Vec::new());
    }
    let position = output
        .shapes
        .iter()
        .find_map(|shape| label_center(&shape.shape))
        .unwrap();
    for pressed in [true, false] {
        let _ = frame(
            app,
            &context,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn function_call_exhaustion_preserves_graph_wires_output_dirty_state_and_history() {
    for document in [MappingDocument::Main, MappingDocument::Target(1)] {
        for (arguments, highest) in [
            (0, NodeId::MAX),
            (1, NodeId::MAX),
            (2, NodeId::MAX - 1),
            (2, NodeId::MAX - 2),
        ] {
            for dirty in [false, true] {
                let (mut app, function) = call_app(arguments, highest);
                select_document(&mut app, document);
                clean_baseline(&mut app);
                if dirty {
                    app.project.graph.nodes.insert(
                        0,
                        Node::Const {
                            value: Value::String("earlier edit".into()),
                        },
                    );
                    app.observe_editor_history(std::time::Instant::now(), false);
                }
                let before = project_state(&app);
                let before_canvas = canvas_state(selected_canvas(&app));
                let before_editor =
                    editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
                let before_history = app.history.undo_len();
                assert_keep_outputs(&app, "keep", "keep");
                app.insert_function_call(function);
                app.observe_editor_history(std::time::Instant::now(), false);
                assert_eq!(project_state(&app), before);
                assert_eq!(canvas_state(selected_canvas(&app)), before_canvas);
                assert!(
                    editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace)
                        == before_editor
                );
                assert_eq!(app.history.undo_len(), before_history);
                assert_eq!(app.is_dirty(), dirty);
                assert_eq!(app.status, "function call edit failed");
                assert_eq!(app.diagnostics.len(), 1);
                assert!(
                    app.diagnostics.items()[0]
                        .message
                        .contains("No mapping node identifiers remain")
                );
                assert_keep_outputs(&app, "keep", "keep");
            }
        }
    }
}

#[test]
fn function_call_preflight_does_not_materialize_missing_canvases_on_exhaustion() {
    let (mut app, function) = call_app(1, NodeId::MAX);
    app.mapping_workspace.active = MappingDocument::Target(1);
    clean_baseline(&mut app);
    let before = project_state(&app);
    assert!(app.mapping_workspace.target_canvases.is_empty());
    assert!(app.try_insert_function_call(function).is_err());
    assert!(app.mapping_workspace.target_canvases.is_empty());
    assert_eq!(project_state(&app), before);
    assert!(!app.is_dirty());

    let caller = app.create_function(&draft("caller", 0)).unwrap();
    let body = &mut app.project.user_functions.get_mut(&caller).unwrap().body;
    body.nodes = [(
        NodeId::MAX,
        Node::Const {
            value: Value::String("caller".into()),
        },
    )]
    .into();
    app.project.user_functions.get_mut(&caller).unwrap().output = NodeId::MAX;
    app.mapping_workspace.active = MappingDocument::Function(caller);
    clean_baseline(&mut app);
    let before = project_state(&app);
    assert!(app.mapping_workspace.function_canvases.is_empty());
    assert!(app.try_insert_function_call(function).is_err());
    assert!(app.mapping_workspace.function_canvases.is_empty());
    assert_eq!(project_state(&app), before);
    assert!(!app.is_dirty());
}

#[test]
fn function_call_navigator_reports_exhaustion_on_main_named_and_isolated_graphs() {
    for document in [MappingDocument::Main, MappingDocument::Target(1)] {
        let (mut app, _) = call_app(0, NodeId::MAX);
        select_document(&mut app, document);
        clean_baseline(&mut app);
        let before = project_state(&app);
        let canvas = canvas_state(selected_canvas(&app));
        click_add_call(&mut app);
        assert_eq!(project_state(&app), before);
        assert_eq!(canvas_state(selected_canvas(&app)), canvas);
        assert_eq!(app.history.undo_len(), 0);
        assert!(!app.is_dirty());
        assert_eq!(app.diagnostics.len(), 1);
        assert_keep_outputs(&app, "keep", "keep");
    }
    let (mut app, _) = call_app(0, 2);
    let caller = app.create_function(&draft("caller", 0)).unwrap();
    let definition = app.project.user_functions.get_mut(&caller).unwrap();
    definition.body.nodes = [(
        NodeId::MAX,
        Node::Const {
            value: Value::String("caller".into()),
        },
    )]
    .into();
    definition.output = NodeId::MAX;
    select_document(&mut app, MappingDocument::Function(caller));
    clean_baseline(&mut app);
    let before = project_state(&app);
    let canvas = canvas_state(selected_canvas(&app));
    click_add_call(&mut app);
    assert_eq!(project_state(&app), before);
    assert_eq!(canvas_state(selected_canvas(&app)), canvas);
    assert_eq!(app.project.user_functions[&caller].output, NodeId::MAX);
    assert_eq!(app.history.undo_len(), 0);
    assert!(!app.is_dirty());
    assert_eq!(app.diagnostics.len(), 1);
    assert_keep_outputs(&app, "keep", "keep");
}

fn sequence(item: NodeId) -> SequenceExpr {
    SequenceExpr::Generate {
        from: None,
        to: 0,
        item,
    }
}

fn generated_scope(item: NodeId) -> Scope {
    Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Sequence(sequence(item)),
        ..Scope::default()
    }
}

#[test]
fn function_call_reservation_preserves_absent_items_for_every_project_owner() {
    for document in [MappingDocument::Main, MappingDocument::Target(1)] {
        let (mut app, function) = call_app(2, 30);
        // Incomplete authoring metadata must survive an unrelated Add call.
        app.project.root.children.push(generated_scope(31));
        for (index, target) in app.project.extra_targets.iter_mut().enumerate() {
            target
                .root
                .children
                .push(generated_scope(32 + index as NodeId));
        }
        app.project.root.children.push(Scope {
            iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                generated_scope(35),
                Vec::new(),
            )),
            ..Scope::default()
        });
        app.project.root.dynamic_children.push(DynamicChild {
            key: 0,
            scope: generated_scope(36),
        });
        app.project.graph.nodes.extend([
            (
                20,
                Node::SequenceExists {
                    sequence: sequence(37),
                    predicate: 0,
                },
            ),
            (
                21,
                Node::SequenceItemAt {
                    sequence: sequence(38),
                    index: 0,
                },
            ),
            (
                22,
                Node::SequenceAggregate {
                    function: AggregateOp::Count,
                    sequence: sequence(39),
                    predicate: None,
                    expression: None,
                    arg: None,
                },
            ),
        ]);
        app.project.failure_rules.push(FailureRule {
            iteration: FailureIteration::Sequence {
                sequence: sequence(40),
            },
            selection: FailureSelection::All,
            message: None,
        });
        assert_eq!(
            crate::graph_viewer::project_sequence_item_ids(&app.project),
            (31..=40).collect::<BTreeSet<_>>()
        );
        app.main_canvas = CanvasDocumentState::main(&app.project);
        select_document(&mut app, document);
        clean_baseline(&mut app);
        let mut expected = app.project.clone();
        expected.graph.nodes.extend([
            (41, Node::Unconnected),
            (42, Node::Unconnected),
            (
                43,
                Node::UserFunctionCall {
                    function,
                    args: vec![41, 42],
                },
            ),
        ]);
        app.try_insert_function_call(function).unwrap();
        app.observe_editor_history(std::time::Instant::now(), false);
        assert_eq!(
            project_state(&app),
            mapping::project_file::encode_pretty(&expected).unwrap()
        );
        assert!((31..=40).all(|id| !app.project.graph.nodes.contains_key(&id)));
        assert_eq!(
            selected_canvas(&app)
                .nodes()
                .filter(|node| **node == CanvasNode::Graph(43))
                .count(),
            1
        );
        assert!(
            !selected_canvas(&app)
                .nodes()
                .any(|node| matches!(node, CanvasNode::Placeholder(_)))
        );
        assert_eq!(app.history.undo_len(), 1);
        app.undo_project();
        assert!(!app.project.graph.nodes.contains_key(&41));
        assert!(!app.is_dirty());
        app.redo_project();
        assert_eq!(
            project_state(&app),
            mapping::project_file::encode_pretty(&expected).unwrap()
        );
    }
}

#[test]
fn function_call_owned_tail_exhaustion_discards_the_entire_prepared_batch() {
    for document in [MappingDocument::Main, MappingDocument::Target(1)] {
        let (mut app, function) = call_app(1, NodeId::MAX - 2);
        // Only MAX-1 is available: the absent generated owner reserves MAX.
        // Keep this incomplete authoring metadata exactly as it was loaded.
        app.project.root.children.push(generated_scope(NodeId::MAX));
        app.main_canvas = CanvasDocumentState::main(&app.project);
        select_document(&mut app, document);
        clean_baseline(&mut app);
        let before = project_state(&app);
        let canvas = canvas_state(selected_canvas(&app));
        app.insert_function_call(function);
        app.observe_editor_history(std::time::Instant::now(), false);
        assert_eq!(project_state(&app), before);
        assert_eq!(canvas_state(selected_canvas(&app)), canvas);
        assert!(!app.project.graph.nodes.contains_key(&(NodeId::MAX - 1)));
        assert!(!app.project.graph.nodes.contains_key(&NodeId::MAX));
        assert_eq!(app.history.undo_len(), 0);
        assert!(!app.is_dirty());
        assert_eq!(app.diagnostics.len(), 1);
    }
}

#[test]
fn function_call_isolated_ids_ignore_unrelated_project_ownership() {
    let (mut app, callee) = call_app(1, 30);
    let caller = app.create_function(&draft("caller", 0)).unwrap();
    let definition = app.project.user_functions.get_mut(&caller).unwrap();
    definition.body.nodes = [(
        0,
        Node::Const {
            value: Value::String("caller".into()),
        },
    )]
    .into();
    definition.output = 0;
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::Int(3),
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    app.project.graph.nodes.insert(
        30,
        Node::UserFunctionCall {
            function: caller,
            args: Vec::new(),
        },
    );
    let rows =
        SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)]).repeating();
    let ir::SchemaKind::Group { children, .. } = &mut app.project.target.kind else {
        unreachable!()
    };
    children.push(rows);
    let mut scope = generated_scope(1);
    scope.bindings.push(Binding {
        target_field: "Value".into(),
        node: 1,
    });
    app.project.root.children.push(scope);
    app.main_canvas = CanvasDocumentState::main(&app.project);
    select_document(&mut app, MappingDocument::Function(caller));
    clean_baseline(&mut app);
    assert_eq!(
        crate::graph_viewer::project_sequence_item_ids(&app.project),
        [1].into()
    );
    let graph = serde_json::to_string(&app.project.graph).unwrap();
    assert_keep_outputs(&app, "caller", "caller");
    app.try_insert_function_call(callee).unwrap();
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(serde_json::to_string(&app.project.graph).unwrap(), graph);
    assert_eq!(app.project.user_functions[&caller].output, 0);
    assert!(matches!(
        app.project.user_functions[&caller].body.nodes.get(&1),
        Some(Node::Unconnected)
    ));
    assert!(
        matches!(app.project.user_functions[&caller].body.nodes.get(&2), Some(Node::UserFunctionCall { function, args }) if *function == callee && args == &[1])
    );
    assert_keep_outputs(&app, "caller", "caller");
    assert_eq!(app.history.undo_len(), 1);
    app.undo_project();
    assert_eq!(app.project.user_functions[&caller].body.nodes.len(), 1);
    assert!(!app.is_dirty());
    app.redo_project();
    assert_eq!(app.project.user_functions[&caller].body.nodes.len(), 3);
    assert_keep_outputs(&app, "caller", "caller");
}

#[test]
fn function_call_final_legal_batch_can_use_max_without_aliasing_inputs() {
    for document in [MappingDocument::Main, MappingDocument::Target(1)] {
        let (mut app, function) = call_app(2, NodeId::MAX - 3);
        select_document(&mut app, document);
        clean_baseline(&mut app);
        app.try_insert_function_call(function).unwrap();
        app.observe_editor_history(std::time::Instant::now(), false);
        assert!(matches!(
            app.project.graph.nodes.get(&(NodeId::MAX - 2)),
            Some(Node::Unconnected)
        ));
        assert!(matches!(
            app.project.graph.nodes.get(&(NodeId::MAX - 1)),
            Some(Node::Unconnected)
        ));
        assert!(
            matches!(app.project.graph.nodes.get(&NodeId::MAX), Some(Node::UserFunctionCall { function: id, args }) if *id == function && args == &[NodeId::MAX - 2, NodeId::MAX - 1])
        );
        assert_eq!(
            selected_canvas(&app)
                .nodes()
                .filter(|node| **node == CanvasNode::Graph(NodeId::MAX))
                .count(),
            1
        );
        assert_keep_outputs(&app, "keep", "keep");
        let before = project_state(&app);
        app.insert_function_call(function);
        app.observe_editor_history(std::time::Instant::now(), false);
        assert_eq!(project_state(&app), before);
        assert_eq!(app.history.undo_len(), 1);
        app.undo_project();
        assert!(!app.project.graph.nodes.contains_key(&NodeId::MAX));
        assert!(!app.is_dirty());
        app.redo_project();
        assert_eq!(project_state(&app), before);
    }
    let graph = Graph::default();
    assert!(reserve_call_ids(&graph, &BTreeSet::new(), usize::MAX).is_err());
    assert_eq!(
        reserve_call_ids(&graph, &[0, 1].into(), 0).unwrap(),
        (Vec::new(), 2)
    );
    let graph = Graph {
        nodes: [(NodeId::MAX - 2, Node::Unconnected)].into(),
    };
    assert!(reserve_call_ids(&graph, &[NodeId::MAX].into(), 1).is_err());
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-function-call-allocation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn function_call_pointer_success_and_output_survive_undo_save_and_reopen() -> anyhow::Result<()> {
    for document in [MappingDocument::Main, MappingDocument::Target(1)] {
        let directory = TestDirectory::new();
        let path = directory.0.join("project.json");
        let (mut app, _) = call_app(0, 2);
        select_document(&mut app, document);
        clean_baseline(&mut app);
        let before = project_state(&app);
        click_add_call(&mut app);
        let added = project_state(&app);
        assert_ne!(added, before);
        assert!(
            matches!(app.project.graph.nodes.get(&3), Some(Node::UserFunctionCall { args, .. }) if args.is_empty())
        );
        assert_eq!(app.history.undo_len(), 1);
        assert_keep_outputs(&app, "keep", "keep");
        let root = match document {
            MappingDocument::Main => &mut app.project.root,
            MappingDocument::Target(index) => &mut app.project.extra_targets[index].root,
            MappingDocument::Function(_) => unreachable!(),
        };
        root.bindings[0].node = 3;
        app.observe_editor_history(std::time::Instant::now(), false);
        assert_eq!(app.history.undo_len(), 2);
        assert_keep_outputs(&app, "called", "keep");
        app.undo_project();
        assert_eq!(project_state(&app), added);
        assert_keep_outputs(&app, "keep", "keep");
        app.undo_project();
        assert_eq!(project_state(&app), before);
        assert!(!app.is_dirty());
        app.redo_project();
        app.redo_project();
        assert_keep_outputs(&app, "called", "keep");
        let authored = project_state(&app);
        app.save_document_to(&path)?;
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&path);
        assert_eq!(project_state(&reopened), authored);
        select_document(&mut reopened, document);
        assert_keep_outputs(&reopened, "called", "keep");
        assert!(!reopened.is_dirty());
        assert!(!reopened.can_undo());
        assert_eq!(
            selected_canvas(&reopened)
                .nodes()
                .filter(|node| **node == CanvasNode::Graph(3))
                .count(),
            1
        );
    }
    Ok(())
}
