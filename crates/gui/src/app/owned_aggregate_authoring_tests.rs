//! Self-authored inputs exercised through real widgets and public execution.
use super::owned_lookup_authoring_tests::{self as support, *};
use super::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, OwnedAggregate, OwnedAggregateValue, OwnedSourceRef, SourceOwner};
use support::context_regressions::{connection_fixture, observed};
use support::scope_regressions::{click_label, close, label, root, root_mut, settle_inspector};

const INPUT: &str = r#"{"Rows":[{"PrimaryAmount":4,"Other":10},{"PrimaryAmount":9,"Other":20}],"Empty":[{},{}],"Ref":{"Rows":[{"PrimaryAmount":1000}]}}"#;
const NAMED: &str = r#"{"Rows":[{"NamedAmount":30,"Other":50},{"NamedAmount":60,"Other":70},{"NamedAmount":90,"Other":80}],"Empty":[]}"#;

fn owned(op: AggregateOp, owner: SourceOwner, collection: &str, value: &[&str]) -> Node {
    Node::OwnedAggregate(OwnedAggregate {
        function: op,
        collection: OwnedSourceRef {
            owner,
            path: vec![collection.into()],
        },
        value: OwnedAggregateValue::Path(value.iter().map(|s| (*s).into()).collect()),
        arg: None,
    })
}

fn schema(amount: &str) -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar(amount, ScalarType::Int),
                    SchemaNode::scalar("Other", ScalarType::Int),
                    SchemaNode::group("Nested", vec![SchemaNode::scalar("Deep", ScalarType::Int)])
                        .repeating(),
                ],
            )
            .repeating(),
            SchemaNode::group("Empty", vec![]).repeating(),
        ],
    )
}

fn aggregate_fixture(document: MappingDocument) -> FerruleApp {
    let mut app = support::fixture(document);
    app.project.source = schema("PrimaryAmount");
    if let ir::SchemaKind::Group { children, .. } = &mut app.project.source.kind {
        children.push(SchemaNode::group(
            "Ref",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![SchemaNode::scalar("PrimaryAmount", ScalarType::Int)],
                )
                .repeating(),
            ],
        ));
    }
    app.project.extra_sources[0].schema = schema("NamedAmount");
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Result", ScalarType::Int)],
    );
    app.project.extra_targets[0].schema = app.project.target.clone();
    app.project.graph.nodes.insert(
        7,
        owned(AggregateOp::Count, SourceOwner::Primary, "Rows", &[]),
    );
    app.project.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Int(2),
        },
    );
    rebuild(&mut app, document);
    app
}

fn aggregate(app: &FerruleApp) -> &OwnedAggregate {
    let Node::OwnedAggregate(value) = &app.project.graph.nodes[&7] else {
        panic!("aggregate")
    };
    value
}

fn connection(kind: &str) -> FerruleApp {
    if ["generated", "concatenate", "dynamic", "presence"].contains(&kind) {
        return connection_fixture(kind);
    }
    let mut app = connection_fixture("generated");
    if kind != "generated" {
        app.project.root.children[1].iteration =
            mapping::ScopeIteration::Source(vec!["Rows".into()]);
    }
    if kind == "grouped" {
        app.project.root.children[1].group_by = Some(3);
    }
    if kind == "grouped-descendant" {
        app.project.root.group_by = Some(3);
        app.project.root.iteration = mapping::ScopeIteration::Source(vec!["Rows".into()]);
    }
    app.project.extra_targets[0].root = app.project.root.clone();
    app
}

fn choose(app: &mut FerruleApp, ctx: &egui::Context, current: &str, selected: &str) {
    if visible(&settle(app, ctx, true), "Operation").is_empty() {
        open_lookup(app, ctx);
    }
    for text in [current, selected] {
        let output = settle(app, ctx, true);
        let pos = *visible(&output, text)
            .last()
            .unwrap_or_else(|| panic!("missing {text}"));
        pointer(app, ctx, true, pos, egui::PointerButton::Primary);
    }
}

