use super::*;
use crate::canvas_layout::arrange_snarl;
use crate::layout_store::layout_path;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, FunctionId, NamedTarget, Scope, UserFunction};

fn canvas_position(snarl: &Snarl<CanvasNode>, wanted: CanvasNode) -> egui::Pos2 {
    snarl
        .nodes_pos()
        .find_map(|(pos, &node)| (node == wanted).then_some(pos))
        .expect("canvas node exists")
}

fn move_canvas_node(snarl: &mut Snarl<CanvasNode>, wanted: CanvasNode, pos: egui::Pos2) {
    let id = snarl
        .node_ids()
        .find_map(|(id, &node)| (node == wanted).then_some(id))
        .expect("canvas node exists");
    snarl.get_node_info_mut(id).expect("canvas node exists").pos = pos;
}

fn temporary_project_path(test_name: &str) -> PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ferrule-gui-{test_name}-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temporary test directory is created");
    dir.join("project.json")
}

fn pipeline_fixture(path: &Path) -> anyhow::Result<()> {
    let mut project = blank_project();
    let schema = SchemaNode::group(
        "Record",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    project.source = schema.clone();
    project.target = schema;
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project.root.construction = mapping::ScopeConstruction::CopyCurrentSource;
    let pipeline = mapping::Pipeline {
        main_mapping_path: None,
        stages: vec![mapping::PipelineStage {
            id: "prepare".into(),
            mapping_path: None,
            project,
            source: mapping::PipelineInput::Host {
                name: "orders".into(),
            },
            extra_sources: Vec::new(),
        }],
    };
    std::fs::write(path, serde_json::to_vec_pretty(&pipeline)?)?;
    Ok(())
}

fn two_stage_pipeline_fixture(path: &Path) -> anyhow::Result<()> {
    pipeline_fixture(path)?;
    let mut pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(path)?)?;
    let prepare = &mut pipeline.stages[0].project;
    prepare.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("A".into()),
        },
    );
    prepare.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Scope::default()
    };
    let mut finish = prepare.clone();
    finish.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("B".into()),
        },
    );
    pipeline.stages.push(mapping::PipelineStage {
        id: "finish".into(),
        mapping_path: None,
        project: finish,
        source: mapping::PipelineInput::StageTarget {
            stage: "prepare".into(),
            target: None,
        },
        extra_sources: Vec::new(),
    });
    std::fs::write(path, serde_json::to_vec_pretty(&pipeline)?)?;
    Ok(())
}

fn two_stage_pipeline_app(test_name: &str) -> anyhow::Result<(FerruleApp, PathBuf)> {
    let pipeline_path = temporary_project_path(test_name);
    two_stage_pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    std::fs::write(directory.join("orders.json"), r#"{"Value":"source"}"#)?;
    std::fs::write(directory.join("prepare.json"), "old prepare")?;
    std::fs::write(directory.join("finish.json"), "old finish")?;
    let mut app = FerruleApp::default();
    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    assert!(draft.issues.is_empty(), "{:?}", draft.issues);
    draft.inputs[0].path = "orders.json".into();
    draft.outputs[0].path = "prepare.json".into();
    draft.outputs[1].path = "finish.json".into();
    Ok((app, pipeline_path))
}

fn two_stage_pin_pipeline_app(test_name: &str) -> anyhow::Result<(FerruleApp, PathBuf)> {
    let pipeline_path = temporary_project_path(test_name);
    two_stage_pipeline_fixture(&pipeline_path)?;
    let mut pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&pipeline_path)?)?;
    for stage in &mut pipeline.stages {
        stage.project.graph.nodes.insert(
            1,
            Node::Const {
                value: ir::Value::String("!".into()),
            },
        );
        stage.project.graph.nodes.insert(
            2,
            Node::Call {
                function: "concat".into(),
                args: vec![0, 1],
            },
        );
        stage.project.root.bindings[0].node = 2;
    }
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    std::fs::write(directory.join("orders.json"), r#"{"Value":"source"}"#)?;
    std::fs::write(directory.join("prepare.json"), "old prepare")?;
    std::fs::write(directory.join("finish.json"), "old finish")?;
    let mut app = FerruleApp::default();
    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    assert!(draft.issues.is_empty(), "{:?}", draft.issues);
    draft.inputs[0].path = "orders.json".into();
    draft.outputs[0].path = "prepare.json".into();
    draft.outputs[1].path = "finish.json".into();
    Ok((app, pipeline_path))
}

fn two_stage_function_pipeline_app(test_name: &str) -> anyhow::Result<(FerruleApp, PathBuf)> {
    let pipeline_path = temporary_project_path(test_name);
    two_stage_pipeline_fixture(&pipeline_path)?;
    let mut pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&pipeline_path)?)?;
    for stage in &mut pipeline.stages {
        let value = if stage.id == "prepare" { "A" } else { "B" };
        attach_no_arg_function(&mut stage.project, value);
        stage.project.root.bindings[0].node = 2;
    }
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    std::fs::write(directory.join("orders.json"), r#"{"Value":"source"}"#)?;
    std::fs::write(directory.join("prepare.json"), "old prepare")?;
    std::fs::write(directory.join("finish.json"), "old finish")?;
    let mut app = FerruleApp::default();
    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    assert!(draft.issues.is_empty(), "{:?}", draft.issues);
    draft.inputs[0].path = "orders.json".into();
    draft.outputs[0].path = "prepare.json".into();
    draft.outputs[1].path = "finish.json".into();
    Ok((app, pipeline_path))
}

fn two_stage_function_input_pipeline_app(test_name: &str) -> anyhow::Result<(FerruleApp, PathBuf)> {
    let pipeline_path = temporary_project_path(test_name);
    two_stage_pipeline_fixture(&pipeline_path)?;
    let mut pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&pipeline_path)?)?;
    for stage in &mut pipeline.stages {
        let value = if stage.id == "prepare" { "A" } else { "B" };
        attach_input_function(&mut stage.project, value);
        stage.project.root.bindings[0].node = 2;
    }
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    std::fs::write(directory.join("orders.json"), r#"{"Value":"source"}"#)?;
    std::fs::write(directory.join("prepare.json"), "old prepare")?;
    std::fs::write(directory.join("finish.json"), "old finish")?;
    let mut app = FerruleApp::default();
    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    assert!(draft.issues.is_empty(), "{:?}", draft.issues);
    draft.inputs[0].path = "orders.json".into();
    draft.outputs[0].path = "prepare.json".into();
    draft.outputs[1].path = "finish.json".into();
    Ok((app, pipeline_path))
}

#[test]
fn pipeline_runner_uses_unambiguous_stored_host_paths() -> anyhow::Result<()> {
    let pipeline_path = temporary_project_path("pipeline-stored-host-paths");
    pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    let mut pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&pipeline_path)?)?;
    let stage = &mut pipeline.stages[0];
    stage.mapping_path = Some("designs/first.mfd".into());
    stage.project.source_path = Some("inputs/orders.json".into());
    stage.project.extra_sources.push(mapping::NamedSource {
        name: "lookup".into(),
        path: "inputs/lookup.json".into(),
        schema: stage.project.source.clone(),
        options: stage.project.source_options.clone(),
        dynamic_path: None,
    });
    stage.extra_sources.push(mapping::PipelineNamedInput {
        name: "lookup".into(),
        from: mapping::PipelineInput::Host {
            name: "lookup-file".into(),
        },
    });
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;

    let mut draft = crate::pipeline_run::PipelineRunDraft::load(&pipeline_path)?;
    assert!(draft.issues.is_empty(), "{:?}", draft.issues);
    assert_eq!(draft.inputs[0].name, "lookup-file");
    assert_eq!(
        draft.inputs[0].path,
        directory
            .join("designs/inputs/lookup.json")
            .to_string_lossy()
    );
    assert_eq!(draft.inputs[1].name, "orders");
    assert_eq!(
        draft.inputs[1].path,
        directory
            .join("designs/inputs/orders.json")
            .to_string_lossy()
    );
    assert!(draft.outputs[0].path.is_empty());
    draft.outputs[0].path = "result.json".into();
    let (inputs, _) = draft.requests()?;
    assert_eq!(inputs[1].path, directory.join("designs/inputs/orders.json"));

    let mut second = pipeline.stages[0].clone();
    second.id = "second".into();
    second.mapping_path = None;
    second.project.source_path = Some("different.json".into());
    second.extra_sources.clear();
    second.project.extra_sources.clear();
    pipeline.stages.push(second);
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;
    let draft = crate::pipeline_run::PipelineRunDraft::load(&pipeline_path)?;
    assert!(draft.issues.is_empty(), "{:?}", draft.issues);
    assert_eq!(draft.inputs[1].name, "orders");
    assert!(draft.inputs[1].path.is_empty());

    std::fs::remove_dir_all(directory)?;
    Ok(())
}

fn wait_for_pipeline_completion(app: &mut FerruleApp) {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.pending_pipeline_run.is_some() && std::time::Instant::now() < deadline {
        app.poll_pipeline_run(&context);
        if app.pending_pipeline_run.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    assert!(
        app.pending_pipeline_run.is_none(),
        "pipeline worker completes"
    );
}

fn wait_for_pipeline_pause(app: &mut FerruleApp) -> (String, engine::PendingTargetWrite) {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_pipeline_run(&context);
        match app
            .pending_pipeline_run
            .as_ref()
            .map(|pending| &pending.phase)
        {
            Some(pipeline_ui::PipelineRunPhase::Paused(stage, write)) => {
                return (stage.clone(), (**write).clone());
            }
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug pipeline did not pause before deadline: {other:?}"),
        }
    }
}

fn wait_for_pipeline_node_pause(app: &mut FerruleApp) -> (String, engine::PendingNodeValue) {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_pipeline_run(&context);
        match app
            .pending_pipeline_run
            .as_ref()
            .map(|pending| &pending.phase)
        {
            Some(pipeline_ui::PipelineRunPhase::PausedNode(stage, value)) => {
                return (stage.clone(), (**value).clone());
            }
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug pipeline did not pause at a node before deadline: {other:?}"),
        }
    }
}

fn wait_for_pipeline_function_node_pause(
    app: &mut FerruleApp,
) -> (String, engine::PendingFunctionNodeValue) {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_pipeline_run(&context);
        match app
            .pending_pipeline_run
            .as_ref()
            .map(|pending| &pending.phase)
        {
            Some(pipeline_ui::PipelineRunPhase::PausedFunctionNode(stage, value)) => {
                return (stage.clone(), (**value).clone());
            }
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => {
                panic!("debug pipeline did not pause in a function before deadline: {other:?}")
            }
        }
    }
}

fn wait_for_pipeline_input_pause(app: &mut FerruleApp) -> (String, engine::PendingNodeInput) {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_pipeline_run(&context);
        match app
            .pending_pipeline_run
            .as_ref()
            .map(|pending| &pending.phase)
        {
            Some(pipeline_ui::PipelineRunPhase::PausedInput(stage, input)) => {
                return (stage.clone(), (**input).clone());
            }
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug pipeline did not pause at an input before deadline: {other:?}"),
        }
    }
}

fn wait_for_pipeline_function_input_pause(
    app: &mut FerruleApp,
) -> (String, engine::PendingFunctionNodeInput) {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_pipeline_run(&context);
        match app
            .pending_pipeline_run
            .as_ref()
            .map(|pending| &pending.phase)
        {
            Some(pipeline_ui::PipelineRunPhase::PausedFunctionInput(stage, input)) => {
                return (stage.clone(), (**input).clone());
            }
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug pipeline did not pause at a function input: {other:?}"),
        }
    }
}

fn named_target(name: &str) -> NamedTarget {
    NamedTarget {
        name: name.to_owned(),
        path: Some(format!("{name}.json")),
        schema: SchemaNode::group(name, vec![SchemaNode::scalar("value", ScalarType::String)]),
        options: FormatOptions::default(),
        root: Scope::default(),
    }
}

fn user_function(name: &str) -> UserFunction {
    let mut body = Graph::default();
    body.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("result".to_owned()),
        },
    );
    UserFunction {
        library: "local".to_owned(),
        name: name.to_owned(),
        description: None,
        parameters: Vec::new(),
        output_name: "result".to_owned(),
        output_type: ScalarType::String,
        body,
        output: 0,
    }
}

fn attach_no_arg_function(project: &mut mapping::Project, value: &str) {
    let function_id = FunctionId::new(7);
    let mut definition = user_function("debug_value");
    definition.body.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String(value.into()),
        },
    );
    project.user_functions.insert(function_id, definition);
    project.graph.nodes.insert(
        2,
        Node::UserFunctionCall {
            function: function_id,
            args: Vec::new(),
        },
    );
}

fn attach_input_function(project: &mut mapping::Project, value: &str) {
    attach_no_arg_function(project, value);
    let definition = project
        .user_functions
        .get_mut(&FunctionId::new(7))
        .expect("function exists");
    definition.body.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::String("!".into()),
        },
    );
    definition.body.nodes.insert(
        2,
        Node::Call {
            function: "concat".into(),
            args: vec![0, 1],
        },
    );
    definition.output = 2;
}

#[test]
fn extra_target_create_and_rename_roundtrip_through_history() {
    let mut app = FerruleApp {
        extra_target_draft: Some(ExtraTargetDraft {
            name: "audit".to_owned(),
            output_path: "audit.json".to_owned(),
            schema: Some(named_target("audit").schema),
            ..ExtraTargetDraft::default()
        }),
        ..FerruleApp::default()
    };
    app.finish_extra_target();
    app.observe_editor_history(std::time::Instant::now(), false);

    assert_eq!(app.project.extra_targets[0].name, "audit");
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(0));
    assert!(app.is_dirty());

    app.undo_project();
    assert!(app.project.extra_targets.is_empty());
    assert_eq!(app.mapping_workspace.active, MappingDocument::Main);
    assert!(!app.is_dirty());

    app.redo_project();
    assert_eq!(app.project.extra_targets[0].name, "audit");
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(0));

    app.project.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::String("shared".to_owned()),
        },
    );
    assert!(app.ensure_target_canvas(0));
    let custom = egui::pos2(203.0, 407.0);
    move_canvas_node(
        &mut app
            .mapping_workspace
            .target_canvases
            .get_mut(&0)
            .expect("audit target canvas exists")
            .snarl,
        CanvasNode::Graph(7),
        custom,
    );
    app.observe_editor_history(std::time::Instant::now(), false);

    app.edit_extra_target(0);
    app.extra_target_draft
        .as_mut()
        .expect("edit draft exists")
        .name = "archive".to_owned();
    app.finish_extra_target();
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(app.project.extra_targets[0].name, "archive");
    assert_eq!(app.mapping_workspace.tabs[1], MappingDocument::Target(0));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );

    app.undo_project();
    assert_eq!(app.project.extra_targets[0].name, "audit");
    assert_eq!(app.mapping_workspace.tabs[1], MappingDocument::Target(0));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );
    app.redo_project();
    assert_eq!(app.project.extra_targets[0].name, "archive");
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );
}

