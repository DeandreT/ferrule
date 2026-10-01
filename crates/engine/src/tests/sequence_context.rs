use super::*;
use ir::{SchemaKind, SchemaNode};
use mapping::{
    AggregateOp, Binding, DynamicChild, FailureIteration, FailureRule, FailureSelection,
    NamedTarget, ScopeIteration, ScopeSequence, SequenceExpr, SequenceWindow, SortKey,
};

fn item() -> Node {
    Node::SourceField {
        path: Vec::new(),
        frame: None,
    }
}

fn constant(value: i64) -> Node {
    Node::Const {
        value: Value::Int(value),
    }
}

fn call(function: &str, args: &[NodeId]) -> Node {
    Node::Call {
        function: function.into(),
        args: args.to_vec(),
    }
}

fn range(item: NodeId, to: NodeId) -> SequenceExpr {
    SequenceExpr::Generate {
        from: None,
        to,
        item,
    }
}

fn rows(item: NodeId, to: NodeId) -> Scope {
    Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Sequence(range(item, to)),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: item,
        }],
        ..Scope::default()
    }
}

fn row_schema(ty: ScalarType) -> SchemaNode {
    SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ty)]).repeating()
}

fn project() -> Project {
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group("Output", vec![row_schema(ScalarType::Int)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(1, constant(3)), (2, constant(0)), (10, item())].into(),
        },
        root: Scope {
            children: vec![rows(10, 1)],
            ..Scope::default()
        },
    }
}

fn scope_owner(target: ValidationEndpoint, path: &[ValidationScopeStep]) -> ValidationOwner {
    ValidationOwner::Scope(ValidationScopeLocation {
        target,
        path: path.to_vec(),
    })
}

fn rejects(project: &Project, owner: ValidationOwner, expression: NodeId, item: NodeId) {
    let issues = validate(project);
    let message = format!(
        "expression {expression} references generated sequence item node {item} outside its owning context"
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.owner.as_ref() == Some(&owner) && issue.message == message),
        "{issues:#?}"
    );
}

fn output_values(output: &Instance, field: &str) -> Vec<Value> {
    output
        .field("Rows")
        .and_then(Instance::as_repeated)
        .unwrap()
        .iter()
        .map(|row| {
            row.field(field)
                .and_then(Instance::as_scalar)
                .unwrap()
                .clone()
        })
        .collect()
}

fn run_valid(project: &Project) -> ExecutionOutputs {
    assert!(validate(project).is_empty(), "{:#?}", validate(project));
    run_outputs(project, &Instance::Group(Vec::new())).unwrap()
}

#[test]
fn foreign_scope_items_reject_direct_and_indirect_named_and_sibling_bindings() {
    for named in [false, true] {
        for indirect in [false, true] {
            let mut project = project();
            project
                .graph
                .nodes
                .extend([(20, item()), (31, call("add", &[10, 2]))]);
            let mut second = rows(20, 1);
            second.bindings[0].node = if indirect { 31 } else { 10 };
            let owner = if named {
                project.extra_targets.push(NamedTarget {
                    name: "Other".into(),
                    path: None,
                    schema: project.target.clone(),
                    options: Default::default(),
                    root: Scope {
                        children: vec![second],
                        ..Scope::default()
                    },
                });
                scope_owner(
                    ValidationEndpoint::NamedTarget {
                        index: 0,
                        name: "Other".into(),
                    },
                    &[ValidationScopeStep::Child(0)],
                )
            } else {
                second.target_field = "Other".into();
                let SchemaKind::Group { children, .. } = &mut project.target.kind else {
                    unreachable!()
                };
                let mut schema = row_schema(ScalarType::Int);
                schema.name = "Other".into();
                children.push(schema);
                project.root.children.push(second);
                scope_owner(ValidationEndpoint::Target, &[ValidationScopeStep::Child(1)])
            };
            rejects(&project, owner, if indirect { 31 } else { 10 }, 10);
        }
    }
    let mut valid = project();
    valid.graph.nodes.insert(20, item());
    valid.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: None,
        schema: valid.target.clone(),
        options: Default::default(),
        root: Scope {
            children: vec![rows(20, 1)],
            ..Scope::default()
        },
    });
    let outputs = run_valid(&valid);
    assert_eq!(
        output_values(&outputs.primary, "Value"),
        [Value::Int(1), Value::Int(2), Value::Int(3)]
    );
    assert_eq!(
        output_values(&outputs.extras[0].instance, "Value"),
        output_values(&outputs.primary, "Value")
    );
}

