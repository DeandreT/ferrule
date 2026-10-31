//! Presence candidates use the same source ownership and frame selection as
//! execution, not just a similarly shaped schema field.

use super::*;

fn bind(field: &str, node: NodeId) -> Binding {
    Binding {
        target_field: field.into(),
        node,
    }
}

fn boolean(value: bool) -> Node {
    Node::Const {
        value: Value::Bool(value),
    }
}
fn integer(value: i64) -> Node {
    Node::Const {
        value: Value::Int(value),
    }
}
fn field(path: &[&str], frame: Option<&[&str]>) -> Node {
    Node::SourceField {
        path: path.iter().map(|value| (*value).into()).collect(),
        frame: frame.map(|path| path.iter().map(|value| (*value).into()).collect()),
    }
}
fn call(function: &str, args: &[NodeId]) -> Node {
    Node::Call {
        function: function.into(),
        args: args.to_vec(),
    }
}

fn item_scope() -> Scope {
    Scope {
        target_field: "Item".into(),
        iteration: ScopeIteration::OptionalPresence { condition: 0 },
        bindings: vec![bind("Value", 0)],
        ..Default::default()
    }
}
fn item_schema() -> SchemaNode {
    SchemaNode::group("Item", vec![SchemaNode::scalar("Value", ScalarType::Bool)]).repeating()
}

fn app(
    source: SchemaNode,
    target: SchemaNode,
    root: Scope,
    nodes: &[(NodeId, Node)],
    path: &[usize],
) -> FerruleApp {
    let mut app = fixture(MappingDocument::Main, true);
    app.project.source = source;
    app.project.target = target;
    app.project.root = root;
    app.project.graph.nodes = nodes.iter().cloned().collect();
    app.project.extra_targets.clear();
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mapping_workspace.target_canvases.clear();
    app.selected_scope = path.to_vec();
    app.mark_clean();
    app.rebase_history();
    app
}

fn candidates(app: &FerruleApp) -> Vec<NodeId> {
    let (root, schema) = match app.mapping_workspace.active {
        MappingDocument::Target(index) => {
            let target = &app.project.extra_targets[index];
            (&target.root, &target.schema)
        }
        _ => (&app.project.root, &app.project.target),
    };
    crate::scope_editor::presence_controls::profile(
        &app.project,
        root,
        schema,
        &app.selected_scope,
        true,
    )
    .condition_ids()
    .to_vec()
}

fn assert_output_and_preview(app: &mut FerruleApp, input: &str, expected: serde_json::Value) {
    let issues = engine::validate(&app.project);
    assert!(issues.is_empty(), "{issues:#?}");
    let source = format_json::from_str(input, &app.project.source).unwrap();
    let result = engine::run_outputs(&app.project, &source).unwrap();
    let (schema, output) = match app.mapping_workspace.active {
        MappingDocument::Target(index) => (
            &app.project.extra_targets[index].schema,
            &result.extras[index].instance,
        ),
        _ => (&app.project.target, &result.primary),
    };
    let serialized = format_json::to_string(schema, output).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&serialized).unwrap(),
        expected
    );
    let scratch = Scratch::new();
    let previewed = preview(app, &context(), input, &scratch.0.join("unwritten.json"))
        .expect("actual Preview succeeds");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&previewed).unwrap(),
        expected
    );
}