#[test]
fn duplicate_extra_target_name_does_not_mutate_project() {
    let mut app = FerruleApp::default();
    app.project.extra_targets.push(named_target("audit"));
    app.extra_target_draft = Some(ExtraTargetDraft {
        name: " audit ".to_owned(),
        schema: Some(named_target("duplicate").schema),
        ..ExtraTargetDraft::default()
    });

    app.finish_extra_target();

    assert_eq!(app.project.extra_targets.len(), 1);
    assert_eq!(app.project.extra_targets[0].name, "audit");
    assert!(app.extra_target_draft.is_some());
    assert_eq!(app.status, "target is incomplete");
}

#[test]
fn named_target_subtree_expansion_is_one_undoable_saved_edit() {
    let project_path = temporary_project_path("target-scope-expansion");
    let mut app = FerruleApp {
        document: DocumentLocation::untitled(project_path.clone()),
        ..FerruleApp::default()
    };
    let mut audit = named_target("audit");
    audit.schema = SchemaNode::group(
        "audit",
        vec![SchemaNode::group(
            "Section",
            vec![
                SchemaNode::group(
                    "Items",
                    vec![SchemaNode::scalar("Code", ScalarType::String)],
                )
                .repeating(),
            ],
        )],
    );
    app.project.extra_targets.push(audit);
    app.project.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::String("shared".to_owned()),
        },
    );
    app.main_canvas.snarl = build_snarl(&app.project);
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    let custom = egui::pos2(208.0, 319.0);
    move_canvas_node(
        &mut app
            .mapping_workspace
            .target_canvases
            .get_mut(&0)
            .expect("named target canvas exists")
            .snarl,
        CanvasNode::Graph(7),
        custom,
    );
    app.mark_clean();
    app.rebase_history();

    app.expand_selected_target_subtree();
    app.observe_editor_history(std::time::Instant::now(), false);
    let expanded = &app.project.extra_targets[0].root;
    assert_eq!(expanded.children[0].target_field, "Section");
    assert_eq!(expanded.children[0].children[0].target_field, "Items");
    assert!(!expanded.children[0].children[0].iterates());
    assert_eq!(app.history.undo_len(), 1);
    assert!(app.is_dirty());
    assert!(app.status.contains("1 repeating scopes need source paths"));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );

    app.undo_project();
    assert!(app.project.extra_targets[0].root.children.is_empty());
    assert!(!app.is_dirty());
    app.redo_project();
    assert_eq!(app.project.extra_targets[0].root.children.len(), 1);
    app.save_document_to(&project_path)
        .expect("expanded target and layout save");
    let mut loaded = FerruleApp::default();
    loaded.load_project_from(&project_path);
    assert_eq!(
        loaded.project.extra_targets[0].root.children[0].children[0].target_field,
        "Items"
    );
    assert_eq!(
        canvas_position(
            &loaded.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );
    assert!(!loaded.is_dirty());

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn primary_auto_connect_is_one_undoable_position_preserving_mutation() {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "source",
        vec![
            SchemaNode::scalar("order_id", ScalarType::String),
            SchemaNode::scalar("amount", ScalarType::Int),
        ],
    );
    app.project.target = SchemaNode::group(
        "target",
        vec![
            SchemaNode::scalar("OrderId", ScalarType::String),
            SchemaNode::scalar("amount", ScalarType::Float),
        ],
    );
    app.main_canvas.snarl = build_snarl(&app.project);
    let custom = egui::pos2(117.0, 259.0);
    move_canvas_node(
        &mut app.main_canvas.snarl,
        CanvasNode::SourceBlock(0),
        custom,
    );
    app.mark_clean();
    app.rebase_history();

    app.begin_auto_connect();
    let pending = app
        .pending_auto_connect
        .as_ref()
        .expect("confirmation is staged");
    assert_eq!(pending.plan.connections.len(), 2);
    assert_eq!(pending.plan.skipped_ambiguous, 0);
    assert_eq!(pending.plan.skipped_incompatible, 0);
    app.apply_pending_auto_connect();
    app.observe_editor_history(std::time::Instant::now(), false);

    assert_eq!(app.project.root.bindings.len(), 2);
    assert_eq!(app.project.graph.nodes.len(), 2);
    assert!(
        app.project
            .graph
            .nodes
            .values()
            .all(|node| matches!(node, Node::SourceField { .. }))
    );
    assert_eq!(
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        custom
    );
    assert_eq!(app.history.undo_len(), 1);

    app.undo_project();
    assert!(app.project.root.bindings.is_empty());
    assert!(app.project.graph.nodes.is_empty());
    assert_eq!(
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        custom
    );
    app.redo_project();
    assert_eq!(app.project.root.bindings.len(), 2);
    assert_eq!(app.project.graph.nodes.len(), 2);
}

#[test]
fn ambiguous_auto_connect_plan_leaves_the_project_unchanged() {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "source",
        vec![
            SchemaNode::group(
                "left",
                vec![SchemaNode::scalar("customer_id", ScalarType::String)],
            ),
            SchemaNode::group(
                "right",
                vec![SchemaNode::scalar("Customer-Id", ScalarType::String)],
            ),
        ],
    );
    app.project.target = SchemaNode::group(
        "target",
        vec![SchemaNode::scalar("CustomerId", ScalarType::String)],
    );
    app.main_canvas.snarl = build_snarl(&app.project);
    app.mark_clean();
    app.rebase_history();

    app.begin_auto_connect();
    let pending = app
        .pending_auto_connect
        .as_ref()
        .expect("confirmation is staged");
    assert!(pending.plan.connections.is_empty());
    assert_eq!(pending.plan.skipped_ambiguous, 1);
    app.apply_pending_auto_connect();
    app.observe_editor_history(std::time::Instant::now(), false);

    assert!(app.project.graph.nodes.is_empty());
    assert!(app.project.root.bindings.is_empty());
    assert!(!app.is_dirty());
    assert!(!app.can_undo());
}

#[test]
fn named_target_auto_connect_uses_its_scope_and_preserves_its_canvas() {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "source",
        vec![SchemaNode::scalar("status", ScalarType::String)],
    );
    app.project.extra_targets.push(NamedTarget {
        name: "audit".to_owned(),
        path: Some("audit.json".to_owned()),
        schema: SchemaNode::group(
            "audit",
            vec![SchemaNode::scalar("Status", ScalarType::String)],
        ),
        options: FormatOptions::default(),
        root: Scope::default(),
    });
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    let custom = egui::pos2(151.0, 313.0);
    move_canvas_node(
        &mut app
            .mapping_workspace
            .target_canvases
            .get_mut(&0)
            .expect("named target canvas exists")
            .snarl,
        CanvasNode::SourceBlock(0),
        custom,
    );
    app.mark_clean();
    app.rebase_history();

    app.begin_auto_connect();
    app.apply_pending_auto_connect();
    app.observe_editor_history(std::time::Instant::now(), false);

    assert!(app.project.root.bindings.is_empty());
    assert_eq!(app.project.extra_targets[0].root.bindings.len(), 1);
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::SourceBlock(0)
        ),
        custom
    );

    app.undo_project();
    assert!(app.project.extra_targets[0].root.bindings.is_empty());
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(0));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::SourceBlock(0)
        ),
        custom
    );
    app.redo_project();
    assert_eq!(app.project.extra_targets[0].root.bindings.len(), 1);
}

#[test]
fn target_removal_rekeys_tabs_and_canvases_without_removing_graph_nodes() {
    let mut app = FerruleApp::default();
    app.project.extra_targets = vec![
        named_target("first"),
        named_target("second"),
        named_target("third"),
    ];
    app.project.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::String("shared".to_owned()),
        },
    );
    app.main_canvas.snarl = build_snarl(&app.project);
    app.open_target_tab(2);
    assert!(app.ensure_target_canvas(2));
    let custom = egui::pos2(321.0, 654.0);
    move_canvas_node(
        &mut app
            .mapping_workspace
            .target_canvases
            .get_mut(&2)
            .expect("third target canvas exists")
            .snarl,
        CanvasNode::Graph(7),
        custom,
    );
    app.mark_clean();
    app.rebase_history();

    app.remove_extra_target_now(0);
    app.observe_editor_history(std::time::Instant::now(), false);

    assert_eq!(
        app.project
            .extra_targets
            .iter()
            .map(|target| target.name.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "third"]
    );
    assert!(app.project.graph.nodes.contains_key(&7));
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(1));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&1].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );

    app.undo_project();
    assert_eq!(app.project.extra_targets.len(), 3);
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(2));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&2].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );

    app.redo_project();
    assert_eq!(app.project.extra_targets.len(), 2);
    assert!(app.project.graph.nodes.contains_key(&7));
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(1));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&1].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );
}

#[test]
fn save_and_reopen_preserve_named_targets_and_mixed_tab_order() {
    let project_path = temporary_project_path("extra-target-roundtrip");
    let mut app = FerruleApp {
        document: DocumentLocation::untitled(project_path.clone()),
        ..Default::default()
    };
    let mut audit = named_target("audit");
    audit.path = Some("outputs/audit.jsonl".to_owned());
    audit.options.json_lines = true;
    audit.root.target_field = "auditRoot".to_owned();
    app.project.extra_targets = vec![audit, named_target("archive")];
    let function = FunctionId::new(42);
    app.project
        .user_functions
        .insert(function, user_function("normalize"));
    app.project.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::String("shared".to_owned()),
        },
    );
    app.main_canvas.snarl = build_snarl(&app.project);

    app.open_target_tab(1);
    app.open_function_tab(function);
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    let custom = egui::pos2(419.0, 287.0);
    move_canvas_node(
        &mut app
            .mapping_workspace
            .target_canvases
            .get_mut(&0)
            .expect("audit target canvas exists")
            .snarl,
        CanvasNode::Graph(7),
        custom,
    );
    let expected_tabs = vec![
        MappingDocument::Main,
        MappingDocument::Target(1),
        MappingDocument::Function(function),
        MappingDocument::Target(0),
    ];
    assert_eq!(app.mapping_workspace.tabs, expected_tabs);

    app.save_document_to(&project_path)
        .expect("project and layout save");
    let mut loaded = FerruleApp::default();
    loaded.load_project_from(&project_path);

    assert_eq!(loaded.project.extra_targets.len(), 2);
    let loaded_audit = &loaded.project.extra_targets[0];
    assert_eq!(loaded_audit.name, "audit");
    assert_eq!(loaded_audit.path.as_deref(), Some("outputs/audit.jsonl"));
    assert!(loaded_audit.options.json_lines);
    assert_eq!(loaded_audit.root.target_field, "auditRoot");
    assert_eq!(loaded.mapping_workspace.tabs, expected_tabs);
    assert_eq!(loaded.mapping_workspace.active, MappingDocument::Target(0));
    assert_eq!(
        canvas_position(
            &loaded.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );
    assert!(
        loaded.mapping_workspace.target_canvases[&0]
            .snarl
            .nodes()
            .all(|node| !matches!(node, CanvasNode::Placeholder(_)))
    );
    assert!(!loaded.is_dirty());

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn legacy_endpoint_layout_entries_migrate_to_the_first_block() {
    let source: PersistedCanvasNode =
        serde_json::from_str(r#"{"kind":"source"}"#).expect("legacy source entry parses");
    let target: PersistedCanvasNode =
        serde_json::from_str(r#"{"kind":"target"}"#).expect("legacy target entry parses");

    assert_eq!(source, PersistedCanvasNode::Source { block: 0 });
    assert_eq!(target, PersistedCanvasNode::Target { block: 0 });
    assert_eq!(
        PersistedCanvasNode::from(CanvasNode::SourceBlock(3)),
        PersistedCanvasNode::Source { block: 3 }
    );
}

#[test]
fn canvas_layout_saves_alongside_backward_compatible_project_json() {
    let project_path = temporary_project_path("layout-roundtrip");
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.main_canvas.snarl = build_snarl(&app.project);
    move_canvas_node(
        &mut app.main_canvas.snarl,
        CanvasNode::SourceBlock(0),
        egui::pos2(73.0, 91.0),
    );
    move_canvas_node(
        &mut app.main_canvas.snarl,
        CanvasNode::Graph(7),
        egui::pos2(517.0, 233.0),
    );
    app.document = DocumentLocation::saved(project_path.clone());
    app.save_document_to(&project_path)
        .expect("project and layout save");

    let project_json = std::fs::read_to_string(&project_path).expect("project was written");
    serde_json::from_str::<Project>(&project_json).expect("project JSON remains unchanged");
    assert!(!project_json.contains("\"layout\""));
    assert!(layout_path(&project_path).is_file());

    let mut loaded = FerruleApp {
        document: DocumentLocation::saved(project_path.clone()),
        ..Default::default()
    };
    loaded.load_project_from(&project_path);
    assert_eq!(
        canvas_position(&loaded.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        egui::pos2(73.0, 91.0)
    );
    assert_eq!(
        canvas_position(&loaded.main_canvas.snarl, CanvasNode::Graph(7)),
        egui::pos2(517.0, 233.0)
    );
    assert!(!loaded.is_dirty());

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn layout_sidecar_restores_placeholder_identity_and_wiring() {
    let project_path = temporary_project_path("placeholder-roundtrip");
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![0],
        },
    );
    let mut snarl = Snarl::new();
    snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::SourceBlock(0));
    let placeholder = snarl.insert_node(egui::pos2(180.0, 210.0), CanvasNode::Placeholder(0));
    let call = snarl.insert_node(egui::pos2(480.0, 210.0), CanvasNode::Graph(1));
    snarl.insert_node(egui::pos2(780.0, 0.0), CanvasNode::TargetBlock(0));
    snarl.connect(
        OutPinId {
            node: placeholder,
            output: 0,
        },
        InPinId {
            node: call,
            input: 0,
        },
    );
    app.main_canvas.snarl = snarl;
    app.document = DocumentLocation::saved(project_path.clone());
    app.save_document_to(&project_path)
        .expect("project and layout save");

    let mut loaded = FerruleApp {
        document: DocumentLocation::saved(project_path.clone()),
        ..Default::default()
    };
    loaded.load_project_from(&project_path);
    assert_eq!(
        canvas_position(&loaded.main_canvas.snarl, CanvasNode::Placeholder(0)),
        egui::pos2(180.0, 210.0)
    );
    let wires: Vec<_> = loaded
        .main_canvas
        .snarl
        .wires()
        .map(|(from, to)| {
            (
                loaded.main_canvas.snarl[from.node],
                loaded.main_canvas.snarl[to.node],
            )
        })
        .collect();
    assert_eq!(
        wires,
        vec![(CanvasNode::Placeholder(0), CanvasNode::Graph(1))]
    );

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn stale_layout_cannot_reclassify_or_reposition_nodes() {
    let project_path = temporary_project_path("stale-placeholder-layout");
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::Int(1),
        },
    );
    app.main_canvas.snarl = build_snarl(&app.project);
    for node in app.main_canvas.snarl.nodes_mut() {
        if *node == CanvasNode::Graph(0) {
            *node = CanvasNode::Placeholder(0);
        }
    }
    move_canvas_node(
        &mut app.main_canvas.snarl,
        CanvasNode::SourceBlock(0),
        egui::pos2(901.0, 733.0),
    );
    move_canvas_node(
        &mut app.main_canvas.snarl,
        CanvasNode::Graph(1),
        egui::pos2(1201.0, 833.0),
    );
    app.document = DocumentLocation::saved(project_path.clone());
    app.save_document_to(&project_path)
        .expect("project and layout save");

    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("intentional null replacement".into()),
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::Int(2),
        },
    );
    let default_layout = build_snarl(&app.project);
    let expected_source = canvas_position(&default_layout, CanvasNode::SourceBlock(0));
    let expected_graph = canvas_position(&default_layout, CanvasNode::Graph(1));
    std::fs::write(
        &project_path,
        serde_json::to_string_pretty(&app.project).expect("project serializes"),
    )
    .expect("replacement project is written without touching its layout");

    let mut loaded = FerruleApp {
        document: DocumentLocation::saved(project_path.clone()),
        ..Default::default()
    };
    loaded.load_project_from(&project_path);
    assert!(
        loaded
            .main_canvas
            .snarl
            .nodes()
            .any(|node| *node == CanvasNode::Graph(0))
    );
    assert!(
        !loaded
            .main_canvas
            .snarl
            .nodes()
            .any(|node| *node == CanvasNode::Placeholder(0))
    );
    assert_eq!(
        canvas_position(&loaded.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        expected_source
    );
    assert_eq!(
        canvas_position(&loaded.main_canvas.snarl, CanvasNode::Graph(1)),
        expected_graph
    );

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn project_without_layout_sidecar_uses_default_layout() {
    let project_path = temporary_project_path("legacy-project");
    let project = blank_project();
    std::fs::write(
        &project_path,
        serde_json::to_string_pretty(&project).expect("project serializes"),
    )
    .expect("legacy project is written");

    let mut app = FerruleApp {
        document: DocumentLocation::saved(project_path.clone()),
        ..Default::default()
    };
    app.load_project_from(&project_path);
    assert_eq!(
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        egui::pos2(0.0, 0.0)
    );
    assert_eq!(app.status, format!("loaded {}", project_path.display()));
    assert!(!app.is_dirty());

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn canvas_moves_and_arrange_roundtrip_through_history() {
    let mut app = FerruleApp::default();
    let arranged = canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0));
    let custom = egui::pos2(123.0, 456.0);
    move_canvas_node(
        &mut app.main_canvas.snarl,
        CanvasNode::SourceBlock(0),
        custom,
    );
    app.mark_clean();
    app.rebase_history();

    arrange_snarl(
        &mut app.main_canvas.snarl,
        &app.main_canvas.node_sizes,
        crate::appearance::WireAppearance::default(),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        arranged
    );
    assert!(app.is_dirty());

    app.undo_project();
    assert_eq!(
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        custom
    );
    assert!(!app.is_dirty());
    app.redo_project();
    assert_eq!(
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        arranged
    );
    assert!(app.is_dirty());
}

