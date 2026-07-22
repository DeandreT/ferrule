use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, SequenceExpr};

fn project(nodes: Vec<(NodeId, Node)>, output: NodeId) -> Project {
    Project {
        source: SchemaNode::group("source", vec![]),
        target: SchemaNode::group(
            "target",
            vec![SchemaNode::scalar("result", ScalarType::Bool)],
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

fn tokenize_exists(input: &str, needle: &str) -> Project {
    project(
        vec![
            (
                0,
                Node::Const {
                    value: Value::String(input.into()),
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String(",".into()),
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
                Node::Const {
                    value: Value::String(needle.into()),
                },
            ),
            (
                4,
                Node::Call {
                    function: "equal".into(),
                    args: vec![2, 3],
                },
            ),
            (
                5,
                Node::SequenceExists {
                    sequence: SequenceExpr::Tokenize {
                        input: 0,
                        delimiter: 1,
                        item: 2,
                    },
                    predicate: 4,
                },
            ),
        ],
        5,
    )
}

fn output(project: &Project) -> Result<Value, EngineError> {
    let target = run(project, &Instance::Group(Vec::new()))?;
    target
        .field("result")
        .and_then(Instance::as_scalar)
        .cloned()
        .ok_or_else(|| EngineError::MissingSourceField("result".into()))
}

#[test]
fn sequence_exists_matches_any_generated_item() {
    let matching = tokenize_exists("alpha,beta,gamma", "beta");
    assert!(validate(&matching).is_empty(), "{:?}", validate(&matching));
    assert_eq!(output(&matching), Ok(Value::Bool(true)));
    assert_eq!(
        output(&tokenize_exists("alpha,beta,gamma", "delta")),
        Ok(Value::Bool(false))
    );
    assert_eq!(
        output(&tokenize_exists("", "alpha")),
        Ok(Value::Bool(false))
    );
}

#[test]
fn sequence_exists_exposes_one_based_item_position() {
    let project = project(
        vec![
            (
                0,
                Node::Const {
                    value: Value::Int(3),
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
                    function: "equal".into(),
                    args: vec![2, 3],
                },
            ),
            (
                5,
                Node::SequenceExists {
                    sequence: SequenceExpr::Generate {
                        from: None,
                        to: 0,
                        item: 1,
                    },
                    predicate: 4,
                },
            ),
        ],
        5,
    );
    assert_eq!(output(&project), Ok(Value::Bool(true)));
}

#[test]
fn null_arguments_and_empty_ranges_produce_no_matches() {
    let base = vec![
        (0, Node::Const { value: Value::Null }),
        (
            1,
            Node::Const {
                value: Value::String(",".into()),
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
            Node::Const {
                value: Value::Bool(true),
            },
        ),
    ];
    let mut tokenize_nodes = base.clone();
    tokenize_nodes.push((
        4,
        Node::SequenceExists {
            sequence: SequenceExpr::Tokenize {
                input: 0,
                delimiter: 1,
                item: 2,
            },
            predicate: 3,
        },
    ));
    assert_eq!(output(&project(tokenize_nodes, 4)), Ok(Value::Bool(false)));

    let mut range_nodes = base;
    range_nodes[0] = (
        0,
        Node::Const {
            value: Value::Int(0),
        },
    );
    range_nodes.push((
        4,
        Node::SequenceExists {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 0,
                item: 2,
            },
            predicate: 3,
        },
    ));
    assert_eq!(output(&project(range_nodes, 4)), Ok(Value::Bool(false)));
}

#[test]
fn sequence_exists_short_circuits_after_a_match() {
    let mut project = tokenize_exists("hit,bad", "hit");
    project.graph.nodes.extend([
        (
            6,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            7,
            Node::Const {
                value: Value::Int(0),
            },
        ),
        (
            8,
            Node::Call {
                function: "divide".into(),
                args: vec![6, 7],
            },
        ),
        (
            9,
            Node::If {
                condition: 4,
                then: 4,
                else_: 8,
            },
        ),
    ]);
    let Some(Node::SequenceExists { predicate, .. }) = project.graph.nodes.get_mut(&5) else {
        panic!("expected sequence-exists node");
    };
    *predicate = 9;

    assert_eq!(output(&project), Ok(Value::Bool(true)));
}

#[test]
fn sequence_exists_requires_a_boolean_predicate() {
    let mut project = tokenize_exists("alpha", "alpha");
    let Some(Node::SequenceExists { predicate, .. }) = project.graph.nodes.get_mut(&5) else {
        panic!("expected sequence-exists node");
    };
    *predicate = 3;

    assert_eq!(
        output(&project),
        Err(EngineError::NotABool {
            node: 3,
            found: "string"
        })
    );
}

#[test]
fn sequence_argument_cycles_are_reported() {
    let mut project = tokenize_exists("alpha", "alpha");
    let Some(Node::SequenceExists { sequence, .. }) = project.graph.nodes.get_mut(&5) else {
        panic!("expected sequence-exists node");
    };
    let SequenceExpr::Tokenize { input, .. } = sequence else {
        panic!("expected tokenizer");
    };
    *input = 5;

    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.message.contains("cycle reaches node 5"))
    );
    assert_eq!(output(&project), Err(EngineError::Cycle(5)));
}

#[test]
fn validation_confines_sequence_items_to_their_predicate_context() {
    let mut project = tokenize_exists("alpha,beta", "beta");
    let Some(Node::SequenceExists { sequence, .. }) = project.graph.nodes.get_mut(&5) else {
        panic!("expected sequence-exists node");
    };
    let SequenceExpr::Tokenize { input, .. } = sequence else {
        panic!("expected tokenizer");
    };
    *input = 2;
    assert!(validate(&project).iter().any(|issue| {
        issue
            .message
            .contains("sequence argument depends on its own item node 2")
    }));

    let mut project = tokenize_exists("alpha,beta", "beta");
    project.root.filter = Some(4);
    assert!(validate(&project).iter().any(|issue| {
        issue
            .message
            .contains("item-dependent node 4 is also referenced by a scope")
    }));

    project.root.filter = None;
    project.graph.nodes.extend([
        (
            6,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            7,
            Node::SequenceExists {
                sequence: SequenceExpr::Tokenize {
                    input: 0,
                    delimiter: 1,
                    item: 6,
                },
                predicate: 4,
            },
        ),
    ]);
    assert!(validate(&project).iter().any(|issue| {
        issue
            .message
            .contains("predicate references sequence item node 2 owned by another")
    }));
}

#[test]
fn validation_rejects_shared_or_invalid_sequence_items() {
    let mut project = tokenize_exists("alpha", "alpha");
    let second = Node::SequenceExists {
        sequence: SequenceExpr::Tokenize {
            input: 0,
            delimiter: 1,
            item: 2,
        },
        predicate: 4,
    };
    project.graph.nodes.insert(6, second);
    let messages: Vec<_> = validate(&project)
        .into_iter()
        .map(|issue| issue.message)
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("already owned"))
    );

    let Some(Node::SequenceExists { sequence, .. }) = project.graph.nodes.get_mut(&6) else {
        panic!("expected sequence-exists node");
    };
    let SequenceExpr::Tokenize { item, .. } = sequence else {
        panic!("expected tokenizer");
    };
    *item = 0;
    let messages: Vec<_> = validate(&project)
        .into_iter()
        .map(|issue| issue.message)
        .collect();
    assert!(messages.iter().any(|message| {
        message.contains("sequence item must reference an unframed empty-path source field")
    }));
}
