use super::*;
use crate::appearance::{SemanticThemeColors, WireColorMode};
use crate::canvas_endpoints::EndpointScrollState;
use crate::path_picker::SourcePathCatalog;
use egui_snarl::ui::SnarlViewer;
use egui_snarl::{InPinId, OutPinId, Snarl};
use ir::Instance;
use mapping::{Binding, Scope};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-function-output-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn function_draft(name: &str) -> NewFunctionDraft {
    NewFunctionDraft {
        library: "text".to_owned(),
        name: name.to_owned(),
        description: String::new(),
        parameters: vec![ParameterDraft {
            name: "input".to_owned(),
            ty: ScalarType::String,
        }],
        output_name: "result".to_owned(),
        output_type: ScalarType::String,
        error: None,
    }
}

fn function_app(name: &str) -> (FerruleApp, FunctionId) {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::scalar("Name", ScalarType::String)],
    );
    app.project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("result", ScalarType::String)],
    );
    let function = app
        .create_function(&function_draft(name))
        .expect("new function");
    app.project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Name".to_owned()],
            frame: None,
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::UserFunctionCall {
            function,
            args: vec![0],
        },
    );
    app.project.root = Scope {
        bindings: vec![Binding {
            target_field: "result".to_owned(),
            node: 1,
        }],
        ..Scope::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    (app, function)
}

fn with_function_viewer<R>(
    app: &mut FerruleApp,
    id: FunctionId,
    edit: impl FnOnce(&mut GraphViewer<'_>, &mut Snarl<CanvasNode>) -> R,
) -> R {
    assert!(app.ensure_function_canvas(id));
    let names = app.function_names();
    let inputs = app.function_inputs();
    let definition = app
        .project
        .user_functions
        .get_mut(&id)
        .expect("function definition");
    let canvas = app
        .mapping_workspace
        .function_canvases
        .get_mut(&id)
        .expect("function canvas");
    let output = definition.output;
    let parameters = definition
        .parameters
        .iter()
        .map(|parameter| (parameter.id, parameter.name.clone()))
        .collect();
    let source = SchemaNode::group("function", Vec::new());
    let source_paths = SourcePathCatalog::new(&source, &[]);
    let mut root = Scope::default();
    let mut endpoint_scroll = EndpointScrollState::default();
    let mut viewer = GraphViewer {
        graph: &mut definition.body,
        root_scope: &mut root,
        extra_targets: &[],
        inactive_target_scopes: &[],
        project_references: Default::default(),
        source_blocks: &[],
        target_blocks: &[],
        source_x12: false,
        target_x12: false,
        source_paths: &source_paths,
        function_names: names,
        function_inputs: inputs,
        parameter_names: parameters,
        protected_output: Some(output),
        function_output: Some(&mut definition.output),
        requested_function_open: None,
        colors: SemanticThemeColors::default(),
        wire_color_mode: WireColorMode::Theme,
        endpoint_scroll: &mut endpoint_scroll,
        value_map_wheel: None,
        endpoint_search_match: None,
        node_sizes: None,
        hovered_node: None,
        hovered_node_this_frame: None,
        camera_pan: egui::Vec2::ZERO,
        camera_focus: None,
        canvas_transform: None,
        pin_interaction_ids: Vec::new(),
        error: None,
    };
    edit(&mut viewer, &mut canvas.snarl)
}

fn canvas_node(snarl: &Snarl<CanvasNode>, id: NodeId) -> egui_snarl::NodeId {
    snarl
        .node_ids()
        .find_map(|(canvas, node)| (*node == CanvasNode::Graph(id)).then_some(canvas))
        .expect("body node on function canvas")
}

fn input(value: &str) -> Instance {
    Instance::Group(
        (vec![(
            "Name".to_owned(),
            Instance::Scalar(Value::String(value.to_owned())),
        )])
        .into(),
    )
}

fn assert_result(project: &Project, input_value: &str, expected: &str) {
    let issues = engine::validate(project);
    assert!(issues.is_empty(), "valid function mapping: {issues:#?}");
    let result = engine::run(project, &input(input_value)).expect("function result");
    assert_eq!(
        result.field("result").and_then(Instance::as_scalar),
        Some(&Value::String(expected.to_owned()))
    );
}

#[test]
fn selected_computed_function_output_executes_and_survives_history_save_and_reopen()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let source_path = directory.0.join("source.json");
    let output_path = directory.0.join("output.json");
    let project_path = directory.0.join("mapping.json");
    std::fs::write(&source_path, br#"{"Name":"Ada"}"#)?;
    let (mut app, id) = function_app("uppercase");
    app.project.source_path = Some(source_path.to_str().unwrap().to_owned());
    app.project.target_path = Some(output_path.to_str().unwrap().to_owned());
    let definition = app.project.user_functions.get_mut(&id).unwrap();
    let initial_output = definition.output;
    definition.body.nodes.insert(
        2,
        Node::Call {
            function: "upper".to_owned(),
            args: vec![3],
        },
    );
    definition.body.nodes.insert(3, Node::Unconnected);
    assert!(app.ensure_function_canvas(id));
    app.open_function_tab(id);
    app.mark_clean();
    app.rebase_history();
    let main_graph = serde_json::to_string(&app.project.graph)?;

    with_function_viewer(&mut app, id, |viewer, snarl| {
        let parameter = canvas_node(snarl, 0);
        let upper = canvas_node(snarl, 2);
        let previous = canvas_node(snarl, initial_output);
        let from = snarl.out_pin(OutPinId {
            node: parameter,
            output: 0,
        });
        let to = snarl.in_pin(InPinId {
            node: upper,
            input: 0,
        });
        // These are the real wire-assignment and node-menu actions.
        viewer.connect(&from, &to, snarl);
        assert!(
            matches!(viewer.graph.nodes.get(&2), Some(Node::Call { args, .. }) if args == &[0])
        );
        assert!(!viewer.graph.nodes.contains_key(&3));
        assert!(viewer.select_function_output(2));
        assert_eq!(viewer.protected_output, Some(2));
        assert!(viewer.title(&CanvasNode::Graph(2)).ends_with("(output)"));
        assert_eq!(viewer.remove_snarl_nodes(&[previous], snarl), 1);
        assert!(!viewer.graph.nodes.contains_key(&initial_output));
        assert_eq!(viewer.remove_snarl_nodes(&[upper], snarl), 0);
        assert!(viewer.graph.nodes.contains_key(&2));
    });

    assert_eq!(app.project.user_functions[&id].output, 2);
    assert_eq!(
        serde_json::to_string(&app.project.graph)?,
        main_graph,
        "isolated body action must not edit the main graph"
    );
    assert_result(&app.project, "Ada", "ADA");
    assert!(app.is_dirty());
    app.observe_editor_history(std::time::Instant::now(), false);
    app.undo_project();
    assert_eq!(app.project.user_functions[&id].output, initial_output);
    assert!(
        app.project.user_functions[&id]
            .body
            .nodes
            .contains_key(&initial_output)
    );
    app.redo_project();
    assert_eq!(app.project.user_functions[&id].output, 2);
    assert!(
        !app.project.user_functions[&id]
            .body
            .nodes
            .contains_key(&initial_output)
    );
    assert_result(&app.project, "Ada", "ADA");
    app.save_document_to(&project_path)?;
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&project_path);
    assert_eq!(reopened.project.user_functions[&id].output, 2);
    assert!(!reopened.is_dirty());
    assert_result(&reopened.project, "Ada", "ADA");
    cli::run_project_with_paths(&project_path, None, None)?;
    let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&output_path)?)?;
    assert_eq!(written["result"], "ADA");
    std::fs::remove_file(&source_path)?;
    std::fs::write(&output_path, b"sentinel")?;
    let options = cli::PayloadRunOptions::new(cli::PayloadDocument::new(
        &source_path,
        br#"{"Name":"Grace"}"#,
    )?)
    .with_output_path(&output_path);
    let output = cli::run_project_payloads(&project_path, &options)?;
    assert_eq!(output.artifacts.len(), 1);
    let rendered: serde_json::Value = serde_json::from_slice(&output.artifacts[0].bytes)?;
    assert_eq!(rendered["result"], "GRACE");
    assert_eq!(std::fs::read(&output_path)?, b"sentinel");
    Ok(())
}