#[test]
fn arrange_preserves_placeholder_identity_and_wiring() {
    let mut project = blank_project();
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    project.graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![0],
        },
    );
    let mut current = build_snarl(&project);
    for node in current.nodes_mut() {
        if *node == CanvasNode::Graph(0) {
            *node = CanvasNode::Placeholder(0);
        }
    }

    let identities_before: Vec<_> = current.node_ids().map(|(id, node)| (id, *node)).collect();
    let wires_before: Vec<_> = current.wires().collect();
    arrange_snarl(
        &mut current,
        &std::collections::BTreeMap::new(),
        crate::appearance::WireAppearance::default(),
    );
    assert!(
        current
            .nodes()
            .any(|node| *node == CanvasNode::Placeholder(0))
    );
    assert_eq!(
        current
            .node_ids()
            .map(|(id, node)| (id, *node))
            .collect::<Vec<_>>(),
        identities_before
    );
    assert_eq!(current.wires().collect::<Vec<_>>(), wires_before);
}

#[test]
fn project_dirty_state_tracks_saved_content() {
    let mut app = FerruleApp::default();
    assert!(!app.is_dirty());

    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("changed".into()),
        },
    );
    assert!(app.is_dirty());

    app.project.graph.nodes.clear();
    assert!(
        !app.is_dirty(),
        "restoring saved content clears dirty state"
    );
}

#[test]
fn destructive_actions_wait_for_confirmation_when_dirty() {
    let mut app = FerruleApp::default();
    assert_eq!(
        app.request_destructive_action(DestructiveAction::NewProject),
        Some(DestructiveAction::NewProject)
    );

    app.history.mark_unsaved();
    assert_eq!(
        app.request_destructive_action(DestructiveAction::OpenProject),
        None
    );
    assert_eq!(
        app.pending_destructive_action,
        Some(DestructiveAction::OpenProject)
    );
}

#[test]
fn failed_open_preserves_the_current_document_and_dirty_state() {
    let old_path = temporary_project_path("failed-open-current");
    let invalid_path = old_path.with_file_name("invalid.json");
    std::fs::write(&invalid_path, "not json").expect("invalid project is written");
    let mut app = FerruleApp {
        document: DocumentLocation::saved(old_path.clone()),
        ..Default::default()
    };
    app.project.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::String("unsaved".into()),
        },
    );
    assert!(app.is_dirty());

    app.load_project_from(&invalid_path);

    assert_eq!(app.document, DocumentLocation::saved(old_path.clone()));
    assert!(app.project.graph.nodes.contains_key(&7));
    assert!(app.is_dirty());
    assert_eq!(app.diagnostics.items().len(), 1);
    std::fs::remove_dir_all(old_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn failed_save_does_not_change_the_document_association() {
    let old_path = temporary_project_path("failed-save-current");
    let directory = old_path.parent().expect("project has parent").to_path_buf();
    let mut app = FerruleApp {
        document: DocumentLocation::saved(old_path.clone()),
        ..Default::default()
    };
    app.project.graph.nodes.insert(
        8,
        Node::Const {
            value: ir::Value::Null,
        },
    );

    assert!(app.save_document_to(&directory).is_err());
    assert_eq!(app.document, DocumentLocation::saved(old_path.clone()));
    assert!(app.is_dirty());
    std::fs::remove_dir_all(&directory).expect("temporary test directory is removed");
}

#[test]
fn invalid_run_does_not_save_or_clear_dirty_state() {
    let project_path = temporary_project_path("invalid-run");
    let mut app = FerruleApp::default();
    app.save_document_to(&project_path)
        .expect("baseline project is saved");
    let saved = std::fs::read_to_string(&project_path).expect("baseline project is readable");
    app.project.root.bindings.push(Binding {
        target_field: "missing".into(),
        node: 999,
    });

    app.run(&egui::Context::default());

    assert_eq!(
        std::fs::read_to_string(&project_path).expect("project remains readable"),
        saved
    );
    assert!(app.is_dirty());
    assert!(!app.diagnostics.items().is_empty());
    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn diagnostic_ownership_survives_save_and_reload() {
    let project_path = temporary_project_path("diagnostic-ownership");
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        42,
        Node::Call {
            function: "missing-function".into(),
            args: vec![],
        },
    );
    let expected =
        crate::diagnostics::DiagnosticLocation::Validation(engine::ValidationOwner::GraphNode {
            function: None,
            node: 42,
        });
    let outcome = app
        .save_document_to(&project_path)
        .expect("invalid projects remain saveable");
    app.apply_save_outcome(&project_path, outcome);
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.location.as_ref() == Some(&expected))
    );
    let diagnostic = app
        .diagnostics
        .items()
        .iter()
        .find(|item| item.location.as_ref() == Some(&expected))
        .expect("owned diagnostic")
        .clone();
    assert!(app.navigate_to_diagnostic(&diagnostic));
    assert!(app.main_canvas.pending_focus.is_some());

    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&project_path);
    assert!(
        reopened
            .diagnostics
            .items()
            .iter()
            .any(|item| item.location.as_ref() == Some(&expected))
    );
    let diagnostic = reopened
        .diagnostics
        .items()
        .iter()
        .find(|item| item.location.as_ref() == Some(&expected))
        .expect("reloaded owned diagnostic")
        .clone();
    assert!(reopened.navigate_to_diagnostic(&diagnostic));
    assert!(reopened.main_canvas.pending_focus.is_some());
    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn blank_run_paths_fall_back_to_stored_project_paths() {
    let project_path = temporary_project_path("stored-run-paths");
    let directory = project_path.parent().expect("project has parent");
    std::fs::write(directory.join("input.xml"), "<root/>").expect("input instance is written");
    let mut app = FerruleApp {
        document: DocumentLocation::untitled(project_path.clone()),
        ..Default::default()
    };
    app.project.source_path = Some("input.xml".into());
    app.project.target_path = Some("output.xml".into());
    app.save_document_to(&project_path)
        .expect("project with stored paths is saved");
    app.input_path.clear();
    app.output_path.clear();

    app.run(&egui::Context::default());
    wait_for_file_run_completion(&mut app);

    assert!(directory.join("output.xml").is_file(), "{}", app.status);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    assert!(app.show_run_report);
    let report = app
        .run_report
        .as_ref()
        .expect("successful run has a report");
    assert_eq!(report.selected_output(), 0);
    assert_eq!(report.report.outputs.len(), 1);
    assert_eq!(report.report.outputs[0].path, directory.join("output.xml"));
    assert!(!app.is_dirty());
    std::fs::remove_dir_all(directory).expect("temporary test directory is removed");
}

#[test]
fn first_save_rebases_relative_paths_from_the_untitled_document_base() {
    let project_path = temporary_project_path("first-save-rebase");
    let mut app = FerruleApp::default();
    app.project.source_path = Some("Cargo.toml".into());

    app.save_document_to(&project_path)
        .expect("untitled project saves");

    let stored = app
        .project
        .source_path
        .as_deref()
        .expect("source path remains configured");
    let resolved = project_path
        .parent()
        .expect("project has a parent")
        .join(stored);
    assert_eq!(
        std::fs::canonicalize(resolved).expect("rebased source exists"),
        std::fs::canonicalize("Cargo.toml").expect("workspace manifest exists")
    );
    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn layout_failure_keeps_saved_project_and_editor_on_the_same_base() {
    let project_path = temporary_project_path("layout-failure");
    std::fs::create_dir_all(layout_path(&project_path))
        .expect("layout destination is blocked by a directory");
    let mut app = FerruleApp::default();
    app.project.source_path = Some("Cargo.toml".into());

    let outcome = app
        .save_document_to(&project_path)
        .expect("project save succeeds even when layout fails");

    assert!(outcome.layout_warning.is_some());
    assert_eq!(app.document, DocumentLocation::saved(project_path.clone()));
    let saved: Project =
        serde_json::from_slice(&std::fs::read(&project_path).expect("saved project is readable"))
            .expect("saved project parses");
    assert_eq!(saved.source_path, app.project.source_path);
    assert!(!app.is_dirty());
    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn preview_executes_an_unsaved_project_without_writing_its_logical_output() -> anyhow::Result<()> {
    let directory = std::env::temp_dir().join(format!(
        "ferrule-gui-preview-unsaved-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory)?;
    let logical_output = directory.join("must-not-be-written.xml");
    let mut app = FerruleApp {
        preview_draft: Some(crate::preview::PreviewDraft {
            target: crate::preview::PreviewTarget::Primary,
            input_identity: "input.xml".into(),
            output_identity: logical_output.display().to_string(),
            input_text: "<root/>".into(),
            debug_breakpoint: None,
        }),
        ..FerruleApp::default()
    };

    app.execute_preview();
    wait_for_preview_completion(&mut app);

    assert!(
        app.preview_draft.is_none(),
        "successful preview closes setup"
    );
    assert!(app.show_run_report);
    assert!(!logical_output.exists());
    assert!(app.document.saved_path().is_none());
    assert!(!app.is_dirty());
    let Some(report) = app.run_report.as_mut() else {
        anyhow::bail!("successful preview has no report");
    };
    assert_eq!(
        report.report.kind,
        crate::run_report::RunReportKind::Preview
    );
    assert_eq!(report.report.outputs.len(), 1);
    assert!(matches!(
        report.report.outputs[0].preview(),
        crate::run_report::OutputPreview::Text { content, .. }
            if content.contains("<root")
    ));
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

fn wait_for_preview_completion(app: &mut FerruleApp) {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.pending_preview.is_some() && std::time::Instant::now() < deadline {
        app.poll_preview(&context);
        if app.pending_preview.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    assert!(app.pending_preview.is_none(), "preview worker completes");
}

fn wait_for_file_run_completion(app: &mut FerruleApp) {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.pending_file_run.is_some() && std::time::Instant::now() < deadline {
        app.poll_file_run(&context);
        if app.pending_file_run.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    assert!(app.pending_file_run.is_none(), "file run worker completes");
}

fn wait_for_file_run_pause(app: &mut FerruleApp) -> engine::PendingTargetWrite {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_file_run(&context);
        match app.pending_file_run.as_ref().map(|pending| &pending.phase) {
            Some(run_ui::FileRunPhase::Paused(write)) => return (**write).clone(),
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug file run did not pause before deadline: {other:?}"),
        }
    }
}

fn wait_for_file_node_pause(app: &mut FerruleApp) -> engine::PendingNodeValue {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_file_run(&context);
        match app.pending_file_run.as_ref().map(|pending| &pending.phase) {
            Some(run_ui::FileRunPhase::PausedNode(value)) => return (**value).clone(),
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug file run did not pause at a node before deadline: {other:?}"),
        }
    }
}

fn wait_for_file_function_node_pause(app: &mut FerruleApp) -> engine::PendingFunctionNodeValue {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_file_run(&context);
        match app.pending_file_run.as_ref().map(|pending| &pending.phase) {
            Some(run_ui::FileRunPhase::PausedFunctionNode(value)) => return (**value).clone(),
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => {
                panic!("debug file run did not pause in a function before deadline: {other:?}")
            }
        }
    }
}

fn wait_for_file_input_pause(app: &mut FerruleApp) -> engine::PendingNodeInput {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_file_run(&context);
        match app.pending_file_run.as_ref().map(|pending| &pending.phase) {
            Some(run_ui::FileRunPhase::PausedInput(input)) => return (**input).clone(),
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug file run did not pause at an input before deadline: {other:?}"),
        }
    }
}

fn wait_for_file_function_input_pause(app: &mut FerruleApp) -> engine::PendingFunctionNodeInput {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_file_run(&context);
        match app.pending_file_run.as_ref().map(|pending| &pending.phase) {
            Some(run_ui::FileRunPhase::PausedFunctionInput(input)) => return (**input).clone(),
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug file run did not pause at a function input: {other:?}"),
        }
    }
}

fn wait_for_debug_pause(app: &mut FerruleApp) -> engine::PendingTargetWrite {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_preview(&context);
        match app.pending_preview.as_ref().map(|pending| &pending.phase) {
            Some(preview_ui::PreviewPhase::Paused(write)) => return (**write).clone(),
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug preview did not pause before deadline: {other:?}"),
        }
    }
}

fn wait_for_preview_node_pause(app: &mut FerruleApp) -> engine::PendingNodeValue {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_preview(&context);
        match app.pending_preview.as_ref().map(|pending| &pending.phase) {
            Some(preview_ui::PreviewPhase::PausedNode(value)) => return (**value).clone(),
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug preview did not pause at a node before deadline: {other:?}"),
        }
    }
}

