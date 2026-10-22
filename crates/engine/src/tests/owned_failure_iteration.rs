//! #471 explicit owned failure-rule iteration over Primary or named rows.
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, Graph, NamedSource, NamedTarget,
    Node, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project, Scope, SourceOwner,
};

use crate::{EngineError, ExecutionContext, OwnedCapabilityContext, OwnedSourceError};

fn named() -> SourceOwner {
    SourceOwner::Named {
        name: "Lookup".into(),
    }
}
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn owned_rows(owner: SourceOwner, parts: &[&str]) -> FailureIteration {
    FailureIteration::OwnedSource {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
    }
}
fn field(owner: SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: frame.map(path),
    })
}
fn position(owner: SourceOwner, parts: Option<&[&str]>) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner,
        collection: parts.map(path),
    })
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn row(flag: bool, message: &str) -> Instance {
    group(vec![
        ("Flag", Instance::Scalar(Value::Bool(flag))),
        ("Message", text(message)),
    ])
}
fn row_schema() -> SchemaNode {
    SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Flag", ScalarType::Bool),
            SchemaNode::scalar("Message", ScalarType::String),
        ],
    )
    .repeating()
}
fn endpoint_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![SchemaNode::scalar("Note", ScalarType::String), row_schema()],
    )
}
fn endpoint(note: &str, rows: Vec<Instance>) -> Instance {
    group(vec![
        ("Note", text(note)),
        ("Rows", Instance::Repeated(rows)),
    ])
}

/// Primary and Lookup share every path; Lookup's rows carry opposite flags.
fn project(rules: Vec<FailureRule>, nodes: Vec<(u32, Node)>) -> Project {
    let target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let output = |node| Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    };
    let mut graph = Graph::default();
    graph.nodes.insert(
        100,
        Node::Const {
            value: Value::String("primary.out".into()),
        },
    );
    graph.nodes.insert(
        101,
        Node::Const {
            value: Value::String("named.out".into()),
        },
    );
    graph.nodes.extend(nodes);
    Project {
        source: endpoint_schema("Input"),
        target: target.clone(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Lookup".into(),
            path: "lookup.json".into(),
            schema: endpoint_schema("Lookup"),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: None,
            schema: target,
            options: Default::default(),
            root: output(101),
        }],
        failure_rules: rules,
        user_functions: Default::default(),
        graph,
        root: output(100),
    }
}
fn when_true(iteration: FailureIteration, predicate: u32, message: Option<u32>) -> FailureRule {
    FailureRule {
        iteration,
        selection: FailureSelection::WhenTrue { predicate },
        message,
    }
}
fn primary_input() -> Instance {
    endpoint(
        "P.note",
        vec![row(false, "P1"), row(true, "P2"), row(true, "P3")],
    )
}
fn lookup_input() -> Instance {
    endpoint(
        "N.note",
        vec![row(true, "N1"), row(false, "N2"), row(false, "N3")],
    )
}
fn inputs() -> Vec<(String, Instance)> {
    vec![("Lookup".into(), lookup_input())]
}

/// One call's ordered `(target name, tree)` outputs or its typed failure.
type Outcome = Result<Vec<(Option<String>, Instance)>, EngineError>;

