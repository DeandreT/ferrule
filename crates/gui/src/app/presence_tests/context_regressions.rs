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

fn singleton_app(walks: &[&[&str]], document: MappingDocument) -> FerruleApp {
    // A shifted ancestor pin would read this string instead of Rows' Boolean.
    let detail = SchemaNode::group(
        "Detail",
        vec![
            SchemaNode::scalar("Flag", ScalarType::String),
            SchemaNode::scalar("Ready", ScalarType::Bool),
        ],
    );
    let nested = walks
        .iter()
        .flat_map(|walk| walk.iter())
        .any(|segment| *segment == "Wrapper");
    let child = if nested {
        SchemaNode::group("Wrapper", vec![detail])
    } else {
        detail
    };
    let mut schema = SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("Value", ScalarType::Bool),
            SchemaNode::scalar("Absolute", ScalarType::Bool),
            SchemaNode::scalar("Child", ScalarType::String),
            SchemaNode::scalar("Ready", ScalarType::Bool),
            SchemaNode::scalar("Position", ScalarType::Int),
        ],
    )
    .repeating();
    let mut scope = Scope {
        bindings: vec![
            bind("Value", 2),
            bind("Absolute", 4),
            bind("Child", 10),
            bind("Ready", 6),
            bind("Position", 7),
        ],
        ..item_scope()
    };
    for walk in walks.iter().rev() {
        let name = walk.last().unwrap();
        schema = SchemaNode::group(*name, vec![schema]).repeating();
        scope = Scope {
            target_field: (*name).into(),
            iteration: ScopeIteration::Source(
                walk.iter().map(|segment| (*segment).into()).collect(),
            ),
            children: vec![scope],
            ..Default::default()
        };
    }
    let mut app = app(
        SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![SchemaNode::scalar("Flag", ScalarType::Bool), child],
                )
                .repeating(),
                SchemaNode::group("Other", vec![SchemaNode::scalar("Flag", ScalarType::Bool)])
                    .repeating(),
            ],
        ),
        SchemaNode::group("Output", vec![schema]),
        Scope {
            children: vec![scope],
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
            (10, field(&["Flag"], None)),
            (11, field(&["Flag"], Some(&["Other"]))),
            (12, field(&["Ready"], Some(&["Detail"]))),
            (
                13,
                Node::Position {
                    collection: vec!["Detail".into()],
                },
            ),
            (14, call("equal", &[13, 9])),
        ],
        &vec![0; walks.len() + 1],
    );
    if document == MappingDocument::Target(0) {
        app.project.extra_targets.push(NamedTarget {
            name: "Other".into(),
            path: Some("other.json".into()),
            schema: app.project.target.clone(),
            options: app.project.target_options.clone(),
            root: app.project.root.clone(),
        });
        app.open_target_tab(0);
        assert!(app.ensure_target_canvas(0));
    }
    app.mapping_workspace.active = document;
    app.mapping_workspace.focused = document;
    app.selected_scope = vec![0; walks.len() + 1];
    app.mark_clean();
    app.rebase_history();
    app
}

fn singleton_input(walks: &[&[&str]]) -> String {
    let rows: Vec<_> = [
        (true, false, "first"),
        (false, true, "second"),
        (true, true, "third"),
    ]
    .into_iter()
    .map(|(flag, ready, child)| {
        let detail = serde_json::json!({"Flag":child,"Ready":ready});
        if walks
            .iter()
            .flat_map(|walk| walk.iter())
            .any(|segment| *segment == "Wrapper")
        {
            serde_json::json!({"Flag":flag,"Wrapper":{"Detail":detail}})
        } else {
            serde_json::json!({"Flag":flag,"Detail":detail})
        }
    })
    .collect();
    serde_json::json!({"Rows":rows,"Other":[{"Flag":false}]}).to_string()
}