fn execute(app: &FerruleApp, primary: &str, named: &str) -> engine::ExecutionOutputs {
    assert!(
        engine::validate(&app.project).is_empty(),
        "{:?}",
        engine::validate(&app.project)
    );
    engine::run_outputs_with_sources_and_context(
        &app.project,
        &format_json::from_str(primary, &app.project.source).unwrap(),
        vec![(
            "Ref".into(),
            format_json::from_str(named, &app.project.extra_sources[0].schema).unwrap(),
        )],
        &engine::ExecutionContext::new(Path::new("aggregate.json")),
    )
    .unwrap()
}

fn assert_result(app: &FerruleApp, primary: &str, named: &str, value: i64) {
    let result = execute(app, primary, named);
    for output in [&result.primary, &result.extras[0].instance] {
        assert_eq!(
            output.field("Result"),
            Some(&ir::Instance::Scalar(Value::Int(value)))
        );
    }
}

fn assert_complete_public_result(
    app: &FerruleApp,
    primary: &str,
    named: &str,
    value: Value,
) -> anyhow::Result<()> {
    let expected_json = match &value {
        Value::Int(number) => serde_json::json!({"Result": number}),
        Value::Float(number) => serde_json::json!({"Result": number}),
        _ => panic!("numeric aggregate result"),
    };
    let expected = ir::Instance::Group(vec![("Result".into(), ir::Instance::Scalar(value))].into());
    let result = execute(app, primary, named); // Includes engine::validate.
    assert_eq!(result.primary, expected);
    assert_eq!(
        result.extras,
        vec![engine::NamedOutput {
            name: "Audit".into(),
            instance: expected
        }],
    );

    let scratch = Scratch::new();
    let mut project = app.project.clone();
    project.source_path = Some(scratch.0.join("input.json").display().to_string());
    project.target_path = Some(scratch.0.join("output.json").display().to_string());
    project.extra_sources[0].path = scratch.0.join("named.json").display().to_string();
    project.extra_targets[0].path = Some(scratch.0.join("audit.json").display().to_string());
    assert!(engine::validate(&project).is_empty());
    std::fs::write(scratch.0.join("input.json"), primary)?;
    std::fs::write(scratch.0.join("named.json"), named)?;
    let project_path = scratch.0.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec(&project)?)?;
    let run = cli::run_project_with_paths(&project_path, None, None)?;
    assert_eq!(run.artifacts.len(), 2);
    for path in ["output.json", "audit.json"] {
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(scratch.0.join(path))?)?,
            expected_json,
        );
    }
    Ok(())
}

#[test]
fn offered_root_count_handles_group_flat_rows_and_empty_owned_roots() -> anyhow::Result<()> {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        for repeating_schema in [false, true] {
            let mut app = aggregate_fixture(document);
            app.project.source.repeating = repeating_schema;
            app.project.extra_sources[0].schema.repeating = repeating_schema;
            rebuild(&mut app, document);
            let ctx = context();
            choose(&mut app, &ctx, "Rows", "Current rows");
            assert!(aggregate(&app).collection.path.is_empty());
            assert_eq!(aggregate(&app).value, OwnedAggregateValue::Path(vec![]));
            assert_eq!(aggregate(&app).function, AggregateOp::Count);
            assert!(aggregate(&app).arg.is_none());
            if !repeating_schema {
                // A Group root is one item, including a present empty object.
                assert_complete_public_result(&app, INPUT, NAMED, Value::Int(1))?;
                assert_complete_public_result(&app, "{}", NAMED, Value::Int(1))?;
            }
            // Non-repeating object schemas also admit the flat-root-row format.
            let primary = r#"[{},{}]"#;
            let named = r#"[{},{},{}]"#;
            assert!(matches!(
                format_json::from_str(primary, &app.project.source)?,
                ir::Instance::Repeated(rows) if rows.len() == 2
            ));
            assert_complete_public_result(&app, primary, named, Value::Int(2))?;
            assert_complete_public_result(&app, "[]", named, Value::Int(0))?;
            choose(&mut app, &ctx, "Primary input", "Named input: Ref");
            assert!(aggregate(&app).collection.path.is_empty());
            assert_complete_public_result(&app, primary, named, Value::Int(3))?;
            // Primary remains populated; it cannot supply the empty named root.
            assert_complete_public_result(&app, primary, "[]", Value::Int(0))?;
        }
    }
    Ok(())
}