fn wait_for_preview_function_node_pause(app: &mut FerruleApp) -> engine::PendingFunctionNodeValue {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_preview(&context);
        match app.pending_preview.as_ref().map(|pending| &pending.phase) {
            Some(preview_ui::PreviewPhase::PausedFunctionNode(value)) => return (**value).clone(),
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug preview did not pause in a function before deadline: {other:?}"),
        }
    }
}

fn wait_for_preview_input_pause(app: &mut FerruleApp) -> engine::PendingNodeInput {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_preview(&context);
        match app.pending_preview.as_ref().map(|pending| &pending.phase) {
            Some(preview_ui::PreviewPhase::PausedInput(input)) => return (**input).clone(),
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug preview did not pause at an input before deadline: {other:?}"),
        }
    }
}

fn wait_for_preview_function_input_pause(app: &mut FerruleApp) -> engine::PendingFunctionNodeInput {
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.poll_preview(&context);
        match app.pending_preview.as_ref().map(|pending| &pending.phase) {
            Some(preview_ui::PreviewPhase::PausedFunctionInput(input)) => return (**input).clone(),
            Some(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            other => panic!("debug preview did not pause at a function input: {other:?}"),
        }
    }
}

fn two_field_debug_preview_app() -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "root",
        vec![SchemaNode::scalar("input", ScalarType::String)],
    );
    app.project.target = SchemaNode::group(
        "root",
        vec![
            SchemaNode::scalar("first", ScalarType::String),
            SchemaNode::scalar("second", ScalarType::String),
        ],
    );
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("A".into()),
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::String("B".into()),
        },
    );
    app.project.root.bindings = vec![
        Binding {
            target_field: "first".into(),
            node: 0,
        },
        Binding {
            target_field: "second".into(),
            node: 1,
        },
    ];
    app.preview_draft = Some(crate::preview::PreviewDraft {
        target: crate::preview::PreviewTarget::Primary,
        input_identity: "input.xml".into(),
        output_identity: "output.xml".into(),
        input_text: "<root><input>source value</input></root>".into(),
        debug_breakpoint: None,
    });
    app
}

fn two_field_file_run_app(test_name: &str) -> (FerruleApp, PathBuf, PathBuf) {
    let project_path = temporary_project_path(test_name);
    let directory = project_path.parent().expect("project has parent");
    let input = directory.join("input.xml");
    let output = directory.join("output.xml");
    std::fs::write(&input, "<root><input>source value</input></root>")
        .expect("input instance is written");
    std::fs::write(&output, "old output").expect("old output is written");
    let mut app = two_field_debug_preview_app();
    app.preview_draft = None;
    app.save_document_to(&project_path)
        .expect("file run project is saved");
    app.input_path = input.display().to_string();
    app.output_path = output.display().to_string();
    (app, project_path, output)
}

#[test]
fn debug_file_run_steps_then_publishes_on_continue() {
    let (mut app, project_path, output) = two_field_file_run_app("debug-file-step");
    app.debug_run(&egui::Context::default());
    let first = wait_for_file_run_pause(&mut app);
    assert_eq!(first.field, "first");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    assert!(app.run_report.is_none());

    app.file_run_command(run_ui::FileRunCommand::Step);
    let second = wait_for_file_run_pause(&mut app);
    assert_eq!(second.field, "second");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");

    app.file_run_command(run_ui::FileRunCommand::Continue);
    wait_for_file_run_completion(&mut app);
    assert!(app.show_run_report, "{}", app.status);
    assert!(
        std::fs::read_to_string(&output)
            .unwrap()
            .contains("<second>B</second>")
    );
    std::fs::remove_dir_all(project_path.parent().unwrap()).unwrap();
}

#[test]
fn file_run_position_condition_rejects_zero_and_never_matches_root_writes() {
    let (mut app, project_path, output) = two_field_file_run_app("debug-file-position");
    app.file_run_position_condition.enabled = true;
    app.file_run_position_condition.text = "0".into();
    app.debug_run(&egui::Context::default());
    assert!(app.pending_file_run.is_none());
    assert_eq!(app.status, "debug run blocked");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");

    app.file_run_position_condition.text = "1".into();
    app.debug_run(&egui::Context::default());
    wait_for_file_run_completion(&mut app);
    assert!(app.show_run_report, "{}", app.status);
    assert!(
        std::fs::read_to_string(&output)
            .unwrap()
            .contains("<second>B</second>")
    );
    std::fs::remove_dir_all(project_path.parent().unwrap()).unwrap();
}

#[test]
fn cancelling_paused_file_run_preserves_existing_output() {
    let (mut app, project_path, output) = two_field_file_run_app("debug-file-cancel");
    app.debug_run(&egui::Context::default());
    assert_eq!(wait_for_file_run_pause(&mut app).field, "first");
    app.file_run_command(run_ui::FileRunCommand::Cancel);
    wait_for_file_run_completion(&mut app);
    assert_eq!(app.status, "run cancelled");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    assert!(app.run_report.is_none());
    std::fs::remove_dir_all(project_path.parent().unwrap()).unwrap();
}

#[test]
fn cancelling_ordinary_file_run_suppresses_late_success_and_publication() {
    let (mut app, project_path, output) = two_field_file_run_app("file-run-cancel");
    app.run(&egui::Context::default());
    app.file_run_command(run_ui::FileRunCommand::Cancel);
    wait_for_file_run_completion(&mut app);
    assert_eq!(app.status, "run cancelled");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    assert!(app.run_report.is_none());
    std::fs::remove_dir_all(project_path.parent().unwrap()).unwrap();
}

#[test]
fn conditional_file_run_skips_nonmatching_write_and_can_cancel() {
    let (mut app, project_path, output) = two_field_file_run_app("file-run-condition");
    app.file_run_value_condition.enabled = true;
    app.file_run_value_condition.text = "B".into();
    app.debug_run(&egui::Context::default());
    let pending = wait_for_file_run_pause(&mut app);
    assert_eq!(pending.field, "second");
    assert_eq!(pending.draft.fields[0].name, "first");
    app.file_run_command(run_ui::FileRunCommand::Cancel);
    wait_for_file_run_completion(&mut app);
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    std::fs::remove_dir_all(project_path.parent().unwrap()).unwrap();
}

#[test]
fn source_field_condition_combines_with_target_write_in_file_run() {
    let (mut app, project_path, output) = two_field_file_run_app("file-source-condition");
    app.file_run_source_condition.enabled = true;
    app.file_run_source_condition.field = "input".into();
    app.file_run_source_condition.text = "source value".into();
    app.file_run_value_condition.enabled = true;
    app.file_run_value_condition.text = "B".into();
    app.debug_run(&egui::Context::default());
    let pending = wait_for_file_run_pause(&mut app);
    assert_eq!(pending.field, "second");
    assert_eq!(
        pending
            .source_field_probe
            .as_ref()
            .and_then(|probe| probe.preview.as_ref())
            .and_then(|preview| preview.value.as_ref())
            .map(|value| value.preview.as_str()),
        Some("source value")
    );
    app.file_run_command(run_ui::FileRunCommand::Cancel);
    wait_for_file_run_completion(&mut app);
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    std::fs::remove_dir_all(project_path.parent().unwrap()).unwrap();
}

#[test]
fn value_node_condition_distinguishes_equal_file_run_values() {
    let (mut app, project_path, output) = two_field_file_run_app("file-value-node");
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("B".into()),
        },
    );
    app.file_run_node_condition.enabled = true;
    app.file_run_node_condition.text = "1".into();
    app.debug_run(&egui::Context::default());
    let pending = wait_for_file_run_pause(&mut app);
    assert_eq!(pending.field, "second");
    assert_eq!(
        pending.binding,
        engine::TraceTargetFieldBinding::StaticBinding { value: 1 }
    );
    assert_eq!(pending.draft.fields[0].name, "first");
    assert_eq!(
        pending.draft.fields[0]
            .preview
            .value
            .as_ref()
            .unwrap()
            .preview,
        "B"
    );
    app.file_run_command(run_ui::FileRunCommand::Cancel);
    wait_for_file_run_completion(&mut app);
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    std::fs::remove_dir_all(project_path.parent().unwrap()).unwrap();
}

fn three_field_debug_preview_app() -> FerruleApp {
    let mut app = two_field_debug_preview_app();
    app.project.target = SchemaNode::group(
        "root",
        vec![
            SchemaNode::scalar("first", ScalarType::String),
            SchemaNode::scalar("second", ScalarType::String),
            SchemaNode::scalar("third", ScalarType::String),
        ],
    );
    app.project.graph.nodes.insert(
        2,
        Node::Const {
            value: ir::Value::String("C".into()),
        },
    );
    app.project.root.bindings.push(Binding {
        target_field: "third".into(),
        node: 2,
    });
    app
}

fn select_debug_breakpoint(app: &mut FerruleApp, target_path: &[&str], field: &str) {
    let draft = app.preview_draft.as_mut().expect("preview draft exists");
    let choice = crate::preview::PreviewBreakpoint {
        target_path: target_path.iter().map(|part| (*part).into()).collect(),
        field: field.into(),
    };
    assert!(
        crate::preview::breakpoint_candidates(&app.project, &draft.target).contains(&choice),
        "breakpoint is selectable from declared target writes"
    );
    draft.debug_breakpoint = Some(choice);
}

#[test]
fn debug_preview_steps_before_target_writes_and_continues() {
    let mut app = two_field_debug_preview_app();
    app.execute_debug_preview();
    assert!(app.pending_preview.is_some());

    let first = wait_for_debug_pause(&mut app);
    assert_eq!(first.field, "first");
    assert!(first.draft.fields.is_empty());
    assert_eq!(first.pending.value.as_ref().unwrap().preview, "A");
    let input = first
        .source
        .frames
        .iter()
        .flat_map(|frame| &frame.fields)
        .find(|field| field.name == "input")
        .expect("active source frame includes the input field");
    assert_eq!(
        input.preview.value.as_ref().unwrap().preview,
        "source value"
    );
    assert!(app.run_report.is_none());

    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| app.show_preview_setup(ui.ctx()));
    assert!(
        app.pending_preview.is_some(),
        "showing paused UI is nonblocking"
    );

    app.preview_command(preview_ui::PreviewCommand::Step);
    let second = wait_for_debug_pause(&mut app);
    assert_eq!(second.field, "second");
    assert_eq!(second.draft.fields[0].name, "first");
    assert_eq!(
        second.draft.fields[0]
            .preview
            .value
            .as_ref()
            .unwrap()
            .preview,
        "A"
    );

    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    assert!(app.show_run_report);
    assert_eq!(app.status, "previewed 1 record(s) for Primary");
    let report = app.run_report.as_mut().expect("completed preview report");
    assert!(matches!(
        report.report.outputs[0].preview(),
        crate::run_report::OutputPreview::Text { content, .. }
            if content.contains("<first>A</first>") && content.contains("<second>B</second>")
    ));
    assert_eq!(
        report
            .report
            .trace
            .events
            .iter()
            .filter(|event| matches!(event, cli::TraceEvent::TargetFieldWritten { .. }))
            .count(),
        2
    );
}

#[test]
fn conditional_preview_matches_scalar_type_and_step_ignores_condition() {
    let mut app = three_field_debug_preview_app();
    app.project.target = SchemaNode::group(
        "root",
        vec![
            SchemaNode::scalar("first", ScalarType::Int),
            SchemaNode::scalar("second", ScalarType::String),
            SchemaNode::scalar("third", ScalarType::String),
        ],
    );
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Int(1),
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::String("1".into()),
        },
    );
    app.preview_value_condition.enabled = true;
    app.preview_value_condition.text = "1".into();

    app.execute_debug_preview();
    let second = wait_for_debug_pause(&mut app);
    assert_eq!(second.field, "second");
    assert_eq!(second.draft.fields[0].name, "first");

    app.preview_command(preview_ui::PreviewCommand::Step);
    let third = wait_for_debug_pause(&mut app);
    assert_eq!(third.field, "third");
    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    assert!(app.show_run_report, "{}", app.status);
}