fn singleton_expected(walks: &[&[&str]], condition: NodeId) -> serde_json::Value {
    let rows: Vec<_> = [
        (true, false, "first"),
        (false, true, "second"),
        (true, true, "third"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (flag, ready, child))| {
        let present = match condition {
            0 => true,
            2 | 4 => flag,
            3 | 5 => !flag,
            6 => ready,
            8 => index == 0,
            _ => panic!("unexpected condition {condition}"),
        };
        let items = if present {
            vec![serde_json::json!({
                "Value":flag,"Absolute":flag,"Child":child,"Ready":ready,"Position":index + 1
            })]
        } else {
            vec![]
        };
        let mut row = serde_json::json!({"Item":items});
        for walk in walks.iter().skip(1).rev() {
            row = serde_json::json!({(*walk.last().unwrap()):[row]});
        }
        row
    })
    .collect();
    serde_json::json!({(*walks[0].last().unwrap()):rows})
}

#[test]
fn presence_singleton_walk_offers_retained_frames_with_real_controls_and_durable_history()
-> anyhow::Result<()> {
    let walks: &[&[&[&str]]] = &[
        &[&["Rows"], &["Detail"]],
        &[&["Rows"], &["Wrapper"], &["Detail"]],
        &[&["Rows"], &["Wrapper", "Detail"]],
        &[&["Rows", "Wrapper", "Detail"]],
    ];
    for walks in walks {
        for document in [MappingDocument::Main, MappingDocument::Target(0)] {
            let mut app = singleton_app(walks, document);
            let offered = candidates(&app);
            assert_eq!(offered, vec![0, 2, 3, 4, 5, 6, 8]);
            let selected_path = app.selected_scope.clone();
            let original_layout = layout(&app);
            let original_content =
                crate::scope_editor::scope_at_mut(&mut app.project.root, &selected_path).clone();
            let input = singleton_input(walks);
            let context = context();
            let locked = key(&app);
            click(&mut app, &context, false, "Optional presence", false);
            click(&mut app, &context, false, "0: expression", false);
            assert_eq!(key(&app), locked);
            assert!(!app.is_dirty() && !app.can_undo() && !app.history.can_redo());
            let scratch = Scratch::new();
            let mut previous = 0;
            for condition in [2, 3, 4, 5, 6, 8] {
                let before = key(&app);
                choose(&mut app, &context, previous, condition);
                let root = match document {
                    MappingDocument::Target(_) => &mut app.project.extra_targets[0].root,
                    _ => &mut app.project.root,
                };
                let selected = crate::scope_editor::scope_at_mut(root, &selected_path);
                assert_eq!(selected.presence(), Some(condition));
                assert_eq!(selected.bindings, original_content.bindings);
                assert_eq!(selected.children, original_content.children);
                assert_eq!(layout(&app), original_layout);
                assert!(matches!(
                    app.history.next_undo(),
                    Some(HistoryRecord::Command(_))
                ));
                let edited = key(&app);
                app.undo_project();
                assert_eq!(key(&app), before);
                app.redo_project();
                assert_eq!(key(&app), edited);
                app.selected_scope = selected_path.clone();
                let expected = singleton_expected(walks, condition);
                assert_output_and_preview(&mut app, &input, expected.clone());

                let path = scratch.0.join(format!("project-{condition}.json"));
                let outcome = app.save_document_to(&path)?;
                assert!(outcome.validation_issues.is_empty() && outcome.layout_warning.is_none());
                let saved = key(&app);
                let project_bytes = std::fs::read(&path)?;
                let layout_path = crate::layout_store::layout_path(&path);
                let layout_bytes = std::fs::read(&layout_path)?;
                let mut reopened = FerruleApp::default();
                reopened.load_project_from(&path);
                if document == MappingDocument::Target(0) {
                    reopened.open_target_tab(0);
                    assert!(reopened.ensure_target_canvas(0));
                }
                reopened.mapping_workspace.active = document;
                reopened.mapping_workspace.focused = document;
                reopened.selected_scope = selected_path.clone();
                assert_eq!(key(&reopened), saved);
                assert_eq!(layout(&reopened), original_layout);
                assert_eq!(candidates(&reopened), offered);
                frame(&mut reopened, &context, true, vec![]);
                assert_eq!(key(&reopened), saved, "inspection is read-only");
                assert_output_and_preview(&mut reopened, &input, expected);
                assert_eq!(std::fs::read(&path)?, project_bytes);
                assert_eq!(std::fs::read(&layout_path)?, layout_bytes);
                previous = condition;
            }
        }
    }
    Ok(())
}
