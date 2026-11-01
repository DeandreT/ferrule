//! Direct numeric reducers share the existing owner-local editor and runtime.
use super::*;

const REDUCERS: [(AggregateOp, &str); 3] = [
    (AggregateOp::Avg, "Average"),
    (AggregateOp::Min, "Minimum"),
    (AggregateOp::Max, "Maximum"),
];

fn numeric_target(app: &mut FerruleApp) {
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar_union(
            "Result",
            ir::ScalarTypeSet::new([ScalarType::Int, ScalarType::Float]).unwrap(),
        )],
    );
    app.project.extra_targets[0].schema = app.project.target.clone();
}

fn expected(op: AggregateOp, avg: f64, min: Value, max: Value) -> Value {
    match op {
        AggregateOp::Avg => Value::Float(avg),
        AggregateOp::Min => min,
        AggregateOp::Max => max,
        _ => panic!("direct reducer"),
    }
}

#[test]
fn operation_and_owner_controls_preserve_paths_wires_ids_and_typed_history() -> anyhow::Result<()> {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = aggregate_fixture(document);
        numeric_target(&mut app);
        rebuild(&mut app, document);
        let ctx = context();
        settle_all(&mut app, &ctx);
        app.mark_clean();
        app.rebase_history();
        let before = snapshot(&app);
        let wires = observed(&app)["wires"].clone();
        let mut current = "Count";
        for (op, label) in REDUCERS {
            choose(&mut app, &ctx, current, label);
            assert_eq!(aggregate(&app).function, op);
            assert_eq!(aggregate(&app).collection.path, ["Rows"]);
            if current == "Count" {
                choose(&mut app, &ctx, "Other", "PrimaryAmount");
            }
            assert_eq!(
                aggregate(&app).value,
                OwnedAggregateValue::Path(vec!["PrimaryAmount".into()])
            );
            assert_complete_public_result(
                &app,
                INPUT,
                NAMED,
                expected(op, 6.5, Value::Int(4), Value::Int(9)),
            )?;
            choose(&mut app, &ctx, "Primary input", "Named input: Ref");
            assert_eq!(
                aggregate(&app).value,
                OwnedAggregateValue::Path(vec!["NamedAmount".into()])
            );
            assert_complete_public_result(
                &app,
                INPUT,
                NAMED,
                expected(op, 60.0, Value::Int(30), Value::Int(90)),
            )?;
            // A completely unsupplied named endpoint is an error, never a
            // fallback to the populated primary endpoint with the same path.
            assert_eq!(
                engine::run_outputs_with_sources_and_context(
                    &app.project,
                    &format_json::from_str(INPUT, &app.project.source)?,
                    vec![],
                    &engine::ExecutionContext::new(Path::new("missing-owner.json")),
                )
                .unwrap_err(),
                engine::EngineError::OwnedSourceAccess {
                    node: Some(7),
                    scope: None,
                    source: engine::OwnedSourceError::SourceUnavailable {
                        owner: SourceOwner::Named { name: "Ref".into() },
                    },
                },
            );
            choose(&mut app, &ctx, "NamedAmount", "Other");
            choose(&mut app, &ctx, "Named input: Ref", "Primary input");
            assert_eq!(
                aggregate(&app).value,
                OwnedAggregateValue::Path(vec!["Other".into()])
            );
            assert_complete_public_result(
                &app,
                INPUT,
                NAMED,
                expected(op, 15.0, Value::Int(10), Value::Int(20)),
            )?;
            choose(&mut app, &ctx, "Other", "PrimaryAmount");
            assert!(aggregate(&app).arg.is_none());
            assert_typed_edit(&app);
            assert_eq!(observed(&app)["wires"], wires);
            current = label;
        }
        // Count deliberately drops the value; returning to a reducer selects
        // the first valid field without changing collection or ownership.
        choose(&mut app, &ctx, "Maximum", "Count");
        assert_eq!(aggregate(&app).value, OwnedAggregateValue::Path(vec![]));
        choose(&mut app, &ctx, "Rows", "Empty");
        choose(&mut app, &ctx, "Count", "Average");
        assert_eq!(aggregate(&app).collection.path, ["Rows"]);
        assert_eq!(
            aggregate(&app).value,
            OwnedAggregateValue::Path(vec!["Other".into()])
        );
        let after = snapshot(&app);
        assert_eq!(before["layout"], after["layout"]);
        let edits = app.history.undo_len();
        for _ in 0..edits {
            app.undo_project();
        }
        assert_eq!(snapshot(&app), before);
        assert_eq!(observed(&app)["wires"], wires);
        for _ in 0..edits {
            app.redo_project();
        }
        assert_eq!(snapshot(&app), after);
        assert_eq!(observed(&app)["wires"], wires);
    }
    Ok(())
}

