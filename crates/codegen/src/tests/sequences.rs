use super::*;
use crate::TargetConstruction;
use mapping::ScopeConstruction;

#[test]
fn lowers_portable_generated_sequences_and_their_item_roots() {
    let project = |sequence: mapping::SequenceExpr, nodes: Vec<(u32, Node)>| {
        let mut project = supported_project();
        project.graph.nodes.extend(nodes);
        project.root.children[0].iteration = ScopeIteration::Sequence(sequence);
        project.root.children[0].bindings[0].node = 42;
        project
    };
    let item = || Node::SourceField {
        path: Vec::new(),
        frame: None,
    };

    let tokenize = project(
        mapping::SequenceExpr::Tokenize {
            input: 40,
            delimiter: 41,
            item: 42,
        },
        vec![
            (
                40,
                Node::Const {
                    value: Value::String("a,b".into()),
                },
            ),
            (
                41,
                Node::Const {
                    value: Value::String(",".into()),
                },
            ),
            (42, item()),
        ],
    );
    let lowered = lower(&tokenize).expect("literal tokenize lowers");
    assert_eq!(
        lowered.root.children[0].iteration,
        Some(IterationPlan::generated(GeneratedSequence::Tokenize {
            input: 40,
            delimiter: 41,
            item: 42,
        }))
    );
    assert_eq!(
        lowered
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        vec![10, 20, 40, 41, 42]
    );

    let chunks = project(
        mapping::SequenceExpr::TokenizeByLength {
            input: 40,
            length: 41,
            item: 42,
        },
        vec![
            (
                40,
                Node::Const {
                    value: Value::String("abcd".into()),
                },
            ),
            (
                41,
                Node::Const {
                    value: Value::Int(2),
                },
            ),
            (42, item()),
        ],
    );
    assert_eq!(
        lower(&chunks)
            .expect("length tokenize lowers")
            .root
            .children[0]
            .iteration,
        Some(IterationPlan::generated(
            GeneratedSequence::TokenizeByLength {
                input: 40,
                length: 41,
                item: 42,
            }
        ))
    );

    let range = project(
        mapping::SequenceExpr::Generate {
            from: Some(40),
            to: 41,
            item: 42,
        },
        vec![
            (
                40,
                Node::Const {
                    value: Value::Int(2),
                },
            ),
            (
                41,
                Node::Const {
                    value: Value::Int(4),
                },
            ),
            (42, item()),
        ],
    );
    assert_eq!(
        lower(&range).expect("inclusive range lowers").root.children[0].iteration,
        Some(IterationPlan::generated(GeneratedSequence::Range {
            from: Some(40),
            to: 41,
            item: 42,
        }))
    );

    let recursive = project(
        mapping::SequenceExpr::RecursiveCollect {
            collection: vec!["directory".into()],
            children: vec!["children".into()],
            descent_value: vec!["name".into()],
            values: vec!["files".into()],
            value: vec!["name".into()],
            prefix: 40,
            separator: 41,
            item: 42,
        },
        vec![
            (
                40,
                Node::Const {
                    value: Value::String(String::new()),
                },
            ),
            (
                41,
                Node::Const {
                    value: Value::String("/".into()),
                },
            ),
            (42, item()),
        ],
    );
    assert_eq!(
        lower(&recursive)
            .expect("recursive collect lowers")
            .root
            .children[0]
            .iteration,
        Some(IterationPlan::generated(
            GeneratedSequence::RecursiveCollect {
                collection: vec!["directory".into()],
                children: vec!["children".into()],
                descent_value: vec!["name".into()],
                values: vec!["files".into()],
                value: vec!["name".into()],
                prefix: 40,
                separator: 41,
                item: 42,
            }
        ))
    );
}

#[test]
fn lowers_recursive_collect_into_a_repeating_scalar_scope() {
    let source = SchemaNode::group(
        "directory",
        vec![
            scalar("name"),
            SchemaNode::group("file", vec![scalar("name")]).repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
        ],
    );
    let target = SchemaNode::group("Files", vec![scalar("File").repeating()]);
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                1,
                Node::Const {
                    value: Value::String(String::new()),
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::String("/".into()),
                },
            ),
            (
                3,
                Node::SourceField {
                    path: Vec::new(),
                    frame: None,
                },
            ),
            (
                4,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
            (
                5,
                Node::If {
                    condition: 4,
                    then: 3,
                    else_: 3,
                },
            ),
        ]),
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
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "File".into(),
                iteration: ScopeIteration::Sequence(mapping::SequenceExpr::RecursiveCollect {
                    collection: Vec::new(),
                    children: vec!["directory".into()],
                    descent_value: vec!["name".into()],
                    values: vec!["file".into()],
                    value: vec!["name".into()],
                    prefix: 1,
                    separator: 2,
                    item: 3,
                }),
                construction: ScopeConstruction::Scalar { value: 5 },
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };

    let program = lower(&project).expect("canonical recursive collect lowers");
    let scope = &program.root.children[0];
    assert_eq!(
        scope.construction,
        TargetConstruction::Scalar { expression: 5 }
    );
    assert!(matches!(scope.iteration, Some(IterationPlan { .. })));
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
}

