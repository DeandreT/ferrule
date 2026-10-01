use super::*;
use mapping::{AggregateOp, SequenceExpr, SequenceWindow, SortKey};

fn project() -> Project {
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Rows", vec![typed_scalar("Value", ScalarType::Int)]).repeating(),
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
            nodes: [
                (
                    1,
                    Node::Const {
                        value: Value::Int(3),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (10, item()),
            ]
            .into(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(range(10, 1)),
                bindings: vec![MappingBinding {
                    target_field: "Value".into(),
                    node: 10,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

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

fn rejects(project: &Project, location: &str, expression: u32, item: u32) {
    let error = lower(project).expect_err("invalid item context cannot produce a Program");
    let message = format!(
        "expression {expression} references generated sequence item node {item} outside its owning context"
    );
    assert!(error.diagnostics().iter().any(|diagnostic| matches!(diagnostic,
        Diagnostic::Validation { location: found, message: text } if found == location && text == &message
    )), "{:#?}", error.diagnostics());
}

#[test]
fn lower_rejects_foreign_named_scope_items_before_portable_ir_exists() {
    for indirect in [false, true] {
        let mut project = project();
        project.graph.nodes.extend([
            (20, item()),
            (
                31,
                Node::Call {
                    function: "add".into(),
                    args: vec![10, 2],
                },
            ),
        ]);
        let mut root = project.root.clone();
        root.children[0].set_sequence(Some(range(20, 1)));
        root.children[0].bindings[0].node = if indirect { 31 } else { 10 };
        project.extra_targets.push(NamedTarget {
            name: "Other".into(),
            path: None,
            schema: project.target.clone(),
            options: Default::default(),
            root,
        });
        rejects(
            &project,
            "extra target `Other` scope `Rows`",
            if indirect { 31 } else { 10 },
            10,
        );
    }
}

#[test]
fn lower_accepts_active_parent_aggregate_inputs_and_parent_arguments() {
    for argument in [false, true] {
        let mut project = project();
        project.graph.nodes.extend([
            (30, item()),
            (
                40,
                Node::SequenceAggregate {
                    function: if argument {
                        AggregateOp::ItemAt
                    } else {
                        AggregateOp::Sum
                    },
                    sequence: range(30, if argument { 1 } else { 10 }),
                    predicate: None,
                    expression: None,
                    arg: argument.then_some(10),
                },
            ),
        ]);
        project.root.children[0].bindings[0].node = 40;
        let program = lower(&project).expect("active parent aggregate context lowers");
        assert_eq!(validate_program(&program), Ok(()));
        let expected_to = if argument { 1 } else { 10 };
        assert!(
            matches!(program.expressions.iter().find(|expression| expression.id == 40).map(|node| &node.expression),
                Some(Expression::SequenceAggregate { sequence: GeneratedSequence::Range { to, .. }, arg, .. })
                    if *to == expected_to && *arg == argument.then_some(10)
            )
        );
    }
}

#[test]
fn lower_preserves_nested_parent_arguments_and_ancestor_item_reads() {
    let mut project = project();
    project.graph.nodes.insert(20, item());
    let SchemaKind::Group { children, .. } = &mut project.target.kind else {
        unreachable!()
    };
    let SchemaKind::Group {
        children: fields, ..
    } = &mut children[0].kind
    else {
        unreachable!()
    };
    fields.push(
        SchemaNode::group("Children", vec![typed_scalar("Value", ScalarType::Int)]).repeating(),
    );
    project.root.children[0].children.push(Scope {
        target_field: "Children".into(),
        iteration: ScopeIteration::Sequence(range(20, 10)),
        windows: vec![SequenceWindow::First { count: 10 }],
        bindings: vec![MappingBinding {
            target_field: "Value".into(),
            node: 10,
        }],
        ..Scope::default()
    });
    assert_eq!(validate_program(&lower(&project).unwrap()), Ok(()));
    project.root.children[0].children[0].windows[0] = SequenceWindow::First { count: 20 };
    rejects(&project, "scope `Rows/Children`", 20, 20);
}

#[test]
fn lower_rejects_private_secondary_sort_and_scalar_roots_at_engine_validation() {
    for scalar in [false, true] {
        for indirect in [false, true] {
            let mut project = project();
            project.graph.nodes.extend([
                (30, item()),
                (
                    35,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![30, 2],
                    },
                ),
                (
                    40,
                    Node::SequenceExists {
                        sequence: range(30, 1),
                        predicate: 35,
                    },
                ),
            ]);
            let SchemaKind::Group { children, .. } = &mut project.target.kind else {
                unreachable!()
            };
            let SchemaKind::Group {
                children: fields, ..
            } = &mut children[0].kind
            else {
                unreachable!()
            };
            fields.push(typed_scalar("Flag", ScalarType::Bool));
            let scope = &mut project.root.children[0];
            scope.bindings.push(MappingBinding {
                target_field: "Flag".into(),
                node: 40,
            });
            let expression = if indirect { 35 } else { 30 };
            let location = if scalar {
                fields[0] = typed_scalar(
                    "Value",
                    if indirect {
                        ScalarType::Bool
                    } else {
                        ScalarType::Int
                    },
                );
                scope
                    .bindings
                    .retain(|binding| binding.target_field != "Value");
                scope.children.push(Scope {
                    target_field: "Value".into(),
                    construction: ScopeConstruction::Scalar { value: expression },
                    ..Scope::default()
                });
                "scope `Rows/Value`"
            } else {
                scope.sort_by = Some(2);
                scope.sort_then_by.push(SortKey {
                    node: expression,
                    descending: false,
                });
                "scope `Rows`"
            };
            rejects(&project, location, expression, 30);
        }
    }
}

fn dynamic_item_project() -> Project {
    let mut project = project();
    project.source = SchemaNode::group(
        "Input",
        vec![typed_scalar("File", ScalarType::String).repeating()],
    );
    project.extra_sources.push(mapping::NamedSource {
        name: "document".into(),
        path: String::new(),
        schema: SchemaNode::group("Document", Vec::new()),
        options: Default::default(),
        dynamic_path: Some(mapping::DynamicSourcePath {
            node: 0,
            iteration: vec!["File".into()],
        }),
    });
    project.graph.nodes.extend([
        (
            0,
            Node::SourceField {
                path: Vec::new(),
                frame: Some(vec!["File".into()]),
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
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            5,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            80,
            Node::SequenceExists {
                sequence: range(30, 1),
                predicate: 4,
            },
        ),
        (
            81,
            Node::SequenceItemAt {
                sequence: range(40, 1),
                index: 3,
            },
        ),
        (
            82,
            Node::SequenceAggregate {
                function: AggregateOp::Count,
                sequence: range(50, 1),
                predicate: None,
                expression: None,
                arg: None,
            },
        ),
    ]);
    for item_id in [20, 30, 40, 50, 60] {
        project.graph.nodes.insert(item_id, item());
    }
    let SchemaKind::Group { children, .. } = &mut project.target.kind else {
        unreachable!()
    };
    children.extend([
        typed_scalar("Flag", ScalarType::Bool),
        typed_scalar("Selected", ScalarType::Int),
        typed_scalar("Count", ScalarType::Int),
    ]);
    project.root.bindings.extend([
        MappingBinding {
            target_field: "Flag".into(),
            node: 80,
        },
        MappingBinding {
            target_field: "Selected".into(),
            node: 81,
        },
        MappingBinding {
            target_field: "Count".into(),
            node: 82,
        },
    ]);
    let mut named_root = project.root.clone();
    named_root.bindings.clear();
    named_root.children[0].set_sequence(Some(range(20, 1)));
    named_root.children[0].bindings[0].node = 20;
    project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: named_root,
    });
    project.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Sequence {
            sequence: range(60, 1),
        },
        selection: mapping::FailureSelection::WhenFalse { predicate: 4 },
        message: None,
    });
    project
}

fn use_dynamic_private_join(project: &mut Project) {
    project.graph.nodes.insert(
        82,
        Node::SequenceAggregate {
            function: AggregateOp::Join,
            sequence: SequenceExpr::Tokenize {
                input: 0,
                delimiter: 5,
                item: 50,
            },
            predicate: None,
            expression: Some(50),
            arg: Some(5),
        },
    );
    project
        .root
        .bindings
        .retain(|binding| binding.target_field != "Count");
    project.extra_sources[0].dynamic_path.as_mut().unwrap().node = 82;
}

#[test]
fn lower_rejects_dynamic_paths_to_all_generated_item_owner_kinds() {
    for item in [10, 20, 30, 40, 50, 60] {
        for indirect in [false, true] {
            let mut project = dynamic_item_project();
            project.graph.nodes.insert(
                70,
                Node::Call {
                    function: "string".into(),
                    args: vec![item],
                },
            );
            let expression = if indirect { 70 } else { item };
            project.extra_sources[0].dynamic_path.as_mut().unwrap().node = expression;
            rejects(&project, "extra source `document`", expression, item);
        }
    }
}

#[test]
fn public_program_dynamic_paths_use_the_complete_item_inventory() {
    let program = lower(&dynamic_item_project()).expect("ordinary driver path lowers");
    for item in [10, 20, 30, 40, 50, 60] {
        for indirect in [false, true] {
            let mut invalid = program.clone();
            invalid.expressions.push(crate::ExpressionNode {
                id: 70,
                expression: Expression::Call {
                    function: ScalarFunction::String,
                    args: vec![item],
                },
            });
            let expression = if indirect { 70 } else { item };
            invalid.extra_sources[0].dynamic.as_mut().unwrap().path = expression;
            assert_eq!(
                validate_program(&invalid),
                Err(ProgramValidationError::SequenceItemOutOfContext {
                    owner: crate::SequenceOwner::DynamicSource("document".into()),
                    expression,
                    item,
                })
            );
        }
    }
}

#[test]
fn public_program_dynamic_reducer_input_and_value_errors_keep_specific_owners() {
    let mut project = dynamic_item_project();
    use_dynamic_private_join(&mut project);
    let program = lower(&project).expect("private dynamic reducer lowers");
    for foreign_value in [false, true] {
        let mut invalid = program.clone();
        invalid.expressions.push(crate::ExpressionNode {
            id: 70,
            expression: Expression::Call {
                function: ScalarFunction::String,
                args: vec![10],
            },
        });
        let Expression::SequenceAggregate {
            sequence,
            expression,
            ..
        } = &mut invalid
            .expressions
            .iter_mut()
            .find(|expression| expression.id == 82)
            .unwrap()
            .expression
        else {
            unreachable!()
        };
        if foreign_value {
            *expression = Some(10);
        } else {
            *sequence = GeneratedSequence::Tokenize {
                input: 70,
                delimiter: 5,
                item: 50,
            };
        }
        assert_eq!(
            validate_program(&invalid),
            Err(ProgramValidationError::SequenceItemOutOfContext {
                owner: crate::SequenceOwner::Expression(82),
                expression: 82,
                item: 10,
            })
        );
    }
}

#[test]
fn lower_preserves_dynamic_driver_frames_and_legal_private_reducer_permissions() {
    for private in [false, true] {
        let mut project = dynamic_item_project();
        if private {
            use_dynamic_private_join(&mut project);
        }
        let program = lower(&project).expect("legal dynamic path context lowers");
        assert_eq!(validate_program(&program), Ok(()));
        assert_eq!(
            program.extra_sources[0].dynamic.as_ref().unwrap().path,
            if private { 82 } else { 0 }
        );
        assert!(
            matches!(program.expressions.iter().find(|expression| expression.id == 0).map(|expression| &expression.expression), Some(Expression::SourceField { frame: Some(frame), path }) if frame == &["File"] && path.is_empty())
        );
    }
}
