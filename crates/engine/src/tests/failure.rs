use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    DynamicSourcePath, FailureIteration, FailureRule, FailureSelection, Graph, NamedSource, Node,
    Project, Scope, SequenceExpr,
};

use crate::{EngineError, run, validate};

fn row(code: &str, valid: bool) -> Instance {
    Instance::Group(
        (vec![
            ("Code".into(), Instance::Scalar(Value::String(code.into()))),
            ("Valid".into(), Instance::Scalar(Value::Bool(valid))),
        ])
        .into(),
    )
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
    let source = Instance::Group(
        (vec![(
            "Rows".into(),
            Instance::Repeated(vec![row("A", true), row("B", false), row("C", false)]),
        )])
        .into(),
    );

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
    let source = Instance::Group(
        (vec![(
            "Rows".into(),
            Instance::Repeated(vec![row("first", false), row("second", false)]),
        )])
        .into(),
    );

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
        run(&project, &Instance::Group((Vec::new()).into())),
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
    let source = Instance::Group(
        (vec![(
            "Rows".into(),
            Instance::Repeated(vec![row("not-bool", true)]),
        )])
        .into(),
    );

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
        dynamic_path: Some(DynamicSourcePath::legacy(50, Vec::new())),
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
        dynamic_path: Some(DynamicSourcePath::legacy(1, Vec::new())),
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

fn root_failure_group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn root_failure_text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn root_failure_input(flag: bool, message: &str) -> Instance {
    root_failure_group(vec![
        ("Flag", Instance::Scalar(Value::Bool(flag))),
        ("Message", root_failure_text(message)),
        (
            "Ref",
            root_failure_group(vec![
                ("Flag", Instance::Scalar(Value::Bool(false))),
                ("Message", root_failure_text("primary shadow")),
            ]),
        ),
    ])
}
fn root_failure_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Flag", ScalarType::Bool),
            SchemaNode::scalar("Message", ScalarType::String),
            SchemaNode::group(
                "Ref",
                vec![
                    SchemaNode::scalar("Flag", ScalarType::Bool),
                    SchemaNode::scalar("Message", ScalarType::String),
                ],
            ),
        ],
    )
}
fn root_failure_field(owner: mapping::SourceOwner, name: &str, frame: Option<Vec<String>>) -> Node {
    Node::OwnedSourceField(mapping::OwnedSourceField {
        source: mapping::OwnedSourceRef {
            owner,
            path: vec![name.into()],
        },
        frame,
    })
}
fn root_failure_project(owner: mapping::SourceOwner, frame: Option<Vec<String>>) -> Project {
    let target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let output_scope = |node| Scope {
        bindings: vec![mapping::Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    };
    Project {
        source: root_failure_schema(),
        target: target.clone(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Ref".into(),
            path: "ref.json".into(),
            schema: root_failure_schema(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![mapping::NamedTarget {
            name: "Other".into(),
            path: None,
            schema: target,
            options: Default::default(),
            root: output_scope(11),
        }],
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Source { collection: vec![] },
            selection: FailureSelection::WhenTrue { predicate: 1 },
            message: Some(2),
        }],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (1, root_failure_field(owner.clone(), "Flag", frame.clone())),
                (2, root_failure_field(owner, "Message", frame)),
                (
                    10,
                    Node::Const {
                        value: Value::String("primary.out".into()),
                    },
                ),
                (
                    11,
                    Node::Const {
                        value: Value::String("named.out".into()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: output_scope(10),
    }
}
fn assert_root_failure_every_selection(
    project: &Project,
    input: &Instance,
    extras: Vec<(String, Instance)>,
    expected: impl Fn() -> EngineError,
) {
    let execution = crate::ExecutionContext::new(std::path::Path::new("test.json"));
    assert_eq!(
        crate::run_with_sources(project, input, extras.clone()).unwrap_err(),
        expected()
    );
    assert_eq!(
        crate::run_outputs_with_sources_and_context(project, input, extras.clone(), &execution)
            .unwrap_err(),
        expected()
    );
    for selection in [
        crate::TargetSelection::Primary,
        crate::TargetSelection::Named("Other"),
    ] {
        assert_eq!(
            crate::run_selected_target_with_sources_and_context(
                project,
                input,
                extras.clone(),
                &execution,
                selection,
            )
            .unwrap_err(),
            expected()
        );
    }
}

#[test]
fn owned_root_failure_exact_owners_and_empty_pins_precede_every_target() {
    for frame in [None, Some(vec![])] {
        for (owner, message) in [
            (mapping::SourceOwner::Primary, "P.failure"),
            (
                mapping::SourceOwner::Named { name: "Ref".into() },
                "N.failure",
            ),
        ] {
            let mut p = root_failure_project(owner, frame.clone());
            // Target evaluation would fail, so each complete failure proves the pre-target pass.
            p.graph.nodes.insert(10, Node::Raise { message: None });
            p.graph.nodes.insert(11, Node::Raise { message: None });
            let primary = root_failure_input(true, "P.failure");
            let extras = vec![("Ref".into(), root_failure_input(true, "N.failure"))];
            assert_root_failure_every_selection(&p, &primary, extras, || {
                EngineError::MappingFailure {
                    rule: 1,
                    message: Some(message.into()),
                }
            });
        }
    }
}

#[test]
fn owned_root_failure_scalar_roots_and_actual_group_contract_are_preserved() {
    for frame in [None, Some(vec![])] {
        let mut p = root_failure_project(mapping::SourceOwner::Primary, frame.clone());
        p.source = SchemaNode::scalar("Root", ScalarType::Bool);
        for node in [1, 2] {
            p.graph.nodes.insert(
                node,
                Node::OwnedSourceField(mapping::OwnedSourceField {
                    source: mapping::OwnedSourceRef {
                        owner: mapping::SourceOwner::Primary,
                        path: vec![],
                    },
                    frame: frame.clone(),
                }),
            );
        }
        assert_root_failure_every_selection(
            &p,
            &Instance::Scalar(Value::Bool(true)),
            vec![],
            || EngineError::MappingFailure {
                rule: 1,
                message: Some("true".into()),
            },
        );
        assert_eq!(
            run(&p, &Instance::Scalar(Value::Null)),
            Err(EngineError::NotABool {
                node: 1,
                found: "null"
            })
        );
    }
    // The actual Group is permitted for unpinned fields, even if the declaration repeats.
    let mut p = root_failure_project(mapping::SourceOwner::Primary, None);
    p.source.repeating = true;
    assert_eq!(
        run(&p, &root_failure_input(true, "group")),
        Err(EngineError::MappingFailure {
            rule: 1,
            message: Some("group".into()),
        })
    );
}

#[test]
fn owned_root_failure_messages_and_if_branches_stay_lazy_with_potential_owners() {
    let mut p = root_failure_project(mapping::SourceOwner::Primary, None);
    p.graph.nodes.insert(
        2,
        root_failure_field(
            mapping::SourceOwner::Named { name: "Ref".into() },
            "Message",
            Some(vec![]),
        ),
    );
    p.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    p.graph.nodes.insert(4, Node::Raise { message: None });
    p.graph.nodes.insert(
        5,
        Node::If {
            condition: 3,
            then: 2,
            else_: 4,
        },
    );
    p.failure_rules[0].message = Some(5);
    let required = crate::required_sources_for_target(&p, crate::TargetSelection::Primary).unwrap();
    assert_eq!(required.static_sources.len(), 1);
    assert!(std::ptr::eq(
        required.static_sources[0],
        &p.extra_sources[0]
    ));
    assert!(required.dynamic_sources.is_empty());
    let primary = root_failure_input(false, "unselected");
    assert_eq!(
        run(&p, &primary),
        Ok(root_failure_group(vec![(
            "Value",
            root_failure_text("primary.out")
        )]))
    );
    assert_eq!(
        crate::run_with_sources(
            &p,
            &primary,
            vec![("Ref".into(), root_failure_text("unread malformed root"))]
        ),
        Ok(root_failure_group(vec![(
            "Value",
            root_failure_text("primary.out")
        )]))
    );
    let execution = crate::ExecutionContext::new(std::path::Path::new("test.json"));
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &primary,
            vec![],
            &execution,
            crate::TargetSelection::Named("Other")
        ),
        Ok(crate::SelectedTargetOutput::Named(crate::NamedOutput {
            name: "Other".into(),
            instance: root_failure_group(vec![("Value", root_failure_text("named.out"))]),
        }))
    );
    let all = crate::run_outputs(&p, &primary).unwrap();
    assert_eq!(
        all.primary,
        root_failure_group(vec![("Value", root_failure_text("primary.out"))])
    );
    assert_eq!(
        all.extras,
        vec![crate::NamedOutput {
            name: "Other".into(),
            instance: root_failure_group(vec![("Value", root_failure_text("named.out"))]),
        }]
    );
    let primary = root_failure_input(true, "selected");
    assert_root_failure_every_selection(
        &p,
        &primary,
        vec![("Ref".into(), root_failure_input(false, "lazy.branch"))],
        || EngineError::MappingFailure {
            rule: 1,
            message: Some("lazy.branch".into()),
        },
    );
    p.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    assert_eq!(
        run(&p, &primary),
        Err(EngineError::MappingException {
            node: 4,
            message: None
        })
    );
}

#[test]
fn owned_root_failure_rule_order_and_unknown_target_priority_are_unchanged() {
    let mut p = root_failure_project(mapping::SourceOwner::Primary, None);
    p.graph.nodes.insert(3, Node::Raise { message: None });
    p.failure_rules = vec![
        FailureRule {
            iteration: FailureIteration::Source { collection: vec![] },
            selection: FailureSelection::WhenTrue { predicate: 1 },
            message: Some(3),
        },
        FailureRule {
            iteration: FailureIteration::Source { collection: vec![] },
            selection: FailureSelection::WhenFalse { predicate: 1 },
            message: Some(2),
        },
        FailureRule {
            iteration: FailureIteration::Source { collection: vec![] },
            selection: FailureSelection::All,
            message: Some(999),
        },
    ];
    assert_root_failure_every_selection(
        &p,
        &root_failure_input(false, "first stored match"),
        vec![],
        || EngineError::MappingFailure {
            rule: 2,
            message: Some("first stored match".into()),
        },
    );
    p.graph.nodes.insert(
        2,
        root_failure_field(
            mapping::SourceOwner::Named {
                name: "Missing".into(),
            },
            "Message",
            None,
        ),
    );
    assert_eq!(
        crate::run_selected_target(
            &p,
            &root_failure_input(false, "unused"),
            crate::TargetSelection::Named("Absent")
        ),
        Err(EngineError::UnknownTarget {
            name: "Absent".into()
        })
    );
    assert_eq!(
        crate::required_sources_for_target(&p, crate::TargetSelection::Named("Absent"))
            .unwrap_err(),
        EngineError::UnknownTarget {
            name: "Absent".into()
        }
    );
    assert_eq!(
        run(&p, &root_failure_input(false, "unused")),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: crate::OwnedSourceError::UnknownOwner {
                name: "Missing".into()
            },
        })
    );
}

