use super::*;
use crate::{
    FailureIteration, FailureRule, FailureSelection, GroupingPlan, InnerJoin, JoinConditions,
    JoinId, JoinKey, JoinKeySide, JoinPlan, JoinPlanError, JoinSource, NamedSourceProgram,
    NamedTargetProgram,
};

fn plan(left_path: &[&str], right_collection: &[&str], right_path: &[&str]) -> JoinPlan {
    JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(
            right_collection
                .iter()
                .map(|segment| (*segment).into())
                .collect(),
        ),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            left_path.iter().map(|segment| (*segment).into()).collect(),
            right_path.iter().map(|segment| (*segment).into()).collect(),
        )),
    )
    .unwrap()
}

fn join_program() -> Program {
    let mut program = program();
    program.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "A",
                vec![
                    SchemaNode::scalar("id", ScalarType::Int),
                    SchemaNode::scalar("label", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    program.extra_sources.push(NamedSourceProgram {
        name: "Catalog".into(),
        source: SchemaNode::group(
            "CatalogDocument",
            vec![
                SchemaNode::group(
                    "B",
                    vec![
                        SchemaNode::scalar("aid", ScalarType::Int),
                        SchemaNode::scalar("label", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
    });
    program.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Row",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::scalar("Position", ScalarType::Int),
                    SchemaNode::group(
                        "Static",
                        vec![SchemaNode::scalar("Echo", ScalarType::String)],
                    ),
                ],
            )
            .repeating(),
        ],
    );
    program.expressions = vec![
        ExpressionNode {
            id: 1,
            expression: Expression::JoinField {
                join: JoinId::new(7),
                collection: vec!["A".into()],
                path: vec!["label".into()],
            },
        },
        ExpressionNode {
            id: 2,
            expression: Expression::JoinPosition {
                join: JoinId::new(7),
            },
        },
        ExpressionNode {
            id: 3,
            expression: Expression::Const {
                value: Value::Bool(true),
            },
        },
        ExpressionNode {
            id: 4,
            expression: Expression::Const {
                value: Value::Int(2),
            },
        },
    ];
    program.root = TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: vec![TargetScope {
            target_field: "Row".into(),
            repeating: true,
            iteration: Some(IterationPlan::new(
                InnerJoin::new(JoinId::new(7), plan(&["id"], &["Catalog", "B"], &["aid"])),
                Some(3),
                Some(SortPlan::new(
                    SortKey {
                        expression: 1,
                        descending: false,
                    },
                    Vec::new(),
                    SortFilterOrder::SortThenFilter,
                )),
                vec![SequenceWindow::First { count: 4 }],
                IterationOutput::Repeated,
            )),
            construction: TargetConstruction::Group,
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    expression: 1,
                    target_type: ScalarType::String,
                    repeating: false,
                },
                Binding {
                    target_field: "Position".into(),
                    expression: 2,
                    target_type: ScalarType::Int,
                    repeating: false,
                },
            ],
            children: vec![TargetScope {
                target_field: "Static".into(),
                repeating: false,
                iteration: None,
                construction: TargetConstruction::Group,
                bindings: vec![Binding {
                    target_field: "Echo".into(),
                    expression: 1,
                    target_type: ScalarType::String,
                    repeating: false,
                }],
                children: Vec::new(),
            }],
        }],
    };
    program
}

fn set_join(program: &mut Program, join_plan: JoinPlan) {
    program.root.children[0].iteration = Some(IterationPlan::join(InnerJoin::new(
        JoinId::new(7),
        join_plan,
    )));
}

