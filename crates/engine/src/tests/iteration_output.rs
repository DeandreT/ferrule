use std::collections::BTreeMap;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{Binding, Graph, IterationOutput, Node, Project, Scope, SequenceWindow};

use super::{EngineError, run, validate};

fn project() -> Project {
    project_with_output(IterationOutput::First)
}

fn project_with_output(iteration_output: IterationOutput) -> Project {
    let person = SchemaNode::group(
        "Person",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Keep", ScalarType::Bool),
        ],
    )
    .repeating();
    let source = SchemaNode::group(
        "root",
        vec![SchemaNode::group("Department", vec![person]).repeating()],
    );
    let selected = SchemaNode::group(
        "Selected",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("DepartmentPosition", ScalarType::Int),
            SchemaNode::scalar("PersonPosition", ScalarType::Int),
        ],
    );
    let target = SchemaNode::group(
        "root",
        vec![
            SchemaNode::group(
                "Department",
                vec![SchemaNode::scalar("Position", ScalarType::Int), selected],
            )
            .repeating(),
        ],
    );
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::SourceField {
                    path: vec!["Name".into()],
                    frame: None,
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec!["Keep".into()],
                    frame: None,
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::Int(2),
                },
            ),
            (
                3,
                Node::Position {
                    collection: vec!["Department".into()],
                },
            ),
            (
                4,
                Node::Position {
                    collection: vec!["Person".into()],
                },
            ),
        ]),
    };
    let selected_scope = Scope {
        target_field: "Selected".into(),
        iteration: mapping::ScopeIteration::Source(vec!["Person".into()]),
        filter: Some(1),
        windows: vec![SequenceWindow::First { count: 2 }],
        iteration_output,
        bindings: vec![
            Binding {
                target_field: "Name".into(),
                node: 0,
            },
            Binding {
                target_field: "DepartmentPosition".into(),
                node: 3,
            },
            Binding {
                target_field: "PersonPosition".into(),
                node: 4,
            },
        ],
        ..Scope::default()
    };
    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Department".into(),
                iteration: mapping::ScopeIteration::Source(vec!["Department".into()]),
                bindings: vec![Binding {
                    target_field: "Position".into(),
                    node: 3,
                }],
                children: vec![selected_scope],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn person(name: &str, keep: bool) -> Instance {
    Instance::Group(vec![
        ("Name".into(), Instance::Scalar(Value::String(name.into()))),
        ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
    ])
}

fn department(people: Vec<Instance>) -> Instance {
    Instance::Group(vec![("Person".into(), Instance::Repeated(people))])
}

#[test]
fn first_output_applies_filter_and_window_and_preserves_nested_positions() {
    let project = project();
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let source = Instance::Group(vec![(
        "Department".into(),
        Instance::Repeated(vec![
            department(vec![
                person("discarded", false),
                person("first", true),
                person("not reached", true),
            ]),
            department(vec![person("also discarded", false)]),
        ]),
    )]);

    let output = run(&project, &source).unwrap();
    let departments = output
        .field("Department")
        .and_then(Instance::as_repeated)
        .unwrap();
    let selected = departments[0].field("Selected").unwrap();
    assert_eq!(
        selected.field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("first".into()))
    );
    assert_eq!(
        selected
            .field("DepartmentPosition")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
    assert_eq!(
        selected
            .field("PersonPosition")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
    assert_eq!(
        departments[1]
            .field("Position")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(2))
    );
    assert_eq!(
        departments[1].field("Selected"),
        Some(&Instance::Group(Vec::new()))
    );
}

#[test]
fn validation_rejects_invalid_first_output_cardinality() {
    let mut repeating_target = project();
    let SchemaKind::Group { children, .. } = &mut repeating_target.target.kind else {
        panic!("test target must be a group");
    };
    let Some(department) = children.iter_mut().find(|node| node.name == "Department") else {
        panic!("test target must contain Department");
    };
    let SchemaKind::Group { children, .. } = &mut department.kind else {
        panic!("Department must be a group");
    };
    let Some(selected) = children.iter_mut().find(|node| node.name == "Selected") else {
        panic!("Department must contain Selected");
    };
    selected.repeating = true;
    let issues = validate(&repeating_target);
    assert!(issues.iter().any(|issue| {
        issue
            .message
            .contains("first-item output requires a non-repeating target group schema")
    }));

    let mut without_iteration = project();
    without_iteration.root.iteration_output = IterationOutput::First;
    let issues = validate(&without_iteration);
    assert!(issues.iter().any(|issue| {
        issue
            .message
            .contains("first-item output requires an iterated source")
    }));
    assert_eq!(
        run(&without_iteration, &Instance::Group(Vec::new())),
        Err(EngineError::FirstOutputWithoutIteration)
    );
}

#[test]
fn non_iterating_scope_constructs_one_item_for_a_repeating_target_group() {
    let project = Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Entry",
                    vec![SchemaNode::scalar("Label", ScalarType::String)],
                )
                .repeating(),
            ],
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
            nodes: BTreeMap::from([(
                0,
                Node::Const {
                    value: Value::String("static".into()),
                },
            )]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Entry".into(),
                bindings: vec![Binding {
                    target_field: "Label".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));

    let output = run(&project, &Instance::Group(Vec::new())).unwrap();
    let entries = output
        .field("Entry")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].field("Label").and_then(Instance::as_scalar),
        Some(&Value::String("static".into()))
    );
}