fn sum_rows(field: &str, values: Vec<serde_json::Value>) -> String {
    let mut rows = values
        .into_iter()
        .map(|value| serde_json::json!({(field): value}))
        .collect::<Vec<_>>();
    rows.push(serde_json::json!({})); // Missing values are skipped, not zero items.
    serde_json::json!({"Rows": rows}).to_string()
}

#[test]
fn offered_string_and_scalar_union_sum_keep_exact_runtime_semantics() -> anyhow::Result<()> {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = aggregate_fixture(document);
        app.project.source = SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("TextAmount", ScalarType::String),
                        SchemaNode::scalar_union(
                            "MixedAmount",
                            ir::ScalarTypeSet::new([
                                ScalarType::String,
                                ScalarType::Int,
                                ScalarType::Float,
                                ScalarType::Bool,
                            ])
                            .unwrap(),
                        ),
                    ],
                )
                .repeating(),
            ],
        );
        app.project.extra_sources[0].schema = app.project.source.clone();
        app.project.target = SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar_union(
                "Result",
                ir::ScalarTypeSet::new([ScalarType::Int, ScalarType::Float]).unwrap(),
            )],
        );
        app.project.extra_targets[0].schema = app.project.target.clone();
        rebuild(&mut app, document);
        let ctx = context();
        choose(&mut app, &ctx, "Count", "Sum");
        // Both fields must actually be offered by the real value picker.
        choose(&mut app, &ctx, "MixedAmount", "TextAmount");
        for field in ["TextAmount", "MixedAmount"] {
            if field == "MixedAmount" {
                choose(&mut app, &ctx, "TextAmount", "MixedAmount");
            }
            assert_eq!(
                aggregate(&app).value,
                OwnedAggregateValue::Path(vec![field.into()])
            );
            assert!(aggregate(&app).arg.is_none());
            let integers = if field == "TextAmount" {
                vec![
                    serde_json::json!("9007199254740993"),
                    serde_json::json!(" 2 "),
                    serde_json::json!("-1"),
                    serde_json::json!("skip"),
                ]
            } else {
                vec![
                    serde_json::json!(9_007_199_254_740_993_i64),
                    serde_json::json!(" 2 "),
                    serde_json::json!(-1),
                    serde_json::json!(true),
                    serde_json::json!("skip"),
                ]
            };
            let integral = sum_rows(field, integers);
            let other = sum_rows(field, vec![serde_json::json!("777")]);
            assert_complete_public_result(
                &app,
                &integral,
                &other,
                Value::Int(9_007_199_254_740_994),
            )?;
            let values = if field == "TextAmount" {
                vec![
                    serde_json::json!("1.25"),
                    serde_json::json!("2.5"),
                    serde_json::json!("1.25"),
                    serde_json::json!("skip"),
                ]
            } else {
                vec![
                    serde_json::json!(2),
                    serde_json::json!("2"),
                    serde_json::json!(4.0),
                    serde_json::json!(true),
                    serde_json::json!("skip"),
                ]
            };
            let fractional = sum_rows(field, values);
            if field == "MixedAmount" {
                let parsed = format_json::from_str(&fractional, &app.project.source)?;
                let ir::Instance::Repeated(rows) = parsed.field("Rows").unwrap() else {
                    panic!("rows")
                };
                assert_eq!(
                    rows[0].field(field).and_then(ir::Instance::as_scalar),
                    Some(&Value::Int(2))
                );
                assert_eq!(
                    rows[1].field(field).and_then(ir::Instance::as_scalar),
                    Some(&Value::String("2".into()))
                );
                assert_eq!(
                    rows[2].field(field).and_then(ir::Instance::as_scalar),
                    Some(&Value::Float(4.0))
                );
                assert_eq!(
                    rows[3].field(field).and_then(ir::Instance::as_scalar),
                    Some(&Value::Bool(true))
                );
            }
            assert_complete_public_result(
                &app,
                &fractional,
                &other,
                Value::Float(if field == "TextAmount" { 5.0 } else { 8.0 }),
            )?;
            assert_complete_public_result(
                &app,
                &sum_rows(
                    field,
                    vec![serde_json::json!("skip"), serde_json::json!("")],
                ),
                &other,
                Value::Int(0),
            )?;
            assert_complete_public_result(&app, r#"{"Rows":[]}"#, &other, Value::Int(0))?;
            choose(&mut app, &ctx, "Primary input", "Named input: Ref");
            assert_complete_public_result(
                &app,
                &other,
                &fractional,
                Value::Float(if field == "TextAmount" { 5.0 } else { 8.0 }),
            )?;
            assert_complete_public_result(&app, &integral, r#"{"Rows":[]}"#, Value::Int(0))?;
            choose(&mut app, &ctx, "Named input: Ref", "Primary input");
        }
    }
    Ok(())
}