#[test]
fn conditional_preview_never_matches_truncated_string_prefix() {
    let mut app = two_field_debug_preview_app();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("x".repeat(161)),
        },
    );
    app.preview_value_condition.enabled = true;
    app.preview_value_condition.text = "x".repeat(160);

    app.execute_debug_preview();
    wait_for_preview_completion(&mut app);
    assert!(app.show_run_report, "{}", app.status);
}

#[test]
fn debug_breakpoint_skips_earlier_field_then_step_pauses_at_next_write() {
    let mut app = three_field_debug_preview_app();
    select_debug_breakpoint(&mut app, &[], "second");
    app.execute_debug_preview();

    let second = wait_for_debug_pause(&mut app);
    assert_eq!(second.field, "second");
    assert_eq!(second.scope.target_path, Vec::<String>::new());
    assert_eq!(second.draft.fields.len(), 1);
    assert_eq!(second.draft.fields[0].name, "first");

    app.preview_command(preview_ui::PreviewCommand::Step);
    let third = wait_for_debug_pause(&mut app);
    assert_eq!(third.field, "third");
    assert_eq!(third.draft.fields.len(), 2);

    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    assert!(app.show_run_report);
}

#[test]
fn debug_breakpoint_continue_skips_other_fields() {
    let mut app = three_field_debug_preview_app();
    select_debug_breakpoint(&mut app, &[], "second");
    app.execute_debug_preview();
    assert_eq!(wait_for_debug_pause(&mut app).field, "second");

    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    let report = app.run_report.as_mut().expect("completed preview report");
    assert!(matches!(
        report.report.outputs[0].preview(),
        crate::run_report::OutputPreview::Text { content, .. }
            if content.contains("<third>C</third>")
    ));
}

fn repeated_row_debug_preview_app(rows: usize) -> FerruleApp {
    let mut app = FerruleApp::default();
    let mut source_row = SchemaNode::group("row", Vec::new());
    source_row.repeating = true;
    let mut target_row =
        SchemaNode::group("row", vec![SchemaNode::scalar("value", ScalarType::String)]);
    target_row.repeating = true;
    app.project.source = SchemaNode::group("root", vec![source_row]);
    app.project.target = SchemaNode::group("root", vec![target_row]);
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("X".into()),
        },
    );
    app.project.root.children.push(Scope {
        target_field: "row".into(),
        iteration: mapping::ScopeIteration::Source(vec!["row".into()]),
        bindings: vec![Binding {
            target_field: "value".into(),
            node: 0,
        }],
        ..Scope::default()
    });
    app.preview_draft = Some(crate::preview::PreviewDraft {
        target: crate::preview::PreviewTarget::Primary,
        input_identity: "input.xml".into(),
        output_identity: "output.xml".into(),
        input_text: format!("<root>{}</root>", "<row/>".repeat(rows)),
        debug_breakpoint: None,
    });
    app
}

#[test]
fn debug_breakpoint_continue_stops_at_next_matching_row() {
    let mut app = repeated_row_debug_preview_app(2);
    select_debug_breakpoint(&mut app, &["row"], "value");
    app.execute_debug_preview();

    let first = wait_for_debug_pause(&mut app);
    assert_eq!(first.field, "value");
    assert_eq!(
        first.positions.last().map(|position| position.index),
        Some(1)
    );
    app.preview_command(preview_ui::PreviewCommand::Continue);

    let second = wait_for_debug_pause(&mut app);
    assert_eq!(second.field, "value");
    assert_eq!(
        second.positions.last().map(|position| position.index),
        Some(2)
    );
    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    assert!(app.run_report.is_some());
}

#[test]
fn position_breakpoint_combines_with_field_and_value_then_step_overrides_it() {
    let mut app = repeated_row_debug_preview_app(3);
    select_debug_breakpoint(&mut app, &["row"], "value");
    app.preview_value_condition.enabled = true;
    app.preview_value_condition.text = "X".into();
    app.preview_position_condition.enabled = true;
    app.preview_position_condition.text = "2".into();
    app.execute_debug_preview();

    let second = wait_for_debug_pause(&mut app);
    assert_eq!(second.field, "value");
    assert_eq!(
        second.positions.last().map(|position| position.index),
        Some(2)
    );
    app.preview_command(preview_ui::PreviewCommand::Step);
    let third = wait_for_debug_pause(&mut app);
    assert_eq!(
        third.positions.last().map(|position| position.index),
        Some(3)
    );
    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    assert!(app.run_report.is_some());
}

#[test]
fn source_field_breakpoint_selects_active_row_and_step_overrides_it() {
    let mut app = repeated_row_debug_preview_app(3);
    let mut source_row =
        SchemaNode::group("row", vec![SchemaNode::scalar("id", ScalarType::String)]);
    source_row.repeating = true;
    app.project.source = SchemaNode::group("root", vec![source_row]);
    app.preview_draft.as_mut().unwrap().input_text =
        "<root><row><id>A</id></row><row><id>B</id></row><row><id>C</id></row></root>".into();
    app.preview_source_condition.enabled = true;
    app.preview_source_condition.field = "id".into();
    app.preview_source_condition.text = "B".into();
    app.execute_debug_preview();

    let second = wait_for_debug_pause(&mut app);
    assert_eq!(
        second.positions.last().map(|position| position.index),
        Some(2)
    );
    assert_eq!(
        second
            .source_field_probe
            .as_ref()
            .and_then(|probe| probe.preview.as_ref())
            .and_then(|preview| preview.value.as_ref())
            .map(|value| value.preview.as_str()),
        Some("B")
    );
    app.preview_command(preview_ui::PreviewCommand::Step);
    let third = wait_for_debug_pause(&mut app);
    assert_eq!(
        third.positions.last().map(|position| position.index),
        Some(3)
    );
    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    assert!(app.show_run_report, "{}", app.status);
}

#[test]
fn value_node_breakpoint_selects_equal_preview_value_and_step_overrides_it() {
    let mut app = three_field_debug_preview_app();
    for node in [0, 1, 2] {
        app.project.graph.nodes.insert(
            node,
            Node::Const {
                value: ir::Value::String("B".into()),
            },
        );
    }
    app.preview_node_condition.enabled = true;
    app.preview_node_condition.text = "1".into();
    app.execute_debug_preview();

    let second = wait_for_debug_pause(&mut app);
    assert_eq!(second.field, "second");
    assert_eq!(second.draft.fields[0].name, "first");
    assert_eq!(
        second.draft.fields[0]
            .preview
            .value
            .as_ref()
            .unwrap()
            .preview,
        "B"
    );
    app.preview_command(preview_ui::PreviewCommand::Step);
    let third = wait_for_debug_pause(&mut app);
    assert_eq!(third.field, "third");
    assert_eq!(
        third.binding,
        engine::TraceTargetFieldBinding::StaticBinding { value: 2 }
    );
    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    assert!(app.show_run_report, "{}", app.status);
}

#[test]
fn source_field_condition_rejects_invalid_frame_and_field() {
    let mut condition = crate::preview::BreakpointSourceConditionDraft {
        enabled: true,
        frame_from_inner: "4".into(),
        field: "id".into(),
        value_type: crate::preview::ScalarValueType::String,
        text: "B".into(),
    };
    assert!(condition.compile().is_err());
    condition.frame_from_inner = "0".into();
    condition.field.clear();
    assert!(condition.compile().is_err());
    condition.field = "x".repeat(161);
    assert!(condition.compile().is_err());
}

#[test]
fn position_condition_ui_rejects_zero_and_preserves_overlong_input() {
    let mut condition = crate::preview::BreakpointPositionConditionDraft {
        enabled: true,
        text: "0".into(),
    };
    let context = egui::Context::default();
    let mut valid = true;
    let _ = context.run_ui(Default::default(), |ui| {
        valid = preview_ui::show_breakpoint_position_condition(ui, &mut condition);
    });
    assert!(!valid);
    condition.text = "9".repeat(100);
    let original = condition.text.clone();
    let _ = context.run_ui(Default::default(), |ui| {
        valid = preview_ui::show_breakpoint_position_condition(ui, &mut condition);
    });
    assert!(!valid);
    assert_eq!(condition.text, original, "invalid input is never truncated");
}

#[test]
fn debug_breakpoint_scope_distinguishes_equal_field_names() {
    let mut app = two_field_debug_preview_app();
    app.project.target = SchemaNode::group(
        "root",
        vec![
            SchemaNode::scalar("first", ScalarType::String),
            SchemaNode::group(
                "nested",
                vec![SchemaNode::scalar("first", ScalarType::String)],
            ),
        ],
    );
    app.project.root.bindings.pop();
    app.project.root.children.push(Scope {
        target_field: "nested".into(),
        bindings: vec![Binding {
            target_field: "first".into(),
            node: 1,
        }],
        ..Scope::default()
    });
    select_debug_breakpoint(&mut app, &["nested"], "first");
    app.execute_debug_preview();

    let nested = wait_for_debug_pause(&mut app);
    assert_eq!(nested.field, "first");
    assert_eq!(nested.scope.target_path, ["nested"]);
    assert!(nested.draft.fields.is_empty());
    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    assert!(app.run_report.is_some());
}

#[test]
fn cancelling_paused_debug_preview_produces_no_report() {
    let mut app = two_field_debug_preview_app();
    app.execute_debug_preview();
    let first = wait_for_debug_pause(&mut app);
    assert_eq!(first.field, "first");
    app.preview_command(preview_ui::PreviewCommand::Cancel);
    wait_for_preview_completion(&mut app);
    assert_eq!(app.status, "preview cancelled");
    assert!(app.run_report.is_none());
    assert!(app.preview_draft.is_none());
    assert!(app.diagnostics.is_empty());
}

#[test]
fn cancelling_ordinary_preview_discards_even_a_completed_worker_result() {
    let mut app = two_field_debug_preview_app();
    app.execute_preview();
    assert!(app.pending_preview.is_some());
    app.preview_command(preview_ui::PreviewCommand::Cancel);
    wait_for_preview_completion(&mut app);
    assert_eq!(app.status, "preview cancelled");
    assert!(app.run_report.is_none());
    assert!(app.preview_draft.is_none());
}

#[test]
fn pipeline_runner_keeps_dirty_project_open_and_reports_written_output() -> anyhow::Result<()> {
    let pipeline_path = temporary_project_path("pipeline-run");
    pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().expect("pipeline has a directory");
    std::fs::write(directory.join("orders.json"), r#"{"Value":"from host"}"#)?;

    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        9,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    let before = serde_json::to_vec(&app.project)?;
    assert!(app.is_dirty());
    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    assert!(draft.issues.is_empty());
    assert_eq!(draft.inputs.len(), 1);
    assert_eq!(draft.outputs.len(), 1);
    draft.inputs[0].path = "orders.json".into();
    draft.outputs[0].path = "result.json".into();
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_pipeline_run_setup(ui.ctx());
    });
    app.start_pipeline_run();
    for _ in 0..100 {
        app.poll_pipeline_run(&egui::Context::default());
        if app.pending_pipeline_run.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.pending_pipeline_run.is_none(), "pipeline run completes");
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("result.json"))?)?;
    assert_eq!(result["Value"].as_str(), Some("from host"));
    let report = app.run_report.as_ref().expect("pipeline report exists");
    assert_eq!(
        report.report.kind,
        crate::run_report::RunReportKind::Pipeline
    );
    assert_eq!(report.report.outputs[0].name, "prepare / Primary");
    assert_eq!(serde_json::to_vec(&app.project)?, before);
    assert!(app.document.saved_path().is_none());
    assert!(app.is_dirty());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn pipeline_report_retains_stage_attributed_node_events() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-stage-trace")?;
    let directory = pipeline_path.parent().unwrap();
    app.start_pipeline_run();
    wait_for_pipeline_completion(&mut app);
    let report = &app.run_report.as_ref().expect("pipeline report").report;
    assert_eq!(report.trace.stages, ["prepare", "finish"]);
    assert_eq!(report.trace.event_stages.len(), report.trace.events.len());
    assert!(
        report
            .trace
            .events
            .iter()
            .enumerate()
            .any(|(index, event)| {
                report.trace.event_stages[index] == 0
                    && matches!(event, cli::TraceEvent::NodeValue { node: 0, .. })
            })
    );
    assert!(
        report
            .trace
            .events
            .iter()
            .enumerate()
            .any(|(index, event)| {
                report.trace.event_stages[index] == 1
                    && matches!(event, cli::TraceEvent::NodeValue { node: 0, .. })
            })
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn debug_pipeline_steps_across_stages_then_publishes() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-debug-step")?;
    let directory = pipeline_path.parent().unwrap();
    app.start_pipeline_debug_run();
    let (stage, write) = wait_for_pipeline_pause(&mut app);
    assert_eq!(stage, "prepare");
    assert_eq!(write.field, "Value");
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );

    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Step);
    let (stage, write) = wait_for_pipeline_pause(&mut app);
    assert_eq!(stage, "finish");
    assert_eq!(write.field, "Value");
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );

    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Continue);
    wait_for_pipeline_completion(&mut app);
    assert!(app.show_run_report, "{}", app.status);
    let prepare: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("prepare.json"))?)?;
    let finish: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("finish.json"))?)?;
    assert_eq!(prepare["Value"], "A");
    assert_eq!(finish["Value"], "B");
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn pipeline_position_condition_rejects_zero_and_never_matches_root_writes() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-debug-position")?;
    let directory = pipeline_path.parent().unwrap();
    app.pipeline_run_position_condition.enabled = true;
    app.pipeline_run_position_condition.text = "0".into();
    app.start_pipeline_debug_run();
    assert!(app.pending_pipeline_run.is_none());
    assert_eq!(app.status, "debug pipeline blocked");
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );

    app.pipeline_run_position_condition.text = "1".into();
    app.start_pipeline_debug_run();
    wait_for_pipeline_completion(&mut app);
    assert!(app.show_run_report, "{}", app.status);
    let finish: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("finish.json"))?)?;
    assert_eq!(finish["Value"], "B");
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn stage_breakpoint_skips_earlier_stage_and_cancel_preserves_outputs() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-debug-breakpoint")?;
    let directory = pipeline_path.parent().unwrap();
    app.pipeline_run_breakpoint =
        pipeline_ui::breakpoint_candidates(&app.pipeline_run_draft.as_ref().unwrap().pipeline)
            .into_iter()
            .find(|candidate| {
                candidate.stage == "finish"
                    && candidate.target == engine::TraceTarget::Primary
                    && candidate.field.field == "Value"
            });
    assert!(app.pipeline_run_breakpoint.is_some());
    app.pipeline_run_value_condition.enabled = true;
    app.pipeline_run_value_condition.text = "B".into();
    app.start_pipeline_debug_run();
    let (stage, write) = wait_for_pipeline_pause(&mut app);
    assert_eq!(stage, "finish");
    assert_eq!(write.field, "Value");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline cancelled");
    assert!(!app.show_run_report);
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn pipeline_source_condition_matches_only_the_later_stage_input() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-source-condition")?;
    let directory = pipeline_path.parent().unwrap();
    app.pipeline_run_source_condition.enabled = true;
    app.pipeline_run_source_condition.field = "Value".into();
    app.pipeline_run_source_condition.text = "A".into();
    app.start_pipeline_debug_run();
    let (stage, write) = wait_for_pipeline_pause(&mut app);
    assert_eq!(stage, "finish");
    assert_eq!(write.field, "Value");
    assert_eq!(
        write
            .source_field_probe
            .as_ref()
            .and_then(|probe| probe.preview.as_ref())
            .and_then(|preview| preview.value.as_ref())
            .map(|value| value.preview.as_str()),
        Some("A")
    );
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline cancelled");
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn value_node_breakpoint_combines_with_pipeline_stage_selection() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-value-node-stage")?;
    let directory = pipeline_path.parent().unwrap();
    app.pipeline_run_breakpoint =
        pipeline_ui::breakpoint_candidates(&app.pipeline_run_draft.as_ref().unwrap().pipeline)
            .into_iter()
            .find(|candidate| candidate.stage == "finish" && candidate.field.field == "Value");
    assert!(app.pipeline_run_breakpoint.is_some());
    app.pipeline_run_node_condition.enabled = true;
    app.pipeline_run_node_condition.text = "0".into();
    app.start_pipeline_debug_run();
    let (stage, write) = wait_for_pipeline_pause(&mut app);
    assert_eq!(stage, "finish");
    assert_eq!(
        write.binding,
        engine::TraceTargetFieldBinding::StaticBinding { value: 0 }
    );
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn pipeline_step_overrides_value_node_and_stage_conditions() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-value-node-step")?;
    let directory = pipeline_path.parent().unwrap();
    app.pipeline_run_breakpoint =
        pipeline_ui::breakpoint_candidates(&app.pipeline_run_draft.as_ref().unwrap().pipeline)
            .into_iter()
            .find(|candidate| candidate.stage == "prepare" && candidate.field.field == "Value");
    assert!(app.pipeline_run_breakpoint.is_some());
    app.pipeline_run_node_condition.enabled = true;
    app.pipeline_run_node_condition.text = "0".into();
    app.start_pipeline_debug_run();
    assert_eq!(wait_for_pipeline_pause(&mut app).0, "prepare");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Step);
    assert_eq!(wait_for_pipeline_pause(&mut app).0, "finish");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn expression_breakpoint_pauses_preview_at_exact_typed_node_and_steps_to_next_node() {
    let mut app = two_field_debug_preview_app();
    app.preview_expression_condition.enabled = true;
    app.preview_expression_condition.node_text = "0".into();
    app.preview_expression_condition.value.enabled = true;
    app.preview_expression_condition.value.text = "A".into();
    app.execute_debug_preview();
    let first = wait_for_preview_node_pause(&mut app);
    assert_eq!(first.node, 0);
    assert_eq!(first.value.value_type, "string");
    assert_eq!(first.value.preview, "A");
    app.preview_command(preview_ui::PreviewCommand::Step);
    let second = wait_for_preview_node_pause(&mut app);
    assert_eq!(second.node, 1);
    assert_eq!(second.value.preview, "B");
    app.preview_command(preview_ui::PreviewCommand::Cancel);
    wait_for_preview_completion(&mut app);
    assert_eq!(app.status, "preview cancelled");
    assert!(app.run_report.is_none());
}