fn outcomes(project: &Project, source: &Instance, extras: Vec<(String, Instance)>) -> Vec<Outcome> {
    let execution = ExecutionContext::new(std::path::Path::new("owned471.mapping"));
    let all =
        crate::run_outputs_with_sources_and_context(project, source, extras.clone(), &execution)
            .map(|outputs| {
                std::iter::once((None, outputs.primary))
                    .chain(
                        outputs
                            .extras
                            .into_iter()
                            .map(|output| (Some(output.name), output.instance)),
                    )
                    .collect()
            });
    let mut results = vec![all];
    for selection in [
        crate::TargetSelection::Primary,
        crate::TargetSelection::Named("audit"),
    ] {
        results.push(
            crate::run_selected_target_with_sources_and_context(
                project,
                source,
                extras.clone(),
                &execution,
                selection,
            )
            .map(|output| match output {
                crate::SelectedTargetOutput::Primary(instance) => vec![(None, instance)],
                crate::SelectedTargetOutput::Named(output) => {
                    vec![(Some(output.name), output.instance)]
                }
            }),
        );
    }
    results
}
fn assert_failure(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
    expected: EngineError,
) {
    assert!(engine_valid(project));
    for outcome in outcomes(project, source, extras) {
        assert_eq!(outcome.as_ref().err(), Some(&expected), "{outcome:?}");
    }
}
fn assert_success(project: &Project, source: &Instance, extras: Vec<(String, Instance)>) {
    assert!(engine_valid(project));
    let primary = group(vec![("Value", text("primary.out"))]);
    let audit = group(vec![("Value", text("named.out"))]);
    assert_eq!(
        outcomes(project, source, extras),
        vec![
            Ok(vec![
                (None, primary.clone()),
                (Some("audit".into()), audit.clone())
            ]),
            Ok(vec![(None, primary)]),
            Ok(vec![(Some("audit".into()), audit)]),
        ]
    );
}
fn engine_valid(project: &Project) -> bool {
    let issues = crate::validate(project);
    assert!(issues.is_empty(), "{issues:?}");
    true
}
fn failure(rule: usize, message: Option<&str>) -> EngineError {
    EngineError::MappingFailure {
        rule,
        message: message.map(str::to_owned),
    }
}

#[test]
fn same_path_owners_with_opposite_predicates_select_only_the_declared_endpoint() {
    for (owner, first_true, first_false) in
        [(SourceOwner::Primary, "P2", "P1"), (named(), "N1", "N2")]
    {
        let nodes = vec![
            (1, field(owner.clone(), &["Rows", "Flag"], None)),
            (2, field(owner.clone(), &["Rows", "Message"], None)),
        ];
        let rows = owned_rows(owner.clone(), &["Rows"]);
        let p = project(vec![when_true(rows.clone(), 1, Some(2))], nodes.clone());
        assert_failure(&p, &primary_input(), inputs(), failure(1, Some(first_true)));
        let p = project(
            vec![FailureRule {
                iteration: rows,
                selection: FailureSelection::WhenFalse { predicate: 1 },
                message: Some(2),
            }],
            nodes,
        );
        assert_failure(
            &p,
            &primary_input(),
            inputs(),
            failure(1, Some(first_false)),
        );
    }
    // Legacy reads in an owned row resolve against that row before outer endpoints.
    for (owner, expected) in [(SourceOwner::Primary, "P2"), (named(), "N1")] {
        let p = project(
            vec![when_true(owned_rows(owner, &["Rows"]), 1, Some(2))],
            vec![
                (
                    1,
                    Node::SourceField {
                        path: path(&["Flag"]),
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: path(&["Message"]),
                        frame: None,
                    },
                ),
            ],
        );
        assert_failure(&p, &primary_input(), inputs(), failure(1, Some(expected)));
    }
}

#[test]
fn all_selection_positions_and_cross_owner_reads_use_exact_frames() {
    for (owner, other, expected) in [
        (SourceOwner::Primary, named(), "1:P1:N.note:1"),
        (named(), SourceOwner::Primary, "1:N1:P.note:1"),
    ] {
        let p = project(
            vec![FailureRule {
                iteration: owned_rows(owner.clone(), &["Rows"]),
                selection: FailureSelection::All,
                message: Some(9),
            }],
            vec![
                (1, position(owner.clone(), Some(&["Rows"]))),
                (
                    2,
                    field(owner.clone(), &["Rows", "Message"], Some(&["Rows"])),
                ),
                (3, field(other, &["Note"], Some(&[]))),
                (4, position(owner, None)),
                (
                    5,
                    Node::Const {
                        value: Value::String(":".into()),
                    },
                ),
                (
                    9,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![1, 5, 2, 5, 3, 5, 4],
                    },
                ),
            ],
        );
        assert_failure(&p, &primary_input(), inputs(), failure(1, Some(expected)));
    }
    // Absent and explicitly empty messages remain distinct.
    let p = project(
        vec![FailureRule {
            iteration: owned_rows(named(), &["Rows"]),
            selection: FailureSelection::All,
            message: None,
        }],
        vec![],
    );
    assert_failure(&p, &primary_input(), inputs(), failure(1, None));
}

