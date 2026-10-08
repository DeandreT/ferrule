use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Scope};

const SOURCE: &str = "{\"Unused\":\"source\"}";

fn project(value: &str) -> Project {
    let mut project = demo_project();
    project.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Unused", ScalarType::String)],
    );
    project.target = SchemaNode::group(
        "Result",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project.graph = Graph {
        nodes: [(
            0,
            Node::Const {
                value: Value::String(value.into()),
            },
        )]
        .into(),
    };
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Default::default()
    };
    project
}

fn fixture() -> (DemoApp, egui::Context) {
    let mut app = DemoApp::new();
    app.install_project(project("one"));
    app.source_text = SOURCE.into();
    app.output = "held output".into();
    app.live_run = false;
    app.run_pending = false;
    app.history.clear();
    app.record_project(None, true);
    let context = egui::Context::default();
    settle(&mut app, &context);
    (app, context)
}

fn whole(project: &Project) -> String {
    project_document::to_json(project).expect("complete finite fixture")
}

fn frame(app: &mut DemoApp, context: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    input_frame(
        app,
        context,
        egui::RawInput {
            events,
            ..Default::default()
        },
    )
}

fn input_frame(
    app: &mut DemoApp,
    context: &egui::Context,
    mut input: egui::RawInput,
) -> egui::FullOutput {
    input.screen_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(1400.0, 900.0),
    ));
    input.time = Some(context.cumulative_frame_nr() as f64 / 10.0);
    eprintln!("browser-history input original: {input:#?}");
    let output = context.run_ui(input, |ui| app.show_workspace(ui));
    eprintln!(
        "browser-history full original: model={};draft={:?};source={:?};output={:?};formats={:?}/{:?};status={:?};diagnostic={:?};history={:?};paint={:?}",
        whole(&app.project),
        app.project_json,
        app.source_text,
        app.output,
        app.source_format,
        app.target_format,
        app.status,
        app.diagnostic,
        app.history.retained(),
        output.shapes,
    );
    output
}

fn settle(app: &mut DemoApp, context: &egui::Context) -> egui::FullOutput {
    let mut output = frame(app, context, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, Vec::new());
    }
    output
}

fn painted(output: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
    fn visit(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        found: &mut Vec<(String, egui::Rect, egui::Rect)>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) => found.push((
                text.galley.text().to_owned(),
                text.visual_bounding_rect(),
                clip,
            )),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, &mut found);
    }
    found
}

fn point(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    let points = painted(output)
        .into_iter()
        .filter_map(|(text, rect, clip)| {
            (text == label && rect.is_positive() && clip.contains_rect(rect))
                .then_some(rect.center())
        })
        .collect::<Vec<_>>();
    eprintln!("browser-history observed {label:?} points: {points:?}");
    assert_eq!(points.len(), 1, "one fully painted actual control");
    points[0]
}

fn click(app: &mut DemoApp, context: &egui::Context, pos: egui::Pos2) {
    frame(app, context, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            app,
            context,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}

fn click_label(app: &mut DemoApp, context: &egui::Context, label: &str) {
    let output = settle(app, context);
    click(app, context, point(&output, label));
}

fn command(key: egui::Key, shift: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            ctrl: true,
            command: true,
            shift,
            ..Default::default()
        },
    }
}

fn replace_constant(app: &mut DemoApp, context: &egui::Context, old: &str, new: &str) {
    click_label(app, context, old);
    assert!(
        context.text_edit_focused(),
        "actual canvas text field focused"
    );
    frame(
        app,
        context,
        vec![command(egui::Key::A, false), egui::Event::Text(new.into())],
    );
}

fn output(app: &DemoApp, expected: &str) {
    let source = Instance::Group(
        vec![(
            "Unused".into(),
            Instance::Scalar(Value::String("source".into())),
        )]
        .into(),
    );
    let target = engine::run(&app.project, &source);
    let text = runtime::run(&app.project, SOURCE, DataFormat::Json, DataFormat::Json);
    eprintln!(
        "browser-history complete engine original: {target:#?};public text original: {text:#?}"
    );
    assert_eq!(
        target.unwrap(),
        Instance::Group(
            vec![(
                "Value".into(),
                Instance::Scalar(Value::String(expected.into()))
            )]
            .into()
        )
    );
    assert_eq!(
        text.unwrap(),
        format!("{{\n  \"Value\": \"{expected}\"\n}}\n")
    );
}