#[test]
fn controls_count_empty_groups_and_sum_owner_local_fields_with_history() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = aggregate_fixture(document);
        let ctx = context();
        settle_all(&mut app, &ctx);
        app.mark_clean();
        app.rebase_history();
        let before = snapshot(&app);
        assert_result(&app, INPUT, NAMED, 2);
        choose(&mut app, &ctx, "Rows", "Empty");
        assert_result(&app, INPUT, NAMED, 2);
        choose(&mut app, &ctx, "Primary input", "Named input: Ref");
        assert_result(&app, INPUT, NAMED, 0); // Never count Primary's nonempty collection.
        choose(&mut app, &ctx, "Empty", "Rows");
        assert_result(&app, INPUT, NAMED, 3);
        choose(&mut app, &ctx, "Count", "Sum");
        assert_eq!(
            aggregate(&app).value,
            OwnedAggregateValue::Path(vec!["NamedAmount".into()])
        );
        assert_result(&app, INPUT, NAMED, 180);
        choose(&mut app, &ctx, "NamedAmount", "Other");
        assert_result(&app, INPUT, NAMED, 200);
        choose(&mut app, &ctx, "Named input: Ref", "Primary input");
        assert_result(&app, INPUT, NAMED, 30);
        choose(&mut app, &ctx, "Other", "PrimaryAmount");
        assert_result(&app, INPUT, NAMED, 13);
        click_nth(&mut app, &ctx, true, "PrimaryAmount", 0);
        let choices = settle(&mut app, &ctx, true);
        assert!(visible(&choices, "NamedAmount").is_empty());
        assert!(visible(&choices, "Nested / Deep").is_empty());
        assert!(visible(&choices, "Deep").is_empty());
        assert_typed_edit(&app);
        assert!(aggregate(&app).arg.is_none());
        let after = snapshot(&app);
        assert_eq!(before["layout"], after["layout"]);
        let edits = app.history.undo_len();
        for _ in 0..edits {
            app.undo_project();
        }
        assert_eq!(snapshot(&app), before);
        for _ in 0..edits {
            app.redo_project();
        }
        assert_eq!(snapshot(&app), after);
    }
}

#[test]
fn changing_operation_never_falls_back_to_another_owner() {
    let mut app = aggregate_fixture(MappingDocument::Main);
    app.project.extra_sources[0].schema = SchemaNode::group(
        "Input",
        vec![SchemaNode::group("Empty", vec![]).repeating()],
    );
    app.project.graph.nodes.insert(
        7,
        owned(
            AggregateOp::Count,
            SourceOwner::Named { name: "Ref".into() },
            "Empty",
            &[],
        ),
    );
    rebuild(&mut app, MappingDocument::Main);
    let ctx = context();
    open_lookup(&mut app, &ctx);
    let before = observed(&app);
    choose(&mut app, &ctx, "Count", "Sum");
    assert_eq!(observed(&app), before);
    assert_eq!(aggregate(&app).function, AggregateOp::Count);
    assert_eq!(
        aggregate(&app).collection.owner,
        SourceOwner::Named { name: "Ref".into() }
    );
}