#[test]
fn generator_window_and_block_arguments_cannot_read_their_own_future_item() {
    for role in 0..3 {
        for indirect in [false, true] {
            let mut project = project();
            project.graph.nodes.insert(31, call("add", &[10, 2]));
            let expression = if indirect { 31 } else { 10 };
            let rows = &mut project.root.children[0];
            match role {
                0 => rows.set_sequence(Some(SequenceExpr::Generate {
                    from: Some(expression),
                    to: 1,
                    item: 10,
                })),
                1 => rows
                    .windows
                    .push(SequenceWindow::First { count: expression }),
                _ => rows.group_into_blocks = Some(expression),
            }
            rejects(
                &project,
                scope_owner(ValidationEndpoint::Target, &[ValidationScopeStep::Child(0)]),
                expression,
                10,
            );
        }
    }
}

#[derive(Clone, Copy)]
enum PrivateOwner {
    Exists,
    Aggregate,
    Failure,
}

#[test]
fn private_items_cannot_escape_through_secondary_sorts_or_scalar_construction() {
    for owner in [
        PrivateOwner::Exists,
        PrivateOwner::Aggregate,
        PrivateOwner::Failure,
    ] {
        for scalar in [false, true] {
            for indirect in [false, true] {
                let mut project = project();
                project.graph.nodes.insert(30, item());
                let (expression, extra_field, extra_type) = match owner {
                    PrivateOwner::Exists => {
                        project.graph.nodes.extend([
                            (35, call("greater_than", &[30, 2])),
                            (
                                40,
                                Node::SequenceExists {
                                    sequence: range(30, 1),
                                    predicate: 35,
                                },
                            ),
                        ]);
                        (35, Some("Flag"), ScalarType::Bool)
                    }
                    PrivateOwner::Aggregate => {
                        project.graph.nodes.extend([
                            (35, call("add", &[30, 2])),
                            (
                                40,
                                Node::SequenceAggregate {
                                    function: AggregateOp::Sum,
                                    sequence: range(30, 1),
                                    predicate: None,
                                    expression: Some(35),
                                    arg: None,
                                },
                            ),
                        ]);
                        (35, Some("Sum"), ScalarType::Int)
                    }
                    PrivateOwner::Failure => {
                        project.graph.nodes.insert(35, call("less_than", &[30, 2]));
                        project.failure_rules.push(FailureRule {
                            iteration: FailureIteration::Sequence {
                                sequence: range(30, 1),
                            },
                            selection: FailureSelection::WhenTrue { predicate: 35 },
                            message: None,
                        });
                        (35, None, ScalarType::Bool)
                    }
                };
                let leaked = if indirect { expression } else { 30 };
                let SchemaKind::Group { children, .. } = &mut project.target.kind else {
                    unreachable!()
                };
                let SchemaKind::Group {
                    children: fields, ..
                } = &mut children[0].kind
                else {
                    unreachable!()
                };
                let scope = &mut project.root.children[0];
                if let Some(name) = extra_field {
                    fields.push(SchemaNode::scalar(name, extra_type));
                    scope.bindings.push(Binding {
                        target_field: name.into(),
                        node: 40,
                    });
                }
                let path = if scalar {
                    fields[0] = SchemaNode::scalar(
                        "Value",
                        if indirect {
                            extra_type
                        } else {
                            ScalarType::Int
                        },
                    );
                    scope
                        .bindings
                        .retain(|binding| binding.target_field != "Value");
                    scope.children.push(Scope {
                        target_field: "Value".into(),
                        construction: ScopeConstruction::Scalar { value: leaked },
                        ..Scope::default()
                    });
                    vec![ValidationScopeStep::Child(0), ValidationScopeStep::Child(0)]
                } else {
                    scope.sort_by = Some(2);
                    scope.sort_then_by.push(SortKey {
                        node: leaked,
                        descending: false,
                    });
                    vec![ValidationScopeStep::Child(0)]
                };
                rejects(
                    &project,
                    scope_owner(ValidationEndpoint::Target, &path),
                    leaked,
                    30,
                );
            }
        }
    }
}