#[test]
fn expression_breakpoint_ignores_equal_value_from_distinct_preview_node() {
    let mut app = two_field_debug_preview_app();
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::String("A".into()),
        },
    );
    app.preview_expression_condition.enabled = true;
    app.preview_expression_condition.node_text = "0".into();
    app.execute_debug_preview();
    assert_eq!(wait_for_preview_node_pause(&mut app).node, 0);
    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    assert!(app.show_run_report, "{}", app.status);
}

#[test]
fn expression_breakpoint_pauses_for_filter_without_a_target_write() {
    let mut app = repeated_row_debug_preview_app(1);
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::Bool(false),
        },
    );
    app.project.root.children[0].filter = Some(1);
    app.preview_expression_condition.enabled = true;
    app.preview_expression_condition.node_text = "1".into();
    app.preview_expression_condition.value.enabled = true;
    app.preview_expression_condition.value.value_type = crate::preview::ScalarValueType::Bool;
    app.preview_expression_condition.value.text = "false".into();
    app.execute_debug_preview();
    let value = wait_for_preview_node_pause(&mut app);
    assert_eq!(value.node, 1);
    assert_eq!(value.value.preview, "false");
    assert_eq!(
        value.positions.last().map(|position| position.index),
        Some(1)
    );
    app.preview_command(preview_ui::PreviewCommand::Cancel);
    wait_for_preview_completion(&mut app);
    assert_eq!(app.status, "preview cancelled");
}

#[test]
fn expression_breakpoint_cancels_saved_run_before_publication() {
    let (mut app, project_path, output) = two_field_file_run_app("expression-file-cancel");
    app.file_run_expression_condition.enabled = true;
    app.file_run_expression_condition.node_text = "0".into();
    app.debug_run(&egui::Context::default());
    let value = wait_for_file_node_pause(&mut app);
    assert_eq!(value.node, 0);
    assert_eq!(value.value.preview, "A");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    app.file_run_command(run_ui::FileRunCommand::Cancel);
    wait_for_file_run_completion(&mut app);
    assert_eq!(app.status, "run cancelled");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    std::fs::remove_dir_all(project_path.parent().unwrap()).unwrap();
}

#[test]
fn expression_breakpoint_selects_pipeline_stage_and_cancels_all_publication() -> anyhow::Result<()>
{
    let (mut app, pipeline_path) = two_stage_pipeline_app("expression-pipeline-stage")?;
    let directory = pipeline_path.parent().unwrap();
    app.pipeline_run_expression_condition.enabled = true;
    app.pipeline_run_expression_condition.node_text = "0".into();
    app.pipeline_run_expression_stage = Some("finish".into());
    app.start_pipeline_debug_run();
    let (stage, value) = wait_for_pipeline_node_pause(&mut app);
    assert_eq!(stage, "finish");
    assert_eq!(value.node, 0);
    assert_eq!(value.value.preview, "B");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline cancelled");
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn expression_step_crosses_pipeline_stage_even_when_breakpoint_selects_one_stage()
-> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("expression-pipeline-step")?;
    let directory = pipeline_path.parent().unwrap();
    app.pipeline_run_expression_condition.enabled = true;
    app.pipeline_run_expression_condition.node_text = "0".into();
    app.pipeline_run_expression_stage = Some("prepare".into());
    app.start_pipeline_debug_run();
    assert_eq!(wait_for_pipeline_node_pause(&mut app).0, "prepare");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Step);
    let (stage, value) = wait_for_pipeline_node_pause(&mut app);
    assert_eq!(stage, "finish");
    assert_eq!(value.value.preview, "B");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn function_expression_breakpoint_pauses_preview_without_main_node_id_collision() {
    let mut app = two_field_debug_preview_app();
    attach_no_arg_function(&mut app.project, "A");
    app.project.root.bindings[1].node = 2;
    app.preview_expression_condition.enabled = true;
    app.preview_expression_condition.node_text = "0".into();
    app.preview_expression_condition.function_text = "7".into();
    app.preview_expression_condition.value.enabled = true;
    app.preview_expression_condition.value.text = "A".into();
    app.execute_debug_preview();
    let paused = wait_for_preview_function_node_pause(&mut app);
    assert_eq!(paused.function, FunctionId::new(7));
    assert_eq!(paused.node, 0);
    assert_eq!(paused.value.value_type, "string");
    assert_eq!(paused.value.preview, "A");
    assert!(paused.source.frames.is_empty());
    app.preview_command(preview_ui::PreviewCommand::Step);
    assert_eq!(wait_for_preview_node_pause(&mut app).node, 2);
    app.preview_command(preview_ui::PreviewCommand::Cancel);
    wait_for_preview_completion(&mut app);
    assert_eq!(app.status, "preview cancelled");
    assert!(app.run_report.is_none());
}

#[test]
fn function_expression_breakpoint_cancels_saved_run_before_publication() {
    let (mut app, project_path, output) = two_field_file_run_app("function-expression-file");
    attach_no_arg_function(&mut app.project, "A");
    app.project.root.bindings[1].node = 2;
    app.save_document_to(&project_path).unwrap();
    app.file_run_expression_condition.enabled = true;
    app.file_run_expression_condition.node_text = "0".into();
    app.file_run_expression_condition.function_text = "7".into();
    app.debug_run(&egui::Context::default());
    let paused = wait_for_file_function_node_pause(&mut app);
    assert_eq!(paused.function, FunctionId::new(7));
    assert_eq!(paused.value.preview, "A");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    app.file_run_command(run_ui::FileRunCommand::Cancel);
    wait_for_file_run_completion(&mut app);
    assert_eq!(app.status, "run cancelled");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    std::fs::remove_dir_all(project_path.parent().unwrap()).unwrap();
}

#[test]
fn function_expression_breakpoint_selects_pipeline_stage_and_steps_before_publication()
-> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_function_pipeline_app("function-expression-pipeline")?;
    let directory = pipeline_path.parent().unwrap();
    app.pipeline_run_expression_condition.enabled = true;
    app.pipeline_run_expression_condition.node_text = "0".into();
    app.pipeline_run_expression_condition.function_text = "7".into();
    app.pipeline_run_expression_stage = Some("finish".into());
    app.start_pipeline_debug_run();
    let (stage, paused) = wait_for_pipeline_function_node_pause(&mut app);
    assert_eq!(stage, "finish");
    assert_eq!(paused.function, FunctionId::new(7));
    assert_eq!(paused.node, 0);
    assert_eq!(paused.value.preview, "B");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Step);
    let (stage, next) = wait_for_pipeline_node_pause(&mut app);
    assert_eq!(stage, "finish");
    assert_eq!(next.node, 2);
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline cancelled");
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn function_input_breakpoint_qualifies_preview_pin_and_steps_in_delivery_order() {
    let mut app = two_field_debug_preview_app();
    attach_input_function(&mut app.project, "A");
    app.project.graph.nodes.insert(
        2,
        Node::Call {
            function: "concat".into(),
            args: vec![0, 1],
        },
    );
    app.project.graph.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: FunctionId::new(7),
            args: Vec::new(),
        },
    );
    app.project.root.bindings[0].node = 2;
    app.project.root.bindings[1].node = 3;
    app.preview_input_condition.enabled = true;
    app.preview_input_condition.consumer_text = "2".into();
    app.preview_input_condition.function_text = "7".into();
    app.preview_input_condition.input_number_text = "1".into();
    app.preview_input_condition.value.enabled = true;
    app.preview_input_condition.value.text = "A".into();
    let draft = app.preview_draft.clone();
    app.execute_debug_preview();
    let first = wait_for_preview_function_input_pause(&mut app);
    assert_eq!(
        (first.function, first.consumer, first.input_index),
        (FunctionId::new(7), 2, 0)
    );
    assert_eq!(first.value.preview, "A");
    assert!(first.source.frames.is_empty());
    app.preview_command(preview_ui::PreviewCommand::Step);
    let next = wait_for_preview_function_input_pause(&mut app);
    assert_eq!(
        (next.input, next.input_index, next.value.preview.as_str()),
        (1, 1, "!")
    );
    app.preview_command(preview_ui::PreviewCommand::Cancel);
    wait_for_preview_completion(&mut app);
    assert_eq!(app.status, "preview cancelled");
    assert!(app.run_report.is_none());

    app.preview_draft = draft;
    app.execute_debug_preview();
    let resumed = wait_for_preview_function_input_pause(&mut app);
    assert_eq!(resumed.input_index, 0);
    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    assert!(app.run_report.is_some());
}

#[test]
fn function_input_breakpoint_cancels_saved_run_before_publication() {
    let (mut app, project_path, output) = two_field_file_run_app("function-input-file");
    attach_input_function(&mut app.project, "A");
    app.project.root.bindings[1].node = 2;
    app.save_document_to(&project_path).unwrap();
    app.file_run_input_condition.enabled = true;
    app.file_run_input_condition.consumer_text = "2".into();
    app.file_run_input_condition.function_text = "7".into();
    app.file_run_input_condition.input_number_text = "2".into();
    app.file_run_input_condition.value.enabled = true;
    app.file_run_input_condition.value.text = "!".into();
    app.debug_run(&egui::Context::default());
    let paused = wait_for_file_function_input_pause(&mut app);
    assert_eq!(
        (paused.function, paused.consumer, paused.input_index),
        (FunctionId::new(7), 2, 1)
    );
    assert_eq!(paused.value.preview, "!");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    app.file_run_command(run_ui::FileRunCommand::Cancel);
    wait_for_file_run_completion(&mut app);
    assert_eq!(app.status, "run cancelled");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    std::fs::remove_dir_all(project_path.parent().unwrap()).unwrap();
}