fn correlated_join_aggregate_program() -> Program {
    let join = InnerJoin::new(
        JoinId::new(8),
        JoinPlan::new(
            JoinSource::singleton(vec!["Sku".into()]),
            JoinSource::new(vec!["Catalog".into(), "Product".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["Sku".into()],
                Vec::new(),
                vec!["Sku".into()],
            )),
        )
        .expect("correlated join plan"),
    );
    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Line",
                    vec![
                        SchemaNode::scalar("Sku", ScalarType::String),
                        SchemaNode::scalar("Quantity", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
        extra_sources: vec![NamedSourceProgram {
            name: "Catalog".into(),
            source: SchemaNode::group(
                "Catalog",
                vec![
                    SchemaNode::group(
                        "Product",
                        vec![
                            SchemaNode::scalar("Sku", ScalarType::String),
                            SchemaNode::scalar("Price", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                ],
            ),
        }],
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group("Row", vec![SchemaNode::scalar("Total", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        expressions: vec![
            ExpressionNode {
                id: 20,
                expression: Expression::JoinField {
                    join: JoinId::new(8),
                    collection: vec!["Catalog".into(), "Product".into()],
                    path: vec!["Price".into()],
                },
            },
            ExpressionNode {
                id: 21,
                expression: Expression::JoinAggregate {
                    function: AggregateFunction::Sum,
                    join,
                    expression: Some(20),
                    arg: None,
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: Vec::new(),
            children: vec![TargetScope {
                target_field: "Row".into(),
                repeating: true,
                iteration: Some(IterationPlan::source(vec!["Line".into()])),
                construction: TargetConstruction::Group,
                bindings: vec![Binding {
                    target_field: "Total".into(),
                    expression: 21,
                    target_type: ScalarType::Int,
                    repeating: false,
                }],
                children: Vec::new(),
            }],
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn validates_root_join_controls_and_static_descendants() {
    assert_eq!(validate_program(&join_program()), Ok(()));
}

#[test]
fn validates_only_bounded_correlated_join_aggregates() {
    let program = correlated_join_aggregate_program();
    assert_eq!(validate_program(&program), Ok(()));

    let mut grouped = program.clone();
    grouped.expressions.push(ExpressionNode {
        id: 24,
        expression: Expression::SourceField {
            frame: None,
            path: vec!["Sku".into()],
        },
    });
    grouped.root.children[0].iteration = Some(
        IterationPlan::source(vec!["Line".into()]).with_grouping(GroupingPlan::By { key: 24 }),
    );
    assert_eq!(
        validate_program(&grouped),
        Err(ProgramValidationError::JoinAggregateRequiresRootContext {
            node: 21,
            join: JoinId::new(8),
        })
    );

    let mut all_repeating = program.clone();
    let Some(ExpressionNode {
        expression: Expression::JoinAggregate { join, .. },
        ..
    }) = all_repeating
        .expressions
        .iter_mut()
        .find(|expression| expression.id == 21)
    else {
        panic!("correlated join aggregate fixture");
    };
    *join = InnerJoin::new(
        JoinId::new(8),
        JoinPlan::new(
            JoinSource::new(vec!["Line".into()]),
            JoinSource::new(vec!["Catalog".into(), "Product".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["Line".into()],
                vec!["Sku".into()],
                vec!["Sku".into()],
            )),
        )
        .expect("all-repeating join plan"),
    );
    assert_eq!(
        validate_program(&all_repeating),
        Err(ProgramValidationError::JoinAggregateRequiresRootContext {
            node: 21,
            join: JoinId::new(8),
        })
    );

    let mut generated = program;
    generated.expressions.extend([
        ExpressionNode {
            id: 22,
            expression: Expression::Const {
                value: Value::Int(1),
            },
        },
        ExpressionNode {
            id: 23,
            expression: Expression::SourceField {
                frame: None,
                path: Vec::new(),
            },
        },
    ]);
    generated.root.children[0].iteration =
        Some(IterationPlan::generated(crate::GeneratedSequence::Range {
            from: Some(22),
            to: 22,
            item: 23,
        }));
    assert_eq!(
        validate_program(&generated),
        Err(ProgramValidationError::JoinAggregateRequiresRootContext {
            node: 21,
            join: JoinId::new(8),
        })
    );
}

#[test]
fn validates_join_sources_and_both_key_sides() {
    let mut program = join_program();
    set_join(
        &mut program,
        plan(&["id"], &["Catalog", "Missing"], &["aid"]),
    );
    assert!(matches!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidJoinSource { join, .. })
            if join == JoinId::new(7)
    ));

    set_join(
        &mut program,
        plan(&["missing"], &["Catalog", "B"], &["aid"]),
    );
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidJoinKey {
            join: JoinId::new(7),
            side: JoinKeySide::Left,
            collection: vec!["A".into()],
            path: vec!["missing".into()],
        })
    );

    set_join(&mut program, plan(&["id"], &["Catalog", "B"], &["missing"]));
    assert!(matches!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidJoinKey {
            side: JoinKeySide::Right,
            ..
        })
    ));
}

#[test]
fn validates_join_field_owner_collection_and_scalar_path() {
    let mut program = join_program();
    program.expressions[0].expression = Expression::JoinField {
        join: JoinId::new(7),
        collection: vec!["Missing".into()],
        path: vec!["label".into()],
    };
    assert!(matches!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidJoinFieldCollection { node: 1, .. })
    ));

    program.expressions[0].expression = Expression::JoinField {
        join: JoinId::new(7),
        collection: vec!["A".into()],
        path: vec!["missing".into()],
    };
    assert!(matches!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidJoinFieldPath { node: 1, .. })
    ));

    program.expressions[0].expression = Expression::JoinField {
        join: JoinId::new(99),
        collection: vec!["A".into()],
        path: vec!["label".into()],
    };
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InactiveJoinExpression {
            node: 1,
            join: JoinId::new(99),
        })
    );
}