#[test]
fn active_parent_aggregate_inputs_and_arguments_execute_in_the_enclosing_context() {
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
        let outputs = run_valid(&project);
        assert_eq!(
            output_values(&outputs.primary, "Value"),
            if argument {
                vec![Value::Int(1), Value::Int(2), Value::Int(3)]
            } else {
                vec![Value::Int(1), Value::Int(3), Value::Int(6)]
            }
        );
        let reopened = mapping::project_file::decode_str(
            &mapping::project_file::encode_pretty(&project).unwrap(),
        )
        .unwrap();
        assert_eq!(
            output_values(&run_valid(&reopened).primary, "Value"),
            output_values(&outputs.primary, "Value")
        );
    }
}

#[test]
fn nested_reducer_aggregate_inputs_can_use_the_active_private_parent_item() {
    let mut project = project();
    project.target = SchemaNode::group("Output", vec![row_schema(ScalarType::Bool)]);
    project.graph.nodes.extend([
        (30, item()),
        (60, item()),
        (
            50,
            Node::SequenceAggregate {
                function: AggregateOp::Sum,
                sequence: range(60, 30),
                predicate: None,
                expression: None,
                arg: None,
            },
        ),
        (70, call("greater_than", &[50, 2])),
        (
            40,
            Node::SequenceExists {
                sequence: range(30, 10),
                predicate: 70,
            },
        ),
    ]);
    project.root.children[0].bindings[0].node = 40;
    assert_eq!(
        output_values(&run_valid(&project).primary, "Value"),
        [Value::Bool(true), Value::Bool(true), Value::Bool(true)]
    );
}

#[test]
fn reducer_private_bodies_do_not_inherit_outer_items_and_item_at_stays_isolated() {
    for form in 0..4 {
        let mut project = project();
        project.graph.nodes.insert(30, item());
        project.graph.nodes.insert(
            40,
            match form {
                0 => Node::SequenceAggregate {
                    function: AggregateOp::Sum,
                    sequence: range(30, 1),
                    predicate: None,
                    expression: Some(10),
                    arg: None,
                },
                1 => Node::SequenceExists {
                    sequence: range(30, 1),
                    predicate: 35,
                },
                2 => Node::SequenceItemAt {
                    sequence: range(30, 10),
                    index: 1,
                },
                _ => Node::SequenceItemAt {
                    sequence: range(30, 1),
                    index: 10,
                },
            },
        );
        if form == 1 {
            project
                .graph
                .nodes
                .insert(35, call("greater_than", &[10, 2]));
        }
        project.root.children[0].bindings[0].node = 40;
        rejects(
            &project,
            ValidationOwner::GraphNode {
                function: None,
                node: 40,
            },
            40,
            10,
        );
    }
}

