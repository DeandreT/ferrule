use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, ScopeIteration};

fn record(fields: &[(&str, Value)]) -> Instance {
    Instance::Group(
        fields
            .iter()
            .map(|(name, value)| ((*name).into(), Instance::Scalar(value.clone())))
            .collect(),
    )
}

fn repeated(records: Vec<Instance>) -> Instance {
    Instance::Repeated(records)
}

fn join_plan() -> JoinPlan {
    JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(vec!["B".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            vec!["id".into()],
            vec!["aid".into()],
        )),
    )
    .unwrap()
}

fn target(fields: &[(&str, ScalarType)]) -> SchemaNode {
    SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Row",
                fields
                    .iter()
                    .map(|(name, ty)| SchemaNode::scalar(*name, *ty))
                    .collect(),
            )
            .repeating(),
        ],
    )
}

fn project(
    nodes: impl IntoIterator<Item = (NodeId, Node)>,
    plan: JoinPlan,
    bindings: Vec<Binding>,
    fields: &[(&str, ScalarType)],
) -> Project {
    Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: target(fields),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::InnerJoin {
                    id: JoinId::new(7),
                    plan,
                },
                bindings,
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn scalar<'a>(row: &'a Instance, field: &str) -> &'a Value {
    row.field(field)
        .and_then(Instance::as_scalar)
        .unwrap_or_else(|| panic!("missing scalar `{field}`"))
}

#[test]
fn left_deep_inner_join_preserves_duplicates_order_and_raw_positions() {
    let plan = join_plan()
        .then(
            JoinSource::new(vec!["C".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["B".into()],
                vec!["code".into()],
                vec!["code".into()],
            )),
        )
        .unwrap();
    let nodes = [
        (
            0,
            Node::JoinField {
                join: JoinId::new(7),
                collection: vec!["A".into()],
                path: vec!["label".into()],
            },
        ),
        (
            1,
            Node::JoinField {
                join: JoinId::new(7),
                collection: vec!["B".into()],
                path: vec!["tag".into()],
            },
        ),
        (
            2,
            Node::JoinField {
                join: JoinId::new(7),
                collection: vec!["C".into()],
                path: vec!["value".into()],
            },
        ),
        (
            3,
            Node::JoinPosition {
                join: JoinId::new(7),
            },
        ),
        (
            4,
            Node::Position {
                collection: vec!["A".into()],
            },
        ),
        (
            5,
            Node::Position {
                collection: vec!["B".into()],
            },
        ),
        (
            6,
            Node::Position {
                collection: vec!["C".into()],
            },
        ),
    ];
    let bindings = ["A", "B", "C", "JoinPos", "APos", "BPos", "CPos"]
        .into_iter()
        .enumerate()
        .map(|(node, target_field)| Binding {
            target_field: target_field.into(),
            node: node as NodeId,
        })
        .collect();
    let project = project(
        nodes,
        plan,
        bindings,
        &[
            ("A", ScalarType::String),
            ("B", ScalarType::String),
            ("C", ScalarType::String),
            ("JoinPos", ScalarType::Int),
            ("APos", ScalarType::Int),
            ("BPos", ScalarType::Int),
            ("CPos", ScalarType::Int),
        ],
    );
    let source = Instance::Group(vec![
        (
            "A".into(),
            repeated(vec![
                record(&[("id", Value::Int(1)), ("label", Value::String("A1".into()))]),
                record(&[("id", Value::Int(1)), ("label", Value::String("A2".into()))]),
                record(&[("id", Value::Null), ("label", Value::String("AN".into()))]),
            ]),
        ),
        (
            "B".into(),
            repeated(vec![
                record(&[
                    ("aid", Value::String("1".into())),
                    ("code", Value::String("X".into())),
                    ("tag", Value::String("BX".into())),
                ]),
                record(&[
                    ("aid", Value::Int(1)),
                    ("code", Value::String("Y".into())),
                    ("tag", Value::String("BY".into())),
                ]),
                record(&[
                    ("aid", Value::Null),
                    ("code", Value::String("X".into())),
                    ("tag", Value::String("BN".into())),
                ]),
            ]),
        ),
        (
            "C".into(),
            repeated(vec![
                record(&[
                    ("code", Value::String("X".into())),
                    ("value", Value::String("CX1".into())),
                ]),
                record(&[
                    ("code", Value::String("X".into())),
                    ("value", Value::String("CX2".into())),
                ]),
                record(&[
                    ("code", Value::String("Y".into())),
                    ("value", Value::String("CY".into())),
                ]),
            ]),
        ),
    ]);

    let output = run(&project, &source).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 6);
    let tuples: Vec<_> = rows
        .iter()
        .map(|row| {
            (
                scalar(row, "A").clone(),
                scalar(row, "B").clone(),
                scalar(row, "C").clone(),
                scalar(row, "JoinPos").clone(),
                scalar(row, "APos").clone(),
                scalar(row, "BPos").clone(),
                scalar(row, "CPos").clone(),
            )
        })
        .collect();
    assert_eq!(
        tuples,
        vec![
            tuple("A1", "BX", "CX1", 1, 1, 1, 1),
            tuple("A1", "BX", "CX2", 2, 1, 1, 2),
            tuple("A1", "BY", "CY", 3, 1, 2, 3),
            tuple("A2", "BX", "CX1", 4, 2, 1, 1),
            tuple("A2", "BX", "CX2", 5, 2, 1, 2),
            tuple("A2", "BY", "CY", 6, 2, 2, 3),
        ]
    );
}

