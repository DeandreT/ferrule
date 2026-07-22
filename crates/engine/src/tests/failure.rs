use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    DynamicSourcePath, FailureIteration, FailureRule, FailureSelection, Graph, NamedSource, Node,
    Project, Scope, SequenceExpr,
};

use crate::{EngineError, run, validate};

fn row(code: &str, valid: bool) -> Instance {
    Instance::Group(vec![
        ("Code".into(), Instance::Scalar(Value::String(code.into()))),
        ("Valid".into(), Instance::Scalar(Value::Bool(valid))),
    ])
}

fn rows_project(
    selection: FailureSelection,
    message: Option<u32>,
    nodes: impl IntoIterator<Item = (u32, Node)>,
) -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Code", ScalarType::String),
                        SchemaNode::scalar("Valid", ScalarType::Bool),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group("Target", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Source {
                collection: vec!["Rows".into()],
            },
            selection,
            message,
        }],
        user_functions: Default::default(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root: Scope::default(),
    }
}

#[test]
fn false_branch_raises_with_message_from_the_first_matching_item() {
    let project = rows_project(
        FailureSelection::WhenFalse { predicate: 0 },
        Some(3),
        [
            (
                0,
                Node::SourceField {
                    path: vec!["Valid".into()],
                    frame: None,
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String("invalid:".into()),
                },
            ),
            (
                2,
                Node::SourceField {
                    path: vec!["Code".into()],
                    frame: None,
                },
            ),
            (
                3,
                Node::Call {
                    function: "concat".into(),
                    args: vec![1, 2],
                },
            ),
        ],
    );
    let source = Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(vec![row("A", true), row("B", false), row("C", false)]),
    )]);

    let issues = validate(&project);
    assert!(issues.is_empty(), "{issues:#?}");
    assert_eq!(
        run(&project, &source),
        Err(EngineError::MappingFailure {
            rule: 1,
            message: Some("invalid:B".into()),
        })
    );
}

#[test]
fn empty_source_path_iterates_flat_rows_and_preserves_absent_message() {
    let mut project = rows_project(
        FailureSelection::WhenTrue { predicate: 0 },
        None,
        [(
            0,
            Node::SourceField {
                path: vec!["Valid".into()],
                frame: None,
            },
        )],
    );
    project.source = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            SchemaNode::scalar("Valid", ScalarType::Bool),
        ],
    );
    project.failure_rules[0].iteration = FailureIteration::Source {
        collection: Vec::new(),
    };
    let source = Instance::Repeated(vec![row("A", false), row("B", true)]);

    assert!(validate(&project).is_empty());
    let error = run(&project, &source);
    assert_eq!(
        error,
        Err(EngineError::MappingFailure {
            rule: 1,
            message: None,
        })
    );
    assert_eq!(
        error.unwrap_err().to_string(),
        "mapping failure rule 1: mapping exception was raised"
    );
}

#[test]
fn rule_and_item_order_short_circuit_later_messages() {
    let mut project = rows_project(
        FailureSelection::WhenFalse { predicate: 0 },
        Some(1),
        [
            (
                0,
                Node::SourceField {
                    path: vec!["Valid".into()],
                    frame: None,
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec!["Code".into()],
                    frame: None,
                },
            ),
        ],
    );
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: vec!["Rows".into()],
        },
        selection: FailureSelection::All,
        message: Some(999),
    });
    let source = Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(vec![row("first", false), row("second", false)]),
    )]);

    assert_eq!(
        run(&project, &source),
        Err(EngineError::MappingFailure {
            rule: 1,
            message: Some("first".into()),
        })
    );
}

#[test]
fn generated_sequence_message_uses_its_owned_item_context() {
    let mut project = Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group("Target", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Sequence {
                sequence: SequenceExpr::Generate {
                    from: Some(0),
                    to: 1,
                    item: 2,
                },
            },
            selection: FailureSelection::WhenTrue { predicate: 4 },
            message: Some(2),
        }],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Int(3),
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
                        value: Value::Int(1),
                    },
                ),
                (
                    4,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![2, 3],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope::default(),
    };

    let issues = validate(&project);
    assert!(issues.is_empty(), "{issues:#?}");
    assert_eq!(
        run(&project, &Instance::Group(Vec::new())),
        Err(EngineError::MappingFailure {
            rule: 1,
            message: Some("2".into()),
        })
    );

    project.graph.nodes.insert(
        5,
        Node::Call {
            function: "string".into(),
            args: vec![2],
        },
    );
    assert!(validate(&project).iter().any(|issue| {
        issue.location == "failure rule 1"
            && issue
                .message
                .contains("also consumed by graph node 5 outside this failure rule")
    }));
}