#[test]
fn nested_parent_inputs_windows_and_ancestor_ids_keep_innermost_value_resolution() {
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
    let mut schema = row_schema(ScalarType::Int);
    schema.name = "Children".into();
    fields.push(schema);
    let mut child = rows(20, 10);
    child.target_field = "Children".into();
    child.bindings[0].node = 10;
    child.windows.push(SequenceWindow::First { count: 10 });
    project.root.children[0].children.push(child);
    let output = run_valid(&project).primary;
    let rows = output
        .field("Rows")
        .and_then(Instance::as_repeated)
        .unwrap();
    for (index, row) in rows.iter().enumerate() {
        let values = row
            .field("Children")
            .and_then(Instance::as_repeated)
            .unwrap()
            .iter()
            .map(|child| {
                child
                    .field("Value")
                    .and_then(Instance::as_scalar)
                    .unwrap()
                    .clone()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            values,
            (1..=index as i64 + 1).map(Value::Int).collect::<Vec<_>>()
        );
    }
    let mut invalid = project.clone();
    invalid.root.children[0].children[0].windows[0] = SequenceWindow::First { count: 20 };
    rejects(
        &invalid,
        scope_owner(
            ValidationEndpoint::Target,
            &[ValidationScopeStep::Child(0), ValidationScopeStep::Child(0)],
        ),
        20,
        20,
    );
}

#[test]
fn nested_block_size_uses_parent_item_and_concatenated_segments_do_not_share_items() {
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
    fields.push(SchemaNode::group("Blocks", Vec::new()).repeating());
    project.root.children[0].children.push(Scope {
        target_field: "Blocks".into(),
        iteration: ScopeIteration::Sequence(range(20, 1)),
        group_into_blocks: Some(10),
        ..Scope::default()
    });
    let output = run_valid(&project).primary;
    let counts = output
        .field("Rows")
        .and_then(Instance::as_repeated)
        .unwrap()
        .iter()
        .map(|row| {
            row.field("Blocks")
                .and_then(Instance::as_repeated)
                .unwrap()
                .len()
        })
        .collect::<Vec<_>>();
    assert_eq!(counts, [3, 2, 1]);

    let mut concatenated = self::project();
    concatenated.graph.nodes.insert(20, item());
    let mut first = rows(10, 1);
    first.target_field.clear();
    let mut second = rows(20, 1);
    second.target_field.clear();
    concatenated.root.children[0] = Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Concatenate(ScopeSequence::new(first, vec![second])),
        ..Scope::default()
    };
    assert_eq!(
        output_values(&run_valid(&concatenated).primary, "Value"),
        [
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3)
        ]
    );
    let segments = concatenated.root.children[0].concatenated_mut().unwrap();
    let second = segments.iter_mut().nth(1).unwrap();
    second.set_sequence(Some(range(20, 10)));
    rejects(
        &concatenated,
        scope_owner(
            ValidationEndpoint::Target,
            &[
                ValidationScopeStep::Child(0),
                ValidationScopeStep::Segment(1),
            ],
        ),
        10,
        10,
    );
}

#[test]
fn dynamic_child_keys_use_parent_items_before_child_generation() {
    let mut project = project();
    project.graph.nodes.extend([
        (20, item()),
        (
            3,
            Node::Const {
                value: Value::String("k".into()),
            },
        ),
        (31, call("concat", &[3, 10])),
        (32, call("concat", &[3, 20])),
    ]);
    let dynamic =
        SchemaNode::group("Child", vec![SchemaNode::scalar("Value", ScalarType::Int)]).repeating();
    let target = row_schema(ScalarType::Int)
        .with_dynamic_fields(dynamic)
        .unwrap();
    project.target = SchemaNode::group("Output", vec![target]);
    let mut child = rows(20, 10);
    child.target_field.clear();
    child.bindings[0].node = 10;
    project.root.children[0]
        .dynamic_children
        .push(DynamicChild {
            key: 31,
            scope: child,
        });
    let output = run_valid(&project).primary;
    for (index, row) in output
        .field("Rows")
        .and_then(Instance::as_repeated)
        .unwrap()
        .iter()
        .enumerate()
    {
        let children = row
            .field(&format!("k{}", index + 1))
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(children.len(), index + 1);
        assert_eq!(
            children
                .last()
                .unwrap()
                .field("Value")
                .and_then(Instance::as_scalar),
            Some(&Value::Int(index as i64 + 1))
        );
    }
    project.root.children[0].dynamic_children[0].key = 32;
    rejects(
        &project,
        scope_owner(ValidationEndpoint::Target, &[ValidationScopeStep::Child(0)]),
        32,
        20,
    );
}