#[test]
fn owned_root_failure_rejects_actual_expanding_bases_before_any_rule_expression() {
    let mut p = root_failure_project(mapping::SourceOwner::Named { name: "Ref".into() }, None);
    p.graph.nodes.insert(1, Node::Raise { message: None });
    for input in [
        Instance::Repeated(vec![]),
        Instance::Repeated(vec![
            root_failure_input(true, "first"),
            root_failure_input(false, "sibling"),
        ]),
        Instance::DocumentSet(vec![]),
        Instance::DocumentSet(vec![
            ir::DocumentMember::new("first.json", root_failure_input(true, "first")).unwrap(),
            ir::DocumentMember::new("second.json", root_failure_input(false, "sibling")).unwrap(),
        ]),
        Instance::MappedSequence(vec![]),
        Instance::MappedSequence(vec![root_failure_input(true, "generated")]),
    ] {
        assert_root_failure_every_selection(
            &p,
            &input,
            vec![("Ref".into(), root_failure_input(true, "named"))],
            || EngineError::UnsupportedOwnedContext {
                node: None,
                function: None,
                scope: None,
                context: crate::OwnedCapabilityContext::FailureRule,
            },
        );
    }
}

#[test]
fn owned_root_failure_named_root_reader_keeps_full_lazy_errors_without_rescue() {
    let p = root_failure_project(mapping::SourceOwner::Named { name: "Ref".into() }, None);
    let primary = root_failure_input(true, "shadow cannot rescue");
    assert_root_failure_every_selection(&p, &primary, vec![], || EngineError::OwnedSourceAccess {
        node: Some(1),
        scope: None,
        source: crate::OwnedSourceError::SourceUnavailable {
            owner: mapping::SourceOwner::Named { name: "Ref".into() },
        },
    });
    for (root, source) in [
        (
            Instance::Repeated(vec![]),
            crate::OwnedSourceError::UniteratedOwnedRepetition {
                path: vec!["Flag".into()],
            },
        ),
        (
            Instance::Repeated(vec![
                root_failure_input(true, "first"),
                root_failure_input(false, "sibling"),
            ]),
            crate::OwnedSourceError::UniteratedOwnedRepetition {
                path: vec!["Flag".into()],
            },
        ),
        (
            root_failure_text("malformed"),
            crate::OwnedSourceError::MalformedOwnedTraversal {
                path: vec!["Flag".into()],
                found: "a non-null scalar",
            },
        ),
        (
            root_failure_group(vec![(
                "Flag",
                Instance::Repeated(vec![
                    Instance::Scalar(Value::Bool(true)),
                    Instance::Scalar(Value::Bool(false)),
                ]),
            )]),
            crate::OwnedSourceError::UniteratedOwnedRepetition {
                path: vec!["Flag".into()],
            },
        ),
        (
            root_failure_group(vec![(
                "Flag",
                root_failure_group(vec![("Value", root_failure_text("not a scalar"))]),
            )]),
            crate::OwnedSourceError::MalformedOwnedTraversal {
                path: vec!["Flag".into()],
                found: "a group",
            },
        ),
    ] {
        assert_root_failure_every_selection(&p, &primary, vec![("Ref".into(), root)], || {
            EngineError::OwnedSourceAccess {
                node: Some(1),
                scope: None,
                source: source.clone(),
            }
        });
    }
    for (root, context) in [
        (
            Instance::MappedSequence(vec![]),
            crate::OwnedCapabilityContext::GeneratedItem,
        ),
        (
            Instance::DocumentSet(vec![]),
            crate::OwnedCapabilityContext::Document,
        ),
    ] {
        assert_root_failure_every_selection(&p, &primary, vec![("Ref".into(), root)], || {
            EngineError::UnsupportedOwnedContext {
                node: Some(1),
                function: None,
                scope: None,
                context,
            }
        });
    }
    let p = root_failure_project(
        mapping::SourceOwner::Named { name: "Ref".into() },
        Some(vec![]),
    );
    assert_root_failure_every_selection(
        &p,
        &primary,
        vec![("Ref".into(), Instance::Repeated(vec![]))],
        || EngineError::OwnedSourceAccess {
            node: Some(1),
            scope: None,
            source: crate::OwnedSourceError::InactiveSourceFrame {
                owner: mapping::SourceOwner::Named { name: "Ref".into() },
                collection: Some(vec![]),
            },
        },
    );
}

