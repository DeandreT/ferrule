use super::*;

fn draft() -> NewFunctionDraft {
    NewFunctionDraft {
        library: "orders".to_string(),
        name: "line_total".to_string(),
        description: "Multiply a price and quantity".to_string(),
        parameters: vec![
            ParameterDraft {
                name: "price".to_string(),
                ty: ScalarType::Float,
            },
            ParameterDraft {
                name: "quantity".to_string(),
                ty: ScalarType::Int,
            },
        ],
        output_name: "total".to_string(),
        output_type: ScalarType::Float,
        error: None,
    }
}

#[test]
fn blank_function_creation_builds_stable_inputs_and_a_null_output() {
    let mut app = FerruleApp::default();
    let id = app.create_function(&draft()).expect("valid function");
    let function = app.project.user_functions.get(&id).expect("function");

    assert_eq!(function_label(function), "orders:line_total");
    assert_eq!(function.parameters.len(), 2);
    assert_eq!(function.parameters[0].id, FunctionParameterId::new(1));
    assert_eq!(function.parameters[1].id, FunctionParameterId::new(2));
    assert!(matches!(
        function.body.nodes.get(&function.output),
        Some(Node::Const { value: Value::Null })
    ));
}

#[test]
fn adding_a_function_call_leaves_each_input_visually_empty() {
    let mut app = FerruleApp::default();
    let id = app.create_function(&draft()).expect("valid function");
    app.insert_function_call(id);

    let (args, call_id) = app
        .project
        .graph
        .nodes
        .iter()
        .find_map(|(&node, expression)| match expression {
            Node::UserFunctionCall { function, args } if *function == id => {
                Some((args.clone(), node))
            }
            _ => None,
        })
        .expect("call");
    assert_eq!(args.len(), 2);
    assert!(
        args.iter()
            .all(|node| matches!(app.project.graph.nodes.get(node), Some(Node::Unconnected)))
    );
    assert_eq!(
        app.main_canvas
            .snarl
            .wires()
            .filter(|(_, input)| {
                app.main_canvas.snarl[input.node] == CanvasNode::Graph(call_id)
            })
            .count(),
        0
    );
}

#[test]
fn a_function_moves_between_tab_split_and_floating_hosts() {
    let mut app = FerruleApp::default();
    let id = app.create_function(&draft()).expect("valid function");

    app.open_function_tab(id);
    assert_eq!(app.mapping_workspace.active, MappingDocument::Function(id));
    app.open_function_split(id);
    assert_eq!(
        app.mapping_workspace.split,
        Some(MappingDocument::Function(id))
    );
    assert!(
        !app.mapping_workspace
            .tabs
            .contains(&MappingDocument::Function(id))
    );
    app.float_function(id);
    assert!(app.mapping_workspace.split.is_none());
    assert!(app.mapping_workspace.floating.contains(&id));
    app.open_function_tab(id);
    assert!(!app.mapping_workspace.floating.contains(&id));
    assert!(
        app.mapping_workspace
            .tabs
            .contains(&MappingDocument::Function(id))
    );
}

#[test]
fn function_canvas_reconstructs_body_wires_without_boundary_nodes() {
    let mut app = FerruleApp::default();
    let id = app.create_function(&draft()).expect("valid function");
    let function = app.project.user_functions.get_mut(&id).expect("function");
    let args = function
        .parameters
        .iter()
        .enumerate()
        .map(|(node, _)| node as NodeId)
        .collect::<Vec<_>>();
    let output = function.output;
    function.body.nodes.insert(
        output,
        Node::Call {
            function: "multiply".to_string(),
            args,
        },
    );

    assert!(app.ensure_function_canvas(id));
    let canvas = app
        .mapping_workspace
        .function_canvases
        .get(&id)
        .expect("canvas");
    assert_eq!(canvas.snarl.nodes().count(), 3);
    assert_eq!(canvas.snarl.wires().count(), 2);
    assert!(
        canvas
            .snarl
            .nodes()
            .all(|node| matches!(node, CanvasNode::Graph(_)))
    );
}