fn tuple(
    a: &str,
    b: &str,
    c: &str,
    join: i64,
    a_pos: i64,
    b_pos: i64,
    c_pos: i64,
) -> (Value, Value, Value, Value, Value, Value, Value) {
    (
        Value::String(a.into()),
        Value::String(b.into()),
        Value::String(c.into()),
        Value::Int(join),
        Value::Int(a_pos),
        Value::Int(b_pos),
        Value::Int(c_pos),
    )
}

#[test]
fn join_controls_compact_flat_positions_without_changing_raw_positions() {
    let rank = Node::JoinField {
        join: JoinId::new(7),
        collection: vec!["B".into()],
        path: vec!["rank".into()],
    };
    let nodes = [
        (0, rank),
        (
            1,
            Node::Const {
                value: Value::Int(10),
            },
        ),
        (
            2,
            Node::Call {
                function: "greater_than".into(),
                args: vec![0, 1],
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            4,
            Node::JoinPosition {
                join: JoinId::new(7),
            },
        ),
        (
            5,
            Node::Position {
                collection: vec!["A".into()],
            },
        ),
        (
            6,
            Node::Position {
                collection: vec!["B".into()],
            },
        ),
    ];
    let bindings = [4, 5, 6, 0]
        .into_iter()
        .zip(["JoinPos", "APos", "BPos", "Rank"])
        .map(|(node, target_field)| Binding {
            target_field: target_field.into(),
            node,
        })
        .collect();
    let mut project = project(
        nodes,
        join_plan(),
        bindings,
        &[
            ("JoinPos", ScalarType::Int),
            ("APos", ScalarType::Int),
            ("BPos", ScalarType::Int),
            ("Rank", ScalarType::Int),
        ],
    );
    {
        let row_scope = &mut project.root.children[0];
        row_scope.filter = Some(2);
        row_scope.sort_by = Some(0);
        row_scope.sort_descending = true;
        row_scope.take = Some(3);
    }
    let source = Instance::Group(vec![
        (
            "A".into(),
            repeated(vec![
                record(&[("id", Value::Int(1))]),
                record(&[("id", Value::Int(1))]),
            ]),
        ),
        (
            "B".into(),
            repeated(vec![
                record(&[("aid", Value::Int(1)), ("rank", Value::Int(10))]),
                record(&[("aid", Value::Int(1)), ("rank", Value::Int(30))]),
                record(&[("aid", Value::Int(1)), ("rank", Value::Int(20))]),
            ]),
        ),
    ]);

    let output = run(&project, &source).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    let positions: Vec<_> = rows
        .iter()
        .map(|row| {
            (
                scalar(row, "JoinPos").clone(),
                scalar(row, "APos").clone(),
                scalar(row, "BPos").clone(),
                scalar(row, "Rank").clone(),
            )
        })
        .collect();
    assert_eq!(
        positions,
        vec![
            (Value::Int(1), Value::Int(1), Value::Int(2), Value::Int(30)),
            (Value::Int(2), Value::Int(2), Value::Int(2), Value::Int(30)),
            (Value::Int(3), Value::Int(1), Value::Int(3), Value::Int(20)),
        ]
    );

    project.root.children[0].iteration_output = IterationOutput::First;
    project.root.children[0].filter = Some(2);
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Int(100),
        },
    );
    let output = run(&project, &source).unwrap();
    assert!(matches!(output.field("Row"), Some(Instance::Group(fields)) if fields.is_empty()));
}

