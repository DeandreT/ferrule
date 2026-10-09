use super::*;
use crate::filter_map_editor::observations::{self, Control};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FunctionId, SequenceExpr};
use serde_json::{Value as Json, json};
use std::cell::RefCell;

struct TestDir(std::path::PathBuf);
impl TestDir {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-filter-map-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!(
                "Retained complete filter/map editor artifacts: {}",
                self.0.display()
            );
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

pub(crate) fn literal(id: &str) -> Json {
    let corpus: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/fixtures/scalar-filter-map-v1.json"
    )))
    .unwrap();
    corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == id)
        .unwrap()
        .clone()
}

fn fixture(id: &str) -> FerruleApp {
    let case = literal(id);
    let sequence: SequenceExpr =
        serde_json::from_value(case["sequence_descriptor"].clone()).unwrap();
    let ty = match &sequence {
        SequenceExpr::FilterMapV1(value) => value.output_type,
        _ => ScalarType::Int,
    };
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group("Input", Vec::new());
    app.project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ty),
                    SchemaNode::scalar("Position", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    );
    app.project.graph = serde_json::from_value(case["parent_graph"].clone()).unwrap();
    app.project.user_functions = serde_json::from_value(case["user_functions"].clone()).unwrap();
    app.project.graph.nodes.insert(
        40,
        mapping::Node::Position {
            collection: Vec::new(),
        },
    );
    app.project.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Sequence(sequence),
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    node: 11,
                },
                Binding {
                    target_field: "Position".into(),
                    node: 40,
                },
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    app.project.extra_targets.clear();
    app.project.failure_rules.clear();
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.selected_scope = vec![0];
    app.mark_clean();
    app.rebase_history();
    app
}