#[test]
fn legacy_root_failure_repetition_is_preserved_when_a_different_target_uses_owned() {
    let mut p = rows_project(
        FailureSelection::WhenTrue { predicate: 0 },
        None,
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
                root_failure_field(mapping::SourceOwner::Primary, "Code", None),
            ),
        ],
    );
    p.source = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            SchemaNode::scalar("Valid", ScalarType::Bool),
        ],
    );
    p.failure_rules[0].iteration = FailureIteration::Source { collection: vec![] };
    p.extra_targets.push(mapping::NamedTarget {
        name: "Owned".into(),
        path: None,
        schema: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        options: Default::default(),
        root: Scope {
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    });
    let input = Instance::Repeated(vec![row("first", false), row("second", true)]);
    assert_eq!(
        run(&p, &input),
        Err(EngineError::MappingFailure {
            rule: 1,
            message: None
        })
    );
    assert_eq!(
        crate::run_outputs(&p, &input).unwrap_err(),
        EngineError::MappingFailure {
            rule: 1,
            message: None
        }
    );
    assert_eq!(
        crate::run_selected_target(&p, &input, crate::TargetSelection::Named("Owned")).unwrap_err(),
        EngineError::MappingFailure {
            rule: 1,
            message: None
        }
    );
}

fn primary_failure_field(path: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(mapping::OwnedSourceField {
        source: mapping::OwnedSourceRef {
            owner: mapping::SourceOwner::Primary,
            path: path.iter().map(|part| (*part).into()).collect(),
        },
        frame: frame.map(|path| path.iter().map(|part| (*part).into()).collect()),
    })
}
fn primary_failure_project() -> Project {
    let mut p = root_failure_project(mapping::SourceOwner::Primary, None);
    let mut rows = root_failure_schema();
    rows.name = "Rows".into();
    rows.repeating = true;
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Flag", ScalarType::Bool),
            SchemaNode::scalar("Message", ScalarType::String),
            rows,
            SchemaNode::group(
                "Ref",
                vec![SchemaNode::scalar("Message", ScalarType::String)],
            ),
        ],
    );
    p.failure_rules[0].iteration = FailureIteration::Source {
        collection: vec!["Rows".into()],
    };
    p.graph
        .nodes
        .insert(1, primary_failure_field(&["Rows", "Flag"], None));
    p.graph
        .nodes
        .insert(2, primary_failure_field(&["Rows", "Message"], None));
    p.graph.nodes.insert(
        3,
        Node::OwnedPosition(mapping::OwnedPosition {
            owner: mapping::SourceOwner::Primary,
            collection: Some(vec!["Rows".into()]),
        }),
    );
    p
}
fn primary_failure_input(rows: Vec<Instance>) -> Instance {
    root_failure_group(vec![
        ("Flag", Instance::Scalar(Value::Bool(false))),
        ("Message", root_failure_text("primary.root")),
        ("Rows", Instance::Repeated(rows)),
        (
            "Ref",
            root_failure_group(vec![("Message", root_failure_text("primary.shadow"))]),
        ),
    ])
}
fn primary_failure_success_every_selection(
    p: &Project,
    input: &Instance,
    extras: Vec<(String, Instance)>,
) {
    let primary = root_failure_group(vec![("Value", root_failure_text("primary.out"))]);
    let named = crate::NamedOutput {
        name: "Other".into(),
        instance: root_failure_group(vec![("Value", root_failure_text("named.out"))]),
    };
    let execution = crate::ExecutionContext::new(std::path::Path::new("test.json"));
    assert_eq!(
        crate::run_with_sources(p, input, extras.clone()),
        Ok(primary.clone())
    );
    let crate::ExecutionOutputs {
        primary: actual_primary,
        extras: actual_extras,
    } = crate::run_outputs_with_sources_and_context(p, input, extras.clone(), &execution).unwrap();
    assert_eq!(
        (actual_primary, actual_extras),
        (primary.clone(), vec![named.clone()])
    );
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            p,
            input,
            extras.clone(),
            &execution,
            crate::TargetSelection::Primary
        ),
        Ok(crate::SelectedTargetOutput::Primary(primary))
    );
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            p,
            input,
            extras,
            &execution,
            crate::TargetSelection::Named("Other")
        ),
        Ok(crate::SelectedTargetOutput::Named(named))
    );
}
fn primary_failure_refusal() -> EngineError {
    EngineError::UnsupportedOwnedContext {
        node: None,
        function: None,
        scope: None,
        context: crate::OwnedCapabilityContext::FailureRule,
    }
}

