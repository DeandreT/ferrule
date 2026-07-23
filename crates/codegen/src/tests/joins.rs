use crate::{AggregateFunction, ExpressionNode, UnsupportedNodeKind};
use mapping::{
    JoinConditions as MappingJoinConditions, JoinId as MappingJoinId, JoinKey as MappingJoinKey,
    JoinPlan as MappingJoinPlan, JoinSource as MappingJoinSource, NamedSource,
};

use super::*;

fn join_project() -> Project {
    let plan = MappingJoinPlan::new(
        MappingJoinSource::new(vec!["A".into()]),
        MappingJoinSource::new(vec!["Catalog".into(), "B".into()]),
        MappingJoinConditions::new(MappingJoinKey::new(
            vec!["A".into()],
            vec!["id".into()],
            vec!["aid".into()],
        ))
        .and(MappingJoinKey::new(
            vec!["A".into()],
            vec!["region".into()],
            vec!["region".into()],
        )),
    )
    .and_then(|plan| {
        plan.then(
            MappingJoinSource::singleton(vec!["Config".into(), "Code".into()]),
            MappingJoinConditions::new(MappingJoinKey::new(
                vec!["Catalog".into(), "B".into()],
                vec!["code".into()],
                Vec::new(),
            )),
        )
    })
    .unwrap();
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "A",
                vec![
                    typed_scalar("id", ScalarType::Int),
                    scalar("region"),
                    scalar("label"),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Row",
                vec![
                    scalar("Left"),
                    scalar("Right"),
                    scalar("Code"),
                    typed_scalar("Position", ScalarType::Int),
                    SchemaNode::group("Static", vec![scalar("Echo")]),
                ],
            )
            .repeating(),
        ],
    );
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                1,
                Node::JoinField {
                    join: MappingJoinId::new(7),
                    collection: vec!["A".into()],
                    path: vec!["label".into()],
                },
            ),
            (
                2,
                Node::JoinField {
                    join: MappingJoinId::new(7),
                    collection: vec!["Catalog".into(), "B".into()],
                    path: vec!["tag".into()],
                },
            ),
            (
                3,
                Node::JoinField {
                    join: MappingJoinId::new(7),
                    collection: vec!["Config".into(), "Code".into()],
                    path: Vec::new(),
                },
            ),
            (
                4,
                Node::JoinPosition {
                    join: MappingJoinId::new(7),
                },
            ),
            (
                5,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
            (
                6,
                Node::Const {
                    value: Value::Int(2),
                },
            ),
        ]),
    };
    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: "Catalog".into(),
                path: "catalog.json".into(),
                schema: SchemaNode::group(
                    "CatalogDocument",
                    vec![
                        SchemaNode::group(
                            "B",
                            vec![
                                typed_scalar("aid", ScalarType::Int),
                                scalar("region"),
                                scalar("code"),
                                scalar("tag"),
                            ],
                        )
                        .repeating(),
                    ],
                ),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: "Config".into(),
                path: "config.json".into(),
                schema: SchemaNode::group("ConfigDocument", vec![scalar("Code")]),
                options: Default::default(),
                dynamic_path: None,
            },
        ],
        extra_targets: Vec::new(),
        user_functions: BTreeMap::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::InnerJoin {
                    id: MappingJoinId::new(7),
                    plan,
                },
                filter: Some(5),
                sort_by: Some(2),
                sort_descending: true,
                windows: vec![mapping::SequenceWindow::First { count: 6 }],
                bindings: vec![
                    MappingBinding {
                        target_field: "Left".into(),
                        node: 1,
                    },
                    MappingBinding {
                        target_field: "Right".into(),
                        node: 2,
                    },
                    MappingBinding {
                        target_field: "Code".into(),
                        node: 3,
                    },
                    MappingBinding {
                        target_field: "Position".into(),
                        node: 4,
                    },
                ],
                children: vec![Scope {
                    target_field: "Static".into(),
                    bindings: vec![MappingBinding {
                        target_field: "Echo".into(),
                        node: 1,
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn correlated_aggregate_project() -> Project {
    let join = MappingJoinId::new(8);
    let plan = MappingJoinPlan::new(
        MappingJoinSource::singleton(vec!["Sku".into()]),
        MappingJoinSource::new(vec!["Catalog".into(), "Product".into()]),
        MappingJoinConditions::new(MappingJoinKey::new(
            vec!["Sku".into()],
            Vec::new(),
            vec!["Sku".into()],
        )),
    )
    .expect("correlated join plan");
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Line",
                    vec![scalar("Sku"), typed_scalar("Quantity", ScalarType::Int)],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group("Row", vec![typed_scalar("Total", ScalarType::Int)]).repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Catalog".into(),
            path: "catalog.json".into(),
            schema: SchemaNode::group(
                "Catalog",
                vec![
                    SchemaNode::group(
                        "Product",
                        vec![scalar("Sku"), typed_scalar("Price", ScalarType::Int)],
                    )
                    .repeating(),
                ],
            ),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        user_functions: BTreeMap::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    20,
                    Node::JoinField {
                        join,
                        collection: vec!["Catalog".into(), "Product".into()],
                        path: vec!["Price".into()],
                    },
                ),
                (
                    21,
                    Node::JoinAggregate {
                        function: mapping::AggregateOp::Sum,
                        join,
                        plan,
                        expression: Some(20),
                        arg: None,
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Line".into()]),
                bindings: vec![MappingBinding {
                    target_field: "Total".into(),
                    node: 21,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn lowers_left_deep_composite_named_join_and_tuple_expressions() {
    let project = join_project();
    let program = lower(&project).expect("root-context joins are portable");
    let iteration = program.root.children[0]
        .iteration
        .as_ref()
        .expect("join iteration");
    let join = iteration.inner_join().expect("inner join");

    assert_eq!(join.id(), crate::JoinId::new(7));
    assert_eq!(join.plan().sources().count(), 3);
    let stages = join.plan().stages().collect::<Vec<_>>();
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0].1.iter().count(), 2);
    assert_eq!(
        stages[1].0.cardinality(),
        crate::JoinSourceCardinality::Singleton
    );
    assert_eq!(iteration.filter(), Some(5));
    assert_eq!(iteration.windows().len(), 1);
    assert!(matches!(
        program.expressions.first().map(|node| &node.expression),
        Some(Expression::JoinField { join, collection, path })
            if *join == crate::JoinId::new(7)
                && collection == &["A"]
                && path == &["label"]
    ));
    assert!(matches!(
        program.expressions.get(3).map(|node| &node.expression),
        Some(Expression::JoinPosition { join }) if *join == crate::JoinId::new(7)
    ));
    assert_eq!(
        program.root.children[0].children[0].bindings[0].expression,
        1
    );
}

#[test]
fn lowers_bounded_correlated_join_aggregates() {
    let program = lower(&correlated_aggregate_project()).expect("correlated aggregate lowers");

    assert!(matches!(
        program.expressions.iter().find(|node| node.id == 21),
        Some(ExpressionNode {
            expression: Expression::JoinAggregate {
                function: AggregateFunction::Sum,
                join,
                expression: Some(20),
                arg: None,
            },
            ..
        }) if join.id() == crate::JoinId::new(8)
            && join.plan().sources().count() == 2
            && join.plan().sources().any(|source| {
                source.cardinality() == crate::JoinSourceCardinality::Singleton
                    && source.collection() == ["Sku"]
            })
    ));
}

#[test]
fn rejects_join_below_an_active_iteration_at_its_target_path() {
    let mut project = join_project();
    let row = project.root.children.remove(0);
    project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group("Outer", vec![project.target.child("Row").unwrap().clone()])
                .repeating(),
        ],
    );
    project.root.children.push(Scope {
        target_field: "Outer".into(),
        iteration: ScopeIteration::Source(vec!["A".into()]),
        children: vec![row],
        ..Scope::default()
    });

    let diagnostics = lower(&project)
        .expect_err("correlated joins stay interpreter-only")
        .into_diagnostics();
    assert_eq!(
        diagnostics,
        vec![Diagnostic::UnsupportedScope {
            target_path: vec!["Outer".into(), "Row".into()],
            feature: ScopeFeature::CorrelatedInnerJoin,
        }]
    );
}

#[test]
fn lowers_root_join_aggregates_with_tuple_and_parent_expressions() {
    let mut project = join_project();
    let ScopeIteration::InnerJoin { id, plan } = &project.root.children[0].iteration else {
        panic!("join scope");
    };
    project.graph.nodes.insert(
        10,
        Node::JoinAggregate {
            function: mapping::AggregateOp::Count,
            join: *id,
            plan: plan.clone(),
            expression: None,
            arg: None,
        },
    );
    project.graph.nodes.extend([
        (
            11,
            Node::Call {
                function: "concat".into(),
                args: vec![1, 2],
            },
        ),
        (
            12,
            Node::Const {
                value: Value::String("|".into()),
            },
        ),
        (
            13,
            Node::JoinAggregate {
                function: mapping::AggregateOp::Join,
                join: *id,
                plan: plan.clone(),
                expression: Some(11),
                arg: Some(12),
            },
        ),
    ]);
    project.target = SchemaNode::group(
        "Target",
        vec![
            typed_scalar("Count", ScalarType::Int),
            typed_scalar("Joined", ScalarType::String),
        ],
    );
    project.root = Scope {
        bindings: vec![
            MappingBinding {
                target_field: "Count".into(),
                node: 10,
            },
            MappingBinding {
                target_field: "Joined".into(),
                node: 13,
            },
        ],
        ..Scope::default()
    };

    let program = lower(&project).expect("root join aggregates lower");
    assert!(matches!(
        program.expressions.iter().find(|node| node.id == 10),
        Some(ExpressionNode {
            expression: Expression::JoinAggregate {
                function: AggregateFunction::Count,
                join,
                expression: None,
                arg: None,
            },
            ..
        }) if join.id() == crate::JoinId::new(7) && join.plan().sources().count() == 3
    ));
    assert!(matches!(
        program.expressions.iter().find(|node| node.id == 13),
        Some(ExpressionNode {
            expression: Expression::JoinAggregate {
                function: AggregateFunction::Join,
                join,
                expression: Some(11),
                arg: Some(12),
            },
            ..
        }) if join.id() == crate::JoinId::new(7)
    ));
}

#[test]
fn reports_correlated_join_aggregates_at_the_owning_node() {
    let mut project = join_project();
    let ScopeIteration::InnerJoin { id, plan } = &project.root.children[0].iteration else {
        panic!("join scope");
    };
    project.graph.nodes.insert(
        10,
        Node::JoinAggregate {
            function: mapping::AggregateOp::Count,
            join: *id,
            plan: plan.clone(),
            expression: None,
            arg: None,
        },
    );
    project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::group("Outer", vec![typed_scalar("Count", ScalarType::Int)]).repeating()],
    );
    project.root = Scope {
        children: vec![Scope {
            target_field: "Outer".into(),
            iteration: ScopeIteration::Source(vec!["A".into()]),
            bindings: vec![MappingBinding {
                target_field: "Count".into(),
                node: 10,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };

    let diagnostics = lower(&project)
        .expect_err("unbounded correlated join aggregate remains interpreter-only")
        .into_diagnostics();
    assert_eq!(
        diagnostics,
        vec![Diagnostic::UnsupportedNode {
            node: 10,
            kind: UnsupportedNodeKind::CorrelatedJoinAggregate,
        }]
    );
}
