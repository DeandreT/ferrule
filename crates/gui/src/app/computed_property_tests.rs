use super::*;
use crate::scope_editor::computed_properties::{self, Profile};
use ir::{Instance, ScalarType, ScalarTypeSet, SchemaNode, Value};
use mapping::{Binding, DynamicBinding, DynamicChild, ScopeConstruction, ScopeIteration};

mod support;
use support::*;

#[test]
fn computed_properties_actual_main_and_named_rows_keep_order_and_canvas_identity() {
    let mut retained = Retained::new();
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let (mut app, context) = setup(&retained, false, document);
        let graph = serde_json::to_string(&app.project.graph).unwrap();
        let canvas = canvas_state(&app);
        retained.record("original-canvas", &canvas);
        outputs(
            &app,
            &retained,
            &expected(&[]),
            FIXED,
            &expected(&[]),
            FIXED,
        );
        add_two(&mut app, &context, &retained);
        let pairs = active_scope(&app)
            .dynamic_bindings
            .iter()
            .map(|binding| (binding.key, binding.value))
            .collect::<Vec<_>>();
        retained.record("authored-pairs", &pairs);
        assert_eq!(pairs, [(1, 2), (3, 4)]);
        assert_eq!(serde_json::to_string(&app.project.graph).unwrap(), graph);
        assert_eq!(canvas_state(&app), canvas);
        assert!(app.is_dirty() && app.can_undo());
        if document == MappingDocument::Main {
            outputs(&app, &retained, &full(), FULL, &expected(&[]), FIXED);
        } else {
            outputs(&app, &retained, &expected(&[]), FIXED, &full(), FULL);
        }
        click_label(&mut app, &context, &retained, "Remove property 1");
        let city = expected(&[("city", Value::String("Oslo".into()))]);
        if document == MappingDocument::Main {
            outputs(&app, &retained, &city, CITY, &expected(&[]), FIXED);
        } else {
            outputs(&app, &retained, &expected(&[]), FIXED, &city, CITY);
        }
        assert_eq!(canvas_state(&app), canvas);
        assert_eq!(serde_json::to_string(&app.project.graph).unwrap(), graph);
    }
    retained.complete = true;
}