fn scalar_fixture(document: MappingDocument) -> FerruleApp {
    let mut app = aggregate_fixture(document);
    let source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("FloatAmount", ScalarType::Float),
                    SchemaNode::scalar_union(
                        "MixedAmount",
                        ir::ScalarTypeSet::new([
                            ScalarType::Int,
                            ScalarType::Float,
                            ScalarType::String,
                            ScalarType::Bool,
                        ])
                        .unwrap(),
                    ),
                    SchemaNode::scalar("TextAmount", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    app.project.source = source.clone();
    app.project.extra_sources[0].schema = source;
    numeric_target(&mut app);
    rebuild(&mut app, document);
    app
}

#[test]
fn direct_reducers_offer_float_union_and_numeric_string_fields_and_native_empty_results()
-> anyhow::Result<()> {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        for (op, label) in REDUCERS {
            let mut app = scalar_fixture(document);
            let ctx = context();
            choose(&mut app, &ctx, "Count", label);
            for (field, values, result) in [
                (
                    "FloatAmount",
                    vec![serde_json::json!(1.25), serde_json::json!(3.75)],
                    expected(op, 2.5, Value::Float(1.25), Value::Float(3.75)),
                ),
                (
                    "MixedAmount",
                    vec![
                        serde_json::json!(2),
                        serde_json::json!(" 4 "),
                        serde_json::json!(1.5),
                        serde_json::json!(true),
                        serde_json::json!("skip"),
                    ],
                    expected(op, 2.5, Value::Float(1.5), Value::Int(4)),
                ),
                (
                    "TextAmount",
                    vec![
                        serde_json::json!("1.25"),
                        serde_json::json!("2.5"),
                        serde_json::json!(" 3.75 "),
                        serde_json::json!("skip"),
                        serde_json::json!(""),
                    ],
                    expected(op, 2.5, Value::Float(1.25), Value::Float(3.75)),
                ),
            ] {
                let OwnedAggregateValue::Path(path) = &aggregate(&app).value else {
                    panic!("path")
                };
                let current = path.join(" / ");
                if current != field {
                    choose(&mut app, &ctx, &current, field);
                }
                assert_eq!(aggregate(&app).function, op);
                let input = sum_rows(field, values);
                let other = sum_rows(field, vec![serde_json::json!("777")]);
                // Float schemas cannot read the other owner's string fixture.
                let other = if field == "FloatAmount" {
                    sum_rows(field, vec![serde_json::json!(777.0)])
                } else {
                    other
                };
                assert_complete_public_result(&app, &input, &other, result.clone())?;
                for empty in [r#"{"Rows":[]}"#, "{}"] {
                    assert_complete_public_result(&app, empty, &other, Value::Null)?;
                }
                if field != "FloatAmount" {
                    let nonnumeric = sum_rows(
                        field,
                        vec![serde_json::json!("skip"), serde_json::json!("")],
                    );
                    assert_complete_public_result(&app, &nonnumeric, &other, Value::Null)?;
                }
                choose(&mut app, &ctx, "Primary input", "Named input: Ref");
                assert_complete_public_result(&app, &other, &input, result)?;
                for empty in [r#"{"Rows":[]}"#, "{}"] {
                    assert_complete_public_result(&app, &other, empty, Value::Null)?;
                }
                choose(&mut app, &ctx, "Named input: Ref", "Primary input");
            }
        }
    }
    Ok(())
}

#[test]
fn exact_mixed_extrema_and_large_finite_average_remain_native() -> anyhow::Result<()> {
    for (op, label) in REDUCERS {
        let mut app = scalar_fixture(MappingDocument::Main);
        let ctx = context();
        choose(&mut app, &ctx, "Count", label);
        choose(&mut app, &ctx, "FloatAmount", "MixedAmount");
        let input = sum_rows(
            "MixedAmount",
            vec![
                serde_json::json!(9_007_199_254_740_993_i64),
                serde_json::json!(9_007_199_254_740_992.0),
            ],
        );
        assert_complete_public_result(
            &app,
            &input,
            "{}",
            expected(
                op,
                9_007_199_254_740_992.0,
                Value::Float(9_007_199_254_740_992.0),
                Value::Int(9_007_199_254_740_993),
            ),
        )?;
        let input = sum_rows(
            "MixedAmount",
            vec![serde_json::json!(f64::MAX), serde_json::json!(f64::MAX)],
        );
        assert_complete_public_result(&app, &input, "{}", Value::Float(f64::MAX))?;
    }
    Ok(())
}