#[test]
fn static_descendant_can_read_its_parent_join_tuple() {
    let nodes = [
        (
            0,
            Node::JoinField {
                join: JoinId::new(7),
                collection: vec!["A".into()],
                path: vec!["label".into()],
            },
        ),
        (
            1,
            Node::JoinField {
                join: JoinId::new(7),
                collection: vec!["B".into()],
                path: vec!["tag".into()],
            },
        ),
    ];
    let mut project = project(nodes, join_plan(), Vec::new(), &[]);
    project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Row",
                vec![SchemaNode::group(
                    "Static",
                    vec![
                        SchemaNode::scalar("AValue", ScalarType::String),
                        SchemaNode::scalar("BValue", ScalarType::String),
                    ],
                )],
            )
            .repeating(),
        ],
    );
    project.root.children[0].children.push(Scope {
        target_field: "Static".into(),
        bindings: vec![
            Binding {
                target_field: "AValue".into(),
                node: 0,
            },
            Binding {
                target_field: "BValue".into(),
                node: 1,
            },
        ],
        ..Scope::default()
    });
    let source = Instance::Group(vec![
        (
            "A".into(),
            repeated(vec![record(&[
                ("id", Value::Int(1)),
                ("label", Value::String("kept".into())),
            ])]),
        ),
        (
            "B".into(),
            repeated(vec![record(&[
                ("aid", Value::Int(1)),
                ("tag", Value::String("matched".into())),
            ])]),
        ),
    ]);

    let output = run(&project, &source).unwrap();
    let row = &output.field("Row").and_then(Instance::as_repeated).unwrap()[0];
    let static_group = row.field("Static").unwrap();
    assert_eq!(
        scalar(static_group, "AValue"),
        &Value::String("kept".into())
    );
    assert_eq!(
        scalar(static_group, "BValue"),
        &Value::String("matched".into())
    );
}

#[test]
fn runtime_rejects_grouping_a_join_scope() {
    let nodes = [(
        0,
        Node::JoinField {
            join: JoinId::new(7),
            collection: vec!["A".into()],
            path: vec!["id".into()],
        },
    )];
    let mut project = project(nodes, join_plan(), Vec::new(), &[]);
    project.root.children[0].group_by = Some(0);
    let source = Instance::Group(vec![
        ("A".into(), repeated(vec![record(&[("id", Value::Int(1))])])),
        (
            "B".into(),
            repeated(vec![record(&[("aid", Value::Int(1))])]),
        ),
    ]);

    assert_eq!(
        run(&project, &source),
        Err(EngineError::JoinGroupingUnsupported)
    );
}

fn validation_source() -> SchemaNode {
    SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("A", vec![SchemaNode::scalar("id", ScalarType::Int)]).repeating(),
            SchemaNode::group("B", vec![SchemaNode::scalar("aid", ScalarType::Int)]).repeating(),
            SchemaNode::group("C", vec![SchemaNode::scalar("id", ScalarType::Int)]).repeating(),
        ],
    )
}