#[test]
fn owned_primary_failure_rows_keep_exact_items_and_structural_positions() {
    let mut p = primary_failure_project();
    p.extra_sources.push(NamedSource {
        name: "Rows".into(),
        path: "rows.json".into(),
        schema: root_failure_schema().repeating(),
        options: Default::default(),
        dynamic_path: None,
    });
    let input = primary_failure_input(vec![
        root_failure_input(false, "first"),
        root_failure_input(true, "second"),
        root_failure_input(true, "third"),
    ]);
    let extras = vec![(
        "Rows".into(),
        Instance::Repeated(vec![root_failure_input(true, "foreign.first")]),
    )];
    for pin in [None, Some(vec!["Rows".into()])] {
        p.graph.nodes.insert(
            2,
            Node::OwnedSourceField(mapping::OwnedSourceField {
                source: mapping::OwnedSourceRef {
                    owner: mapping::SourceOwner::Primary,
                    path: vec!["Rows".into(), "Message".into()],
                },
                frame: pin,
            }),
        );
        assert_root_failure_every_selection(&p, &input, extras.clone(), || {
            EngineError::MappingFailure {
                rule: 1,
                message: Some("second".into()),
            }
        });
    }
    for collection in [None, Some(vec!["Rows".into()])] {
        p.graph.nodes.insert(
            3,
            Node::OwnedPosition(mapping::OwnedPosition {
                owner: mapping::SourceOwner::Primary,
                collection,
            }),
        );
        p.failure_rules[0].message = Some(3);
        assert_root_failure_every_selection(&p, &input, extras.clone(), || {
            EngineError::MappingFailure {
                rule: 1,
                message: Some("2".into()),
            }
        });
    }
    p.graph
        .nodes
        .insert(2, primary_failure_field(&["Message"], None));
    p.failure_rules[0].message = Some(2);
    assert_root_failure_every_selection(&p, &input, extras, || EngineError::MappingFailure {
        rule: 1,
        message: Some("primary.root".into()),
    });
    p.graph.nodes.insert(10, Node::Raise { message: None });
    p.graph.nodes.insert(11, Node::Raise { message: None });
    assert_root_failure_every_selection(&p, &input, vec![], || EngineError::MappingFailure {
        rule: 1,
        message: Some("primary.root".into()),
    });
}