#[test]
fn rule_order_first_selected_row_and_lazy_messages_are_preserved() {
    let nodes = vec![
        (1, field(SourceOwner::Primary, &["Rows", "Flag"], None)),
        (2, field(named(), &["Rows", "Flag"], None)),
        (3, field(named(), &["Rows", "Message"], None)),
        (4, Node::Raise { message: Some(3) }),
        (
            5,
            Node::If {
                condition: 2,
                then: 3,
                else_: 4,
            },
        ),
        (
            6,
            Node::Const {
                value: Value::String("not boolean".into()),
            },
        ),
        (
            7,
            Node::If {
                condition: 2,
                then: 2,
                else_: 6,
            },
        ),
    ];
    // Rule 1 never selects; rule 2 selects its first Lookup row, and its message
    // raise is never reached for later rows. Rule 3 would fail but is unreached.
    let rules = vec![
        when_true(owned_rows(named(), &["Missing"]), 1, Some(4)),
        when_true(owned_rows(named(), &["Rows"]), 7, Some(5)),
        when_true(owned_rows(SourceOwner::Primary, &["Rows"]), 6, None),
    ];
    let mut p = project(rules, nodes.clone());
    p.extra_sources[0].schema = SchemaNode::group(
        "Lookup",
        vec![
            SchemaNode::scalar("Note", ScalarType::String),
            row_schema(),
            {
                let mut missing = row_schema();
                missing.name = "Missing".into();
                missing
            },
        ],
    );
    assert_failure(&p, &primary_input(), inputs(), failure(2, Some("N1")));
    // When the first Lookup row is false, its predicate is non-Boolean.
    let flipped = vec![(
        "Lookup".into(),
        endpoint("N.note", vec![row(false, "N1"), row(true, "N2")]),
    )];
    assert_failure(
        &p,
        &primary_input(),
        flipped,
        EngineError::NotABool {
            node: 7,
            found: "string",
        },
    );
    // The third rule is reached once no earlier row selects.
    let none = vec![("Lookup".into(), endpoint("N.note", vec![]))];
    assert_failure(
        &p,
        &primary_input(),
        none,
        EngineError::NotABool {
            node: 6,
            found: "string",
        },
    );
}

#[test]
fn foreign_rows_never_repair_selected_owner_absence() {
    let nodes = vec![
        (1, field(named(), &["Rows", "Flag"], None)),
        (2, field(named(), &["Rows", "Message"], None)),
        (3, field(SourceOwner::Primary, &["Rows", "Flag"], None)),
        (4, field(SourceOwner::Primary, &["Rows", "Message"], None)),
    ];
    let p = project(
        vec![
            when_true(owned_rows(named(), &["Rows"]), 1, Some(2)),
            when_true(owned_rows(SourceOwner::Primary, &["Rows"]), 3, Some(4)),
        ],
        nodes,
    );
    // Absent and empty selected collections are empty; Primary rows stay foreign.
    for lookup in [
        group(vec![("Note", text("N.note"))]),
        endpoint("N.note", vec![]),
        group(vec![
            ("Note", text("N.note")),
            ("Rows", Instance::Scalar(Value::Null)),
        ]),
    ] {
        assert_failure(
            &p,
            &primary_input(),
            vec![("Lookup".into(), lookup.clone())],
            failure(2, Some("P2")),
        );
        assert_success(
            &p,
            &endpoint("P.note", vec![row(false, "P1")]),
            vec![("Lookup".into(), lookup)],
        );
    }
    let primary_absent = group(vec![("Note", text("P.note"))]);
    let p = project(
        vec![when_true(
            owned_rows(SourceOwner::Primary, &["Rows"]),
            3,
            Some(4),
        )],
        vec![
            (3, field(SourceOwner::Primary, &["Rows", "Flag"], None)),
            (4, field(SourceOwner::Primary, &["Rows", "Message"], None)),
        ],
    );
    assert_success(&p, &primary_absent, inputs());
    // An unsupplied owner is a typed failure, never another endpoint's rows.
    let p = project(
        vec![when_true(owned_rows(named(), &["Rows"]), 1, Some(2))],
        vec![
            (1, field(named(), &["Rows", "Flag"], None)),
            (2, field(named(), &["Rows", "Message"], None)),
        ],
    );
    assert_failure(
        &p,
        &primary_input(),
        vec![],
        EngineError::OwnedSourceAccess {
            node: None,
            scope: None,
            source: OwnedSourceError::SourceUnavailable { owner: named() },
        },
    );
}