#[test]
fn lowers_portable_sequence_reducers_and_private_item_roots() {
    let mut project = supported_project();
    project.target = SchemaNode::group(
        "Target",
        vec![typed_scalar("Found", ScalarType::Bool), scalar("Selected")],
    );
    project.graph.nodes.extend([
        (
            40,
            Node::Const {
                value: Value::String("alpha,beta".into()),
            },
        ),
        (
            41,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            42,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            43,
            Node::Const {
                value: Value::String("beta".into()),
            },
        ),
        (
            44,
            Node::Call {
                function: "equal".into(),
                args: vec![42, 43],
            },
        ),
        (
            45,
            Node::SequenceExists {
                sequence: mapping::SequenceExpr::Tokenize {
                    input: 40,
                    delimiter: 41,
                    item: 42,
                },
                predicate: 44,
            },
        ),
        (
            46,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            47,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            48,
            Node::SequenceItemAt {
                sequence: mapping::SequenceExpr::Tokenize {
                    input: 40,
                    delimiter: 41,
                    item: 46,
                },
                index: 47,
            },
        ),
    ]);
    project.root.bindings = vec![
        MappingBinding {
            target_field: "Found".into(),
            node: 45,
        },
        MappingBinding {
            target_field: "Selected".into(),
            node: 48,
        },
    ];
    project.root.children.clear();

    let program = lower(&project).expect("portable sequence reducers lower");

    assert!(program.expressions.iter().any(|node| {
        node.id == 45
            && node.expression
                == Expression::SequenceExists {
                    sequence: GeneratedSequence::Tokenize {
                        input: 40,
                        delimiter: 41,
                        item: 42,
                    },
                    predicate: 44,
                }
    }));
    assert!(program.expressions.iter().any(|node| {
        node.id == 48
            && node.expression
                == Expression::SequenceItemAt {
                    sequence: GeneratedSequence::Tokenize {
                        input: 40,
                        delimiter: 41,
                        item: 46,
                    },
                    index: 47,
                }
    }));
    assert!(program.expressions.iter().any(|node| node.id == 42));
    assert!(program.expressions.iter().any(|node| node.id == 46));
}

#[test]
fn reports_other_iteration_forms_at_the_static_target_path() {
    let mut project = supported_project();
    project.root.children[0].iteration =
        ScopeIteration::Sequence(mapping::SequenceExpr::TokenizeRegex {
            input: 20,
            pattern: 20,
            flags: None,
            item: 40,
        });
    project.graph.nodes.insert(
        40,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    project.graph.nodes.insert(
        41,
        Node::Call {
            function: "parse_datetime".into(),
            args: vec![20, 20],
        },
    );
    project.root.children[0].filter = Some(41);

    let diagnostics = lower(&project)
        .expect_err("regex tokenize remains outside the portable subset")
        .into_diagnostics();

    assert!(diagnostics.contains(&Diagnostic::UnsupportedScope {
        target_path: vec!["Details".into()],
        feature: ScopeFeature::GeneratedSequence(UnsupportedSequenceKind::TokenizeRegex),
    }));
    assert!(diagnostics.contains(&Diagnostic::UnsupportedFunction {
        node: 41,
        function: "parse_datetime".into(),
    }));
}

#[test]
fn reports_nonportable_sequence_reducers_without_partial_lowering() {
    let mut project = supported_project();
    project.graph.nodes.extend([
        (
            40,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            41,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            42,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            43,
            Node::SequenceExists {
                sequence: mapping::SequenceExpr::TokenizeRegex {
                    input: 20,
                    pattern: 40,
                    flags: None,
                    item: 41,
                },
                predicate: 42,
            },
        ),
    ]);
    project.root.bindings[1].node = 43;

    let diagnostics = lower(&project)
        .expect_err("regex-backed reducers remain outside the portable subset")
        .into_diagnostics();

    assert_eq!(
        diagnostics,
        vec![Diagnostic::UnsupportedNode {
            node: 43,
            kind: UnsupportedNodeKind::SequenceExists,
        }]
    );
}