#[test]
fn owned_primary_failure_row_predicates_and_messages_remain_lazy() {
    let mut p = primary_failure_project();
    p.graph.nodes.insert(
        2,
        root_failure_field(
            mapping::SourceOwner::Named { name: "Ref".into() },
            "Message",
            Some(vec![]),
        ),
    );
    p.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    p.graph.nodes.insert(5, Node::Raise { message: None });
    p.graph.nodes.insert(
        6,
        Node::If {
            condition: 4,
            then: 2,
            else_: 5,
        },
    );
    p.failure_rules[0].message = Some(6);
    for selection in [
        crate::TargetSelection::Primary,
        crate::TargetSelection::Named("Other"),
    ] {
        let required = crate::required_sources_for_target(&p, selection).unwrap();
        assert_eq!(required.static_sources.len(), 1);
        assert!(std::ptr::eq(
            required.static_sources[0],
            &p.extra_sources[0]
        ));
        assert!(required.dynamic_sources.is_empty());
    }
    let unselected = primary_failure_input(vec![root_failure_input(false, "unread")]);
    primary_failure_success_every_selection(&p, &unselected, vec![]);
    primary_failure_success_every_selection(
        &p,
        &unselected,
        vec![("Ref".into(), root_failure_text("unread malformed"))],
    );
    let selected = primary_failure_input(vec![
        root_failure_input(false, "first"),
        root_failure_input(true, "second"),
    ]);
    assert_root_failure_every_selection(
        &p,
        &selected,
        vec![("Ref".into(), root_failure_input(false, "lazy.named"))],
        || EngineError::MappingFailure {
            rule: 1,
            message: Some("lazy.named".into()),
        },
    );
    p.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    assert_root_failure_every_selection(&p, &selected, vec![], || EngineError::MappingException {
        node: 5,
        message: None,
    });
    p.graph
        .nodes
        .insert(2, primary_failure_field(&["Rows", "Message"], None));
    p.failure_rules[0].message = Some(2);
    let input = primary_failure_input(vec![
        root_failure_group(vec![
            ("Flag", Instance::Scalar(Value::Bool(false))),
            ("Message", root_failure_group(vec![])),
        ]),
        root_failure_input(true, "selected.message"),
    ]);
    assert_root_failure_every_selection(&p, &input, vec![], || EngineError::MappingFailure {
        rule: 1,
        message: Some("selected.message".into()),
    });
}

