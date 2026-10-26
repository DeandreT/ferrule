//! #449 owned join scopes render one owned join and member reads by identity;
//! #450 joined aggregates render one owned join per aggregate.

#[test]
fn decimal_sum_owned_joined_emission_folds_each_value_immediately() {
    for reached in [true, false] {
        let mut program =
            codegen::lower_with_basic_owned_joins(&joined_aggregate_project()).unwrap();
        let expression = &mut program
            .expressions
            .iter_mut()
            .find(|node| node.id == 1)
            .unwrap()
            .expression;
        let codegen::Expression::OwnedJoinAggregate(aggregate) = expression else {
            panic!("authored joined aggregate");
        };
        aggregate.function = mapping::AggregateOp::DecimalSum;
        aggregate.arg = None;
        if !reached {
            program
                .root
                .bindings
                .retain(|binding| binding.expression != 1);
        }
        let artifacts = crate::emit_with_basic_owned_joins(&program).unwrap();
        if reached {
            let source = artifacts
                .files()
                .iter()
                .find(|file| file.path.as_str() == "GeneratedMapping.cs")
                .unwrap();
            assert!(String::from_utf8_lossy(&source.contents).contains("FerruleAggregates.CollectValue(global::Ferrule.Runtime.FerruleAggregateOperation.DecimalSum, values_1,"));
        }
    }
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn rows(owner: mapping::SourceOwner) -> mapping::OwnedSourceRef {
    mapping::OwnedSourceRef {
        owner,
        path: path(&["Rows"]),
    }
}
fn named() -> mapping::SourceOwner {
    mapping::SourceOwner::Named { name: "Ref".into() }
}

/// Primary `Rows` joined to named `Rows` by id, filtered, sorted and windowed.
fn owned_join_project() -> mapping::Project {
    let source = ir::SchemaNode::group(
        "Input",
        vec![
            ir::SchemaNode::group(
                "Rows",
                vec![
                    ir::SchemaNode::scalar("id", ir::ScalarType::Int),
                    ir::SchemaNode::scalar("code", ir::ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let join = mapping::JoinId::new(4);
    let plan = mapping::OwnedJoinPlan::new(
        mapping::OwnedJoinSource::new(rows(mapping::SourceOwner::Primary)),
        mapping::OwnedJoinSource::new(rows(named())),
        mapping::OwnedJoinConditions::new(mapping::OwnedJoinKey::new(
            rows(mapping::SourceOwner::Primary),
            path(&["id"]),
            path(&["id"]),
        )),
    )
    .expect("authored owned plan");
    let bind = |target_field: &str, node| mapping::Binding {
        target_field: target_field.into(),
        node,
    };
    mapping::Project {
        source: source.clone(),
        target: ir::SchemaNode::group(
            "Output",
            vec![
                ir::SchemaNode::group(
                    "Row",
                    vec![
                        ir::SchemaNode::scalar("code", ir::ScalarType::String),
                        ir::SchemaNode::scalar("at", ir::ScalarType::Int),
                        ir::SchemaNode::scalar("tuple", ir::ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        user_functions: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "Ref".into(),
            path: "ref.json".into(),
            schema: source,
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        root: mapping::Scope {
            children: vec![mapping::Scope {
                target_field: "Row".into(),
                iteration: mapping::ScopeIteration::OwnedInnerJoin { id: join, plan },
                bindings: vec![bind("code", 1), bind("at", 2), bind("tuple", 3)],
                filter: Some(4),
                sort_by: Some(2),
                windows: vec![mapping::SequenceWindow::First { count: 5 }],
                ..Default::default()
            }],
            ..Default::default()
        },
        graph: mapping::Graph {
            nodes: [
                (
                    1,
                    mapping::Node::OwnedJoinField(mapping::OwnedJoinField {
                        join,
                        collection: rows(named()),
                        path: path(&["code"]),
                    }),
                ),
                (
                    2,
                    mapping::Node::OwnedJoinSourcePosition(mapping::OwnedJoinSourcePosition {
                        join,
                        collection: rows(named()),
                    }),
                ),
                (3, mapping::Node::JoinPosition { join }),
                (
                    4,
                    mapping::Node::Const {
                        value: ir::Value::Bool(true),
                    },
                ),
                (
                    5,
                    mapping::Node::Const {
                        value: ir::Value::Int(2),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        failure_rules: Vec::new(),
    }
}

#[test]
fn owned_join_scopes_render_the_owned_pipeline_and_exact_member_reads() {
    let program = codegen::lower_with_basic_owned_joins(&owned_join_project()).unwrap();
    let artifacts = crate::emit_with_basic_owned_joins(&program).unwrap();
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str().ends_with("GeneratedMapping.cs"))
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .unwrap();
    for expected in [
        "context.OwnedInnerJoin(4UL,",
        "new global::Ferrule.Runtime.FerruleOwnedJoinSource(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary, new string[] { \"Rows\" })",
        "new global::Ferrule.Runtime.FerruleOwnedJoinKey[] { new(0, new string[] { \"id\" }, new string[] { \"id\" }), }",
        "context.OwnedJoinField(4UL, global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"), new string[] { \"Rows\" }, new string[] { \"code\" })",
        "context.OwnedJoinSourcePosition(4UL, global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"), new string[] { \"Rows\" })",
        "context.OwnedJoinPosition(4UL)",
        ".WithOwnedJoinPosition(4UL, sorted_",
        ".WithOwnedJoinPosition(4UL, items_",
        "global::Ferrule.Runtime.FerruleSequences.ApplyWindows(candidates_",
    ] {
        assert!(source.contains(expected), "{expected}\n{source}");
    }
    assert_eq!(source.matches("context.OwnedInnerJoin(").count(), 1);
}

#[test]
fn older_csharp_emitters_refuse_owned_joins() {
    let program = codegen::lower_with_basic_owned_joins(&owned_join_project()).unwrap();
    for result in [
        crate::emit(&program),
        crate::emit_with_basic_owned_sources(&program),
        crate::emit_with_basic_owned_lookup_find(&program),
        crate::emit_with_basic_owned_recursive_filter(&program),
    ] {
        assert!(
            matches!(&result, Err(crate::EmitError::ProgramValidation(
                codegen::ProgramValidationError::OwnedSource { diagnostic }))
                if matches!(diagnostic.as_ref(), codegen::OwnedSourceDiagnostic::Capability {
                    reason: codegen::OwnedSourceCapabilityReason::Join, .. })),
            "{result:?}"
        );
    }
}

/// #450: a root `Codes` binding joins each tuple's named code with a parent
/// separator, beside a Count without a tuple expression.
fn joined_aggregate_project() -> mapping::Project {
    let mut project = owned_join_project();
    let mapping::ScopeIteration::OwnedInnerJoin { plan, .. } =
        project.root.children.remove(0).iteration
    else {
        unreachable!("authored owned join scope");
    };
    let join = mapping::JoinId::new(6);
    project.target = ir::SchemaNode::group(
        "Output",
        vec![
            ir::SchemaNode::scalar("Codes", ir::ScalarType::String),
            ir::SchemaNode::scalar("Count", ir::ScalarType::Int),
        ],
    );
    project.graph.nodes = [
        (
            1,
            mapping::Node::OwnedJoinAggregate(mapping::OwnedJoinAggregate {
                function: mapping::AggregateOp::Join,
                join,
                plan: plan.clone(),
                expression: Some(2),
                arg: Some(3),
            }),
        ),
        (
            2,
            mapping::Node::OwnedJoinField(mapping::OwnedJoinField {
                join,
                collection: rows(named()),
                path: path(&["code"]),
            }),
        ),
        (
            3,
            mapping::Node::Const {
                value: ir::Value::String("-".into()),
            },
        ),
        (
            4,
            mapping::Node::OwnedJoinAggregate(mapping::OwnedJoinAggregate {
                function: mapping::AggregateOp::Count,
                join: mapping::JoinId::new(7),
                plan,
                expression: None,
                arg: None,
            }),
        ),
    ]
    .into_iter()
    .collect();
    project.root.bindings = vec![
        mapping::Binding {
            target_field: "Codes".into(),
            node: 1,
        },
        mapping::Binding {
            target_field: "Count".into(),
            node: 4,
        },
    ];
    project
}

#[test]
fn joined_aggregates_render_one_owned_join_tuple_values_and_a_parent_argument() {
    let program = codegen::lower_with_basic_owned_joins(&joined_aggregate_project()).unwrap();
    let artifacts = crate::emit_with_basic_owned_joins(&program).unwrap();
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str().ends_with("GeneratedMapping.cs"))
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .unwrap();
    for expected in [
        "tuples_1 = context.OwnedInnerJoin(6UL,",
        "catch (global::Ferrule.Runtime.FerruleRuntimeException error_1) { error_1.WithOwnedSourceNode(1U); throw; }",
        "values_1.Add(Node_2(tuple_1));",
        "global::Ferrule.Runtime.FerruleValue? argument_1 = Node_3(context);",
        "global::Ferrule.Runtime.FerruleAggregateOperation.Join, values_1, argument_1);",
        "values_4.Add(global::Ferrule.Runtime.FerruleValue.Null);",
        "global::Ferrule.Runtime.FerruleAggregateOperation.Count, values_4, argument_4);",
        "context.OwnedJoinField(6UL, global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"), new string[] { \"Rows\" }, new string[] { \"code\" })",
    ] {
        assert!(source.contains(expected), "{expected}\n{source}");
    }
    assert_eq!(source.matches("context.OwnedInnerJoin(").count(), 2);
}

#[test]
fn older_csharp_emitters_refuse_joined_aggregates() {
    let program = codegen::lower_with_basic_owned_joins(&joined_aggregate_project()).unwrap();
    for result in [
        crate::emit(&program),
        crate::emit_with_basic_owned_sources(&program),
        crate::emit_with_basic_owned_lookup_find(&program),
        crate::emit_with_basic_owned_collection_aggregates(&program),
        crate::emit_with_basic_owned_xml_mixed_content(&program),
        crate::emit_with_basic_owned_xml_mixed_construction(&program),
    ] {
        assert!(
            matches!(&result, Err(crate::EmitError::ProgramValidation(
                codegen::ProgramValidationError::OwnedSource { diagnostic }))
                if matches!(diagnostic.as_ref(), codegen::OwnedSourceDiagnostic::Capability {
                    reason: codegen::OwnedSourceCapabilityReason::Join, .. })),
            "{result:?}"
        );
    }
}