#[test]
fn direct_reducers_keep_nonfinite_and_decimal_failures() {
    for (op, label) in REDUCERS {
        let mut app = scalar_fixture(MappingDocument::Main);
        let ctx = context();
        choose(&mut app, &ctx, "Count", label);
        for (value, error) in [
            (
                Value::Float(f64::INFINITY),
                engine::EngineError::AggregateNonFinite { function: op },
            ),
            (
                Value::String("NaN".into()),
                engine::EngineError::AggregateNonFinite { function: op },
            ),
            (
                Value::String("1e999".into()),
                engine::EngineError::AggregateNonFinite { function: op },
            ),
            (
                Value::decimal(ir::Decimal96::parse_exact("0.1").unwrap()),
                engine::EngineError::AggregateUnsupportedScalar {
                    function: op,
                    found: "decimal",
                },
            ),
        ] {
            let input = ir::Instance::Group(
                vec![(
                    "Rows".into(),
                    ir::Instance::Repeated(vec![ir::Instance::Group(
                        vec![("FloatAmount".into(), ir::Instance::Scalar(value))].into(),
                    )]),
                )]
                .into(),
            );
            assert_eq!(
                engine::run_outputs_with_sources_and_context(
                    &app.project,
                    &input,
                    vec![],
                    &engine::ExecutionContext::new(Path::new("direct.json"))
                )
                .unwrap_err(),
                error
            );
        }
    }
}

#[test]
fn saved_direct_forms_never_normalize_on_inspection_or_bypass_exclusions() {
    for (op, _) in REDUCERS {
        for case in [
            "expression",
            "argument",
            "missing-owner",
            "missing-path",
            "nested-path",
            "locked",
        ] {
            let mut app = aggregate_fixture(MappingDocument::Main);
            let mut node = match owned(op, SourceOwner::Primary, "Rows", &["PrimaryAmount"]) {
                Node::OwnedAggregate(node) => node,
                _ => unreachable!(),
            };
            match case {
                "expression" => node.value = OwnedAggregateValue::Expression(3),
                "argument" => node.arg = Some(3),
                "missing-owner" => {
                    node.collection.owner = SourceOwner::Named {
                        name: "Missing".into(),
                    }
                }
                "missing-path" => node.collection.path = vec!["SavedMissing".into()],
                "nested-path" => {
                    node.value = OwnedAggregateValue::Path(vec!["Nested".into(), "Deep".into()])
                }
                _ => (),
            }
            app.project
                .graph
                .nodes
                .insert(7, Node::OwnedAggregate(node));
            rebuild(&mut app, MappingDocument::Main);
            let before = observed(&app);
            let ctx = context();
            if case == "locked" {
                settle(&mut app, &ctx, false);
            } else {
                open_lookup(&mut app, &ctx);
            }
            assert_eq!(observed(&app), before, "{op:?}: {case}");
            if ["expression", "argument", "missing-owner"].contains(&case) {
                assert!(
                    wire_action(
                        &mut app,
                        CanvasNode::Graph(7),
                        0,
                        CanvasNode::TargetBlock(0),
                        0,
                        false
                    )
                    .is_err()
                );
                assert_eq!(observed(&app), before);
            }
            // Invalid saved paths are displayed verbatim. Only choosing a
            // valid replacement in the existing picker changes their metadata.
            if case == "missing-path" {
                choose(&mut app, &ctx, "SavedMissing", "Rows");
                assert_eq!(aggregate(&app).collection.path, ["Rows"]);
                assert_eq!(
                    aggregate(&app).value,
                    OwnedAggregateValue::Path(vec!["PrimaryAmount".into()])
                );
            }
            if case == "nested-path" {
                choose(&mut app, &ctx, "Nested / Deep", "PrimaryAmount");
                assert_eq!(
                    aggregate(&app).value,
                    OwnedAggregateValue::Path(vec!["PrimaryAmount".into()])
                );
            }
        }
    }
}