#[test]
fn owned_primary_failure_first_selected_row_precedes_later_unsupported_rows() {
    let mut p = primary_failure_project();
    for unsupported in [
        Instance::DocumentSet(vec![]),
        Instance::DocumentSet(vec![
            ir::DocumentMember::new("member.json", root_failure_input(true, "document")).unwrap(),
        ]),
        Instance::MappedSequence(vec![]),
        Instance::MappedSequence(vec![root_failure_input(true, "generated")]),
    ] {
        let input = primary_failure_input(vec![
            root_failure_input(true, "first.selected"),
            unsupported.clone(),
        ]);
        assert_root_failure_every_selection(&p, &input, vec![], || EngineError::MappingFailure {
            rule: 1,
            message: Some("first.selected".into()),
        });
        p.graph.nodes.insert(1, Node::Raise { message: None });
        p.failure_rules[0].message = Some(3);
        let input =
            primary_failure_input(vec![unsupported.clone(), root_failure_input(true, "later")]);
        assert_root_failure_every_selection(&p, &input, vec![], primary_failure_refusal);
        p.graph.nodes.insert(
            2,
            root_failure_field(
                mapping::SourceOwner::Named { name: "Ref".into() },
                "Message",
                None,
            ),
        );
        p.failure_rules[0].message = Some(2);
        assert_root_failure_every_selection(
            &p,
            &input,
            vec![("Ref".into(), root_failure_input(false, "named"))],
            primary_failure_refusal,
        );
        p.graph
            .nodes
            .insert(1, primary_failure_field(&["Rows", "Flag"], None));
        p.graph
            .nodes
            .insert(2, primary_failure_field(&["Rows", "Message"], None));
        p.failure_rules[0].message = Some(2);
    }
    primary_failure_success_every_selection(&p, &primary_failure_input(vec![]), vec![]);
}