#[test]
fn focused_function_arrange_and_fit_leave_main_canvas_untouched() {
    let mut app = FerruleApp::default();
    let id = app.create_function(&draft()).expect("valid function");
    app.open_function_tab(id);
    assert!(app.ensure_function_canvas(id));
    let main_position = egui::pos2(777.0, 555.0);
    let main_node = app
        .main_canvas
        .snarl
        .node_ids()
        .next()
        .map(|(node, _)| node)
        .expect("main node");
    app.main_canvas
        .snarl
        .get_node_info_mut(main_node)
        .expect("main node info")
        .pos = main_position;
    let main_generation = app.main_canvas.view_generation;
    let function_generation = app
        .mapping_workspace
        .function_canvases
        .get(&id)
        .map(|canvas| canvas.view_generation)
        .expect("function canvas");

    assert!(app.arrange_document(MappingDocument::Function(id), ArrangeMode::LeftToRight));
    assert_eq!(
        app.main_canvas
            .snarl
            .get_node_info(main_node)
            .map(|node| node.pos),
        Some(main_position)
    );
    assert_eq!(app.main_canvas.view_generation, main_generation);
    assert_eq!(
        app.mapping_workspace
            .function_canvases
            .get(&id)
            .map(|canvas| canvas.view_generation),
        Some(function_generation + 1)
    );

    assert!(app.fit_document(MappingDocument::Function(id)));
    assert_eq!(app.main_canvas.view_generation, main_generation);
    assert_eq!(
        app.mapping_workspace
            .function_canvases
            .get(&id)
            .map(|canvas| canvas.view_generation),
        Some(function_generation + 2)
    );
}

#[test]
fn workspace_layout_restores_function_tabs_and_canvas_positions_docked() {
    let mut app = FerruleApp::default();
    let id = app.create_function(&draft()).expect("valid function");
    app.open_function_tab(id);
    assert!(app.ensure_function_canvas(id));
    let expected = egui::pos2(321.0, 123.0);
    let canvas = app
        .mapping_workspace
        .function_canvases
        .get_mut(&id)
        .expect("canvas");
    let node = canvas
        .snarl
        .node_ids()
        .find_map(|(node, canvas)| (*canvas == CanvasNode::Graph(0)).then_some(node))
        .expect("parameter node");
    canvas.snarl.get_node_info_mut(node).expect("node").pos = expected;
    app.float_function(id);

    let layout =
        CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
    let restored = MappingWorkspace::from_layout(&app.project, Some(&layout));

    assert!(restored.tabs.contains(&MappingDocument::Function(id)));
    assert!(restored.floating.is_empty());
    let canvas = restored.function_canvases.get(&id).expect("canvas");
    let actual = canvas
        .snarl
        .nodes_pos()
        .find_map(|(position, canvas)| (*canvas == CanvasNode::Graph(0)).then_some(position))
        .expect("parameter position");
    assert_eq!(actual, expected);
}

#[test]
fn function_creation_and_navigator_calls_are_typed_commands_matching_snapshots() {
    use super::super::typed_edit_oracle::{assert_typed_edit_matches_snapshots, newest_command};
    use editor_ui::{GraphAction, GraphDocument, TargetId};
    let blank = || {
        let mut app = FerruleApp::default();
        app.main_canvas = CanvasDocumentState::main(&app.project);
        app.mark_clean();
        app.rebase_history();
        app
    };
    let app = assert_typed_edit_matches_snapshots(
        blank,
        |app| {
            app.create_function(&draft()).expect("valid function");
        },
        "Create function",
    );
    let id = *app.project.user_functions.keys().next().expect("function");
    assert_eq!(newest_command(&app).document(), &GraphDocument::Project);
    assert_eq!(
        newest_command(&app).action(),
        &GraphAction::CreateFunction { function: id }
    );

    let with_function = || {
        let mut app = blank();
        app.create_function(&draft()).expect("valid function");
        app.mark_clean();
        app.rebase_history();
        app
    };
    let app = assert_typed_edit_matches_snapshots(
        with_function,
        |app| app.insert_function_call(id),
        "Insert node",
    );
    let command = newest_command(&app);
    assert_eq!(
        command.document(),
        &GraphDocument::Target(TargetId::Primary)
    );
    let GraphAction::InsertNode { node, .. } = command.action() else {
        panic!("an insertion, got {:?}", command.action());
    };
    assert!(matches!(
        app.project.graph.nodes.get(node),
        Some(Node::UserFunctionCall { function, .. }) if *function == id
    ));
    assert_eq!(
        command.patch().added_nodes().count(),
        3,
        "two inputs and the call"
    );
}