#[test]
fn browser_history_real_constant_edits_coalesce_and_buttons_restore_complete_projects() {
    let (mut app, context) = fixture();
    let before = whole(&project("one"));
    output(&app, "one");
    replace_constant(&mut app, &context, "one", "two");
    frame(&mut app, &context, vec![egui::Event::Text("!".into())]);
    eprintln!("browser-history edited original: {}", whole(&app.project));
    assert_eq!(whole(&app.project), whole(&project("two!")));
    assert_eq!(
        app.history.retained().0,
        1,
        "one continuous focused canvas edit"
    );
    output(&app, "two!");
    click_label(&mut app, &context, "Undo");
    assert_eq!(whole(&app.project), before);
    output(&app, "one");
    click_label(&mut app, &context, "Redo");
    assert_eq!(whole(&app.project), whole(&project("two!")));
    output(&app, "two!");
    assert_eq!(app.source_text, SOURCE);
    assert_eq!(
        app.output, "held output",
        "live-off does not rewrite output"
    );
    assert!(app.run_pending);
    assert!(!app.project_json_dirty);
    assert_eq!(app.project_json, whole(&app.project));
}

#[test]
fn browser_history_valid_apply_drop_invalid_inputs_and_reset_have_explicit_boundaries() {
    let (mut app, context) = fixture();
    let mut imported = project("two");
    imported.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Float(f64::from_bits(0x0031_fa18_2c40_c60e)),
        },
    );
    imported.graph.nodes.insert(
        3,
        Node::ValueMap {
            input: 0,
            input_type: None,
            table: vec![(Value::Int(7), Value::Bool(true))],
            default: Some(Value::Null),
        },
    );
    let imported_json = whole(&imported);
    app.project_json = imported_json.clone();
    app.project_json_dirty = true;
    click_label(&mut app, &context, "Project");
    click_label(&mut app, &context, "Apply");
    assert_eq!(whole(&app.project), imported_json);
    assert_eq!(app.history.retained().0, 1);
    output(&app, "two");
    click_label(&mut app, &context, "Undo");
    assert_eq!(whole(&app.project), whole(&project("one")));
    let history_before = app.history.state();
    app.project_json = "{invalid".into();
    app.project_json_dirty = true;
    click_label(&mut app, &context, "Project");
    click_label(&mut app, &context, "Apply");
    assert_eq!(whole(&app.project), whole(&project("one")));
    assert_eq!(app.history.state(), history_before);
    assert!(app.history.redo_json().is_some());
    assert_eq!(app.project_json, "{invalid");
    assert_eq!(app.status, "Project not applied");
    assert!(app.diagnostic.is_some());

    let dropped = whole(&project("six"));
    input_frame(
        &mut app,
        &context,
        egui::RawInput {
            dropped_files: vec![egui::DroppedFile {
                name: "mapping.json".into(),
                bytes: Some(dropped.clone().into_bytes().into()),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    assert_eq!(whole(&app.project), dropped);
    assert!(
        app.history.redo_json().is_none(),
        "new committed drop clears redo"
    );
    assert!(!app.project_json_dirty);
    output(&app, "six");
    let committed = whole(&app.project);
    let counts = app.history.state();
    for bytes in [b"{broken".to_vec(), vec![0xff]] {
        input_frame(
            &mut app,
            &context,
            egui::RawInput {
                dropped_files: vec![egui::DroppedFile {
                    name: "bad.json".into(),
                    bytes: Some(bytes.into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        assert_eq!(whole(&app.project), committed);
        assert_eq!(app.history.state(), counts);
        assert!(app.diagnostic.is_some());
    }
    let mut invalid = project("ten");
    invalid.root.bindings[0].node = 999;
    app.project_json = whole(&invalid);
    app.project_json_dirty = true;
    click_label(&mut app, &context, "Project");
    click_label(&mut app, &context, "Apply");
    assert_eq!(whole(&app.project), committed);
    assert_eq!(app.history.state(), counts);
    assert_eq!(app.status, "Project not applied");
    click_label(&mut app, &context, "Reset");
    assert_eq!(whole(&app.project), whole(&demo_project()));
    assert_eq!(app.source_text, SAMPLE_XML);
    assert_eq!(app.source_format, DataFormat::Xml);
    assert_eq!(app.target_format, DataFormat::Xml);
    assert_eq!(app.history.retained().0, 0);
    assert!(app.history.undo_json().is_none() && app.history.redo_json().is_none());
    assert!(!app.project_json_dirty);
    assert!(app.live_run);
    assert_eq!(
        app.status, "Mapping completed",
        "reset schedules the original live run"
    );
}

#[test]
fn browser_history_keyboard_keeps_text_undo_drafts_and_buffers_separate_from_model() {
    let (mut app, context) = fixture();
    replace_constant(&mut app, &context, "one", "two");
    let generation = app.canvas_view_generation;
    let counts = app.history.retained();
    frame(&mut app, &context, vec![command(egui::Key::Z, false)]);
    assert_eq!(
        app.canvas_view_generation, generation,
        "focused canvas field did not restore a project"
    );
    assert_eq!(
        app.history.retained().0,
        counts.0,
        "text undo stays in the coalesced edit"
    );
    assert!(app.history.redo_json().is_none());
    // Settle the field's local undo, then explicitly re-enter the desired edit.
    let value = match &app.project.graph.nodes[&0] {
        Node::Const {
            value: Value::String(value),
        } => value.clone(),
        _ => panic!("text constant"),
    };
    replace_constant(&mut app, &context, &value, "two");
    click_label(&mut app, &context, "Project");
    let view = settle(&mut app, &context);
    let json_fields = painted(&view)
        .into_iter()
        .filter_map(|(text, rect, clip)| {
            if text.starts_with('{') {
                let painted = rect.intersect(clip);
                painted.is_positive().then_some(painted.center())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    eprintln!("browser-history actual JSON draft points: {json_fields:?}");
    assert_eq!(json_fields.len(), 1);
    click(&mut app, &context, json_fields[0]);
    assert!(context.text_edit_focused());
    frame(
        &mut app,
        &context,
        vec![
            command(egui::Key::A, false),
            egui::Event::Text("unapplied draft".into()),
        ],
    );
    assert!(app.project_json_dirty);
    let committed = whole(&app.project);
    let counts = app.history.retained();
    frame(&mut app, &context, vec![command(egui::Key::Z, false)]);
    assert_eq!(
        whole(&app.project),
        committed,
        "JSON text undo cannot restore a model"
    );
    assert_eq!(app.history.retained(), counts);
    let draft = app.project_json.clone();
    click_label(&mut app, &context, "Mapping");
    settle(&mut app, &context);
    assert!(!context.text_edit_focused());
    frame(&mut app, &context, vec![command(egui::Key::Z, false)]);
    assert_eq!(whole(&app.project), whole(&project("one")));
    assert_eq!(
        app.project_json, draft,
        "unapplied text is excluded from project restore"
    );
    frame(&mut app, &context, vec![command(egui::Key::Z, true)]);
    assert_eq!(whole(&app.project), whole(&project("two")));
    assert_eq!(app.project_json, draft);
    frame(&mut app, &context, vec![command(egui::Key::Z, false)]);
    frame(&mut app, &context, vec![command(egui::Key::Y, false)]);
    assert_eq!(whole(&app.project), whole(&project("two")));
    assert_eq!(app.source_text, SOURCE);
    assert_eq!(app.output, "held output");
    app.live_run = true;
    frame(&mut app, &context, Vec::new());
    assert_eq!(app.output, "{\n  \"Value\": \"two\"\n}\n");
    assert!(!app.run_pending);
    click_label(&mut app, &context, "Undo");
    replace_constant(&mut app, &context, "one", "six");
    assert!(app.history.redo_json().is_none());
    assert_eq!(app.project_json, draft);
    output(&app, "six");
}

#[test]
fn browser_history_full_snapshots_enforce_exact_bytes_count_coalescing_and_capture_limits() {
    use history::{ProjectHistory, RecordResult, SnapshotError};
    let mut history = ProjectHistory::default();
    for index in 0..67 {
        let value = format!("{index:03}");
        let result = history.capture_and_record(&project(&value), None);
        eprintln!(
            "browser-history count original {index}: {result:?}, ledger={:?}",
            history.retained()
        );
        assert_eq!(result.unwrap(), RecordResult::Recorded);
        assert_eq!(history.retained().0, index.min(64));
    }
    for value in (2..66).rev() {
        let snapshot = history.undo_json().unwrap();
        let restored = mapping::project_file::decode_str(snapshot).unwrap();
        eprintln!("browser-history full count selected original: {restored:#?}");
        assert_eq!(whole(&restored), whole(&project(&format!("{value:03}"))));
        assert!(history.undo());
        assert!(history.retained().1 <= history::MAX_SNAPSHOT_BYTES);
    }
    assert!(history.undo_json().is_none());
    assert_eq!(history.retained().0, 64);

    let mut sizing = ProjectHistory::default();
    sizing.capture_and_record(&project("one"), None).unwrap();
    let bytes = sizing.retained().1;
    let mut bounded = ProjectHistory::with_limits(64, 2 * bytes);
    bounded.capture_and_record(&project("one"), None).unwrap();
    bounded
        .capture_and_record(&project("two"), Some(0))
        .unwrap();
    assert_eq!(
        bounded.retained(),
        (1, 2 * bytes),
        "before plus latest exactly at byte cap"
    );
    bounded
        .capture_and_record(&project("six"), Some(0))
        .unwrap();
    assert_eq!(bounded.retained(), (1, 2 * bytes));
    assert_eq!(
        whole(&mapping::project_file::decode_str(bounded.undo_json().unwrap()).unwrap()),
        whole(&project("one"))
    );
    bounded.finish_focus(None);
    bounded
        .capture_and_record(&project("ten"), Some(0))
        .unwrap();
    assert_eq!(bounded.retained(), (1, 2 * bytes));
    assert_eq!(
        whole(&mapping::project_file::decode_str(bounded.undo_json().unwrap()).unwrap()),
        whole(&project("six")),
        "oldest complete transition evicted"
    );
    assert!(bounded.undo());
    assert_eq!(
        bounded.retained(),
        (1, 2 * bytes),
        "redo snapshots count in the same ledger"
    );
    bounded.capture_and_record(&project("red"), None).unwrap();
    assert!(bounded.redo_json().is_none());
    assert_eq!(bounded.retained(), (1, 2 * bytes));
    let unchanged = bounded.capture_and_record(&project("red"), None).unwrap();
    assert_eq!(unchanged, RecordResult::Unchanged);
    assert_eq!(bounded.retained(), (1, 2 * bytes));

    let mut exact = ProjectHistory::with_limits(64, bytes);
    exact.capture_and_record(&project("one"), None).unwrap();
    assert_eq!(exact.retained(), (0, bytes));
    let result = exact.capture_and_record(&project("four"), None);
    eprintln!(
        "browser-history capture +1 original: {result:?};ledger={:?}",
        exact.retained()
    );
    assert!(matches!(result, Err(SnapshotError::TooLarge { max }) if max == bytes));
    assert_eq!(exact.retained(), (0, 0));
    assert!(exact.undo_json().is_none() && exact.redo_json().is_none());
    exact.capture_and_record(&project("two"), None).unwrap();
    assert_eq!(
        exact.retained(),
        (0, bytes),
        "first small edit after refusal establishes a new baseline"
    );
    for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let mut nested = project("one");
        nested.graph.nodes.insert(
            7,
            Node::ValueMap {
                input: 0,
                input_type: None,
                table: vec![(Value::Int(1), Value::Float(value))],
                default: Some(Value::Null),
            },
        );
        let result = sizing.capture_and_record(&nested, None);
        eprintln!(
            "browser-history nested nonfinite original: project={nested:#?};result={result:?}"
        );
        assert!(matches!(result, Err(SnapshotError::Serialize(_))));
        assert_eq!(
            sizing.retained(),
            (0, 0),
            "nonfinite cannot collapse to a tracked null"
        );
        sizing.capture_and_record(&project("one"), None).unwrap();
    }
}

#[test]
fn browser_history_restores_invalid_intermediates_exact_tags_and_enabled_absent_defaults() {
    let (mut app, context) = fixture();
    let mut imported = project("two");
    imported.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Float(f64::from_bits(0x0031_fa18_2c40_c60e)),
        },
    );
    imported.graph.nodes.insert(
        3,
        Node::ValueMap {
            input: 0,
            input_type: Some(ScalarType::String),
            table: vec![(Value::Int(7), Value::Bool(true))],
            default: Some(Value::Null),
        },
    );
    app.project_json = whole(&imported);
    app.project_json_dirty = true;
    click_label(&mut app, &context, "Project");
    click_label(&mut app, &context, "Apply");
    let before = whole(&app.project);
    app.project.root.bindings[0].node = 999;
    app.project_changed = true;
    app.finish_project_edits();
    let invalid = whole(&app.project);
    let issues = engine::validate(&app.project);
    eprintln!("browser-history full invalid intermediate original: {invalid};issues={issues:#?}");
    assert!(!issues.is_empty());
    app.restore_project(false);
    assert_eq!(whole(&app.project), before);
    app.restore_project(true);
    assert_eq!(
        whole(&app.project),
        invalid,
        "undo/redo adds no engine-validation admission"
    );
    match &app.project.graph.nodes[&2] {
        Node::Const {
            value: Value::Float(value),
        } => assert_eq!(value.to_bits(), 0x0031_fa18_2c40_c60e),
        _ => panic!("float tag preserved"),
    }
    match &app.project.graph.nodes[&3] {
        Node::ValueMap {
            table,
            default,
            input_type,
            ..
        } => {
            assert_eq!(table, &vec![(Value::Int(7), Value::Bool(true))]);
            assert_eq!(default, &Some(Value::Null));
            assert_eq!(input_type, &Some(ScalarType::String));
        }
        _ => panic!("complete typed table preserved"),
    }
    app.project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Float(f64::INFINITY),
        },
    );
    app.project_changed = true;
    app.finish_project_edits();
    let capture = &app.history_notice;
    eprintln!(
        "browser-history untracked nonfinite original: capture={capture:?};model={:?}",
        app.project
    );
    assert!(capture.is_some());
    assert_eq!(app.history.retained(), (0, 0));
    assert!(
        matches!(&app.project.graph.nodes[&2], Node::Const { value: Value::Float(value) } if value.is_infinite())
    );
    assert!(
        app.project_json.is_empty(),
        "existing failed-download serialization behavior retained"
    );
    assert_eq!(app.status, "Project serialization failed");
}