#[test]
fn computed_property_edit_real_undo_redo_save_reopen_and_unwritten_preview() -> anyhow::Result<()> {
    let mut retained = Retained::new();
    let (mut app, context) = setup(&retained, true, MappingDocument::Target(0));
    let before = raw_project(&app, &retained, "history-before");
    let canvas = canvas_state(&app);
    choose(
        &mut app,
        &context,
        &retained,
        1,
        false,
        2,
        "2: field ValueA",
    );
    let changed = raw_project(&app, &retained, "history-after");
    let edited = expected(&[
        ("display_name", Value::String("Ada".into())),
        ("city", Value::String("Ada".into())),
    ]);
    outputs(&app, &retained, &full(), FULL, &edited, CITY_ADA);
    keyboard_history(&mut app, &context, &retained, false);
    assert_eq!(raw_project(&app, &retained, "after-undo"), before);
    assert!(!app.is_dirty());
    outputs(&app, &retained, &full(), FULL, &full(), FULL);
    keyboard_history(&mut app, &context, &retained, true);
    assert_eq!(raw_project(&app, &retained, "after-redo"), changed);
    assert_eq!(canvas_state(&app), canvas);
    let path = retained.path.join("project.json");
    let saved = app.save_document_to(&path);
    match &saved {
        Ok(saved) => retained.record(
            "full-save-outcome",
            (&saved.validation_issues, &saved.layout_warning),
        ),
        Err(error) => retained.record("full-save-error", format!("{error:#?}\n{error:#}")),
    }
    let saved = saved?;
    assert!(saved.validation_issues.is_empty() && saved.layout_warning.is_none());
    let project_bytes = std::fs::read(&path)?;
    let layout_path = crate::layout_store::layout_path(&path);
    let layout_bytes = std::fs::read(&layout_path)?;
    retained.record("original-saved-bytes", (&project_bytes, &layout_bytes));
    let saved_state = raw_project(&app, &retained, "post-save-project");
    let decoded = mapping::project_file::decode_bytes(&project_bytes);
    match &decoded {
        Ok(project) => retained.record("complete-project-decode", project),
        Err(error) => retained.record(
            "complete-project-decode-error",
            format!("{error:#?}\n{error}"),
        ),
    }
    let decoded = decoded?;
    let encoded = mapping::project_file::encode_pretty(&decoded);
    retained.record("reencoded-project", &encoded);
    assert_eq!(encoded?, saved_state);
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    reopened.open_target_tab(0);
    assert!(reopened.ensure_target_canvas(0));
    settle(&mut reopened, &context, &retained);
    assert_eq!(
        raw_project(&reopened, &retained, "actual-reopened-project"),
        saved_state
    );
    outputs(&reopened, &retained, &full(), FULL, &edited, CITY_ADA);
    let logical_output = retained.path.join("preview-must-not-write.json");
    reopened.preview_draft = Some(crate::preview::PreviewDraft {
        target: crate::preview::PreviewTarget::Named("Other".into()),
        input_identity: "input.json".into(),
        output_identity: logical_output.display().to_string(),
        input_text: INPUT.into(),
        debug_breakpoint: None,
    });
    reopened.execute_preview();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while reopened.pending_preview.is_some() && std::time::Instant::now() < deadline {
        reopened.poll_preview(&context);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    retained.record(
        "complete-preview-outcome",
        (
            &reopened.run_report,
            &reopened.status,
            reopened.diagnostics.items(),
        ),
    );
    assert!(reopened.pending_preview.is_none());
    assert!(!logical_output.exists());
    let report = reopened.run_report.as_mut().expect("actual preview report");
    assert_eq!(report.report.outputs.len(), 1);
    let crate::run_report::OutputPreview::Text { content, .. } = report.report.outputs[0].preview()
    else {
        panic!("JSON preview");
    };
    retained.record("complete-preview-bytes", content.as_bytes());
    assert_eq!(content.as_str(), CITY_ADA);
    assert_eq!(std::fs::read(&path)?, project_bytes);
    assert_eq!(std::fs::read(&layout_path)?, layout_bytes);
    retained.complete = true;
    Ok(())
}

#[test]
fn computed_property_real_selections_keep_key_value_error_order_and_null_collisions() {
    let mut retained = Retained::new();
    let (mut app, context) = setup(&retained, true, MappingDocument::Main);
    let duplicate =
        r#"{"KeyA":"display_name","ValueA":"Ada","KeyB":"display_name","ValueB":"Oslo"}"#;
    assert!(
        matches!(failure(&app, &retained, duplicate), engine::EngineError::DuplicateDynamicProperty(name) if name == "display_name")
    );
    let fixed = r#"{"KeyA":"fixed","ValueA":"Ada","KeyB":"city","ValueB":"Oslo"}"#;
    assert!(
        matches!(failure(&app, &retained, fixed), engine::EngineError::DuplicateDynamicProperty(name) if name == "fixed")
    );
    app.project.root.bindings.clear();
    raw_project(&app, &retained, "unwritten-fixed-still-reserved");
    assert!(
        matches!(failure(&app, &retained, fixed), engine::EngineError::DuplicateDynamicProperty(name) if name == "fixed")
    );
    app.project.root.bindings.push(Binding {
        target_field: "fixed".into(),
        node: 0,
    });
    app.mark_clean();
    app.rebase_history();
    choose(&mut app, &context, &retained, 0, false, 7, "7: divide");
    choose(
        &mut app,
        &context,
        &retained,
        0,
        true,
        5,
        "5: constant Int(7)",
    );
    assert!(matches!(
        failure(&app, &retained, INPUT),
        engine::EngineError::DynamicPropertyName {
            node: 5,
            found: "int"
        }
    ));
    choose(
        &mut app,
        &context,
        &retained,
        0,
        true,
        6,
        "6: constant Null",
    );
    assert!(matches!(
        failure(&app, &retained, INPUT),
        engine::EngineError::DynamicPropertyName {
            node: 6,
            found: "null"
        }
    ));
    choose(&mut app, &context, &retained, 0, true, 1, "1: field KeyA");
    assert!(matches!(
        failure(&app, &retained, fixed),
        engine::EngineError::Function(functions::FunctionError::DivideByZero)
    ));
    choose(
        &mut app,
        &context,
        &retained,
        0,
        false,
        6,
        "6: constant Null",
    );
    assert!(
        matches!(failure(&app, &retained, duplicate), engine::EngineError::DuplicateDynamicProperty(name) if name == "display_name")
    );
    outputs(
        &app,
        &retained,
        &expected(&[
            ("display_name", Value::Null),
            ("city", Value::String("Oslo".into())),
        ]),
        CITY,
        &full(),
        FULL,
    );
    choose(
        &mut app,
        &context,
        &retained,
        0,
        false,
        2,
        "2: field ValueA",
    );
    let empty_name = r#"{"KeyA":"","ValueA":"Ada","KeyB":"city","ValueB":"Oslo"}"#;
    let source = input(&app, &retained, empty_name);
    let actual = engine::run(&app.project, &source);
    retained.record("empty-property-name-result", &actual);
    let actual = actual.unwrap();
    let written = format_json::to_string(&app.project.target, &actual);
    retained.record("empty-property-name-writer", &written);
    assert_eq!(
        actual,
        expected(&[
            ("", Value::String("Ada".into())),
            ("city", Value::String("Oslo".into()))
        ])
    );
    assert_eq!(
        written.unwrap(),
        "{\n  \"fixed\": \"kept\",\n  \"\": \"Ada\",\n  \"city\": \"Oslo\"\n}\n"
    );
    retained.complete = true;
}

#[test]
fn computed_properties_real_preview_lock_and_imported_unsupported_states_stay_exact() {
    let mut retained = Retained::new();
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let (mut app, context) = setup(&retained, true, document);
        app.begin_preview();
        assert!(app.preview_draft.is_some() && !app.ui_project_editing_enabled());
        let before = raw_project(&app, &retained, "locked-original");
        let canvas = canvas_state(&app);
        click_label(&mut app, &context, &retained, "+ property");
        click_label(&mut app, &context, &retained, "Remove property 1");
        let output = settle(&mut app, &context, &retained);
        for caption in ["Name expression", "Value expression"] {
            let points = visible(&output, caption);
            retained.record("disabled-expression-captions", (&caption, &points));
            assert_eq!(points.len(), 2);
            let point = points[0];
            let buttons = texts(&output)
                .into_iter()
                .filter_map(|(label, p)| {
                    ((p.y - point.y).abs() < 4.0 && p.x > point.x && label.contains(": "))
                        .then_some(p)
                })
                .collect::<Vec<_>>();
            assert_eq!(buttons.len(), 1);
            click(&mut app, &context, &retained, buttons[0]);
        }
        assert_eq!(
            raw_project(&app, &retained, "locked-after-real-clicks"),
            before
        );
        assert_eq!(canvas_state(&app), canvas);
        assert!(
            !app.is_dirty()
                && !app.can_undo()
                && !app.history.can_redo()
                && app.pending_history.is_none()
        );
        outputs(&app, &retained, &full(), FULL, &full(), FULL);
    }
    let (mut app, context) = setup(&retained, true, MappingDocument::Main);
    app.project.graph.nodes.insert(
        11,
        Node::SourceField {
            path: vec!["Unknown".into()],
            frame: Some(Vec::new()),
        },
    );
    app.project.graph.nodes.insert(
        12,
        Node::SourceField {
            path: vec!["ValueA".into()],
            frame: Some(vec!["MissingFrame".into()]),
        },
    );
    app.project.root.dynamic_bindings = vec![
        DynamicBinding { key: 99, value: 11 },
        DynamicBinding { key: 12, value: 2 },
    ];
    app.project.root.dynamic_children = vec![DynamicChild {
        key: 3,
        scope: Scope::default(),
    }];
    app.project.root.merge_dynamic_fields = true;
    app.mark_clean();
    app.rebase_history();
    let before = raw_project(&app, &retained, "imported-complex-original");
    let canvas = canvas_state(&app);
    let issues = cli::validate(&app.project);
    retained.record("full-imported-validation", &issues);
    assert!(!issues.is_empty());
    let output = settle(&mut app, &context, &retained);
    assert_eq!(
        visible(&output, "Saved name expression #99 is missing.").len(),
        1
    );
    click_label(&mut app, &context, &retained, "+ property");
    click_label(&mut app, &context, &retained, "Remove property 1");
    assert_eq!(
        raw_project(&app, &retained, "imported-complex-after-inspection"),
        before
    );
    assert_eq!(canvas_state(&app), canvas);
    assert!(!app.is_dirty() && !app.can_undo());
    retained.complete = true;
}