#[test]
fn explicit_creation_and_legacy_conversion_preserve_typed_history() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        for (op, label) in [
            (AggregateOp::Count, "Count from one input"),
            (AggregateOp::Sum, "Sum from one input"),
        ] {
            let mut app = aggregate_fixture(document);
            let ctx = context();
            settle_all(&mut app, &ctx);
            app.mark_clean();
            app.rebase_history();
            let before = creation_snapshot(&app, document);
            pointer(
                &mut app,
                &ctx,
                true,
                egui::pos2(1100.0, 850.0),
                egui::PointerButton::Secondary,
            );
            click_nth(&mut app, &ctx, true, label, 0);
            let (&id, _) = app
                .project
                .graph
                .nodes
                .iter()
                .find(|(id, node)| **id != 7 && matches!(node, Node::OwnedAggregate(_)))
                .unwrap();
            assert!(
                matches!(&app.project.graph.nodes[&id], Node::OwnedAggregate(a) if a.function == op && a.arg.is_none())
            );
            assert!(matches!(
                app.history.next_undo(),
                Some(HistoryRecord::Command(_))
            ));
            settle_all(&mut app, &ctx);
            let after = creation_snapshot(&app, document);
            app.undo_project();
            assert_eq!(creation_snapshot(&app, document), before);
            app.redo_project();
            assert_eq!(creation_snapshot(&app, document), after);
        }
        let mut app = aggregate_fixture(document);
        app.project.graph.nodes.insert(
            7,
            Node::Aggregate {
                function: AggregateOp::Count,
                collection: vec!["Saved".into()],
                value: vec!["Retained".into()],
                expression: None,
                arg: None,
            },
        );
        rebuild(&mut app, document);
        let ctx = context();
        let before = snapshot(&app);
        // Use the normal edit command; simply opening it keeps legacy metadata.
        let output = settle(&mut app, &ctx, true);
        let title = visible(&output, "Count")[0];
        frame(&mut app, &ctx, true, vec![egui::Event::PointerMoved(title)]);
        open_lookup(&mut app, &ctx);
        assert_eq!(snapshot(&app), before);
        click_nth(&mut app, &ctx, true, "Use an exact input", 0);
        assert_eq!(aggregate(&app).function, AggregateOp::Count);
        assert!(matches!(
            app.history.next_undo(),
            Some(HistoryRecord::Command(_))
        ));
        app.undo_project();
        assert_eq!(snapshot(&app), before);
    }
}

#[test]
fn unsupported_forms_contexts_and_locks_remain_noops() {
    for case in [
        "expression",
        "argument",
        "other-op",
        "count-value",
        "missing",
        "duplicate",
        "dynamic",
        "document-set",
        "grouped",
        "locked",
        "invalid-path",
    ] {
        let mut app = aggregate_fixture(MappingDocument::Main);
        match case {
            "expression" => {
                if let Node::OwnedAggregate(a) = app.project.graph.nodes.get_mut(&7).unwrap() {
                    a.value = OwnedAggregateValue::Expression(3);
                }
            }
            "argument" => {
                if let Node::OwnedAggregate(a) = app.project.graph.nodes.get_mut(&7).unwrap() {
                    a.arg = Some(3);
                }
            }
            "other-op" => {
                if let Node::OwnedAggregate(a) = app.project.graph.nodes.get_mut(&7).unwrap() {
                    a.function = AggregateOp::Avg;
                }
            }
            "count-value" => {
                if let Node::OwnedAggregate(a) = app.project.graph.nodes.get_mut(&7).unwrap() {
                    a.value = OwnedAggregateValue::Path(vec!["Other".into()]);
                }
            }
            "missing" | "dynamic" | "duplicate" => {
                app.project.graph.nodes.insert(
                    7,
                    owned(
                        AggregateOp::Count,
                        SourceOwner::Named { name: "Ref".into() },
                        "Rows",
                        &[],
                    ),
                );
                if case == "missing" {
                    app.project.extra_sources.clear();
                }
                if case == "duplicate" {
                    app.project
                        .extra_sources
                        .push(app.project.extra_sources[0].clone());
                }
                if case == "dynamic" {
                    app.project.extra_sources[0].dynamic_path =
                        Some(mapping::DynamicSourcePath::legacy(3, vec![]));
                }
            }
            "document-set" => app.project.source_options.local_xml_file_set = true,
            "grouped" => {
                app.project.root.group_by = Some(3);
                app.project.root.iteration = mapping::ScopeIteration::Source(vec!["Rows".into()]);
            }
            "invalid-path" => {
                if let Node::OwnedAggregate(a) = app.project.graph.nodes.get_mut(&7).unwrap() {
                    a.collection.path = vec!["SavedMissing".into()];
                }
            }
            _ => (),
        }
        rebuild(&mut app, MappingDocument::Main);
        let ctx = context();
        let before = observed(&app);
        let enabled = case != "locked";
        settle(&mut app, &ctx, enabled);
        if enabled {
            open_lookup(&mut app, &ctx);
        } else {
            settle(&mut app, &ctx, false);
        }
        for _ in 0..3 {
            frame(
                &mut app,
                &ctx,
                enabled,
                vec![egui::Event::PointerMoved(egui::pos2(620.0, 80.0))],
            );
        }
        assert_eq!(observed(&app), before, "{case}");
    }
}