#[test]
fn presence_private_items_and_their_dependents_are_never_candidates_on_boolean_sources() {
    for rows in [false, true] {
        let (source, target, root, path, input) = if rows {
            (
                SchemaNode::group(
                    "Input",
                    vec![SchemaNode::scalar("Rows", ScalarType::Bool).repeating()],
                ),
                SchemaNode::group(
                    "Output",
                    vec![SchemaNode::group("Rows", vec![item_schema()]).repeating()],
                ),
                Scope {
                    children: vec![Scope {
                        target_field: "Rows".into(),
                        iteration: ScopeIteration::Source(vec!["Rows".into()]),
                        children: vec![item_scope()],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                vec![0, 0],
                r#"{"Rows":[true,false]}"#,
            )
        } else {
            (
                SchemaNode::scalar("Input", ScalarType::Bool),
                SchemaNode::group("Output", vec![item_schema()]),
                Scope {
                    children: vec![item_scope()],
                    ..Default::default()
                },
                vec![0],
                "true",
            )
        };
        let mut app = app(
            source,
            target,
            root,
            &[
                (0, boolean(true)),
                (1, boolean(false)),
                (2, field(&[], None)),
                (3, integer(1)),
                (4, integer(2)),
                (5, call("equal", &[2, 3])),
                (
                    6,
                    Node::SequenceExists {
                        sequence: mapping::SequenceExpr::Generate {
                            item: 2,
                            from: Some(3),
                            to: 4,
                        },
                        predicate: 7,
                    },
                ),
                (7, call("not", &[5])),
                (8, field(&[], None)),
            ],
            &path,
        );
        let offered = candidates(&app);
        assert!(
            offered.contains(&8),
            "an ordinary Boolean source is still available"
        );
        for id in [2, 5, 7] {
            assert!(!offered.contains(&id), "private dependency #{id}");
        }
        let context = context();
        click(&mut app, &context, true, "0: expression", false);
        let labels = texts(&frame(&mut app, &context, true, vec![]));
        for id in [2, 5, 7] {
            assert!(
                !labels
                    .iter()
                    .any(|(text, _)| text.ends_with(&format!(" · #{id}")))
            );
        }
        // A real allowed choice keeps the graph engine-valid and previews the
        // Boolean source, not the private integer generator's current item.
        click(&mut app, &context, true, "8: expression", true);
        let expected = if rows {
            serde_json::json!({"Rows":[{"Item":[{"Value":true}]},{"Item":[]}]})
        } else {
            serde_json::json!({"Item":[{"Value":true}]})
        };
        assert_output_and_preview(&mut app, input, expected);
        let root = &mut app.project.root;
        crate::scope_editor::scope_at_mut(root, &path).iteration =
            ScopeIteration::OptionalPresence { condition: 7 };
        assert!(
            !engine::validate(&app.project).is_empty(),
            "leaking the private item is invalid"
        );
    }
}

#[test]
fn presence_absolute_field_uses_active_collection_before_nested_name_collision() {
    let mut app = app(
        SchemaNode::group(
            "Input",
            vec![
                SchemaNode::scalar("RootFlag", ScalarType::Bool),
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Flag", ScalarType::String),
                        SchemaNode::group(
                            "Rows",
                            vec![SchemaNode::scalar("Flag", ScalarType::Bool)],
                        ),
                    ],
                )
                .repeating(),
            ],
        ),
        SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Rows", vec![item_schema()]).repeating()],
        ),
        Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: vec![item_scope()],
                ..Default::default()
            }],
            ..Default::default()
        },
        &[
            (0, boolean(true)),
            (2, field(&["Rows", "Flag"], None)),
            (8, field(&["Rows", "Rows", "Flag"], None)),
            (9, field(&["RootFlag"], None)),
        ],
        &[0, 0],
    );
    assert!(
        !candidates(&app).contains(&2),
        "Rows/Flag is the current row's String"
    );
    assert!(candidates(&app).contains(&8));
    assert!(
        candidates(&app).contains(&9),
        "ordinary root fallback remains available"
    );
    let input = r#"{"RootFlag":true,"Rows":[{"Flag":"first","Rows":{"Flag":true}},{"Flag":"second","Rows":{"Flag":false}}]}"#;
    let context = context();
    choose(&mut app, &context, 0, 8);
    assert_output_and_preview(
        &mut app,
        input,
        serde_json::json!({"Rows":[{"Item":[{"Value":true}]},{"Item":[]}]}),
    );
    choose(&mut app, &context, 8, 9);
    assert_output_and_preview(
        &mut app,
        input,
        serde_json::json!({"Rows":[{"Item":[{"Value":true}]},{"Item":[{"Value":true}]}]}),
    );
    app.project.root.children[0].children[0].iteration =
        ScopeIteration::OptionalPresence { condition: 2 };
    let input = format_json::from_str(input, &app.project.source).unwrap();
    assert!(matches!(
        engine::run(&app.project, &input),
        Err(engine::EngineError::NotABool {
            node: 2,
            found: "string"
        })
    ));
}