#[test]
fn function_input_breakpoint_filters_pipeline_stage_and_step_overrides_it() -> anyhow::Result<()> {
    let (mut app, pipeline_path) =
        two_stage_function_input_pipeline_app("function-input-pipeline")?;
    let directory = pipeline_path.parent().unwrap();
    app.pipeline_run_input_condition.enabled = true;
    app.pipeline_run_input_condition.consumer_text = "2".into();
    app.pipeline_run_input_condition.function_text = "7".into();
    app.pipeline_run_input_condition.input_number_text = "1".into();
    app.pipeline_run_input_stage = Some("finish".into());
    app.start_pipeline_debug_run();
    let (stage, first) = wait_for_pipeline_function_input_pause(&mut app);
    assert_eq!(stage, "finish");
    assert_eq!(
        (first.function, first.consumer, first.input_index),
        (FunctionId::new(7), 2, 0)
    );
    assert_eq!(first.value.preview, "B");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline cancelled");

    app.pipeline_run_input_condition.input_number_text = "2".into();
    app.pipeline_run_input_stage = Some("prepare".into());
    app.start_pipeline_debug_run();
    let (stage, first) = wait_for_pipeline_function_input_pause(&mut app);
    assert_eq!(
        (
            stage.as_str(),
            first.input_index,
            first.value.preview.as_str()
        ),
        ("prepare", 1, "!")
    );
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Step);
    let (stage, next) = wait_for_pipeline_function_input_pause(&mut app);
    assert_eq!(
        (
            stage.as_str(),
            next.input_index,
            next.value.preview.as_str()
        ),
        ("finish", 0, "B")
    );
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline cancelled");
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn input_pin_breakpoint_selects_exact_preview_consumer_and_steps_to_next_delivery() {
    let mut app = two_field_debug_preview_app();
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::String("A".into()),
        },
    );
    for consumer in [2, 3] {
        app.project.graph.nodes.insert(
            consumer,
            Node::Call {
                function: "concat".into(),
                args: vec![0, 1],
            },
        );
    }
    app.project.root.bindings[0].node = 2;
    app.project.root.bindings[1].node = 3;
    app.preview_input_condition.enabled = true;
    app.preview_input_condition.consumer_text = "2".into();
    app.preview_input_condition.input_number_text = "2".into();
    app.preview_input_condition.value.enabled = true;
    app.preview_input_condition.value.text = "A".into();
    app.execute_debug_preview();
    let first = wait_for_preview_input_pause(&mut app);
    assert_eq!((first.consumer, first.input, first.input_index), (2, 1, 1));
    assert_eq!(first.value.preview, "A");

    app.preview_command(preview_ui::PreviewCommand::Step);
    let next = wait_for_preview_input_pause(&mut app);
    assert_eq!((next.consumer, next.input, next.input_index), (3, 0, 0));
    assert_eq!(next.value.preview, "A");
    app.preview_command(preview_ui::PreviewCommand::Cancel);
    wait_for_preview_completion(&mut app);
    assert_eq!(app.status, "preview cancelled");
    assert!(app.run_report.is_none());
}

#[test]
fn input_pin_breakpoint_can_cancel_a_filtered_preview_without_a_target_write() {
    let mut app = repeated_row_debug_preview_app(1);
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::Bool(true),
        },
    );
    app.project.graph.nodes.insert(
        2,
        Node::Const {
            value: ir::Value::Bool(false),
        },
    );
    app.project.graph.nodes.insert(
        3,
        Node::If {
            condition: 1,
            then: 2,
            else_: 2,
        },
    );
    app.project.root.children[0].filter = Some(3);
    app.preview_input_condition.enabled = true;
    app.preview_input_condition.consumer_text = "3".into();
    app.preview_input_condition.input_number_text = "2".into();
    app.preview_input_condition.value.enabled = true;
    app.preview_input_condition.value.value_type = crate::preview::ScalarValueType::Bool;
    app.preview_input_condition.value.text = "false".into();
    app.execute_debug_preview();
    let input = wait_for_preview_input_pause(&mut app);
    assert_eq!((input.consumer, input.input, input.input_index), (3, 2, 1));
    assert_eq!(input.value.preview, "false");
    assert_eq!(
        input.positions.last().map(|position| position.index),
        Some(1)
    );
    app.preview_command(preview_ui::PreviewCommand::Cancel);
    wait_for_preview_completion(&mut app);
    assert_eq!(app.status, "preview cancelled");
    assert!(app.run_report.is_none());
}

#[test]
fn input_pin_breakpoint_steps_and_cancels_saved_run_before_publication() {
    let (mut app, project_path, output) = two_field_file_run_app("input-pin-file-step");
    app.project.graph.nodes.insert(
        2,
        Node::Call {
            function: "concat".into(),
            args: vec![0, 1],
        },
    );
    app.project.root.bindings[0].node = 2;
    app.save_document_to(&project_path)
        .expect("updated project saves");
    app.file_run_input_condition.enabled = true;
    app.file_run_input_condition.consumer_text = "2".into();
    app.file_run_input_condition.input_number_text = "1".into();
    app.debug_run(&egui::Context::default());
    let first = wait_for_file_input_pause(&mut app);
    assert_eq!((first.consumer, first.input, first.input_index), (2, 0, 0));
    app.file_run_command(run_ui::FileRunCommand::Step);
    let next = wait_for_file_input_pause(&mut app);
    assert_eq!((next.consumer, next.input, next.input_index), (2, 1, 1));
    app.file_run_command(run_ui::FileRunCommand::Cancel);
    wait_for_file_run_completion(&mut app);
    assert_eq!(app.status, "run cancelled");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old output");
    assert!(app.run_report.is_none());
    std::fs::remove_dir_all(project_path.parent().unwrap()).unwrap();
}

#[test]
fn input_pin_breakpoint_selects_pipeline_stage_and_steps_across_stages() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pin_pipeline_app("input-pin-pipeline-step")?;
    let directory = pipeline_path.parent().unwrap();
    app.pipeline_run_input_condition.enabled = true;
    app.pipeline_run_input_condition.consumer_text = "2".into();
    app.pipeline_run_input_condition.input_number_text = "2".into();
    app.pipeline_run_input_stage = Some("prepare".into());
    app.start_pipeline_debug_run();
    let (stage, first) = wait_for_pipeline_input_pause(&mut app);
    assert_eq!(stage, "prepare");
    assert_eq!((first.consumer, first.input, first.input_index), (2, 1, 1));
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Step);
    let (stage, next) = wait_for_pipeline_input_pause(&mut app);
    assert_eq!(stage, "finish");
    assert_eq!((next.consumer, next.input, next.input_index), (2, 0, 0));
    assert_eq!(next.value.preview, "B");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline cancelled");
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn input_pin_breakpoint_ignores_equal_pins_in_unselected_pipeline_stage() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pin_pipeline_app("input-pin-pipeline-stage")?;
    let directory = pipeline_path.parent().unwrap();
    app.pipeline_run_input_condition.enabled = true;
    app.pipeline_run_input_condition.consumer_text = "2".into();
    app.pipeline_run_input_condition.input_number_text = "2".into();
    app.pipeline_run_input_stage = Some("finish".into());
    app.start_pipeline_debug_run();
    let (stage, first) = wait_for_pipeline_input_pause(&mut app);
    assert_eq!(stage, "finish");
    assert_eq!((first.consumer, first.input, first.input_index), (2, 1, 1));
    assert_eq!(first.value.preview, "!");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn cancelling_ordinary_pipeline_suppresses_late_publication() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-cancel")?;
    let directory = pipeline_path.parent().unwrap();
    app.start_pipeline_run();
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline cancelled");
    assert!(app.run_report.is_none());
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn pipeline_runner_requires_reinspection_after_file_changes() -> anyhow::Result<()> {
    let pipeline_path = temporary_project_path("pipeline-changed");
    pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().expect("pipeline has a directory");
    let mut app = FerruleApp::default();
    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    draft.inputs[0].path = "orders.json".into();
    draft.outputs[0].path = "result.json".into();
    std::fs::write(&pipeline_path, "{}")?;
    app.start_pipeline_run();
    assert!(app.pending_pipeline_run.is_none());
    assert!(!directory.join("result.json").exists());
    assert!(app.status.contains("blocked"));
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn pipeline_editor_embeds_project_rewires_rename_and_saves_independently() -> anyhow::Result<()> {
    let pipeline_path = temporary_project_path("pipeline-editor");
    pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    let embedded_directory = directory.join("embedded");
    std::fs::create_dir_all(&embedded_directory)?;
    let project_path = embedded_directory.join("second.json");
    let pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&pipeline_path)?)?;
    let mut project = pipeline.stages[0].project.clone();
    project.source_path = Some("input.json".into());
    project.extra_sources.push(mapping::NamedSource {
        name: "lookup".into(),
        path: "lookup.json".into(),
        schema: project.source.clone(),
        options: project.source_options.clone(),
        dynamic_path: None,
    });
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;

    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        9,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    let original_project = serde_json::to_vec(&app.project)?;
    app.request_pipeline_editor_action(pipeline_editor_ui::PipelineEditorAction::Open(
        pipeline_path.clone(),
    ));
    app.add_pipeline_stage_from_path(&project_path);
    let editor = app.pipeline_editor.as_mut().expect("editor opens");
    assert_eq!(editor.document.pipeline.stages.len(), 2);
    assert_eq!(
        editor.document.pipeline.stages[1].mapping_path.as_deref(),
        Some("embedded/second.json")
    );
    assert_eq!(
        editor.document.pipeline.stages[1]
            .project
            .source_path
            .as_deref(),
        Some("embedded/input.json")
    );
    editor.document.set_input(
        1,
        0,
        mapping::PipelineInput::StageTarget {
            stage: "prepare".into(),
            target: None,
        },
    )?;
    editor.document.set_input(
        1,
        1,
        mapping::PipelineInput::StageTarget {
            stage: "prepare".into(),
            target: None,
        },
    )?;
    editor.document.rename_stage(0, "source")?;
    assert_eq!(
        editor.document.pipeline.stages[1].source,
        mapping::PipelineInput::StageTarget {
            stage: "source".into(),
            target: None,
        }
    );
    assert_eq!(
        editor.document.pipeline.stages[1].extra_sources[0].from,
        mapping::PipelineInput::StageTarget {
            stage: "source".into(),
            target: None,
        }
    );
    assert!(editor.document.remove_stage(0).is_err());
    assert!(editor.document.issues().is_empty());
    editor.document.save()?;
    assert!(!editor.document.is_dirty());
    let saved: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&pipeline_path)?)?;
    assert_eq!(saved.stages[0].id, "source");
    assert_eq!(
        saved.stages[1].source,
        mapping::PipelineInput::StageTarget {
            stage: "source".into(),
            target: None
        }
    );
    assert_eq!(serde_json::to_vec(&app.project)?, original_project);
    assert!(app.is_dirty());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn pipeline_editor_repairs_missing_static_named_source_bindings() -> anyhow::Result<()> {
    let pipeline_path = temporary_project_path("pipeline-editor-missing-binding");
    pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    let mut document = crate::pipeline_edit::PipelineEditorDocument::load(&pipeline_path)?;
    let stage = &mut document.pipeline.stages[0];
    stage.source = mapping::PipelineInput::Host {
        name: "prepare-lookup".into(),
    };
    stage.project.extra_sources.push(mapping::NamedSource {
        name: "lookup".into(),
        path: "lookup.json".into(),
        schema: stage.project.source.clone(),
        options: stage.project.source_options.clone(),
        dynamic_path: None,
    });
    assert!(
        document
            .issues()
            .iter()
            .any(|issue| issue.contains("static named source `lookup` has no pipeline binding"))
    );

    assert_eq!(document.add_missing_static_bindings(0)?, 1);
    assert_eq!(document.add_missing_static_bindings(0)?, 0);
    assert_eq!(
        document.pipeline.stages[0].extra_sources,
        vec![mapping::PipelineNamedInput {
            name: "lookup".into(),
            from: mapping::PipelineInput::Host {
                name: "prepare-lookup-2".into(),
            },
        }]
    );
    assert!(document.issues().is_empty());
    document.save()?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn pipeline_editor_detects_external_change_and_guards_dirty_close() -> anyhow::Result<()> {
    let pipeline_path = temporary_project_path("pipeline-editor-conflict");
    pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    let mut app = FerruleApp::default();
    app.request_pipeline_editor_action(pipeline_editor_ui::PipelineEditorAction::Open(
        pipeline_path.clone(),
    ));
    app.pipeline_editor
        .as_mut()
        .expect("editor opens")
        .document
        .rename_stage(0, "renamed")?;
    app.request_pipeline_editor_action(pipeline_editor_ui::PipelineEditorAction::Close);
    assert!(app.pipeline_editor.is_some());
    assert!(app.pending_pipeline_editor_action.is_some());
    app.pending_pipeline_editor_action = None;
    std::fs::write(&pipeline_path, b"{\"stages\":[]}")?;
    let editor = app
        .pipeline_editor
        .as_mut()
        .expect("dirty editor remains open");
    assert!(editor.document.save().is_err());
    assert!(editor.document.is_dirty());
    assert_eq!(std::fs::read(&pipeline_path)?, b"{\"stages\":[]}");
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn app_close_checks_dirty_pipeline_before_dirty_project() -> anyhow::Result<()> {
    let pipeline_path = temporary_project_path("pipeline-editor-app-close");
    pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    let context = egui::Context::default();

    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        9,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    assert!(app.is_dirty());
    app.request_pipeline_editor_action(pipeline_editor_ui::PipelineEditorAction::Open(
        pipeline_path.clone(),
    ));
    app.pipeline_editor
        .as_mut()
        .expect("editor opens")
        .document
        .rename_stage(0, "renamed")?;
    app.guard_app_close_requested(&context, true);
    assert!(matches!(
        &app.pending_pipeline_editor_action,
        Some(pipeline_editor_ui::PipelineEditorAction::CloseApp)
    ));
    assert!(app.pending_destructive_action.is_none());
    assert!(!app.allow_close);
    app.discard_pending_pipeline_editor_action(&context);
    assert!(app.pipeline_editor.is_none());
    assert_eq!(
        app.pending_destructive_action,
        Some(DestructiveAction::Close)
    );
    assert!(!app.allow_close);

    let mut clean_project_app = FerruleApp::default();
    clean_project_app.request_pipeline_editor_action(
        pipeline_editor_ui::PipelineEditorAction::Open(pipeline_path.clone()),
    );
    clean_project_app
        .pipeline_editor
        .as_mut()
        .expect("editor opens")
        .document
        .rename_stage(0, "another")?;
    clean_project_app.guard_app_close_requested(&context, true);
    clean_project_app.discard_pending_pipeline_editor_action(&context);
    assert!(clean_project_app.pipeline_editor.is_none());
    assert!(clean_project_app.allow_close);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn pipeline_editor_validates_before_creating_new_file() -> anyhow::Result<()> {
    let existing = temporary_project_path("pipeline-editor-new");
    let directory = existing.parent().expect("pipeline has directory");
    let path = directory.join("flow.json");
    let mut document = crate::pipeline_edit::PipelineEditorDocument::create(&path)?;
    assert!(document.is_dirty());
    assert!(document.save().is_err());
    assert!(!path.exists());
    pipeline_fixture(&existing)?;
    let project: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&existing)?)?;
    let project_path = directory.join("mapping.json");
    std::fs::write(
        &project_path,
        serde_json::to_vec_pretty(&project.stages[0].project)?,
    )?;
    document.add_project(&project_path)?;
    assert!(document.issues().is_empty());
    document.save()?;
    assert!(path.exists());
    assert!(!document.is_dirty());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn pipeline_editor_stores_sibling_project_identity_as_relative_path() -> anyhow::Result<()> {
    let fixture_path = temporary_project_path("pipeline-editor-sibling");
    pipeline_fixture(&fixture_path)?;
    let directory = fixture_path.parent().expect("fixture has directory");
    let project_dir = directory.join("projects");
    let pipeline_dir = directory.join("pipelines");
    std::fs::create_dir_all(&project_dir)?;
    std::fs::create_dir_all(&pipeline_dir)?;
    let source: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&fixture_path)?)?;
    let project_path = project_dir.join("stage.json");
    let mut project = source.stages[0].project.clone();
    project.source_path = Some("orders.json".into());
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    let mut document =
        crate::pipeline_edit::PipelineEditorDocument::create(&pipeline_dir.join("flow.json"))?;
    document.add_project(&project_path)?;
    assert_eq!(
        document.pipeline.stages[0].mapping_path.as_deref(),
        Some("../projects/stage.json")
    );
    assert_eq!(
        document.pipeline.stages[0].project.source_path.as_deref(),
        Some("../projects/orders.json")
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn pipeline_editor_rejects_symlinked_file_and_link_replacement() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let pipeline_path = temporary_project_path("pipeline-editor-symlink");
    pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    let link = directory.join("linked.json");
    symlink(&pipeline_path, &link)?;
    assert!(crate::pipeline_edit::PipelineEditorDocument::load(&link).is_err());
    let mut document = crate::pipeline_edit::PipelineEditorDocument::load(&pipeline_path)?;
    document.rename_stage(0, "renamed")?;
    let original = std::fs::read(&pipeline_path)?;
    let target = directory.join("replacement.json");
    std::fs::write(&target, &original)?;
    std::fs::remove_file(&pipeline_path)?;
    symlink(&target, &pipeline_path)?;
    assert!(document.save().is_err());
    assert_eq!(std::fs::read(&target)?, original);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn pipeline_editor_keeps_selected_project_symlink_identity() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let fixture_path = temporary_project_path("pipeline-editor-project-link");
    pipeline_fixture(&fixture_path)?;
    let directory = fixture_path.parent().expect("fixture has directory");
    let project_dir = directory.join("projects");
    let pipeline_dir = directory.join("pipelines");
    std::fs::create_dir_all(&project_dir)?;
    std::fs::create_dir_all(&pipeline_dir)?;
    let source: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&fixture_path)?)?;
    let project_path = project_dir.join("stage.json");
    let mut project = source.stages[0].project.clone();
    project.source_path = Some("orders.json".into());
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    let selected_path = pipeline_dir.join("selected-stage.json");
    symlink(&project_path, &selected_path)?;
    let mut document =
        crate::pipeline_edit::PipelineEditorDocument::create(&pipeline_dir.join("flow.json"))?;
    document.add_project(&selected_path)?;
    assert_eq!(
        document.pipeline.stages[0].mapping_path.as_deref(),
        Some("selected-stage.json")
    );
    assert_eq!(
        document.pipeline.stages[0].project.source_path.as_deref(),
        Some("orders.json")
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn preview_uses_the_active_named_target_only() -> anyhow::Result<()> {
    let mut app = FerruleApp::default();
    app.project.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: Some("audit.json".into()),
        schema: SchemaNode::group("audit", Vec::new()),
        options: FormatOptions::default(),
        root: Scope::default(),
    });
    app.mapping_workspace.active = MappingDocument::Target(0);
    app.begin_preview();
    let Some(draft) = app.preview_draft.as_mut() else {
        anyhow::bail!("named-target preview setup did not open");
    };
    draft.input_identity = "input.xml".into();
    draft.input_text = "<root/>".into();

    app.execute_preview();
    wait_for_preview_completion(&mut app);

    let Some(report) = app.run_report.as_ref() else {
        anyhow::bail!("successful preview has no report");
    };
    assert_eq!(report.report.outputs.len(), 1);
    assert_eq!(report.report.outputs[0].name, "audit");
    assert_eq!(report.report.outputs[0].path, PathBuf::from("audit.json"));
    Ok(())
}