#[test]
fn legacy_numeric_reducers_convert_only_after_explicit_selection() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        for (op, _) in REDUCERS {
            let mut app = aggregate_fixture(document);
            app.project.graph.nodes.insert(
                7,
                Node::Aggregate {
                    function: op,
                    collection: vec!["Saved".into()],
                    value: vec!["Retained".into()],
                    expression: None,
                    arg: None,
                },
            );
            rebuild(&mut app, document);
            let ctx = context();
            let before = snapshot(&app);
            let title = visible(
                &settle(&mut app, &ctx, true),
                &crate::graph_viewer::readable_node_label(&app.project.graph.nodes[&7]),
            )[0];
            frame(&mut app, &ctx, true, vec![egui::Event::PointerMoved(title)]);
            open_lookup(&mut app, &ctx);
            assert_eq!(snapshot(&app), before);
            click_nth(&mut app, &ctx, true, "Use an exact input", 0);
            assert_eq!(aggregate(&app).function, op);
            assert!(aggregate(&app).arg.is_none());
            assert_eq!(aggregate(&app).collection.owner, SourceOwner::Primary);
            assert_eq!(aggregate(&app).collection.path, ["Rows"]);
            assert!(matches!(
                app.history.next_undo(),
                Some(HistoryRecord::Command(_))
            ));
            let after = snapshot(&app);
            app.undo_project();
            assert_eq!(snapshot(&app), before);
            app.redo_project();
            assert_eq!(snapshot(&app), after);
        }
    }
}

#[test]
fn every_direct_reducer_roundtrips_and_previews_complete_outputs() -> anyhow::Result<()> {
    let scratch = Scratch::new();
    let ctx = context();
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        for (op, label) in REDUCERS {
            let mut app = aggregate_fixture(document);
            numeric_target(&mut app);
            rebuild(&mut app, document);
            choose(&mut app, &ctx, "Count", label);
            choose(&mut app, &ctx, "Other", "PrimaryAmount");
            choose(&mut app, &ctx, "Primary input", "Named input: Ref");
            for (named, expected) in [
                (NAMED, expected(op, 60.0, Value::Int(30), Value::Int(90))),
                (r#"{"Rows":[]}"#, Value::Null),
            ] {
                app.project.extra_sources[0].path =
                    scratch.0.join("named.json").display().to_string();
                app.project.extra_targets[0].path =
                    Some(scratch.0.join("audit.json").display().to_string());
                std::fs::write(scratch.0.join("named.json"), named)?;
                let path = scratch.0.join("project.json");
                let before = snapshot(&app);
                let saved = app.save_document_to(&path)?;
                assert!(
                    saved.validation_issues.is_empty(),
                    "{:?}",
                    saved.validation_issues
                );
                assert!(saved.layout_warning.is_none());
                let mut reopened = FerruleApp::default();
                reopened.load_project_from(&path);
                reopened.open_target_tab(0);
                assert!(reopened.ensure_target_canvas(0));
                reopened.mapping_workspace.active = document;
                reopened.mapping_workspace.focused = document;
                assert_eq!(snapshot(&reopened), before);
                assert_complete_public_result(&reopened, INPUT, named, expected.clone())?;
                let expected_json = match expected {
                    Value::Int(value) => serde_json::json!({"Result": value}),
                    Value::Float(value) => serde_json::json!({"Result": value}),
                    Value::Null => serde_json::json!({}),
                    _ => panic!("numeric result"),
                };
                for target in [
                    crate::preview::PreviewTarget::Primary,
                    crate::preview::PreviewTarget::Named("Audit".into()),
                ] {
                    reopened.preview_draft = Some(crate::preview::PreviewDraft {
                        target,
                        input_identity: "input.json".into(),
                        output_identity: scratch.0.join("not-written.json").display().to_string(),
                        input_text: INPUT.into(),
                        debug_breakpoint: None,
                    });
                    reopened.execute_preview();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
                    while reopened.pending_preview.is_some() && std::time::Instant::now() < deadline
                    {
                        reopened.poll_preview(&ctx);
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    assert!(reopened.pending_preview.is_none());
                    let report = reopened.run_report.as_mut().expect("preview report");
                    assert_eq!(report.report.outputs.len(), 1, "{:?}", report.report);
                    for output in &mut report.report.outputs {
                        let crate::run_report::OutputPreview::Text { content, .. } =
                            output.preview()
                        else {
                            panic!("JSON preview")
                        };
                        assert_eq!(
                            serde_json::from_str::<serde_json::Value>(content)?,
                            expected_json
                        );
                    }
                    assert!(!scratch.0.join("not-written.json").exists());
                }
            }
        }
    }
    Ok(())
}