#[test]
fn presence_relative_nested_frames_keep_raw_identity_and_absolute_schema_location_separate() {
    let mut item = item_schema();
    if let ir::SchemaKind::Group { children, .. } = &mut item.kind {
        children.push(SchemaNode::scalar("Line", ScalarType::Int));
    }
    let mut scope = item_scope();
    scope.bindings.push(bind("Line", 3));
    let mut app = app(
        SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::group(
                            "Lines",
                            vec![SchemaNode::scalar("Flag", ScalarType::Bool)],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
                SchemaNode::group("Other", vec![SchemaNode::scalar("Flag", ScalarType::Bool)])
                    .repeating(),
            ],
        ),
        SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![SchemaNode::group("Lines", vec![item]).repeating()],
                )
                .repeating(),
            ],
        ),
        Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: vec![Scope {
                    target_field: "Lines".into(),
                    iteration: ScopeIteration::Source(vec!["Lines".into()]),
                    children: vec![scope],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
        &[
            (0, boolean(true)),
            (1, integer(1)),
            (2, field(&["Flag"], Some(&["Lines"]))),
            (
                3,
                Node::Position {
                    collection: vec!["Lines".into()],
                },
            ),
            (4, call("equal", &[3, 1])),
            (5, field(&["Flag"], Some(&["Other"]))),
            (
                6,
                Node::Position {
                    collection: vec!["Rows".into(), "Lines".into()],
                },
            ),
            (7, call("equal", &[6, 1])),
        ],
        &[0, 0, 0],
    );
    let offered = candidates(&app);
    assert!(offered.contains(&2) && offered.contains(&4));
    assert!(!offered.contains(&5) && !offered.contains(&7));
    let input = r#"{"Rows":[{"Lines":[{"Flag":false},{"Flag":true}]},{"Lines":[{"Flag":true}]}],"Other":[{"Flag":false}]}"#;
    let context = context();
    choose(&mut app, &context, 0, 2);
    assert_output_and_preview(
        &mut app,
        input,
        serde_json::json!({"Rows":[{"Lines":[{"Item":[]},{"Item":[{"Value":true,"Line":2}]}]},{"Lines":[{"Item":[{"Value":true,"Line":1}]}]}]}),
    );
    choose(&mut app, &context, 2, 4);
    assert_output_and_preview(
        &mut app,
        input,
        serde_json::json!({"Rows":[{"Lines":[{"Item":[{"Value":true,"Line":1}]},{"Item":[]}]},{"Lines":[{"Item":[{"Value":true,"Line":1}]}]}]}),
    );
    app.project.root.children[0].children[0].children[0].iteration =
        ScopeIteration::OptionalPresence { condition: 5 };
    let input = format_json::from_str(input, &app.project.source).unwrap();
    assert!(matches!(
        engine::run(&app.project, &input),
        Err(engine::EngineError::NotABool {
            node: 5,
            found: "null"
        })
    ));
}

#[test]
fn presence_root_lookup_has_no_live_pin_until_root_iteration_in_that_target() {
    let mut app = app(
        SchemaNode::group("Input", vec![SchemaNode::scalar("Flag", ScalarType::Bool)]).repeating(),
        SchemaNode::group("Output", vec![item_schema()]),
        Scope {
            children: vec![item_scope()],
            ..Default::default()
        },
        &[
            (0, boolean(true)),
            (1, integer(1)),
            (2, field(&["Flag"], Some(&[]))),
            (3, Node::Position { collection: vec![] }),
            (4, call("equal", &[3, 1])),
            (5, field(&["Flag"], None)),
        ],
        &[0],
    );
    let mut named_root = app.project.root.clone();
    named_root.iteration = ScopeIteration::Source(vec![]);
    named_root.children[0].iteration = ScopeIteration::OptionalPresence { condition: 2 };
    app.project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: Some("other.json".into()),
        schema: app.project.target.clone().repeating(),
        options: app.project.target_options.clone(),
        root: named_root,
    });
    assert!(!candidates(&app).contains(&2) && !candidates(&app).contains(&4));
    assert!(candidates(&app).contains(&5));
    let input = r#"[{"Flag":true},{"Flag":false}]"#;
    assert_output_and_preview(
        &mut app,
        input,
        serde_json::json!({"Item":[{"Value":true}]}),
    );
    app.open_target_tab(0);
    app.selected_scope = vec![0];
    assert!(candidates(&app).contains(&2) && candidates(&app).contains(&4));
    assert_output_and_preview(
        &mut app,
        input,
        serde_json::json!([{"Item":[{"Value":true}]},{"Item":[]} ]),
    );
    // The same saved pin must remain unavailable in the ordinary primary root,
    // even though it is valid in another target that really iterates root rows.
    app.mapping_workspace.active = MappingDocument::Main;
    app.selected_scope = vec![0];
    app.project.root.children[0].iteration = ScopeIteration::OptionalPresence { condition: 2 };
    let input = format_json::from_str(input, &app.project.source).unwrap();
    assert!(matches!(
        engine::run(&app.project, &input),
        Err(engine::EngineError::NotABool {
            node: 2,
            found: "null"
        })
    ));
}