#[test]
fn wires_and_indirect_consumers_share_admission_and_retained_forms_cannot_bypass() {
    for kind in [
        "generated",
        "concatenate",
        "dynamic",
        "presence",
        "grouped",
        "grouped-descendant",
        "ordinary",
    ] {
        for indirect in [false, true] {
            let mut app = connection(kind);
            app.project.graph.nodes.insert(
                7,
                owned(AggregateOp::Count, SourceOwner::Primary, "Rows", &[]),
            );
            app.project.graph.nodes.insert(
                12,
                Node::Call {
                    function: "concat".into(),
                    args: vec![7],
                },
            );
            rebuild(&mut app, MappingDocument::Main);
            let before = observed(&app);
            let block = target_blocks(&app.project.target)
                .iter()
                .position(|block| block.chain == ["Rejected"])
                .unwrap();
            let result = wire_action(
                &mut app,
                CanvasNode::Graph(if indirect { 12 } else { 7 }),
                0,
                CanvasNode::TargetBlock(block),
                0,
                false,
            );
            if kind == "ordinary" {
                assert!(result.is_ok(), "{result:?}");
            } else {
                assert!(result.is_err(), "{kind}");
                assert_eq!(observed(&app), before);
            }
        }
    }
    for consumer in [
        "call",
        "if",
        "value-map",
        "default",
        "aggregate",
        "lookup",
        "dynamic",
        "sequence",
    ] {
        let mut app = aggregate_fixture(MappingDocument::Main);
        let node = match consumer {
            "call" => Node::Call {
                function: "add".into(),
                args: vec![3, 3],
            },
            "if" => Node::If {
                condition: 3,
                then: 3,
                else_: 3,
            },
            "value-map" => Node::ValueMap {
                input: 3,
                input_type: None,
                table: vec![],
                default: None,
            },
            "default" => Node::RuntimeParameterDefault {
                name: "p".into(),
                ty: ScalarType::Int,
                preview: None,
                default: 3,
            },
            "aggregate" => Node::Aggregate {
                function: AggregateOp::Sum,
                collection: vec!["Rows".into()],
                value: vec![],
                expression: Some(3),
                arg: None,
            },
            "lookup" => Node::Lookup {
                collection: vec!["Rows".into()],
                key: vec!["Other".into()],
                matches: 3,
                value: vec!["Other".into()],
            },
            "dynamic" => Node::DynamicSourceField {
                object: vec![],
                frame: None,
                key: 3,
            },
            _ => Node::SequenceItemAt {
                sequence: mapping::SequenceExpr::Generate {
                    item: 20,
                    from: Some(3),
                    to: 3,
                },
                index: 3,
            },
        };
        app.project.graph.nodes.insert(9, node);
        rebuild(&mut app, MappingDocument::Main);
        let before = observed(&app);
        let result = wire_action(
            &mut app,
            CanvasNode::Graph(7),
            0,
            CanvasNode::Graph(9),
            0,
            false,
        );
        if ["call", "if", "value-map", "default"].contains(&consumer) {
            assert!(result.is_ok(), "{consumer}: {result:?}");
        } else {
            assert!(result.is_err(), "{consumer}");
            assert_eq!(observed(&app), before);
        }
    }
}

