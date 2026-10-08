use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::Binding;

const FUNCTION: FunctionId = FunctionId::new(7);

struct Retained {
    path: PathBuf,
    next: std::cell::Cell<u64>,
    complete: bool,
}
impl Retained {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-default-presence-history-{}-{}",
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
    fn record(&self, label: &str, value: impl std::fmt::Debug) {
        let next = self.next.get();
        self.next.set(next + 1);
        std::fs::write(
            self.path.join(format!("{next:04}-{label}.txt")),
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
                "Retained default-presence GUI model originals: {}",
                self.path.display()
            );
        }
    }
}
fn body() -> Graph {
    Graph {
        nodes: [
            (
                0,
                Node::Const {
                    value: Value::String("miss".into()),
                },
            ),
            (
                1,
                Node::ValueMap {
                    input: 0,
                    input_type: None,
                    table: vec![
                        (Value::Null, Value::String("null-key".into())),
                        (Value::String("hit".into()), Value::String("matched".into())),
                    ],
                    default: None,
                },
            ),
        ]
        .into(),
    }
}
fn fixture() -> FerruleApp {
    let mut project = blank_project();
    project.source = SchemaNode::group("Input", vec![]);
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Result", ScalarType::String),
            SchemaNode::scalar("Function", ScalarType::String),
        ],
    );
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project.graph = body();
    project.graph.nodes.insert(
        2,
        Node::UserFunctionCall {
            function: FUNCTION,
            args: vec![],
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
    project.user_functions.insert(
        FUNCTION,
        UserFunction {
            library: "local".into(),
            name: "absent_default".into(),
            description: None,
            parameters: vec![],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: body(),
            output: 1,
        },
    );
    let mut app = FerruleApp {
        project,
        ..FerruleApp::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.open_function_tab(FUNCTION);
    assert!(app.ensure_function_canvas(FUNCTION));
    app.mapping_workspace.active = MappingDocument::Main;
    app.mark_clean();
    app.rebase_history();
    app
}
fn selected_default(project: &Project, function: bool) -> &Option<Value> {
    let graph = if function {
        &project.user_functions[&FUNCTION].body
    } else {
        &project.graph
    };
    let Node::ValueMap { default, .. } = &graph.nodes[&1] else {
        panic!("expected ValueMap");
    };
    default
}
fn set_absent_default(project: &mut Project, function: bool) {
    let graph = if function {
        &mut project.user_functions.get_mut(&FUNCTION).unwrap().body
    } else {
        &mut project.graph
    };
    let Node::ValueMap { default, .. } = graph.nodes.get_mut(&1).unwrap() else {
        panic!("expected ValueMap");
    };
    *default = Some(Value::Null);
}
fn key(app: &FerruleApp, retained: &Retained, label: &str) -> String {
    let state = editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
    retained.record(
        label,
        (
            &app.project,
            &state.serialized_project,
            &state.layout,
            app.is_dirty(),
            app.history.undo_len(),
            app.history.redo_len(),
        ),
    );
    state.serialized_project
}
fn assert_output(app: &FerruleApp, retained: &Retained) {
    let input = Instance::Group(Vec::new().into());
    let validation = engine::validate(&app.project);
    let result = engine::run_outputs(&app.project, &input);
    retained.record(
        "full-engine-default-absence-results-before-comparison",
        (&validation, &result),
    );
    assert!(validation.is_empty());
    let result = result.unwrap();
    assert_eq!(
        result.primary,
        Instance::Group(
            vec![
                ("Result".into(), Instance::Scalar(Value::Null)),
                ("Function".into(), Instance::Scalar(Value::Null)),
            ]
            .into()
        )
    );
    assert!(result.extras.is_empty());
}

#[test]
fn enabled_absent_defaults_have_distinct_gui_dirty_history_and_save_reload_identity() {
    let mut retained = Retained::new();
    for (ordinal, function) in [false, true].into_iter().enumerate() {
        let mut app = fixture();
        let before = key(&app, &retained, "original-disabled-clean-state");
        let original_layout =
            CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
        set_absent_default(&mut app.project, function);
        let after = key(&app, &retained, "enabled-absent-before-history-observation");
        assert_ne!(after, before);
        assert_eq!(selected_default(&app.project, function), &Some(Value::Null));
        assert_eq!(selected_default(&app.project, !function), &None);
        assert!(app.is_dirty());
        app.observe_editor_history(std::time::Instant::now(), false);
        retained.record(
            "full-observed-enabled-absent-state",
            (
                &app.project,
                app.history.undo_len(),
                app.history.redo_len(),
                app.is_dirty(),
            ),
        );
        assert_eq!(app.history.undo_len(), 1);
        assert!(app.can_undo());
        assert_output(&app, &retained);
        app.undo_project();
        assert_eq!(key(&app, &retained, "complete-undo-disabled-state"), before);
        assert_eq!(selected_default(&app.project, function), &None);
        assert!(!app.is_dirty());
        app.redo_project();
        assert_eq!(
            key(&app, &retained, "complete-redo-enabled-absent-state"),
            after
        );
        assert_eq!(selected_default(&app.project, function), &Some(Value::Null));
        assert!(app.is_dirty());
        let redo_layout =
            CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
        retained.record(
            "complete-before-and-redo-layouts",
            (&original_layout, &redo_layout),
        );
        let mut expected_layout = original_layout.clone();
        expected_layout.project_fingerprint = Some(project_fingerprint(&app.project));
        assert_ne!(
            redo_layout.project_fingerprint,
            original_layout.project_fingerprint
        );
        assert_eq!(redo_layout, expected_layout);
        let path = retained.path.join(format!("enabled-absent-{ordinal}.json"));
        let saved = app.save_document_to(&path);
        match &saved {
            Ok(outcome) => retained.record(
                "complete-save-outcome",
                (&outcome.validation_issues, &outcome.layout_warning),
            ),
            Err(error) => retained.record(
                "complete-save-error-and-causes",
                (
                    format!("{error:#?}\n{error:#}"),
                    error
                        .chain()
                        .map(|cause| format!("{cause:#?}\n{cause}"))
                        .collect::<Vec<_>>(),
                ),
            ),
        }
        let saved = saved.unwrap();
        assert!(saved.validation_issues.is_empty() && saved.layout_warning.is_none());
        let bytes = std::fs::read(&path).unwrap();
        let layout_path = crate::layout_store::layout_path(&path);
        let layout_bytes = std::fs::read(&layout_path).unwrap();
        let decoded = mapping::project_file::decode_bytes(&bytes);
        retained.record(
            "complete-saved-project-layout-and-public-decode-result",
            (&bytes, &layout_bytes, &decoded),
        );
        assert_eq!(
            selected_default(&decoded.unwrap(), function),
            &Some(Value::Null)
        );
        assert!(!app.is_dirty());
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&path);
        retained.record(
            "full-GUI-loader-project-status-and-diagnostics",
            (
                &reopened.project,
                &reopened.status,
                reopened.diagnostics.items(),
            ),
        );
        assert_eq!(
            selected_default(&reopened.project, function),
            &Some(Value::Null)
        );
        assert_eq!(selected_default(&reopened.project, !function), &None);
        assert!(!reopened.is_dirty());
        assert_eq!(
            key(&reopened, &retained, "reloaded-enabled-absent-state"),
            after
        );
        assert_output(&reopened, &retained);
        // Saving establishes an enabled-absent save point; Undo is now dirty,
        // while Redo returns to precisely that saved state.
        app.undo_project();
        assert_eq!(selected_default(&app.project, function), &None);
        assert!(app.is_dirty());
        app.redo_project();
        assert_eq!(selected_default(&app.project, function), &Some(Value::Null));
        assert!(!app.is_dirty());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(std::fs::read(&layout_path).unwrap(), layout_bytes);
    }
    retained.complete = true;
}