#[test]
fn failure_rule_sites_have_exact_typed_owners_and_no_target_item_permissions() {
    for generated in [false, true] {
        let mut project = project();
        project
            .graph
            .nodes
            .extend([(30, item()), (31, call("greater_than", &[10, 2]))]);
        project.failure_rules.push(FailureRule {
            iteration: if generated {
                FailureIteration::Sequence {
                    sequence: range(30, 1),
                }
            } else {
                FailureIteration::Source {
                    collection: Vec::new(),
                }
            },
            selection: FailureSelection::WhenTrue { predicate: 31 },
            message: None,
        });
        rejects(&project, ValidationOwner::FailureRule { index: 0 }, 31, 10);
    }
    let mut input = project();
    input.graph.nodes.insert(30, item());
    input.failure_rules.push(FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: range(30, 10),
        },
        selection: FailureSelection::All,
        message: None,
    });
    rejects(&input, ValidationOwner::FailureRule { index: 0 }, 10, 10);
}

#[test]
fn shared_expression_is_checked_in_both_allowed_and_forbidden_contexts() {
    let mut project = project();
    project.graph.nodes.extend([
        (30, item()),
        (31, call("add", &[10, 2])),
        (35, call("greater_than", &[31, 2])),
        (
            40,
            Node::SequenceExists {
                sequence: range(30, 1),
                predicate: 35,
            },
        ),
        (
            80,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            70,
            Node::If {
                condition: 80,
                then: 31,
                else_: 40,
            },
        ),
    ]);
    project.root.children[0].bindings[0].node = 70;
    // The same node31 is allowed in the outer branch but forbidden when
    // reached from the reducer's private predicate, even though it is lazy.
    rejects(
        &project,
        ValidationOwner::GraphNode {
            function: None,
            node: 40,
        },
        70,
        10,
    );
    project
        .graph
        .nodes
        .insert(35, call("greater_than", &[30, 2]));
    assert_eq!(
        output_values(&run_valid(&project).primary, "Value"),
        [Value::Int(1), Value::Int(2), Value::Int(3)]
    );
}

#[test]
fn ordinary_absolute_and_frame_pinned_fields_keep_source_resolution() {
    for pinned in [false, true] {
        let mut project = project();
        project.source = SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group("Items", vec![SchemaNode::scalar("N", ScalarType::Int)])
                    .repeating(),
            ],
        );
        project.graph.nodes.insert(
            50,
            Node::SourceField {
                path: if pinned {
                    vec!["N".into()]
                } else {
                    vec!["Items".into(), "N".into()]
                },
                frame: pinned.then(|| vec!["Items".into()]),
            },
        );
        project.root.children[0].bindings[0].node = 50;
        assert!(validate(&project).is_empty(), "{:#?}", validate(&project));
        let source = Instance::Group(vec![(
            "Items".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![("N".into(), Instance::Scalar(Value::Int(99)))]),
                Instance::Group(vec![("N".into(), Instance::Scalar(Value::Int(100)))]),
            ]),
        )]);
        let uniterated = if pinned { Value::Null } else { Value::Int(99) };
        assert_eq!(
            output_values(&run_outputs(&project, &source).unwrap().primary, "Value"),
            vec![uniterated; 3]
        );

        // An explicit source frame requires an active matching iteration.
        // Its fields broadcast through the descendant generated scalar frame.
        project.target = SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Records", vec![row_schema(ScalarType::Int)]).repeating()],
        );
        project.root.children = vec![Scope {
            target_field: "Records".into(),
            iteration: ScopeIteration::Source(vec!["Items".into()]),
            children: project.root.children.clone(),
            ..Scope::default()
        }];
        assert!(validate(&project).is_empty(), "{:#?}", validate(&project));
        let output = run_outputs(&project, &source).unwrap().primary;
        let values: Vec<_> = output
            .field("Records")
            .and_then(Instance::as_repeated)
            .unwrap()
            .iter()
            .map(|record| output_values(record, "Value"))
            .collect();
        assert_eq!(values, [vec![Value::Int(99); 3], vec![Value::Int(100); 3]]);
    }
}