fn encoded(app: &FerruleApp) -> String {
    mapping::project_file::encode_pretty(&app.project).unwrap()
}
fn sequence(app: &FerruleApp) -> &SequenceExpr {
    app.project.root.children[0].sequence().unwrap()
}
fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
    enabled: bool,
    size: egui::Vec2,
) -> (egui::FullOutput, Vec<Control>) {
    observations::begin();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| app.show_filter_map_scope_editor(ui, enabled),
    );
    let controls = observations::take();
    eprintln!(
        "frame original project={}\ncontrols={controls:#?}\nfull_shapes={:#?}",
        encoded(app),
        output.shapes
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    (output, controls)
}
fn settle(app: &mut FerruleApp, context: &egui::Context, enabled: bool) -> Vec<Control> {
    let mut controls = Vec::new();
    for _ in 0..8 {
        controls = frame(app, context, Vec::new(), enabled, egui::vec2(1200.0, 900.0)).1;
    }
    controls
}
fn click_control(app: &mut FerruleApp, context: &egui::Context, name: &str, enabled: bool) {
    let controls = settle(app, context, enabled);
    let actual = controls
        .iter()
        .find(|control| control.name == name)
        .unwrap();
    eprintln!("pointer target original={actual:#?}");
    let xy = actual.response.rect.center();
    assert!(actual.clip.contains(xy), "visible current control {name}");
    for pressed in [true, false] {
        frame(
            app,
            context,
            vec![
                egui::Event::PointerMoved(xy),
                egui::Event::PointerButton {
                    pos: xy,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            enabled,
            egui::vec2(1200.0, 900.0),
        );
    }
}
fn key(app: &mut FerruleApp, context: &egui::Context, key: egui::Key) {
    frame(
        app,
        context,
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        true,
        egui::vec2(1200.0, 900.0),
    );
}

#[test]
fn pointer_capture_limit_order_removal_and_disabled_editor_preserve_whole_descriptors() {
    let mut app = fixture("identity-all");
    let context = context();
    for count in 0..16 {
        click_control(&mut app, &context, "Add capture", true);
        let actual = encoded(&app);
        eprintln!("capture count={} original={actual}", count + 1);
        let SequenceExpr::FilterMapV1(value) = sequence(&app) else {
            panic!("filter/map")
        };
        assert_eq!(value.captures.len(), count + 1);
        assert_eq!(value.source.item(), 10);
        assert_eq!(value.item, 11);
        assert!(
            value
                .captures
                .iter()
                .all(|capture| capture.node == 1 && capture.ty == ScalarType::Int)
        );
    }
    let before = encoded(&app);
    let controls = settle(&mut app, &context, true);
    let add = controls
        .iter()
        .find(|control| control.name == "Add capture")
        .unwrap();
    eprintln!("sixteen-cap original control={add:#?} project={before}");
    assert!(!add.response.enabled());
    click_control(&mut app, &context, "Add capture", true);
    assert_eq!(encoded(&app), before);
    // An imported 17th capture is preserved, and the visible first row can repair it.
    if let ScopeIteration::Sequence(SequenceExpr::FilterMapV1(value)) =
        &mut app.project.root.children[0].iteration
    {
        value.captures.push(mapping::FilterMapCapture {
            node: 2,
            ty: ScalarType::Int,
        });
    }
    let imported = encoded(&app);
    settle(&mut app, &context, true);
    assert_eq!(encoded(&app), imported);
    click_control(&mut app, &context, "Remove capture 1", true);
    let SequenceExpr::FilterMapV1(value) = sequence(&app) else {
        panic!("filter/map")
    };
    eprintln!("repaired original={value:#?}");
    assert_eq!(value.captures.len(), 16);
    let locked = encoded(&app);
    click_control(&mut app, &context, "Remove capture 1", false);
    key(&mut app, &context, egui::Key::Tab);
    frame(
        &mut app,
        &context,
        vec![egui::Event::Text("ignored".into())],
        false,
        egui::vec2(900.0, 700.0),
    );
    assert_eq!(encoded(&app), locked);
}

#[test]
fn pointer_capture_reordering_and_default_start_history_document_copies_are_independent() {
    let mut app = fixture("identity-all");
    let dir = TestDir::new();
    let path = dir.0.join("sequence.json");
    app.document = crate::document::DocumentLocation::untitled(path.clone());
    if let ScopeIteration::Sequence(SequenceExpr::FilterMapV1(value)) =
        &mut app.project.root.children[0].iteration
    {
        value.captures = vec![
            mapping::FilterMapCapture {
                node: 1,
                ty: ScalarType::Int,
            },
            mapping::FilterMapCapture {
                node: 2,
                ty: ScalarType::Float,
            },
        ];
    }
    app.rebase_history();
    let original = encoded(&app);
    let context = context();
    click_control(&mut app, &context, "Move capture 1 down", true);
    let ordered = encoded(&app);
    eprintln!("full original={original}\nordered={ordered}");
    let SequenceExpr::FilterMapV1(value) = sequence(&app) else {
        panic!("filter/map")
    };
    assert_eq!(
        value.captures,
        vec![
            mapping::FilterMapCapture {
                node: 2,
                ty: ScalarType::Float
            },
            mapping::FilterMapCapture {
                node: 1,
                ty: ScalarType::Int
            }
        ]
    );
    click_control(&mut app, &context, "Default start (1)", true);
    let edited = encoded(&app);
    eprintln!("default-start original={edited}");
    assert!(
        matches!(sequence(&app), SequenceExpr::FilterMapV1(value) if matches!(value.source.as_ref(), SequenceExpr::Generate { from: None, .. }))
    );
    app.undo_project();
    let undone = encoded(&app);
    eprintln!("undo original={undone}");
    assert_eq!(undone, ordered);
    app.undo_project();
    assert_eq!(encoded(&app), original);
    app.redo_project();
    app.redo_project();
    assert_eq!(encoded(&app), edited);
    let mut copy = mapping::project_file::decode_str(&edited).unwrap();
    eprintln!("whole copied document original={copy:#?}");
    assert_eq!(mapping::project_file::encode_pretty(&copy).unwrap(), edited);
    let ScopeIteration::Sequence(SequenceExpr::FilterMapV1(value)) =
        &mut copy.root.children[0].iteration
    else {
        panic!("copy")
    };
    value.captures.clear();
    assert_eq!(encoded(&app), edited);
    assert_eq!(value.source.item(), 10);
    assert_eq!(value.item, 11);
    let saved = app.save_document_to(&path);
    let original_bytes = std::fs::read(&path);
    let saved_original = saved
        .as_ref()
        .map(|outcome| (&outcome.validation_issues, &outcome.layout_warning))
        .map_err(|error| format!("{error:#}"));
    eprintln!("save result original={saved_original:#?}\ncomplete saved bytes={original_bytes:#?}");
    saved.unwrap();
    let body = String::from_utf8(original_bytes.unwrap()).unwrap();
    let loaded = mapping::project_file::decode_str(&body).unwrap();
    assert_eq!(
        mapping::project_file::encode_pretty(&loaded).unwrap(),
        edited
    );
}

#[test]
fn scope_creation_reserves_both_owners_atomically_and_refuses_function_contexts() {
    let mut app = fixture("identity-all");
    app.project.root.children[0].set_sequence(None);
    app.rebase_history();
    let before = encoded(&app);
    let created = app.create_selected_filter_map();
    eprintln!(
        "creation original={created:#?}\nbefore={before}\nafter={}",
        encoded(&app)
    );
    created.unwrap();
    let SequenceExpr::FilterMapV1(value) = sequence(&app) else {
        panic!("filter/map")
    };
    assert_eq!(value.source.item(), 43);
    assert_eq!(value.item, 44);
    assert_eq!(value.predicate, FunctionId::new(100));
    assert_eq!(value.mapper, FunctionId::new(101));
    app.observe_editor_history(std::time::Instant::now(), false);
    app.undo_project();
    assert_eq!(encoded(&app), before);
    app.redo_project();
    let existing = encoded(&app);
    app.mapping_workspace.active = MappingDocument::Function(FunctionId::new(100));
    let refusal = app.create_selected_filter_map();
    eprintln!(
        "isolated creation refusal={refusal:#?} original={}",
        encoded(&app)
    );
    assert!(refusal.is_err());
    assert_eq!(encoded(&app), existing);
    app.mapping_workspace.active = MappingDocument::Main;
    app.project
        .graph
        .nodes
        .insert(mapping::NodeId::MAX, mapping::Node::Unconnected);
    app.project.root.children[0].set_sequence(None);
    let exhausted_before = encoded(&app);
    let exhausted = app.create_selected_filter_map();
    eprintln!(
        "exhaustion original={exhausted:#?}\ncomplete={}",
        encoded(&app)
    );
    assert!(exhausted.is_err());
    assert_eq!(encoded(&app), exhausted_before);
}

fn retain(instance: &Instance, path: &str) {
    observations::retain_instance(instance, path);
}
fn retain_error(error: &engine::EngineError) {
    observations::retain_error(error);
}
#[derive(Default)]
struct Boundaries(RefCell<Vec<engine::FilterMapBoundary>>);
impl engine::FilterMapCancellation for Boundaries {
    fn is_cancelled(&self, boundary: &engine::FilterMapBoundary) -> bool {
        self.0.borrow_mut().push(*boundary);
        false
    }
}
fn rows(values: Vec<Value>) -> Instance {
    Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(
                values
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| {
                        Instance::Group(
                            vec![
                                ("Value".into(), Instance::Scalar(value)),
                                (
                                    "Position".into(),
                                    Instance::Scalar(Value::Int(index as i64 + 1)),
                                ),
                            ]
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    )
}

#[test]
fn edited_scope_native_outputs_tags_positions_and_errors_match_independent_complete_oracles() {
    for (id, values) in [
        (
            "identity-all",
            Some(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        ),
        (
            "positions-parent-source-dense",
            Some(vec![Value::Int(40), Value::Int(51), Value::Int(62)]),
        ),
        ("float-output", Some(vec![Value::Float(1.5); 3])),
        (
            "bool-output",
            Some(vec![
                Value::Bool(false),
                Value::Bool(true),
                Value::Bool(true),
            ]),
        ),
        (
            "string-output",
            Some(vec![
                Value::String("vλ1".into()),
                Value::String("vλ2".into()),
                Value::String("vλ3".into()),
            ]),
        ),
        ("selected-mapper-raise-empty-message", None),
        ("nonfinite-mapper-output", None),
    ] {
        let mut app = fixture(id);
        // This GUI adapter uses an ordinary constant parent capture. The full
        // native corpus independently qualifies its original positioned parent.
        if id == "positions-parent-source-dense" {
            app.project.graph.nodes.insert(
                20,
                mapping::Node::Const {
                    value: Value::Int(7),
                },
            );
        }
        let context = context();
        settle(&mut app, &context, true);
        click_control(&mut app, &context, "Default start (1)", true);
        click_control(&mut app, &context, "Default start (1)", true);
        let source = Instance::Group(Vec::new().into());
        let boundaries = Boundaries::default();
        let execution = engine::ExecutionContext::new(std::path::Path::new("mapping.json"))
            .with_filter_map_cancellation(&boundaries);
        let actual = engine::run_with_context(&app.project, &source, &execution);
        eprintln!(
            "public edited project original={}\nactual={actual:#?}\nall boundaries={:#?}",
            encoded(&app),
            boundaries.0.borrow()
        );
        retain(&source, "source");
        match &actual {
            Ok(instance) => retain(instance, "target"),
            Err(error) => retain_error(error),
        }
        let actual_json = actual
            .as_ref()
            .ok()
            .map(|instance| format_json::to_string(&app.project.target, instance));
        eprintln!("complete JSON output original={actual_json:#?}");
        if let Some(values) = values {
            let expected = rows(values);
            let expected_json = expected_json(id);
            eprintln!(
                "independent complete typed expected={expected:#?}\nwhole expected bytes={:?}",
                expected_json.as_bytes()
            );
            assert_eq!(actual.as_ref().unwrap(), &expected);
            assert_eq!(
                actual_json.unwrap().unwrap().as_bytes(),
                expected_json.as_bytes()
            );
            let actual_positions = boundaries
                .0
                .borrow()
                .iter()
                .filter(|boundary| {
                    boundary.phase == engine::FilterMapPhase::Mapper
                        && boundary.kind == engine::FilterMapBoundaryKind::CallEntry
                        && boundary.function == Some(FunctionId::new(101))
                })
                .filter_map(|boundary| boundary.source_position)
                .collect::<Vec<_>>();
            assert_eq!(
                json!(actual_positions),
                literal(id)["expected"]["kept_source_positions"]
            );
        } else {
            let error = actual.unwrap_err();
            let (position, node, kind, cause) = if id == "selected-mapper-raise-empty-message" {
                (
                    3,
                    4,
                    engine::FilterMapBoundaryKind::NodeEvaluation,
                    engine::EngineError::MappingException {
                        node: 4,
                        message: Some(String::new()),
                    },
                )
            } else {
                (
                    1,
                    3,
                    engine::FilterMapBoundaryKind::Result,
                    engine::EngineError::FilterMapNonFinite {
                        bits: 0x7ff0000000000000,
                    },
                )
            };
            let expected = engine::EngineError::FilterMapRuntime {
                boundary: engine::FilterMapBoundary {
                    item: 11,
                    phase: engine::FilterMapPhase::Mapper,
                    capture_index: None,
                    source_position: Some(position),
                    function: Some(FunctionId::new(101)),
                    node: Some(node),
                    kind,
                },
                source: Box::new(cause),
            };
            eprintln!("complete independently expected error={expected:#?}");
            assert_eq!(error, expected);
        }
    }
}

fn expected_json(id: &str) -> &'static str {
    match id {
        "identity-all" => {
            r#"{
  "Rows": [
    {
      "Value": 1,
      "Position": 1
    },
    {
      "Value": 2,
      "Position": 2
    },
    {
      "Value": 3,
      "Position": 3
    }
  ]
}
"#
        }
        "positions-parent-source-dense" => {
            r#"{
  "Rows": [
    {
      "Value": 40,
      "Position": 1
    },
    {
      "Value": 51,
      "Position": 2
    },
    {
      "Value": 62,
      "Position": 3
    }
  ]
}
"#
        }
        "float-output" => {
            r#"{
  "Rows": [
    {
      "Value": 1.5,
      "Position": 1
    },
    {
      "Value": 1.5,
      "Position": 2
    },
    {
      "Value": 1.5,
      "Position": 3
    }
  ]
}
"#
        }
        "bool-output" => {
            r#"{
  "Rows": [
    {
      "Value": false,
      "Position": 1
    },
    {
      "Value": true,
      "Position": 2
    },
    {
      "Value": true,
      "Position": 3
    }
  ]
}
"#
        }
        "string-output" => {
            r#"{
  "Rows": [
    {
      "Value": "vλ1",
      "Position": 1
    },
    {
      "Value": "vλ2",
      "Position": 2
    },
    {
      "Value": "vλ3",
      "Position": 3
    }
  ]
}
"#
        }
        _ => panic!("no successful editor oracle for {id}"),
    }
}