#[test]
fn selection_predicate_must_be_boolean() {
    let project = rows_project(
        FailureSelection::WhenTrue { predicate: 0 },
        None,
        [(
            0,
            Node::SourceField {
                path: vec!["Code".into()],
                frame: None,
            },
        )],
    );
    let source = Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(vec![row("not-bool", true)]),
    )]);

    assert_eq!(
        run(&project, &source),
        Err(EngineError::NotABool {
            node: 0,
            found: "string",
        })
    );
}

#[test]
fn validation_rejects_missing_nodes_noncollections_and_invalid_sequence_items() {
    let mut project = rows_project(
        FailureSelection::WhenTrue { predicate: 90 },
        Some(91),
        Vec::new(),
    );
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: vec!["Rows".into(), "Code".into()],
        },
        selection: FailureSelection::All,
        message: None,
    });
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 92,
                item: 93,
            },
        },
        selection: FailureSelection::All,
        message: None,
    });

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "failure rule 1"
            && issue
                .message
                .contains("selection predicate references missing node 90")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "failure rule 1"
            && issue.message.contains("message references missing node 91")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "failure rule 2"
            && issue.message.contains("matches no repeating source path")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "failure rule 3"
            && issue
                .message
                .contains("sequence argument 0 references missing node 92")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "failure rule 3"
            && issue
                .message
                .contains("sequence item references missing node 93")
    }));
}

#[test]
fn validation_rejects_dynamic_extra_source_selectors_and_dependencies() {
    let mut project = rows_project(
        FailureSelection::WhenTrue { predicate: 11 },
        Some(10),
        [
            (
                10,
                Node::SourceField {
                    path: vec!["dynamic".into(), "Rows".into(), "Code".into()],
                    frame: None,
                },
            ),
            (
                11,
                Node::Call {
                    function: "equal".into(),
                    args: vec![10, 12],
                },
            ),
            (
                12,
                Node::Const {
                    value: Value::String("blocked".into()),
                },
            ),
            (
                50,
                Node::Const {
                    value: Value::String("dynamic.xml".into()),
                },
            ),
        ],
    );
    project.extra_sources.push(NamedSource {
        name: "dynamic".into(),
        path: String::new(),
        schema: SchemaNode::group(
            "Dynamic",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Code", ScalarType::String)])
                    .repeating(),
            ],
        ),
        options: Default::default(),
        dynamic_path: Some(DynamicSourcePath {
            node: 50,
            iteration: Vec::new(),
        }),
    });
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: vec!["dynamic".into(), "Rows".into()],
        },
        selection: FailureSelection::All,
        message: None,
    });

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "failure rule 1"
            && issue.message.contains("dynamic extra source `dynamic`")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "failure rule 2"
            && issue.message.contains("dynamic extra source `dynamic`")
    }));
}

#[test]
fn validation_rejects_failure_item_reused_as_sequence_exists_predicate() {
    let project = Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group("Target", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Sequence {
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 0,
                    item: 1,
                },
            },
            selection: FailureSelection::WhenTrue { predicate: 3 },
            message: None,
        }],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::Int(2),
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
                        value: Value::Int(1),
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![1, 2],
                    },
                ),
                (
                    4,
                    Node::SequenceExists {
                        sequence: SequenceExpr::Generate {
                            from: None,
                            to: 0,
                            item: 5,
                        },
                        predicate: 3,
                    },
                ),
                (
                    5,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope::default(),
    };

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "failure rule 1"
            && issue
                .message
                .contains("reused as graph node 4's sequence-exists predicate")
    }));
}

#[test]
fn validation_confines_an_unused_failure_item_from_external_roots() {
    let mut project = Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group("Target", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Sequence {
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 0,
                    item: 1,
                },
            },
            selection: FailureSelection::All,
            message: None,
        }],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope::default(),
    };
    project.extra_sources.push(NamedSource {
        name: "dynamic".into(),
        path: String::new(),
        schema: SchemaNode::group("Dynamic", Vec::new()),
        options: Default::default(),
        dynamic_path: Some(DynamicSourcePath {
            node: 1,
            iteration: Vec::new(),
        }),
    });

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "failure rule 1"
            && issue
                .message
                .contains("referenced by dynamic extra source `dynamic`")
    }));
}

#[test]
fn validation_rejects_inactive_join_nodes_in_failure_expressions() {
    let project = Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group("Target", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Source {
                collection: Vec::new(),
            },
            selection: FailureSelection::WhenTrue { predicate: 1 },
            message: Some(2),
        }],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    1,
                    Node::JoinPosition {
                        join: mapping::JoinId::new(9),
                    },
                ),
                (
                    2,
                    Node::JoinField {
                        join: mapping::JoinId::new(9),
                        collection: vec!["Rows".into()],
                        path: vec!["Code".into()],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope::default(),
    };

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "failure rule 1"
            && issue
                .message
                .contains("join position node 1 references inactive join 9")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "failure rule 1"
            && issue
                .message
                .contains("join field node 2 references inactive join 9")
    }));
}