#[test]
fn owned_primary_failure_named_root_reads_do_not_use_primary_row_frames() {
    let mut p = primary_failure_project();
    let input = primary_failure_input(vec![root_failure_input(true, "row.shadow")]);
    for pin in [None, Some(vec![])] {
        p.graph.nodes.insert(
            2,
            root_failure_field(
                mapping::SourceOwner::Named { name: "Ref".into() },
                "Message",
                pin,
            ),
        );
        assert_root_failure_every_selection(
            &p,
            &input,
            vec![("Ref".into(), root_failure_input(false, "exact.named"))],
            || EngineError::MappingFailure {
                rule: 1,
                message: Some("exact.named".into()),
            },
        );
    }
    p.graph.nodes.insert(
        2,
        root_failure_field(
            mapping::SourceOwner::Named { name: "Ref".into() },
            "Message",
            None,
        ),
    );
    assert_root_failure_every_selection(&p, &input, vec![], || EngineError::OwnedSourceAccess {
        node: Some(2),
        scope: None,
        source: crate::OwnedSourceError::SourceUnavailable {
            owner: mapping::SourceOwner::Named { name: "Ref".into() },
        },
    });
    assert_root_failure_every_selection(
        &p,
        &input,
        vec![(
            "Ref".into(),
            Instance::Repeated(vec![
                root_failure_input(false, "first"),
                root_failure_input(false, "sibling"),
            ]),
        )],
        || EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: crate::OwnedSourceError::UniteratedOwnedRepetition {
                path: vec!["Message".into()],
            },
        },
    );
    for (named, context) in [
        (
            Instance::DocumentSet(vec![]),
            crate::OwnedCapabilityContext::Document,
        ),
        (
            Instance::MappedSequence(vec![]),
            crate::OwnedCapabilityContext::GeneratedItem,
        ),
    ] {
        assert_root_failure_every_selection(&p, &input, vec![("Ref".into(), named)], || {
            EngineError::UnsupportedOwnedContext {
                node: Some(2),
                function: None,
                scope: None,
                context,
            }
        });
    }
}

#[test]
fn owned_primary_failure_empty_null_and_singleton_collection_behavior_is_legacy() {
    let mut p = primary_failure_project();
    primary_failure_success_every_selection(&p, &primary_failure_input(vec![]), vec![]);
    primary_failure_success_every_selection(&p, &root_failure_input(false, "absent"), vec![]);
    p.failure_rules[0].selection = FailureSelection::All;
    p.failure_rules[0].message = Some(3);
    for null in [Value::Null, Value::json_null()] {
        let input = primary_failure_input(vec![
            Instance::Scalar(null),
            root_failure_input(true, "second"),
        ]);
        assert_root_failure_every_selection(&p, &input, vec![], || EngineError::MappingFailure {
            rule: 1,
            message: Some("1".into()),
        });
    }
    let singleton = root_failure_group(vec![("Rows", root_failure_input(true, "singleton"))]);
    assert_root_failure_every_selection(&p, &singleton, vec![], || {
        EngineError::OwnedSourceAccess {
            node: Some(3),
            scope: None,
            source: crate::OwnedSourceError::InactiveSourceFrame {
                owner: mapping::SourceOwner::Primary,
                collection: Some(vec!["Rows".into()]),
            },
        }
    });
    p.failure_rules[0].message = Some(2);
    assert_root_failure_every_selection(&p, &singleton, vec![], || EngineError::MappingFailure {
        rule: 1,
        message: Some("singleton".into()),
    });
    p.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    p.failure_rules[0].selection = FailureSelection::WhenTrue { predicate: 1 };
    for null in [Value::Null, Value::json_null()] {
        primary_failure_success_every_selection(
            &p,
            &root_failure_group(vec![("Rows", Instance::Scalar(null))]),
            vec![],
        );
    }
    let ir::SchemaKind::Group { children, .. } = &mut p.source.kind else {
        unreachable!()
    };
    children
        .iter_mut()
        .find(|child| child.name == "Rows")
        .unwrap()
        .repeating = false;
    p.graph
        .nodes
        .insert(1, primary_failure_field(&["Rows", "Flag"], None));
    assert_root_failure_every_selection(
        &p,
        &primary_failure_input(vec![root_failure_input(true, "actual.repeated")]),
        vec![],
        || EngineError::MappingFailure {
            rule: 1,
            message: Some("actual.repeated".into()),
        },
    );
}