#[test]
fn presence_explicit_flat_rows_keep_real_root_pins_without_promoting_json_identity_scopes() {
    let mut app = app(
        SchemaNode::group("Input", vec![SchemaNode::scalar("Flag", ScalarType::Bool)]),
        SchemaNode::group("Output", vec![item_schema()]).repeating(),
        Scope {
            iteration: ScopeIteration::Source(vec![]),
            children: vec![item_scope()],
            ..Default::default()
        },
        &[(0, boolean(true)), (2, field(&["Flag"], Some(&[])))],
        &[0],
    );
    app.project.source_options.json_document = false;
    app.project.source_path = Some("input.csv".into());
    assert!(candidates(&app).contains(&2));
    choose(&mut app, &context(), 0, 2);
    assert!(engine::validate(&app.project).is_empty());
    let row =
        |flag| Instance::Group(vec![("Flag".into(), Instance::Scalar(Value::Bool(flag)))].into());
    let input = Instance::Repeated(vec![row(true), row(false)]);
    let output = engine::run(&app.project, &input).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &format_json::to_string(&app.project.target, &output).unwrap()
        )
        .unwrap(),
        serde_json::json!([{"Item":[{"Value":true}]},{"Item":[]}]),
    );
    let scratch = Scratch::new();
    let previewed = preview(
        &mut app,
        &context(),
        "Flag\ntrue\nfalse\n",
        &scratch.0.join("unwritten.json"),
    )
    .expect("flat CSV root Preview succeeds");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&previewed).unwrap(),
        serde_json::json!([{"Item":[{"Value":true}]},{"Item":[]}])
    );
    // Explicit JSON identity overrides the filename; Source([]) on a Group
    // is an identity walk and does not create the CSV row position above.
    app.project.source_options.json_document = true;
    assert!(!candidates(&app).contains(&2));
    assert!(matches!(
        engine::run(&app.project, &row(true)),
        Err(engine::EngineError::NotABool {
            node: 2,
            found: "null"
        })
    ));
}

#[test]
fn presence_sequence_join_keys_reserve_private_items_in_scopes_and_aggregates() {
    use mapping::{
        AggregateOp, ComputedJoinSourceId, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource,
        SequenceExpr,
    };

    for aggregate in [false, true] {
        let key = ComputedJoinSourceId::new(1);
        let join = JoinId::new(1);
        let plan = JoinPlan::new(
            JoinSource::computed_sequence(
                key,
                SequenceExpr::Tokenize {
                    input: 1,
                    delimiter: 3,
                    item: 2,
                },
            ),
            JoinSource::new(vec![]),
            JoinConditions::new(JoinKey::computed(key, vec![])),
        )
        .unwrap();
        let mut target_children = vec![item_schema()];
        let mut root = Scope {
            children: vec![item_scope()],
            ..Default::default()
        };
        let mut nodes = vec![
            (0, boolean(true)),
            (
                1,
                Node::Const {
                    value: Value::String("true".into()),
                },
            ),
            (2, field(&[], None)),
            (
                3,
                Node::Const {
                    value: Value::String(",".into()),
                },
            ),
            (4, field(&[], None)),
            (5, call("not", &[2])),
            (6, call("not", &[5])),
        ];
        if aggregate {
            target_children.push(SchemaNode::scalar("Count", ScalarType::Int));
            nodes.push((
                9,
                Node::JoinAggregate {
                    function: AggregateOp::Count,
                    join,
                    plan,
                    expression: None,
                    arg: None,
                },
            ));
            root.bindings.push(bind("Count", 9));
        } else {
            target_children.push(
                SchemaNode::group(
                    "Joined",
                    vec![SchemaNode::scalar("Matched", ScalarType::Bool)],
                )
                .repeating(),
            );
            root.children.push(Scope {
                target_field: "Joined".into(),
                iteration: ScopeIteration::InnerJoin { id: join, plan },
                bindings: vec![bind("Matched", 0)],
                ..Default::default()
            });
        }
        let mut app = app(
            SchemaNode::scalar("Input", ScalarType::Bool).repeating(),
            SchemaNode::group("Output", target_children),
            root,
            &nodes,
            &[0],
        );
        assert_eq!(
            crate::graph_viewer::project_sequence_item_ids(&app.project),
            [2].into_iter().collect()
        );
        let offered = candidates(&app);
        assert!(offered.contains(&4));
        for id in [2, 5, 6] {
            assert!(
                !offered.contains(&id),
                "private join input #{id}, aggregate={aggregate}"
            );
        }
        let issues = engine::validate(&app.project);
        assert!(issues.is_empty(), "{issues:#?}");
        let input = "[true,false]";
        let source = format_json::from_str(input, &app.project.source).unwrap();
        let before = engine::run(&app.project, &source).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(&format_json::to_string(&app.project.target, &before).unwrap())
                .unwrap();
        assert_eq!(expected["Item"], serde_json::json!([{"Value":true}]));

        let context = context();
        click(&mut app, &context, true, "0: expression", false);
        let labels = texts(&frame(&mut app, &context, true, vec![]));
        for id in [2, 5, 6] {
            assert!(
                !labels
                    .iter()
                    .any(|(label, _)| label.ends_with(&format!(" · #{id}")))
            );
        }
        click(&mut app, &context, true, "4: expression", true);
        assert_output_and_preview(&mut app, input, expected);
    }
}