#[test]
fn computed_property_profiles_reject_other_domains_owners_and_same_frame_controls_without_repair() {
    let mut retained = Retained::new();
    let (app, _) = setup(&retained, true, MappingDocument::Main);
    let mut cases = Vec::new();
    let mut closed = app.project.clone();
    assert!(closed.target.set_dynamic_fields(None));
    cases.push(("closed", closed));
    let mut repeating = app.project.clone();
    repeating.target.repeating = true;
    cases.push(("repeating-target", repeating));
    for (label, dynamic) in [
        ("group-value", SchemaNode::group("value", Vec::new())),
        (
            "repeated-value",
            SchemaNode::scalar("value", ScalarType::String).repeating(),
        ),
        (
            "union-value",
            SchemaNode::scalar_union(
                "value",
                ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap(),
            ),
        ),
    ] {
        let mut project = app.project.clone();
        assert!(project.target.set_dynamic_fields(Some(dynamic)));
        cases.push((label, project));
    }
    let mut arbitrary = app.project.clone();
    let mut value = SchemaNode::scalar("value", ScalarType::String);
    value.json_any = true;
    assert!(arbitrary.target.set_dynamic_fields(Some(value)));
    cases.push(("arbitrary-json", arbitrary));
    let mut xml = app.project.clone();
    xml.target_options.xml_document = true;
    cases.push(("conflicting-adapter", xml));
    let mut source_iteration = app.project.clone();
    source_iteration.root.iteration = ScopeIteration::Source(Vec::new());
    cases.push(("iteration", source_iteration));
    let mut controlled = app.project.clone();
    controlled.root.filter = Some(5);
    cases.push(("filter", controlled));
    let mut copy = app.project.clone();
    copy.root.construction = ScopeConstruction::CopyCurrentSource;
    cases.push(("whole-copy", copy));
    for (label, project) in cases {
        let profile = computed_properties::profile(
            &project.root,
            &project.target,
            &[],
            &project.target_options,
            true,
        );
        retained.record("full-refused-profile", (&label, &project, profile));
        assert!(matches!(profile, Profile::ReadOnly(_)), "{label}");
        let mut candidate = FerruleApp {
            project,
            ..Default::default()
        };
        candidate.main_canvas = CanvasDocumentState::main(&candidate.project);
        candidate.mark_clean();
        candidate.rebase_history();
        let context = egui::Context::default();
        crate::icons::install(&context);
        let before = raw_project(&candidate, &retained, "refused-original");
        click_label(&mut candidate, &context, &retained, "+ property");
        click_label(&mut candidate, &context, &retained, "Remove property 1");
        assert_eq!(raw_project(&candidate, &retained, "refused-after"), before);
        assert!(!candidate.is_dirty() && !candidate.can_undo());
    }
    for document in [
        MappingDocument::Function(FunctionId::new(77)),
        MappingDocument::Main,
    ] {
        let (mut candidate, context) = setup(&retained, true, MappingDocument::Main);
        candidate.mapping_workspace.active = document;
        if document == MappingDocument::Main {
            candidate.embedded_stage_namespace = Some(egui::Id::new("test-embedded-stage"));
        }
        // The selected document is part of the saved layout, so establish this
        // read-only fixture's baseline after its route has actually rendered.
        settle(&mut candidate, &context, &retained);
        candidate.mark_clean();
        candidate.rebase_history();
        retained.record(
            "unavailable-document-initialized-fixture",
            (
                &candidate.project,
                CanvasLayout::capture(
                    &candidate.project,
                    &candidate.main_canvas.snarl,
                    &candidate.mapping_workspace,
                ),
                canvas_state(&candidate),
                candidate.is_dirty(),
                candidate.can_undo(),
                candidate.history.can_redo(),
            ),
        );
        assert!(!candidate.is_dirty() && !candidate.can_undo());
        let before = raw_project(&candidate, &retained, "unavailable-document-original");
        click_label(&mut candidate, &context, &retained, "+ property");
        click_label(&mut candidate, &context, &retained, "Remove property 1");
        assert_eq!(
            raw_project(&candidate, &retained, "unavailable-document-after"),
            before
        );
        assert!(!candidate.is_dirty() && !candidate.can_undo());
    }
    let missing = computed_properties::profile(
        &app.project.root,
        &app.project.target,
        &[99],
        &app.project.target_options,
        true,
    );
    retained.record("missing-owner-route", missing);
    assert!(matches!(missing, Profile::ReadOnly(_)));
    // A previously editable profile cannot permit a write after a preceding
    // scope control has changed that same owner during this UI frame.
    let mut edited = app.project.root.clone();
    edited.iteration = ScopeIteration::Source(Vec::new());
    let before = serde_json::to_string(&edited).unwrap();
    let context = egui::Context::default();
    let mut last = None;
    for _ in 0..4 {
        last = Some(context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1400.0),
                )),
                ..Default::default()
            },
            |ui| {
                computed_properties::show(
                    ui,
                    &mut edited,
                    &app.project.graph,
                    Profile::ScalarObject(ScalarType::String),
                )
            },
        ));
    }
    let points = visible(last.as_ref().unwrap(), "+ property");
    retained.record(
        "same-frame-disabled-control",
        (&edited, last.as_ref().map(|output| &output.shapes), &points),
    );
    assert_eq!(points.len(), 1);
    for pressed in [true, false] {
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1400.0),
                )),
                events: vec![
                    egui::Event::PointerMoved(points[0]),
                    egui::Event::PointerButton {
                        pos: points[0],
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ui| {
                computed_properties::show(
                    ui,
                    &mut edited,
                    &app.project.graph,
                    Profile::ScalarObject(ScalarType::String),
                )
            },
        );
        retained.record("same-frame-original-widget-shapes", &output.shapes);
    }
    assert_eq!(serde_json::to_string(&edited).unwrap(), before);
    retained.complete = true;
}