#[test]
fn mapped_sequence_preserves_zero_one_or_many_ordered_items() {
    let project = project_with_output(IterationOutput::MappedSequence);
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let cases = [
        (vec![person("discarded", false)], Vec::<&str>::new()),
        (vec![person("one", true)], vec!["one"]),
        (
            vec![person("first", true), person("second", true)],
            vec!["first", "second"],
        ),
    ];

    for (people, expected) in cases {
        let source = Instance::Group(vec![(
            "Department".into(),
            Instance::Repeated(vec![department(people)]),
        )]);
        let output = run(&project, &source).unwrap();
        let departments = output
            .field("Department")
            .and_then(Instance::as_repeated)
            .unwrap();
        let selected = departments[0]
            .field("Selected")
            .and_then(Instance::as_mapped_sequence)
            .unwrap();
        let names = selected
            .iter()
            .filter_map(|item| item.field("Name").and_then(Instance::as_scalar))
            .collect::<Vec<_>>();
        let expected = expected
            .into_iter()
            .map(|name| Value::String(name.into()))
            .collect::<Vec<_>>();
        assert_eq!(names, expected.iter().collect::<Vec<_>>());
    }
}

#[test]
fn concatenated_mapped_sequence_preserves_segment_and_item_order() {
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Domestic",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            )
            .repeating(),
            SchemaNode::group(
                "International",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Target",
        vec![SchemaNode::group(
            "Address",
            vec![SchemaNode::scalar("Name", ScalarType::String)],
        )],
    );
    let segment = |collection: &str, node| Scope {
        iteration: mapping::ScopeIteration::Source(vec![collection.into()]),
        iteration_output: IterationOutput::MappedSequence,
        bindings: vec![Binding {
            target_field: "Name".into(),
            node,
        }],
        ..Scope::default()
    };
    let project = Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: Some(vec!["Domestic".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: Some(vec!["International".into()]),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Address".into(),
                iteration: mapping::ScopeIteration::Concatenate(mapping::ScopeSequence::new(
                    segment("Domestic", 0),
                    vec![segment("International", 1)],
                )),
                iteration_output: IterationOutput::MappedSequence,
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let named = |name: &str| {
        Instance::Group(vec![(
            "Name".into(),
            Instance::Scalar(Value::String(name.into())),
        )])
    };
    let input = Instance::Group(vec![
        (
            "Domestic".into(),
            Instance::Repeated(vec![named("North"), named("South")]),
        ),
        (
            "International".into(),
            Instance::Repeated(vec![named("East")]),
        ),
    ]);

    let output = run(&project, &input).unwrap();
    let addresses = output
        .field("Address")
        .and_then(Instance::as_mapped_sequence)
        .unwrap();
    let names = addresses
        .iter()
        .filter_map(|address| address.field("Name").and_then(Instance::as_scalar))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            &Value::String("North".into()),
            &Value::String("South".into()),
            &Value::String("East".into()),
        ]
    );
}

#[test]
fn validation_rejects_invalid_mapped_sequence_scopes_and_targets() {
    let mut root = project_with_output(IterationOutput::MappedSequence);
    root.root.set_source(Some(vec!["Department".into()]));
    root.root.iteration_output = IterationOutput::MappedSequence;
    let issues = validate(&root);
    assert!(issues.iter().any(|issue| {
        issue
            .message
            .contains("mapped-sequence output is not valid for the project root scope")
    }));

    let mut repeating_target = project_with_output(IterationOutput::MappedSequence);
    let SchemaKind::Group { children, .. } = &mut repeating_target.target.kind else {
        panic!("test target must be a group");
    };
    let SchemaKind::Group { children, .. } = &mut children[0].kind else {
        panic!("Department must be a group");
    };
    children[1].repeating = true;
    let issues = validate(&repeating_target);
    assert!(issues.iter().any(|issue| {
        issue
            .message
            .contains("mapped-sequence output requires a non-repeating target group schema")
    }));

    let mut scalar_target = project_with_output(IterationOutput::MappedSequence);
    let SchemaKind::Group { children, .. } = &mut scalar_target.target.kind else {
        panic!("test target must be a group");
    };
    let SchemaKind::Group { children, .. } = &mut children[0].kind else {
        panic!("Department must be a group");
    };
    children[1].kind = SchemaKind::Scalar {
        ty: ScalarType::String,
    };
    let issues = validate(&scalar_target);
    assert!(issues.iter().any(|issue| {
        issue
            .message
            .contains("mapped-sequence output requires a non-repeating target group schema")
    }));

    let mut without_iteration = project_with_output(IterationOutput::MappedSequence);
    without_iteration.root.children[0].children[0].set_source(None);
    let issues = validate(&without_iteration);
    assert!(issues.iter().any(|issue| {
        issue
            .message
            .contains("mapped-sequence output requires an iterated source")
    }));

    let mut dynamic_merge = project_with_output(IterationOutput::MappedSequence);
    dynamic_merge.root.children[0].children[0].merge_dynamic_fields = true;
    let issues = validate(&dynamic_merge);
    assert!(issues.iter().any(|issue| {
        issue
            .message
            .contains("mapped-sequence output cannot be combined with dynamic object merge")
    }));
}

#[test]
fn first_output_does_not_evaluate_later_unused_bindings() {
    let project = Project {
        source: SchemaNode::group("row", vec![SchemaNode::scalar("Value", ScalarType::String)])
            .repeating(),
        target: SchemaNode::group("row", vec![SchemaNode::scalar("Value", ScalarType::String)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::Call {
                        function: "upper".into(),
                        args: vec![0],
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: mapping::ScopeIteration::Source(Vec::new()),
            iteration_output: IterationOutput::First,
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    };
    let source = Instance::Repeated(vec![
        Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::String("first".into())),
        )]),
        Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(2)))]),
    ]);

    let output = run(&project, &source).unwrap();
    assert_eq!(
        output.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("FIRST".into()))
    );
}