#[test]
fn endpoint_roots_and_nested_rows_keep_exact_positions() {
    // A repeated named root iterates rows; a Group root is one identity row.
    let mut p = project(
        vec![FailureRule {
            iteration: owned_rows(named(), &[]),
            selection: FailureSelection::WhenFalse { predicate: 1 },
            message: Some(9),
        }],
        vec![
            (1, field(named(), &["Flag"], None)),
            (2, field(named(), &["Message"], None)),
            (3, position(named(), Some(&[]))),
            (
                9,
                Node::Call {
                    function: "concat".into(),
                    args: vec![2, 3],
                },
            ),
        ],
    );
    p.extra_sources[0].schema = row_schema();
    let rows = Instance::Repeated(vec![row(true, "R1"), row(false, "R2")]);
    assert_failure(
        &p,
        &primary_input(),
        vec![("Lookup".into(), rows)],
        failure(1, Some("R22")),
    );
    let p = project(
        vec![FailureRule {
            iteration: owned_rows(SourceOwner::Primary, &[]),
            selection: FailureSelection::All,
            message: Some(1),
        }],
        vec![(1, field(SourceOwner::Primary, &["Note"], None))],
    );
    assert_failure(&p, &primary_input(), inputs(), failure(1, Some("P.note")));

    // Nested rows retain their outer and inner positions.
    let lines = SchemaNode::group(
        "Orders",
        vec![
            SchemaNode::scalar("Id", ScalarType::String),
            SchemaNode::group("Lines", vec![SchemaNode::scalar("Flag", ScalarType::Bool)])
                .repeating(),
        ],
    )
    .repeating();
    let mut p = project(
        vec![when_true(
            owned_rows(named(), &["Orders", "Lines"]),
            1,
            Some(9),
        )],
        vec![
            (1, field(named(), &["Orders", "Lines", "Flag"], None)),
            (2, field(named(), &["Orders", "Id"], None)),
            (3, position(named(), Some(&["Orders"]))),
            (4, position(named(), Some(&["Orders", "Lines"]))),
            (
                9,
                Node::Call {
                    function: "concat".into(),
                    args: vec![2, 3, 4],
                },
            ),
        ],
    );
    p.extra_sources[0].schema = SchemaNode::group("Lookup", vec![lines]);
    let line = |flag| group(vec![("Flag", Instance::Scalar(Value::Bool(flag)))]);
    let order = |id: &str, lines: Vec<Instance>| {
        group(vec![("Id", text(id)), ("Lines", Instance::Repeated(lines))])
    };
    let lookup = group(vec![(
        "Orders",
        Instance::Repeated(vec![
            order("A", vec![line(false)]),
            order("B", vec![line(false), line(false), line(true)]),
        ]),
    )]);
    assert_failure(
        &p,
        &primary_input(),
        vec![("Lookup".into(), lookup)],
        failure(1, Some("B23")),
    );
}

