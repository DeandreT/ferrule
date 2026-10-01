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