#[test]
fn computed_property_singular_scalar_descendants_and_maximum_ids_use_existing_expressions_only() {
    let mut retained = Retained::new();
    for (ty, value, literal) in [
        (
            ScalarType::Int,
            Value::Int(7),
            "{\n  \"fixed\": \"kept\",\n  \"display_name\": 7\n}\n",
        ),
        (
            ScalarType::Float,
            Value::Float(2.5),
            "{\n  \"fixed\": \"kept\",\n  \"display_name\": 2.5\n}\n",
        ),
        (
            ScalarType::Bool,
            Value::Bool(true),
            "{\n  \"fixed\": \"kept\",\n  \"display_name\": true\n}\n",
        ),
    ] {
        let (mut app, context) = setup(&retained, false, MappingDocument::Main);
        assert!(
            app.project
                .target
                .set_dynamic_fields(Some(SchemaNode::scalar("value", ty)))
        );
        app.project.graph.nodes.insert(
            5,
            Node::Const {
                value: value.clone(),
            },
        );
        app.main_canvas = CanvasDocumentState::main(&app.project);
        app.mark_clean();
        app.rebase_history();
        let ids = app.project.graph.nodes.keys().copied().collect::<Vec<_>>();
        click_label(&mut app, &context, &retained, "+ property");
        choose(&mut app, &context, &retained, 0, true, 1, "1: field KeyA");
        let label = format!("5: constant {value:?}");
        choose(&mut app, &context, &retained, 0, false, 5, &label);
        let wanted = expected(&[("display_name", value)]);
        outputs(&app, &retained, &wanted, literal, &expected(&[]), FIXED);
        assert_eq!(
            app.project.graph.nodes.keys().copied().collect::<Vec<_>>(),
            ids
        );
        if ty == ScalarType::Int {
            choose(
                &mut app,
                &context,
                &retained,
                0,
                false,
                2,
                "2: field ValueA",
            );
            let source = input(&app, &retained, INPUT);
            let actual = engine::run(&app.project, &source);
            retained.record("wrong-value-domain-full-engine-result", &actual);
            let actual = actual.unwrap();
            let written = format_json::to_string(&app.project.target, &actual);
            retained.record("wrong-value-domain-full-writer-result", &written);
            assert_eq!(
                actual,
                expected(&[("display_name", Value::String("Ada".into()))])
            );
            assert!(
                matches!(written, Err(format_json::JsonFormatError::Shape { name, expected: "integer", got: "string" }) if name == "value")
            );
        }
    }
    let (mut app, context) = setup(&retained, false, MappingDocument::Main);
    let object = app.project.target.clone();
    app.project.target = SchemaNode::group("Envelope", vec![object]);
    app.project.root = Scope {
        children: vec![Scope {
            target_field: "Target".into(),
            bindings: vec![Binding {
                target_field: "fixed".into(),
                node: 0,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    app.selected_scope.clear();
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    let before_graph = serde_json::to_string(&app.project.graph).unwrap();
    let output = settle(&mut app, &context, &retained);
    let headings = visible(&output, "Scopes");
    retained.record("actual-scope-tree-heading", &headings);
    assert_eq!(headings.len(), 1);
    let scopes = visible(&output, "Target")
        .into_iter()
        .filter(|point| point.y > headings[0].y && point.y < headings[0].y + 200.0)
        .collect::<Vec<_>>();
    retained.record("actual-static-child-scope-row", &scopes);
    assert_eq!(scopes.len(), 1);
    click(&mut app, &context, &retained, scopes[0]);
    assert_eq!(app.selected_scope, [0]);
    add_two(&mut app, &context, &retained);
    let source = input(&app, &retained, INPUT);
    let actual = engine::run(&app.project, &source);
    retained.record("full-descendant-engine-result", &actual);
    let actual = actual.unwrap();
    let written = format_json::to_string(&app.project.target, &actual);
    retained.record("full-descendant-writer-result", &written);
    assert_eq!(
        actual,
        Instance::Group(vec![("Target".into(), full())].into())
    );
    assert_eq!(
        written.unwrap(),
        "{\n  \"Target\": {\n    \"fixed\": \"kept\",\n    \"display_name\": \"Ada\",\n    \"city\": \"Oslo\"\n  }\n}\n"
    );
    assert_eq!(
        serde_json::to_string(&app.project.graph).unwrap(),
        before_graph
    );
    let (mut maximum, context) = setup(&retained, false, MappingDocument::Main);
    maximum.project.graph.nodes = [(
        NodeId::MAX,
        Node::Const {
            value: Value::String("last".into()),
        },
    )]
    .into_iter()
    .collect();
    maximum.project.root.bindings[0].node = NodeId::MAX;
    maximum.project.extra_targets[0].root.bindings[0].node = NodeId::MAX;
    maximum.main_canvas = CanvasDocumentState::main(&maximum.project);
    maximum.mapping_workspace.target_canvases.clear();
    assert!(maximum.ensure_target_canvas(0));
    maximum.mark_clean();
    maximum.rebase_history();
    let graph = serde_json::to_string(&maximum.project.graph).unwrap();
    click_label(&mut maximum, &context, &retained, "+ property");
    raw_project(&maximum, &retained, "maximum-id-authored-original");
    assert_eq!(
        (
            active_scope(&maximum).dynamic_bindings[0].key,
            active_scope(&maximum).dynamic_bindings[0].value
        ),
        (NodeId::MAX, NodeId::MAX)
    );
    assert_eq!(
        serde_json::to_string(&maximum.project.graph).unwrap(),
        graph
    );
    let source = input(&maximum, &retained, INPUT);
    let actual = engine::run(&maximum.project, &source);
    retained.record("maximum-id-full-engine-result", &actual);
    assert_eq!(
        actual.unwrap(),
        Instance::Group(
            vec![
                (
                    "fixed".into(),
                    Instance::Scalar(Value::String("last".into()))
                ),
                (
                    "last".into(),
                    Instance::Scalar(Value::String("last".into()))
                )
            ]
            .into()
        )
    );
    let (mut empty, context) = setup(&retained, false, MappingDocument::Main);
    empty.project.graph.nodes.clear();
    empty.project.root.bindings.clear();
    empty.project.extra_targets[0].root.bindings.clear();
    empty.main_canvas = CanvasDocumentState::main(&empty.project);
    empty.mapping_workspace.target_canvases.clear();
    assert!(empty.ensure_target_canvas(0));
    empty.mark_clean();
    empty.rebase_history();
    let before = raw_project(&empty, &retained, "empty-graph-before");
    click_label(&mut empty, &context, &retained, "+ property");
    assert_eq!(raw_project(&empty, &retained, "empty-graph-after"), before);
    assert!(!empty.is_dirty() && !empty.can_undo());
    retained.complete = true;
}