#[test]
fn selecting_a_parameter_as_function_output_creates_a_protected_identity_function() {
    let (mut app, id) = function_app("identity");
    let initial_output = app.project.user_functions[&id].output;
    with_function_viewer(&mut app, id, |viewer, snarl| {
        let parameter = canvas_node(snarl, 0);
        let previous = canvas_node(snarl, initial_output);
        assert!(viewer.select_function_output(0));
        assert_eq!(viewer.protected_output, Some(0));
        assert_eq!(viewer.remove_snarl_nodes(&[previous], snarl), 1);
        assert_eq!(viewer.remove_snarl_nodes(&[parameter], snarl), 0);
    });
    assert_eq!(app.project.user_functions[&id].output, 0);
    assert_result(&app.project, "unchanged 🚀", "unchanged 🚀");
}

#[test]
fn selected_conditional_output_preserves_lazy_and_typed_host_failures() {
    let (mut app, id) = function_app("conditional");
    let definition = app.project.user_functions.get_mut(&id).unwrap();
    definition.body.nodes.insert(
        2,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    definition.body.nodes.insert(
        3,
        Node::RuntimeParameter {
            name: "not_supplied".to_owned(),
            ty: ScalarType::String,
            preview: None,
        },
    );
    definition.body.nodes.insert(
        4,
        Node::If {
            condition: 2,
            then: 0,
            else_: 3,
        },
    );
    with_function_viewer(&mut app, id, |viewer, _| {
        assert!(viewer.select_function_output(4));
        assert_eq!(viewer.protected_output, Some(4));
    });
    assert_result(&app.project, "safe branch", "safe branch");
    app.project
        .user_functions
        .get_mut(&id)
        .unwrap()
        .body
        .nodes
        .insert(
            2,
            Node::Const {
                value: Value::Bool(false),
            },
        );
    let issues = engine::validate(&app.project);
    assert!(issues.is_empty(), "valid lazy host input: {issues:#?}");
    assert!(
        matches!(engine::run(&app.project, &input("safe branch")), Err(engine::EngineError::MissingRuntimeParameter { node: 3, name }) if name == "not_supplied")
    );
}

#[test]
fn output_selection_requires_a_function_slot_and_an_existing_body_node() {
    let (mut app, id) = function_app("guarded");
    let initial_output = app.project.user_functions[&id].output;
    with_function_viewer(&mut app, id, |viewer, _| {
        assert!(!viewer.select_function_output(999));
        assert_eq!(viewer.protected_output, Some(initial_output));
        assert!(
            viewer
                .error
                .as_deref()
                .is_some_and(|error| error.contains("no longer exists"))
        );
        // Main/named canvases supply no writable result slot. The action is
        // absent there, and attempting its mutation path is still a no-op.
        viewer.function_output = None;
        viewer.error = None;
        assert!(!viewer.select_function_output(0));
        assert_eq!(viewer.protected_output, Some(initial_output));
        assert!(viewer.error.is_none());
    });
    assert_eq!(app.project.user_functions[&id].output, initial_output);
}
