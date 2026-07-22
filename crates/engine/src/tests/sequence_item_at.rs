use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, SequenceExpr};

fn project(
    source: SchemaNode,
    target_type: ScalarType,
    nodes: Vec<(NodeId, Node)>,
    output: NodeId,
) -> Project {
    Project {
        source,
        target: SchemaNode::group("target", vec![SchemaNode::scalar("result", target_type)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "result".into(),
                node: output,
            }],
            ..Scope::default()
        },
    }
}

fn output(project: &Project, source: &Instance) -> Result<Value, EngineError> {
    let target = run(project, source)?;
    target
        .field("result")
        .and_then(Instance::as_scalar)
        .cloned()
        .ok_or_else(|| EngineError::MissingSourceField("result".into()))
}

fn generated_item(index: NodeId) -> Project {
    project(
        SchemaNode::group("source", Vec::new()),
        ScalarType::Int,
        vec![
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
                Node::Const {
                    value: Value::Int(3),
                },
            ),
            (
                3,
                Node::SequenceItemAt {
                    sequence: SequenceExpr::Generate {
                        from: None,
                        to: 0,
                        item: 1,
                    },
                    index,
                },
            ),
        ],
        3,
    )
}

#[test]
fn generated_sequence_item_at_is_one_based() {
    let project = generated_item(2);
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    assert_eq!(
        output(&project, &Instance::Group(Vec::new())),
        Ok(Value::Int(3))
    );
}

#[test]
fn generated_sequence_item_at_evaluates_index_in_parent_context() {
    let project = project(
        SchemaNode::group(
            "source",
            vec![
                SchemaNode::scalar("Words", ScalarType::String),
                SchemaNode::scalar("Index", ScalarType::Int),
            ],
        ),
        ScalarType::String,
        vec![
            (
                0,
                Node::SourceField {
                    path: vec!["Words".into()],
                    frame: None,
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String(r"\s+".into()),
                },
            ),
            (
                2,
                Node::SourceField {
                    path: Vec::new(),
                    frame: None,
                },
            ),
            (
                3,
                Node::SourceField {
                    path: vec!["Index".into()],
                    frame: None,
                },
            ),
            (
                4,
                Node::SequenceItemAt {
                    sequence: SequenceExpr::TokenizeRegex {
                        input: 0,
                        pattern: 1,
                        flags: None,
                        item: 2,
                    },
                    index: 3,
                },
            ),
        ],
        4,
    );
    let source = Instance::Group(vec![
        (
            "Words".into(),
            Instance::Scalar(Value::String("alpha beta gamma".into())),
        ),
        ("Index".into(), Instance::Scalar(Value::Int(2))),
    ]);

    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    assert_eq!(output(&project, &source), Ok(Value::String("beta".into())));
}

#[test]
fn generated_sequence_item_at_returns_null_out_of_range() {
    let mut project = generated_item(2);
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Int(8),
        },
    );
    assert_eq!(
        output(&project, &Instance::Group(Vec::new())),
        Ok(Value::Null)
    );

    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::Int(0),
        },
    );
    assert_eq!(
        output(&project, &Instance::Group(Vec::new())),
        Ok(Value::Null)
    );
}

#[test]
fn validation_rejects_item_dependent_parent_inputs_and_shared_owners() {
    let mut project = generated_item(1);
    assert!(validate(&project).iter().any(|issue| {
        issue
            .message
            .contains("index depends on its own sequence item node 1")
    }));

    project.graph.nodes.insert(
        4,
        Node::SequenceItemAt {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 0,
                item: 1,
            },
            index: 2,
        },
    );
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.message.contains("already owned"))
    );
}