#[test]
fn owned_primary_failure_row_reads_preserve_full_malformed_and_inactive_errors() {
    let mut p = primary_failure_project();
    assert_root_failure_every_selection(
        &p,
        &primary_failure_input(vec![root_failure_group(vec![(
            "Message",
            root_failure_text("missing flag"),
        )])]),
        vec![],
        || EngineError::NotABool {
            node: 1,
            found: "null",
        },
    );
    for (row, source) in [
        (
            root_failure_text("bad.row"),
            crate::OwnedSourceError::MalformedOwnedTraversal {
                path: vec!["Flag".into()],
                found: "a non-null scalar",
            },
        ),
        (
            root_failure_group(vec![(
                "Flag",
                Instance::Repeated(vec![
                    Instance::Scalar(Value::Bool(true)),
                    Instance::Scalar(Value::Bool(false)),
                ]),
            )]),
            crate::OwnedSourceError::UniteratedOwnedRepetition {
                path: vec!["Flag".into()],
            },
        ),
        (
            root_failure_group(vec![("Flag", root_failure_group(vec![]))]),
            crate::OwnedSourceError::MalformedOwnedTraversal {
                path: vec!["Flag".into()],
                found: "a group",
            },
        ),
    ] {
        assert_root_failure_every_selection(&p, &primary_failure_input(vec![row]), vec![], || {
            EngineError::OwnedSourceAccess {
                node: Some(1),
                scope: None,
                source: source.clone(),
            }
        });
    }
    p.graph.nodes.insert(
        2,
        primary_failure_field(&["Other", "Message"], Some(&["Other"])),
    );
    assert_root_failure_every_selection(
        &p,
        &primary_failure_input(vec![root_failure_input(true, "row")]),
        vec![],
        || EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: crate::OwnedSourceError::InactiveSourceFrame {
                owner: mapping::SourceOwner::Primary,
                collection: Some(vec!["Other".into()]),
            },
        },
    );
    p.graph
        .nodes
        .insert(2, primary_failure_field(&["Rows", "Message"], None));
    let later = root_failure_group(vec![
        ("Flag", Instance::Scalar(Value::Bool(true))),
        (
            "Message",
            Instance::Repeated(vec![
                root_failure_text("first"),
                root_failure_text("sibling"),
            ]),
        ),
    ]);
    assert_root_failure_every_selection(
        &p,
        &primary_failure_input(vec![root_failure_input(true, "earlier"), later]),
        vec![],
        || EngineError::MappingFailure {
            rule: 1,
            message: Some("earlier".into()),
        },
    );
}

#[test]
fn owned_primary_failure_foreign_wrapper_base_refuses_before_expressions() {
    let mut p = primary_failure_project();
    p.extra_sources.push(NamedSource {
        name: "Rows".into(),
        path: "rows.json".into(),
        schema: root_failure_schema().repeating(),
        options: Default::default(),
        dynamic_path: None,
    });
    p.graph.nodes.insert(1, Node::Raise { message: None });
    let extras = vec![(
        "Rows".into(),
        Instance::Repeated(vec![root_failure_input(true, "foreign")]),
    )];
    assert_root_failure_every_selection(
        &p,
        &root_failure_input(false, "absent"),
        extras.clone(),
        primary_failure_refusal,
    );
    assert_root_failure_every_selection(
        &p,
        &primary_failure_input(vec![root_failure_input(true, "primary")]),
        extras,
        || EngineError::MappingException {
            node: 1,
            message: None,
        },
    );
    for input in [
        Instance::Repeated(vec![root_failure_input(true, "ownerless root")]),
        Instance::MappedSequence(vec![]),
        Instance::DocumentSet(vec![]),
    ] {
        assert_root_failure_every_selection(&p, &input, vec![], primary_failure_refusal);
    }
}

#[test]
fn legacy_row_failures_and_static_root_rules_are_preserved_with_owned_targets() {
    let mut p = primary_failure_project();
    p.graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec!["Flag".into()],
            frame: None,
        },
    );
    p.failure_rules[0].message = None;
    p.graph
        .nodes
        .insert(11, primary_failure_field(&["Message"], None));
    let input = primary_failure_input(vec![
        root_failure_input(false, "first"),
        root_failure_input(true, "legacy.second"),
    ]);
    assert_root_failure_every_selection(&p, &input, vec![], || EngineError::MappingFailure {
        rule: 1,
        message: None,
    });
    p.graph
        .nodes
        .insert(1, primary_failure_field(&["Rows", "Flag"], None));
    p.failure_rules[0].message = Some(2);
    p.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::String("later.rule".into()),
        },
    );
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source { collection: vec![] },
        selection: FailureSelection::All,
        message: Some(8),
    });
    assert_root_failure_every_selection(&p, &input, vec![], || EngineError::MappingFailure {
        rule: 1,
        message: Some("legacy.second".into()),
    });
    assert_root_failure_every_selection(
        &p,
        &primary_failure_input(vec![root_failure_input(false, "none")]),
        vec![],
        || EngineError::MappingFailure {
            rule: 2,
            message: Some("later.rule".into()),
        },
    );
    let execution = crate::ExecutionContext::new(std::path::Path::new("test.json"));
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input,
            vec![],
            &execution,
            crate::TargetSelection::Named("Absent")
        ),
        Err(EngineError::UnknownTarget {
            name: "Absent".into()
        })
    );
    p.failure_rules[0].iteration = FailureIteration::Source { collection: vec![] };
    assert_root_failure_every_selection(
        &p,
        &Instance::Repeated(vec![input]),
        vec![],
        primary_failure_refusal,
    );
}