#[test]
fn successful_save_resumes_a_destructive_continuation() {
    let project_path = temporary_project_path("save-continuation");
    let mut app = FerruleApp::default();
    app.save_document_to(&project_path)
        .expect("baseline project is saved");
    app.project.graph.nodes.insert(
        9,
        Node::Const {
            value: ir::Value::Null,
        },
    );

    app.save_with_continuation(
        Some(SaveContinuation::Destructive(DestructiveAction::NewProject)),
        &egui::Context::default(),
    );

    assert!(app.project.graph.nodes.is_empty());
    assert!(app.document.saved_path().is_none());
    let saved: Project = serde_json::from_str(
        &std::fs::read_to_string(&project_path).expect("saved project is readable"),
    )
    .expect("saved project parses");
    assert!(saved.graph.nodes.contains_key(&9));
    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn history_coalesces_keyboard_edits_and_roundtrips_undo_redo() {
    let mut app = FerruleApp::default();
    let start = std::time::Instant::now();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("a".into()),
        },
    );
    app.observe_editor_history(start, true);
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("ab".into()),
        },
    );
    app.observe_editor_history(start + std::time::Duration::from_millis(100), true);

    assert_eq!(app.history.undo_len(), 0);
    assert!(app.pending_history.is_some());
    app.observe_editor_history(
        start + HISTORY_COALESCE_DELAY + std::time::Duration::from_millis(100),
        true,
    );
    assert_eq!(app.history.undo_len(), 1);

    app.undo_project();
    assert!(app.project.graph.nodes.is_empty());
    app.redo_project();
    assert!(matches!(
        app.project.graph.nodes.get(&0),
        Some(Node::Const {
            value: ir::Value::String(value)
        }) if value == "ab"
    ));
}

#[test]
fn pointer_edits_are_distinct_history_steps() {
    let mut app = FerruleApp::default();
    let start = std::time::Instant::now();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.observe_editor_history(start, false);
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.observe_editor_history(start, false);
    assert_eq!(app.history.undo_len(), 2);

    app.undo_project();
    assert!(app.project.graph.nodes.contains_key(&0));
    assert!(!app.project.graph.nodes.contains_key(&1));
    app.undo_project();
    assert!(app.project.graph.nodes.is_empty());
}

#[test]
fn keyboard_edits_after_the_quiet_period_start_a_new_history_step() {
    let mut app = FerruleApp::default();
    let start = std::time::Instant::now();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("first".into()),
        },
    );
    app.observe_editor_history(start, true);

    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("second".into()),
        },
    );
    app.observe_editor_history(start + HISTORY_COALESCE_DELAY, true);
    assert_eq!(app.history.undo_len(), 1);

    app.undo_project();
    assert!(matches!(
        app.project.graph.nodes.get(&0),
        Some(Node::Const {
            value: ir::Value::String(value)
        }) if value == "first"
    ));
    app.undo_project();
    assert!(app.project.graph.nodes.is_empty());
}

#[test]
fn undo_and_redo_update_dirty_state_against_saved_baseline() {
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    assert!(app.is_dirty());

    app.undo_project();
    assert!(!app.is_dirty());
    app.redo_project();
    assert!(app.is_dirty());

    app.rebase_history();
    assert!(!app.can_undo());
    assert!(!app.history.can_redo());
}

/// Loading the orders-style project must recreate the whole picture:
/// hidden SourceFields become wires from the Source endpoint, function
/// inputs become node-to-node wires, and bindings become wires into
/// the Target endpoint's leaf pins.
#[test]
fn build_snarl_recreates_endpoint_and_binding_wires() {
    let mut graph = Graph::default();
    // 0: hidden SourceField (matches leaf "name"), 1: upper(0)
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["name".into()],
            frame: None,
        },
    );
    graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![0],
        },
    );
    let project = Project {
        source: SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
            ],
        ),
        target: SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("loud_name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            iteration: mapping::ScopeIteration::Source(vec![]),
            bindings: vec![
                Binding {
                    target_field: "loud_name".into(),
                    node: 1,
                },
                // Bound straight from the hidden SourceField? Use a
                // second field to prove Source->Target wires too.
                Binding {
                    target_field: "age".into(),
                    node: 2,
                },
            ],
            ..Scope::default()
        },
    };
    // 2: hidden SourceField for "age", bound directly to the target.
    let mut project = project;
    project.graph.nodes.insert(
        2,
        Node::SourceField {
            path: vec!["age".into()],
            frame: None,
        },
    );

    let snarl = build_snarl(&project);

    // Only Source, Target, and the Call node should be on the canvas.
    let kinds: Vec<CanvasNode> = snarl.nodes().copied().collect();
    assert_eq!(kinds.len(), 3);
    assert!(kinds.contains(&CanvasNode::SourceBlock(0)));
    assert!(kinds.contains(&CanvasNode::TargetBlock(0)));
    assert!(kinds.contains(&CanvasNode::Graph(1)));

    // Wires: Source(name)->Call arg0, Call->Target(loud_name),
    // Source(age)->Target(age).
    let mut wires: Vec<(CanvasNode, usize, CanvasNode, usize)> = snarl
        .wires()
        .map(|(o, i)| (snarl[o.node], o.output, snarl[i.node], i.input))
        .collect();
    // Wire iteration order is not deterministic; compare as a set.
    wires.sort_by_key(|w| format!("{w:?}"));
    let mut expected = vec![
        (CanvasNode::SourceBlock(0), 0, CanvasNode::Graph(1), 0),
        (CanvasNode::Graph(1), 0, CanvasNode::TargetBlock(0), 0),
        (CanvasNode::SourceBlock(0), 1, CanvasNode::TargetBlock(0), 1),
    ];
    expected.sort_by_key(|w| format!("{w:?}"));
    assert_eq!(wires, expected);
}

#[test]
fn build_snarl_matches_hidden_source_fields_by_frame_and_path() {
    let source = SchemaNode::group(
        "root",
        vec![
            SchemaNode::group("A", vec![SchemaNode::scalar("Id", ScalarType::String)]).repeating(),
            SchemaNode::group("B", vec![SchemaNode::scalar("Id", ScalarType::String)]).repeating(),
        ],
    );
    let target = SchemaNode::group(
        "root",
        vec![
            SchemaNode::scalar("AId", ScalarType::String),
            SchemaNode::scalar("BId", ScalarType::String),
        ],
    );
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            frame: Some(vec!["A".into()]),
            path: vec!["Id".into()],
        },
    );
    graph.nodes.insert(
        1,
        Node::SourceField {
            frame: Some(vec!["B".into()]),
            path: vec!["Id".into()],
        },
    );
    let project = Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "AId".into(),
                    node: 0,
                },
                Binding {
                    target_field: "BId".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        },
    };

    let snarl = build_snarl(&project);
    assert_eq!(snarl.nodes().count(), 2, "both source fields stay hidden");
    let mut wires: Vec<_> = snarl
        .wires()
        .map(|(output, input)| (snarl[output.node], output.output, input.input))
        .collect();
    wires.sort_by_key(|wire| format!("{wire:?}"));
    assert_eq!(
        wires,
        vec![
            (CanvasNode::SourceBlock(0), 0, 0),
            (CanvasNode::SourceBlock(0), 1, 1),
        ]
    );
}

#[test]
fn build_snarl_only_hides_legacy_frameless_fields_with_unique_suffixes() {
    let project = |source| {
        let target = SchemaNode::group("root", vec![SchemaNode::scalar("out", ScalarType::String)]);
        let mut graph = Graph::default();
        graph.nodes.insert(
            0,
            Node::SourceField {
                frame: None,
                path: vec!["Id".into()],
            },
        );
        Project {
            source,
            target,
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph,
            root: Scope {
                bindings: vec![Binding {
                    target_field: "out".into(),
                    node: 0,
                }],
                ..Scope::default()
            },
        }
    };
    let group = |name| {
        SchemaNode::group(name, vec![SchemaNode::scalar("Id", ScalarType::String)]).repeating()
    };

    let unique = build_snarl(&project(SchemaNode::group("root", vec![group("A")])));
    assert_eq!(unique.nodes().count(), 2);

    let ambiguous = build_snarl(&project(SchemaNode::group(
        "root",
        vec![group("A"), group("B")],
    )));
    assert!(ambiguous.nodes().any(|node| *node == CanvasNode::Graph(0)));
}

#[test]
fn new_mapping_stages_both_schemas_before_replacing_the_project() {
    let project_path = temporary_project_path("new-mapping");
    let directory = project_path.parent().expect("project has parent");
    let source_path = directory.join("source.xsd");
    let target_path = directory.join("target.schema.json");
    std::fs::write(
        &source_path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="SourceRoot">
    <xs:complexType><xs:sequence>
      <xs:element name="Name" type="xs:string"/>
    </xs:sequence></xs:complexType>
  </xs:element>
</xs:schema>"#,
    )
    .expect("source schema is written");
    std::fs::write(
        &target_path,
        r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "TargetRoot",
  "type": "object",
  "properties": { "Label": { "type": "string" } }
}"#,
    )
    .expect("target schema is written");

    let mut app = FerruleApp::default();
    app.begin_new_mapping();
    app.stage_mapping_schema(SchemaSide::Source, source_path);
    assert_eq!(app.project.source.name, "root");
    app.stage_mapping_schema(SchemaSide::Target, target_path);
    assert_eq!(app.project.target.name, "root");

    app.finish_new_mapping();

    assert_eq!(app.project.source.name, "SourceRoot");
    assert_eq!(app.project.target.name, "TargetRoot");
    assert!(app.new_mapping_setup.is_none());
    assert!(app.is_dirty());
    assert_eq!(app.main_canvas.snarl.nodes().count(), 2);

    std::fs::remove_dir_all(directory).expect("temporary test directory is removed");
}