#[test]
fn presence_singleton_walk_does_not_offer_unproved_retained_frame_fields() {
    let mut app = app(
        SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Flag", ScalarType::Bool),
                        SchemaNode::group(
                            "Detail",
                            vec![
                                SchemaNode::scalar("Flag", ScalarType::String),
                                SchemaNode::scalar("Ready", ScalarType::Bool),
                            ],
                        ),
                    ],
                )
                .repeating(),
            ],
        ),
        SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![SchemaNode::group("Detail", vec![item_schema()]).repeating()],
                )
                .repeating(),
            ],
        ),
        Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: vec![Scope {
                    target_field: "Detail".into(),
                    iteration: ScopeIteration::Source(vec!["Detail".into()]),
                    children: vec![item_scope()],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
        &[
            (0, boolean(true)),
            (2, field(&["Flag"], Some(&["Rows"]))),
            (3, call("not", &[2])),
            (4, field(&["Rows", "Flag"], None)),
            (5, call("not", &[4])),
            (6, field(&["Ready"], None)),
            (
                7,
                Node::Position {
                    collection: vec!["Rows".into()],
                },
            ),
            (8, call("equal", &[7, 9])),
            (9, integer(1)),
        ],
        &[0, 0, 0],
    );
    let offered = candidates(&app);
    for id in [2, 3, 4, 5] {
        assert!(!offered.contains(&id), "unproved frame field #{id}");
    }
    assert!(
        offered.contains(&6) && offered.contains(&8),
        "ordinary fields and raw positions remain usable"
    );
    let input = r#"{"Rows":[{"Flag":true,"Detail":{"Flag":"first","Ready":true}},{"Flag":false,"Detail":{"Flag":"second","Ready":false}}]}"#;
    let context = context();
    choose(&mut app, &context, 0, 6);
    assert_output_and_preview(
        &mut app,
        input,
        serde_json::json!({"Rows":[{"Detail":[{"Item":[{"Value":true}]}]},{"Detail":[{"Item":[]}]}]}),
    );

    app.project.root.children[0].children[0].children[0].iteration =
        ScopeIteration::OptionalPresence { condition: 2 };
    let source = format_json::from_str(input, &app.project.source).unwrap();
    assert!(matches!(
        engine::run(&app.project, &source),
        Err(engine::EngineError::NotABool {
            node: 2,
            found: "string"
        })
    ));
    let retained = key(&app);
    frame(&mut app, &context, true, vec![]);
    assert_eq!(
        key(&app),
        retained,
        "viewing the saved condition does not repair it"
    );
    let scratch = Scratch::new();
    assert!(preview(&mut app, &context, input, &scratch.0.join("unwritten.json")).is_none());
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| format!("{item:?}").contains("string"))
    );
}