#[test]
fn validates_root_join_aggregate_plan_tuple_expression_and_parent_argument() {
    let mut program = join_program();
    let join = program.root.children[0]
        .iteration
        .as_ref()
        .and_then(IterationPlan::inner_join)
        .cloned()
        .expect("join fixture");
    program.expressions.push(ExpressionNode {
        id: 12,
        expression: Expression::JoinAggregate {
            function: AggregateFunction::Sum,
            join: join.clone(),
            expression: Some(1),
            arg: None,
        },
    });
    let row = program.target.child("Row").cloned().expect("row target");
    program.target = SchemaNode::group(
        "Target",
        vec![row, SchemaNode::scalar("Total", ScalarType::Int)],
    );
    program.root.bindings.push(Binding {
        target_field: "Total".into(),
        expression: 12,
        target_type: ScalarType::Int,
        repeating: false,
    });
    assert_eq!(validate_program(&program), Ok(()));

    let mut tuple_argument = program.clone();
    let Some(ExpressionNode {
        expression: Expression::JoinAggregate { arg, .. },
        ..
    }) = tuple_argument
        .expressions
        .iter_mut()
        .find(|node| node.id == 12)
    else {
        panic!("join aggregate fixture");
    };
    *arg = Some(1);
    assert_eq!(
        validate_program(&tuple_argument),
        Err(ProgramValidationError::InactiveJoinExpression {
            node: 1,
            join: JoinId::new(7),
        })
    );

    let mut invalid_plan = program.clone();
    let Some(ExpressionNode {
        expression: Expression::JoinAggregate {
            join: invalid_join, ..
        },
        ..
    }) = invalid_plan
        .expressions
        .iter_mut()
        .find(|node| node.id == 12)
    else {
        panic!("join aggregate fixture");
    };
    *invalid_join = InnerJoin::new(
        JoinId::new(7),
        plan(&["id"], &["Catalog", "Missing"], &["aid"]),
    );
    assert!(matches!(
        validate_program(&invalid_plan),
        Err(ProgramValidationError::InvalidJoinSource { join, .. })
            if join == JoinId::new(7)
    ));

    let mut nested_reducer = program.clone();
    nested_reducer.expressions.push(ExpressionNode {
        id: 13,
        expression: Expression::Aggregate {
            function: AggregateFunction::Count,
            collection: vec!["A".into()],
            value: AggregateValue::Expression(12),
            arg: None,
        },
    });
    nested_reducer.root.bindings[0].expression = 13;
    assert_eq!(
        validate_program(&nested_reducer),
        Err(ProgramValidationError::JoinAggregateRequiresRootContext {
            node: 12,
            join: JoinId::new(7),
        })
    );

    let mut correlated = program;
    correlated.root.bindings.clear();
    correlated.root.children[0].iteration = Some(IterationPlan::new(
        join,
        Some(12),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    assert_eq!(
        validate_program(&correlated),
        Err(ProgramValidationError::JoinAggregateRequiresRootContext {
            node: 12,
            join: JoinId::new(7),
        })
    );
}

#[test]
fn join_windows_and_failure_rules_use_the_parent_context() {
    let mut program = join_program();
    let iteration = program.root.children[0]
        .iteration
        .as_mut()
        .expect("join iteration");
    *iteration = IterationPlan::new(
        iteration.inner_join().expect("join").clone(),
        None,
        None,
        vec![SequenceWindow::First { count: 2 }],
        IterationOutput::Repeated,
    );
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InactiveJoinExpression {
            node: 2,
            join: JoinId::new(7),
        })
    );

    let mut program = join_program();
    program.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
        selection: FailureSelection::WhenTrue(2),
        message: None,
    });
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InactiveJoinExpression {
            node: 2,
            join: JoinId::new(7),
        })
    );
}

#[test]
fn rejects_duplicate_and_nested_join_owners() {
    let mut duplicate = join_program();
    duplicate.extra_targets.push(NamedTargetProgram {
        name: "Audit".into(),
        target: duplicate.target.clone(),
        root: duplicate.root.clone(),
    });
    assert_eq!(
        validate_program(&duplicate),
        Err(ProgramValidationError::DuplicateJoinOwner {
            join: JoinId::new(7),
        })
    );

    let mut nested = join_program();
    let row = nested.root.children.remove(0);
    let row_schema = nested.target.child("Row").expect("row").clone();
    nested.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::group("Outer", vec![row_schema]).repeating()],
    );
    nested.root.children.push(TargetScope {
        target_field: "Outer".into(),
        repeating: true,
        iteration: Some(IterationPlan::source(vec!["A".into()])),
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: vec![row],
    });
    assert_eq!(
        validate_program(&nested),
        Err(ProgramValidationError::JoinRequiresRootContext {
            target_path: vec!["Outer".into(), "Row".into()],
            join: JoinId::new(7),
        })
    );
}

#[test]
fn join_plan_constructors_reject_duplicate_and_forward_sources() {
    let duplicate = JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(vec!["A".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            vec!["id".into()],
            vec!["id".into()],
        )),
    );
    assert!(matches!(
        duplicate,
        Err(JoinPlanError::DuplicateCollection(_))
    ));

    let forward = JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(vec!["B".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["C".into()],
            vec!["id".into()],
            vec!["id".into()],
        )),
    );
    assert!(matches!(
        forward,
        Err(JoinPlanError::UnknownLeftCollection(_))
    ));
}