#[test]
fn validation_and_admission_reject_unsupported_owned_iteration() {
    let issues = |p: &Project| {
        crate::validate(p)
            .into_iter()
            .map(|issue| (issue.location, issue.message))
            .collect::<Vec<_>>()
    };
    let check = |iteration: FailureIteration, fragment: &str| {
        let mut p = project(
            vec![],
            vec![(
                1,
                Node::Const {
                    value: Value::Bool(true),
                },
            )],
        );
        p.extra_sources.push(NamedSource {
            name: "Dynamic".into(),
            path: "dynamic.json".into(),
            schema: endpoint_schema("Dynamic"),
            options: Default::default(),
            dynamic_path: Some(mapping::DynamicSourcePath::legacy(1, vec!["Rows".into()])),
        });
        p.failure_rules = vec![when_true(iteration, 1, None)];
        let found = issues(&p);
        assert!(
            found
                .iter()
                .any(|(location, message)| location == "failure rule 1"
                    && message.contains(fragment)),
            "{fragment}: {found:?}"
        );
    };
    check(
        owned_rows(SourceOwner::Primary, &["Missing"]),
        "matches no path",
    );
    check(
        owned_rows(SourceOwner::Primary, &["Note"]),
        "not a repeating path",
    );
    check(
        owned_rows(
            SourceOwner::Named {
                name: "Unknown".into(),
            },
            &["Rows"],
        ),
        "Unknown",
    );
    check(
        owned_rows(
            SourceOwner::Named {
                name: "Dynamic".into(),
            },
            &["Rows"],
        ),
        "dynamic extra source `Dynamic`",
    );

    // Specialized consumers stay closed in owned failure rows before evaluation.
    for (node, unsupported) in [
        (
            Node::Aggregate {
                function: mapping::AggregateOp::Count,
                collection: path(&["Rows"]),
                value: vec![],
                expression: None,
                arg: None,
            },
            8,
        ),
        (
            Node::UserFunctionCall {
                function: mapping::FunctionId::new(1),
                args: vec![],
            },
            8,
        ),
        // Generation refuses a generated-item consumer in an owned failure
        // walk, so the interpreter refuses it too.
        (
            Node::SequenceExists {
                sequence: mapping::SequenceExpr::Generate {
                    from: None,
                    to: 9,
                    item: 10,
                },
                predicate: 1,
            },
            8,
        ),
    ] {
        let p = project(
            vec![when_true(
                owned_rows(SourceOwner::Primary, &["Rows"]),
                1,
                Some(8),
            )],
            vec![
                (1, field(SourceOwner::Primary, &["Rows", "Flag"], None)),
                (8, node),
            ],
        );
        let outcome = outcomes(&p, &primary_input(), inputs());
        assert!(
            outcome.iter().all(|outcome| *outcome
                == Err(EngineError::UnsupportedOwnedContext {
                    node: Some(unsupported),
                    function: None,
                    scope: None,
                    context: OwnedCapabilityContext::FailureRule,
                })),
            "{outcome:?}"
        );
    }
}

#[test]
fn legacy_source_rules_keep_their_fallback_beside_owned_rules() {
    // A legacy `Lookup/Rows` path still resolves through the named alias.
    let p = project(
        vec![
            when_true(owned_rows(SourceOwner::Primary, &["Missing"]), 1, None),
            FailureRule {
                iteration: FailureIteration::Source {
                    collection: path(&["Lookup", "Rows"]),
                },
                selection: FailureSelection::WhenTrue { predicate: 2 },
                message: Some(3),
            },
        ],
        vec![
            (
                1,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
            (
                2,
                Node::SourceField {
                    path: path(&["Flag"]),
                    frame: None,
                },
            ),
            (
                3,
                Node::SourceField {
                    path: path(&["Message"]),
                    frame: None,
                },
            ),
        ],
    );
    let mut p = p;
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Note", ScalarType::String),
            row_schema(),
            {
                let mut missing = row_schema();
                missing.name = "Missing".into();
                missing
            },
        ],
    );
    assert_failure(&p, &primary_input(), inputs(), failure(2, Some("N1")));
}
