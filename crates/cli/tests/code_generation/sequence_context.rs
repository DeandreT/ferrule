use super::*;
use mapping::{
    AggregateOp, FailureIteration, FailureRule, FailureSelection, SequenceExpr, SequenceWindow,
};

fn item() -> Node {
    Node::SourceField {
        path: Vec::new(),
        frame: None,
    }
}

fn range(item: u32, to: u32) -> SequenceExpr {
    SequenceExpr::Generate {
        from: None,
        to,
        item,
    }
}

fn project_with_parent_reductions() -> Project {
    let mut project = project();
    project.source = SchemaNode::group("Input", vec![int("Limit")]);
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    int("Outer"),
                    int("Sum"),
                    int("SquareSum"),
                    int("ItemAt"),
                    bool_("Exists"),
                    int("IndependentItemAt"),
                    SchemaNode::group("Children", vec![int("Value")]).repeating(),
                ],
            )
            .repeating(),
        ],
    );
    project.graph.nodes = BTreeMap::from([
        (
            1,
            Node::SourceField {
                path: vec!["Limit".into()],
                frame: None,
            },
        ),
        (
            2,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            4,
            Node::Const {
                value: Value::Int(0),
            },
        ),
        (10, item()),
        (20, item()),
        (
            21,
            Node::SequenceAggregate {
                function: AggregateOp::Sum,
                sequence: range(20, 10),
                predicate: None,
                expression: None,
                arg: None,
            },
        ),
        (30, item()),
        (
            31,
            Node::SequenceAggregate {
                function: AggregateOp::Sum,
                sequence: range(30, 10),
                predicate: None,
                expression: Some(32),
                arg: None,
            },
        ),
        (
            32,
            Node::Call {
                function: "multiply".into(),
                args: vec![30, 30],
            },
        ),
        (40, item()),
        (
            41,
            Node::SequenceAggregate {
                function: AggregateOp::ItemAt,
                sequence: range(40, 1),
                predicate: None,
                expression: None,
                arg: Some(10),
            },
        ),
        (50, item()),
        (60, item()),
        (
            61,
            Node::SequenceExists {
                sequence: range(60, 1),
                predicate: 3,
            },
        ),
        (70, item()),
        (
            71,
            Node::SequenceItemAt {
                sequence: range(70, 1),
                index: 2,
            },
        ),
        (80, item()),
    ]);
    project.failure_rules = vec![FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: range(80, 4),
        },
        selection: FailureSelection::All,
        message: None,
    }];
    project.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Sequence(range(10, 1)),
            bindings: [
                ("Outer", 10),
                ("Sum", 21),
                ("SquareSum", 31),
                ("ItemAt", 41),
                ("Exists", 61),
                ("IndependentItemAt", 71),
            ]
            .into_iter()
            .map(|(target_field, node)| Binding {
                target_field: target_field.into(),
                node,
            })
            .collect(),
            children: vec![Scope {
                target_field: "Children".into(),
                iteration: ScopeIteration::Sequence(range(50, 10)),
                windows: vec![SequenceWindow::First { count: 10 }],
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 10,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    project
}

#[test]
fn generated_parent_sequence_context_matches_interpreter_in_rust_and_csharp() -> TestResult<()> {
    let project = project_with_parent_reductions();
    assert!(engine::validate(&project).is_empty());
    let mut cases = Vec::new();
    for limit in [0, 1, 3] {
        let input = serde_json::json!({"Limit": limit});
        let source = format_json::from_str(&input.to_string(), &project.source)?;
        let output = engine::run(&project, &source)?;
        let actual: serde_json::Value =
            serde_json::from_str(&format_json::to_string(&project.target, &output)?)?;
        let rows: Vec<_> = (1..=limit)
            .map(|outer| {
                let children: Vec<_> = (1..=outer)
                    .map(|value| serde_json::json!({"Value": value}))
                    .collect();
                serde_json::json!({
                    "Outer": outer,
                    "Sum": outer * (outer + 1) / 2,
                    "SquareSum": outer * (outer + 1) * (2 * outer + 1) / 6,
                    "ItemAt": outer,
                    "Exists": true,
                    "IndependentItemAt": 1,
                    "Children": children,
                })
            })
            .collect();
        let expected = serde_json::json!({"Rows": rows});
        assert_eq!(actual, expected);
        cases.push(serde_json::json!({"input": input.to_string(), "expected": expected}));
    }
    super::json_text_boundaries::run_generated_boundary_cases(
        &project,
        &cases,
        "parent_sequence_context",
    )
}