#[test]
fn saved_computed_and_argument_inputs_keep_wires_and_orphan_state_readonly() {
    for computed in [false, true] {
        let mut app = aggregate_fixture(MappingDocument::Main);
        if let Node::OwnedAggregate(a) = app.project.graph.nodes.get_mut(&7).unwrap() {
            if computed {
                a.value = OwnedAggregateValue::Expression(3);
            } else {
                a.arg = Some(3);
            }
        }
        rebuild(&mut app, MappingDocument::Main);
        let before = observed(&app);
        for (from, disconnect) in [
            (CanvasNode::Graph(3), true),
            (CanvasNode::SourceBlock(0), false),
        ] {
            assert!(wire_action(&mut app, from, 0, CanvasNode::Graph(7), 0, disconnect).is_err());
            assert_eq!(observed(&app), before);
        }
        // Unsupported forms are not fresh assignable values either.
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
}

#[test]
fn inspector_bindings_controls_and_reverse_grouping_use_common_gate() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        for control in [
            "binding",
            "filter",
            "group",
            "group-start",
            "blocks",
            "post-group",
            "sort",
            "window",
        ] {
            let mut app = connection_fixture("generated");
            app.project.graph.nodes.insert(
                7,
                owned(AggregateOp::Count, SourceOwner::Primary, "Rows", &[]),
            );
            app.project.graph.nodes.insert(
                12,
                Node::Call {
                    function: "concat".into(),
                    args: vec![7],
                },
            );
            let s = &mut app.project.root.children[1];
            let current = match control {
                "filter" => {
                    s.filter = Some(30);
                    30
                }
                "group" => {
                    s.group_by = Some(3);
                    3
                }
                "group-start" => {
                    s.group_starting_with = Some(30);
                    30
                }
                "blocks" => {
                    s.group_into_blocks = Some(21);
                    21
                }
                "post-group" => {
                    s.group_by = Some(3);
                    s.post_group_filter = Some(30);
                    30
                }
                "sort" => {
                    s.sort_by = Some(3);
                    3
                }
                "window" => {
                    s.windows.push(mapping::SequenceWindow::First { count: 21 });
                    21
                }
                _ => 3,
            };
            app.project.extra_targets[0].root = app.project.root.clone();
            rebuild(&mut app, document);
            app.selected_scope = vec![1];
            let ctx = context();
            settle_inspector(&mut app, &ctx, true);
            app.mark_clean();
            app.rebase_history();
            for candidate in [7, 12] {
                let current_label = label(&app, current);
                click_label(&mut app, &ctx, true, &current_label, control == "binding");
                let before = observed(&app);
                let candidate_label = label(&app, candidate);
                click_label(&mut app, &ctx, true, &candidate_label, true);
                assert_eq!(observed(&app), before, "{control}");
                close(&mut app, &ctx);
            }
        }
        // A previously valid direct/indirect binding must block later grouping.
        let mut app = connection("ordinary");
        app.project.graph.nodes.insert(
            7,
            owned(AggregateOp::Count, SourceOwner::Primary, "Rows", &[]),
        );
        app.project.graph.nodes.insert(
            12,
            Node::Call {
                function: "concat".into(),
                args: vec![7],
            },
        );
        app.project.root.children[1].bindings[0].node = 12;
        app.project.extra_targets[0].root = app.project.root.clone();
        rebuild(&mut app, document);
        app.selected_scope = vec![1];
        let ctx = context();
        settle_inspector(&mut app, &ctx, true);
        app.mark_clean();
        app.rebase_history();
        let before = observed(&app);
        click_label(&mut app, &ctx, true, "No grouping", false);
        click_label(&mut app, &ctx, true, "By key", false);
        assert_eq!(observed(&app), before);
        assert!(!root(&app).children[1].has_grouping());
        // Descendants inherit the closed context even with an ordinary child scope.
        root_mut(&mut app).children[1].group_by = Some(3);
        root_mut(&mut app).children[1].children.push(Scope {
            target_field: "Child".into(),
            ..Default::default()
        });
        let paths = SourcePathCatalog::new(&app.project.source, &app.project.extra_sources)
            .with_project_ownership(&app.project);
        assert!(!paths.lookup_binding_supported(root(&app), &["Rejected".into(), "Child".into()]));
    }
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-owned-aggregate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn saved_reopened_count_and_sum_match_preview_and_public_cli() -> anyhow::Result<()> {
    let scratch = Scratch::new();
    let ctx = context();
    for (op, collection, field, expected, named) in [
        (AggregateOp::Count, "Rows", &[][..], 3, NAMED),
        (AggregateOp::Count, "Empty", &[][..], 0, NAMED),
        (AggregateOp::Sum, "Rows", &["NamedAmount"][..], 180, NAMED),
        (
            AggregateOp::Sum,
            "Rows",
            &["NamedAmount"][..],
            0,
            r#"{"Rows":[],"Empty":[]}"#,
        ),
    ] {
        let mut app = aggregate_fixture(MappingDocument::Target(0));
        // Use the same authored forms, changing only the fixture's chosen paths.
        app.project.graph.nodes.insert(
            7,
            owned(
                op,
                SourceOwner::Named { name: "Ref".into() },
                collection,
                field,
            ),
        );
        app.project.extra_sources[0].path = scratch.0.join("named.json").display().to_string();
        app.project.extra_targets[0].path =
            Some(scratch.0.join("audit.json").display().to_string());
        std::fs::write(scratch.0.join("named.json"), named)?;
        rebuild(&mut app, MappingDocument::Target(0));
        assert_result(&app, INPUT, named, expected);
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
        assert_eq!(snapshot(&reopened), before);
        std::fs::write(scratch.0.join("input.json"), INPUT)?;
        let output = scratch.0.join("result.json");
        let run =
            cli::run_project_with_paths(&path, Some(&scratch.0.join("input.json")), Some(&output))?;
        assert_eq!(run.artifacts.len(), 2);
        for output in [&output, &scratch.0.join("audit.json")] {
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&std::fs::read(output)?)?,
                serde_json::json!({"Result": expected})
            );
        }
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
            while reopened.pending_preview.is_some() && std::time::Instant::now() < deadline {
                reopened.poll_preview(&ctx);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(reopened.pending_preview.is_none());
            let report = reopened.run_report.as_mut().expect("preview report");
            assert!(!report.report.outputs.is_empty(), "{:?}", report.report);
            for output in &mut report.report.outputs {
                let crate::run_report::OutputPreview::Text { content, .. } = output.preview()
                else {
                    panic!("JSON preview")
                };
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(content)?,
                    serde_json::json!({"Result": expected})
                );
            }
            assert!(!scratch.0.join("not-written.json").exists());
        }
    }
    Ok(())
}

