use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{AggregateOp, Binding, SequenceExpr};

fn output(project: &Project) -> Result<Value, EngineError> {
    let target = run(project, &Instance::Group(Vec::new()))?;
    target
        .field("result")
        .and_then(Instance::as_scalar)
        .cloned()
        .ok_or_else(|| EngineError::MissingSourceField("result".into()))
}

#[test]
fn generated_sequence_aggregate_filters_in_item_context() {
    let project = Project {
        source: SchemaNode::group("source", Vec::new()),
        target: SchemaNode::group(
            "target",
            vec![SchemaNode::scalar("result", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::Int(5),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::Position {
                        collection: Vec::new(),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    4,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![2, 3],
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::String("|".into()),
                    },
                ),
                (
                    6,
                    Node::SequenceAggregate {
                        function: AggregateOp::Join,
                        sequence: SequenceExpr::Generate {
                            from: None,
                            to: 0,
                            item: 1,
                        },
                        predicate: Some(4),
                        arg: Some(5),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "result".into(),
                node: 6,
            }],
            ..Scope::default()
        },
    };

    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    assert_eq!(output(&project), Ok(Value::String("3|4|5".into())));
}