#[test]
fn ordinary_native_preview_retains_complete_in_memory_bytes_and_does_not_publish_files() {
    for id in ["identity-all", "selected-mapper-raise-empty-message"] {
        let mut app = fixture(id);
        let dir = TestDir::new();
        app.document = crate::document::DocumentLocation::untitled(dir.0.join("mapping.json"));
        let context = context();
        app.begin_preview();
        let output_path = dir.0.join("must-not-publish.json");
        let draft = app.preview_draft.as_mut().unwrap();
        draft.input_text = "{}".into();
        draft.input_identity = "input.json".into();
        draft.output_identity = output_path.display().to_string();
        std::fs::write(dir.0.join("complete-project.json"), encoded(&app)).unwrap();
        app.execute_preview();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.pending_preview.is_some() && std::time::Instant::now() < deadline {
            app.poll_preview(&context);
            if app.pending_preview.is_some() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        let original = format!(
            "status={}\npending={}\ndiagnostics={:#?}\nreport={:#?}",
            app.status,
            app.pending_preview.is_some(),
            app.diagnostics.items(),
            app.run_report.as_ref().map(|view| &view.report)
        );
        std::fs::write(dir.0.join("complete-preview-outcome.txt"), &original).unwrap();
        eprintln!("preview original={original}");
        let actual = app.run_report.as_mut().map(|view| {
            view.report
                .outputs
                .iter_mut()
                .map(|output| {
                    let name = output.name.clone();
                    let path = output.path.clone();
                    let original = match output.preview() {
                        crate::run_report::OutputPreview::Text {
                            content,
                            total_bytes,
                            truncated,
                        } => crate::run_report::OutputPreview::Text {
                            content: content.clone(),
                            total_bytes: *total_bytes,
                            truncated: *truncated,
                        },
                        crate::run_report::OutputPreview::Binary {
                            content,
                            total_bytes,
                            truncated,
                        } => crate::run_report::OutputPreview::Binary {
                            content: content.clone(),
                            total_bytes: *total_bytes,
                            truncated: *truncated,
                        },
                        crate::run_report::OutputPreview::Unavailable { message } => {
                            crate::run_report::OutputPreview::Unavailable {
                                message: message.clone(),
                            }
                        }
                    };
                    (name, path, original)
                })
                .collect::<Vec<_>>()
        });
        eprintln!("complete public preview originals={actual:#?}");
        // RunOutput's test view retains the same bounded in-memory payload;
        // these fixtures are below its limit. Failed Preview exposes diagnostics,
        // while direct public native controls above retain complete typed causes.
        assert!(app.pending_preview.is_none());
        assert!(!output_path.exists());
        if id == "identity-all" {
            let originals = actual.unwrap();
            assert_eq!(originals.len(), 1);
            assert_eq!(
                originals[0].2,
                crate::run_report::OutputPreview::Text {
                    content: expected_json(id).into(),
                    total_bytes: expected_json(id).len() as u64,
                    truncated: false,
                }
            );
            assert!(app.status.starts_with("previewed "));
        } else {
            assert_eq!(app.status, "preview failed");
            assert!(app.run_report.is_none());
            assert!(
                app.diagnostics
                    .items()
                    .iter()
                    .any(|item| item.message.contains("mapping exception:"))
            );
        }
    }
}

fn text_centers(
    shape: &egui::epaint::Shape,
    clip: egui::Rect,
    label: &str,
    centers: &mut Vec<egui::Pos2>,
) {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
            let center = text.visual_bounding_rect().center();
            if clip.contains(center) {
                centers.push(center);
            }
        }
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                text_centers(shape, clip, label, centers);
            }
        }
        _ => {}
    }
}
fn click_text(app: &mut FerruleApp, context: &egui::Context, label: &str) {
    let output = frame(app, context, Vec::new(), true, egui::vec2(1200.0, 900.0)).0;
    let mut centers = Vec::new();
    for shape in &output.shapes {
        text_centers(&shape.shape, shape.clip_rect, label, &mut centers);
    }
    eprintln!("current text choice original label={label:?} centers={centers:?}");
    let xy = *centers.last().unwrap();
    for pressed in [true, false] {
        frame(
            app,
            context,
            vec![
                egui::Event::PointerMoved(xy),
                egui::Event::PointerButton {
                    pos: xy,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            true,
            egui::vec2(1200.0, 900.0),
        );
    }
}

#[test]
fn pointer_stage_selection_changes_only_the_selected_function_and_native_preview_values() {
    let mut app = fixture("identity-all");
    let mut function = app.project.user_functions[&FunctionId::new(101)].clone();
    function.name = "map-double".into();
    function.body.nodes.insert(
        2,
        mapping::Node::Const {
            value: Value::Int(2),
        },
    );
    function.body.nodes.insert(
        3,
        mapping::Node::Call {
            function: "multiply".into(),
            args: vec![1, 2],
        },
    );
    function.output = 3;
    app.project
        .user_functions
        .insert(FunctionId::new(200), function);
    app.rebase_history();
    let before = encoded(&app);
    let context = context();
    click_control(&mut app, &context, "Map stage", true);
    click_text(&mut app, &context, "sequence-design / map-double");
    let after = encoded(&app);
    eprintln!("stage selection whole original before={before}\nafter={after}");
    let SequenceExpr::FilterMapV1(value) = sequence(&app) else {
        panic!("filter/map")
    };
    assert_eq!(value.mapper, FunctionId::new(200));
    assert_eq!(value.predicate, FunctionId::new(100));
    assert_eq!(value.source.item(), 10);
    assert_eq!(value.item, 11);
    assert!(value.captures.is_empty());
    let actual = engine::run(&app.project, &Instance::Group(Vec::new().into()));
    eprintln!("stage selection complete public native original={actual:#?}");
    match &actual {
        Ok(value) => retain(value, "target"),
        Err(error) => retain_error(error),
    }
    assert_eq!(
        actual.unwrap(),
        rows(vec![Value::Int(2), Value::Int(4), Value::Int(6)])
    );
    app.undo_project();
    assert_eq!(encoded(&app), before);
    app.redo_project();
    assert_eq!(encoded(&app), after);
}

#[test]
fn malformed_source_stage_and_signature_are_retained_and_reported_before_repair() {
    let mut app = fixture("identity-all");
    if let ScopeIteration::Sequence(SequenceExpr::FilterMapV1(value)) =
        &mut app.project.root.children[0].iteration
    {
        value.mapper = FunctionId::new(999);
        *value.source = SequenceExpr::Tokenize {
            input: 1,
            delimiter: 2,
            item: 10,
        };
    }
    let before = encoded(&app);
    let context = context();
    let output = frame(
        &mut app,
        &context,
        Vec::new(),
        true,
        egui::vec2(900.0, 700.0),
    )
    .0;
    let after = encoded(&app);
    let validation = cli::validate(&app.project);
    eprintln!(
        "unsupported imported descriptor before={before}\nafter={after}\nfull visible shapes={:#?}\ncomplete validation={validation:#?}",
        output.shapes
    );
    assert_eq!(after, before);
    assert!(!validation.is_empty());
    let SequenceExpr::FilterMapV1(value) = sequence(&app) else {
        panic!("filter/map")
    };
    assert_eq!(value.mapper, FunctionId::new(999));
    assert!(matches!(
        value.source.as_ref(),
        SequenceExpr::Tokenize { item: 10, .. }
    ));
}

#[test]
fn named_scope_creation_uses_fresh_owners_and_complete_two_target_outputs() {
    let mut app = fixture("identity-all");
    app.project.extra_targets.push(mapping::NamedTarget {
        name: "Audit".into(),
        schema: app.project.target.clone(),
        path: Some("audit.json".into()),
        options: app.project.target_options.clone(),
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    });
    app.mapping_workspace.active = MappingDocument::Target(0);
    app.selected_scope = vec![0];
    let before = encoded(&app);
    let created = app.create_selected_filter_map();
    eprintln!(
        "named scope creation original={created:#?}\nbefore={before}\nafter={}",
        encoded(&app)
    );
    created.unwrap();
    let SequenceExpr::FilterMapV1(value) = app.project.extra_targets[0].root.children[0]
        .sequence()
        .unwrap()
    else {
        panic!("filter/map")
    };
    assert_eq!(value.source.item(), 43);
    assert_eq!(value.item, 44);
    assert_eq!(
        crate::graph_viewer::project_sequence_item_ids(&app.project),
        [10, 11, 43, 44].into_iter().collect()
    );
    app.project.extra_targets[0].root.children[0].bindings = vec![
        Binding {
            target_field: "Value".into(),
            node: 44,
        },
        Binding {
            target_field: "Position".into(),
            node: 40,
        },
    ];
    let actual = engine::run_outputs(&app.project, &Instance::Group(Vec::new().into()));
    eprintln!(
        "whole primary and named project={}\ncomplete outputs original={actual:#?}",
        encoded(&app)
    );
    match &actual {
        Ok(outputs) => {
            retain(&outputs.primary, "primary");
            for (index, extra) in outputs.extras.iter().enumerate() {
                eprintln!("named output original index={index} name={:?}", extra.name);
                retain(&extra.instance, &format!("named-{index}"));
            }
        }
        Err(error) => retain_error(error),
    }
    let expected = rows(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    let outputs = actual.unwrap();
    assert_eq!(outputs.primary, expected);
    assert_eq!(outputs.extras.len(), 1);
    assert_eq!(
        outputs.extras[0],
        engine::NamedOutput {
            name: "Audit".into(),
            instance: expected
        }
    );
}