#[test]
fn ordinary_sum_keeps_overflow_nonfinite_and_decimal_errors() {
    let mut app = aggregate_fixture(MappingDocument::Main);
    app.project.graph.nodes.insert(
        7,
        owned(
            AggregateOp::Sum,
            SourceOwner::Primary,
            "Rows",
            &["PrimaryAmount"],
        ),
    );
    for (values, expected) in [
        (
            vec![Value::Int(i64::MAX), Value::Int(1)],
            engine::EngineError::AggregateIntegerOverflow {
                function: AggregateOp::Sum,
            },
        ),
        (
            vec![Value::Float(f64::INFINITY)],
            engine::EngineError::AggregateNonFinite {
                function: AggregateOp::Sum,
            },
        ),
        (
            vec![Value::decimal(ir::Decimal96::parse_exact("0.1").unwrap())],
            engine::EngineError::AggregateUnsupportedScalar {
                function: AggregateOp::Sum,
                found: "decimal",
            },
        ),
    ] {
        let rows = values
            .into_iter()
            .map(|value| {
                ir::Instance::Group(
                    vec![("PrimaryAmount".into(), ir::Instance::Scalar(value))].into(),
                )
            })
            .collect();
        let input = ir::Instance::Group(vec![("Rows".into(), ir::Instance::Repeated(rows))].into());
        let result = engine::run_outputs_with_sources_and_context(
            &app.project,
            &input,
            vec![],
            &engine::ExecutionContext::new(Path::new("sum.json")),
        );
        assert_eq!(result.unwrap_err(), expected);
    }
}