#[test]
fn validation_rejects_inactive_cross_join_nodes_and_direct_grouping() {
    let nodes = [
        (
            0,
            Node::JoinField {
                join: JoinId::new(7),
                collection: vec!["A".into()],
                path: vec!["id".into()],
            },
        ),
        (
            1,
            Node::JoinField {
                join: JoinId::new(99),
                collection: vec!["A".into()],
                path: vec!["id".into()],
            },
        ),
        (
            2,
            Node::JoinPosition {
                join: JoinId::new(99),
            },
        ),
        (
            3,
            Node::JoinField {
                join: JoinId::new(7),
                collection: vec!["C".into()],
                path: vec!["id".into()],
            },
        ),
        (
            4,
            Node::Call {
                function: "concat".into(),
                args: vec![1, 2, 3],
            },
        ),
    ];
    let mut project = project(
        nodes,
        join_plan(),
        vec![Binding {
            target_field: "Value".into(),
            node: 0,
        }],
        &[("Value", ScalarType::Int)],
    );
    project.source = validation_source();
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));

    let scope = &mut project.root.children[0];
    scope.sort_by = Some(4);
    scope.group_by = Some(0);
    let messages: Vec<_> = validate(&project)
        .into_iter()
        .map(|issue| issue.message)
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| { message.contains("join field node 1 references inactive join 99") })
    );
    assert!(
        messages.iter().any(|message| {
            message.contains("join position node 2 references inactive join 99")
        })
    );
    assert!(messages.iter().any(|message| {
        message.contains("join field node 3 collection `C` does not belong to join 7")
    }));
    assert!(messages.iter().any(|message| {
        message.contains("inner join iteration cannot be combined with grouping controls")
    }));
}

#[test]
fn validation_rejects_duplicate_join_ids_and_invalid_plan_paths() {
    let invalid_plan = JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(vec!["B".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            vec!["missing".into()],
            vec!["also_missing".into()],
        )),
    )
    .unwrap();
    let row = Scope {
        target_field: "Row".into(),
        iteration: ScopeIteration::InnerJoin {
            id: JoinId::new(7),
            plan: invalid_plan.clone(),
        },
        ..Scope::default()
    };
    let other = Scope {
        target_field: "Other".into(),
        iteration: ScopeIteration::InnerJoin {
            id: JoinId::new(7),
            plan: invalid_plan,
        },
        ..Scope::default()
    };
    let project = Project {
        source: validation_source(),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group("Row", Vec::new()).repeating(),
                SchemaNode::group("Other", Vec::new()).repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph: Graph::default(),
        root: Scope {
            children: vec![row, other],
            ..Scope::default()
        },
    };

    let messages: Vec<_> = validate(&project)
        .into_iter()
        .map(|issue| issue.message)
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("join left key `missing`"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("join right key `also_missing`"))
    );
    assert!(
        messages
            .iter()
            .any(|message| { message.contains("join id 7 is already owned") })
    );
}

#[test]
fn validation_enforces_join_dominance_and_terminal_collections() {
    let nodes = [(
        0,
        Node::JoinField {
            join: JoinId::new(7),
            collection: vec!["Outer".into(), "Inner".into()],
            path: vec!["id".into()],
        },
    )];
    let invalid_plan = JoinPlan::new(
        JoinSource::new(vec!["Outer".into(), "Inner".into()]),
        JoinSource::new(vec!["B".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Outer".into(), "Inner".into()],
            vec!["id".into()],
            vec!["aid".into()],
        )),
    )
    .unwrap();
    let mut project = project(nodes, invalid_plan, Vec::new(), &[]);
    project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Outer",
                vec![SchemaNode::group(
                    "Inner",
                    vec![SchemaNode::scalar("id", ScalarType::Int)],
                )],
            )
            .repeating(),
            SchemaNode::group("B", vec![SchemaNode::scalar("aid", ScalarType::Int)]).repeating(),
        ],
    );
    project.root.children[0].take = Some(0);

    let messages: Vec<_> = validate(&project)
        .into_iter()
        .map(|issue| issue.message)
        .collect();
    assert!(messages.iter().any(|message| {
        message.contains("collection `Outer/Inner` is missing or not repeating")
    }));
    assert!(
        messages
            .iter()
            .any(|message| message.contains("join field node 0 references inactive join 7"))
    );
}
